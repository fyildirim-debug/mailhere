//! Tek bir kutunun eşitlenmesi: yeni postalar, silinenler, bayraklar.

use std::collections::{HashMap, HashSet};

use async_imap::types::{Fetch, Flag};
use futures::TryStreamExt;
use mh_core::{MessageId, Millis};
use mh_store::{MailboxInfo, NewMessage, Store};

use crate::conn::ImapSession;
use crate::error::SyncResult;

/// İlk eşitlemede indirilecek en yeni posta sayısı.
pub const INITIAL_FETCH: u32 = 300;
/// Bu boyutun üstündeki postaların yalnız başlığı indirilir.
pub const MAX_INLINE_BODY: u32 = 512 * 1024;
/// Bayrakları yenilenen en yeni posta sayısı.
pub const FLAG_REFRESH: usize = 500;
const CHUNK: usize = 50;

/// Kutu eşitlemesinin sonucu.
#[derive(Debug, Default)]
pub struct MailboxOutcome {
    pub changed: bool,
    /// Yeni eklenen ve okunmamış postalar (ilk eşitlemede boş).
    pub new_unread: Vec<MessageId>,
}

fn flags_of(f: &Fetch) -> (bool, bool) {
    let mut seen = false;
    let mut flagged = false;
    for flag in f.flags() {
        match flag {
            Flag::Seen => seen = true,
            Flag::Flagged => flagged = true,
            _ => {}
        }
    }
    (!seen, flagged)
}

/// UID listesini sıkıştırılmış IMAP kümesine çevirir ("1:5,9,12:13").
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
        i += 1;
    }
    parts.join(",")
}

fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}

fn to_message(
    f: &Fetch,
    info: &MailboxInfo,
    uidvalidity: Option<u32>,
    now: Millis,
) -> Option<NewMessage> {
    let uid = f.uid?;
    let internal = f.internal_date().map(|d| d.timestamp_millis());
    let (raw, headers_only) = match f.body() {
        Some(b) => (b.to_vec(), false),
        None => (f.header()?.to_vec(), true),
    };
    let fallback = internal.unwrap_or(now);
    let mut m = NewMessage::from_raw(
        raw,
        headers_only,
        &info.mailbox.account_id,
        &info.mailbox.id,
        fallback,
    )?;
    // Gelecekten gelen (bozuk) tarihlerde sunucunun alış zamanı esas alınır.
    if m.date > now + 86_400_000 {
        m.date = fallback;
    }
    let (unread, starred) = flags_of(f);
    m.uid = Some(uid);
    m.uidvalidity = uidvalidity;
    m.unread = unread;
    m.starred = starred;
    if let Some(size) = f.size {
        m.size = u64::from(size);
    }
    Some(m)
}

async fn fetch_uids(session: &mut ImapSession, seq: bool, set: &str) -> SyncResult<Vec<u32>> {
    let fetches: Vec<Fetch> = if seq {
        session.fetch(set, "(UID)").await?.try_collect().await?
    } else {
        session.uid_fetch(set, "(UID)").await?.try_collect().await?
    };
    Ok(fetches.iter().filter_map(|f| f.uid).collect())
}

/// Kutuyu sunucuyla eşitler. `notify` doğruysa yeni okunmamış postalar
/// sonuçta listelenir (ilk eşitleme hariç).
pub async fn sync_mailbox(
    session: &mut ImapSession,
    store: &Store,
    info: &MailboxInfo,
    notify: bool,
) -> SyncResult<MailboxOutcome> {
    let mut out = MailboxOutcome::default();
    let id = info.mailbox.id.as_str();
    let mb = session.examine(&info.mailbox.path).await?;
    let mut highest = info.highest_uid;
    let mut initial = info.uidvalidity.is_none() || highest == 0;
    if info.uidvalidity.is_some() && info.uidvalidity != mb.uid_validity {
        tracing::info!(mailbox = %info.mailbox.path, "UIDVALIDITY değişti, kutu yeniden indiriliyor");
        store.clear_mailbox_uids(id)?;
        highest = 0;
        initial = true;
        out.changed = true;
    }

    // 1) Yeni UID'ler
    let mut new_uids = if mb.exists == 0 {
        Vec::new()
    } else if highest == 0 {
        let start = mb.exists.saturating_sub(INITIAL_FETCH - 1).max(1);
        fetch_uids(session, true, &format!("{start}:*")).await?
    } else {
        let mut v = fetch_uids(session, false, &format!("{}:*", highest + 1)).await?;
        v.retain(|u| *u > highest);
        v
    };
    // En yeniler önce gelsin ki liste hızla dolsun.
    new_uids.sort_unstable_by(|a, b| b.cmp(a));
    new_uids.dedup();

    let now = now_ms();
    for chunk in new_uids.chunks(CHUNK) {
        let set = uid_set(chunk);
        let sizes: HashMap<u32, u32> = session
            .uid_fetch(&set, "(UID RFC822.SIZE)")
            .await?
            .try_collect::<Vec<Fetch>>()
            .await?
            .iter()
            .filter_map(|f| Some((f.uid?, f.size.unwrap_or(0))))
            .collect();
        let (small, big): (Vec<u32>, Vec<u32>) = chunk
            .iter()
            .copied()
            .filter(|u| sizes.contains_key(u))
            .partition(|u| sizes[u] <= MAX_INLINE_BODY);
        for (uids, query) in [
            (small, "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])"),
            (
                big,
                "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])",
            ),
        ] {
            if uids.is_empty() {
                continue;
            }
            let fetches: Vec<Fetch> = session
                .uid_fetch(uid_set(&uids), query)
                .await?
                .try_collect()
                .await?;
            for f in fetches
                .iter()
                .filter(|f| f.uid.is_some_and(|u| uids.contains(&u)))
            {
                let Some(m) = to_message(f, info, mb.uid_validity, now) else {
                    tracing::warn!(uid = ?f.uid, "posta ayrıştırılamadı, atlandı");
                    continue;
                };
                let unread = m.unread;
                let r = store.upsert_message(&m)?;
                out.changed = true;
                if r.inserted && unread && notify && !initial {
                    out.new_unread.push(r.id);
                }
            }
        }
        highest = highest.max(chunk.iter().copied().max().unwrap_or(0));
        store.set_mailbox_sync_state(id, mb.uid_validity, mb.uid_next, highest)?;
    }
    store.set_mailbox_sync_state(id, mb.uid_validity, mb.uid_next, highest)?;

    // 2) Sunucuda silinenler ve bayrak değişiklikleri
    let local = store.local_uids(id)?;
    if let Some(&(_, min_uid, _, _)) = local.first() {
        let existing: HashSet<u32> = if mb.exists == 0 {
            HashSet::new()
        } else {
            session.uid_search(format!("UID {min_uid}:*")).await?
        };
        let gone: Vec<MessageId> = local
            .iter()
            .filter(|(_, uid, _, _)| !existing.contains(uid))
            .map(|(mid, ..)| *mid)
            .collect();
        if !gone.is_empty() {
            store.delete_messages(&gone)?;
            out.changed = true;
        }
        let recent: Vec<_> = local
            .iter()
            .rev()
            .filter(|(_, uid, _, _)| existing.contains(uid))
            .take(FLAG_REFRESH)
            .collect();
        if let Some(lowest) = recent.last().map(|(_, uid, _, _)| *uid) {
            let by_uid: HashMap<u32, (MessageId, bool, bool)> = recent
                .iter()
                .map(|(mid, uid, u, s)| (*uid, (*mid, *u, *s)))
                .collect();
            let fetches: Vec<Fetch> = session
                .uid_fetch(format!("{lowest}:*"), "(UID FLAGS)")
                .await?
                .try_collect()
                .await?;
            for f in &fetches {
                let Some((mid, was_unread, was_starred)) = f.uid.and_then(|u| by_uid.get(&u))
                else {
                    continue;
                };
                let (unread, starred) = flags_of(f);
                if (unread, starred) != (*was_unread, *was_starred)
                    && store.set_server_flags(*mid, unread, starred)?
                {
                    out.changed = true;
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_kumesi() {
        assert_eq!(uid_set(&[5, 1, 2, 3, 9, 12, 13, 3]), "1:3,5,9,12:13");
        assert_eq!(uid_set(&[7]), "7");
        assert_eq!(uid_set(&[]), "");
    }
}
