//! Tek bir kutunun eşitlenmesi: yeni postalar, silinenler, bayraklar.

use std::collections::{HashMap, HashSet};

use async_imap::types::{Fetch, Flag};
use futures::TryStreamExt;
use mh_core::{AppError, ErrorCode, MessageId, Millis};
use mh_store::{MailboxInfo, NewMessage, Store, UpsertOutcome};

use crate::conn::ImapSession;
use crate::error::{SyncError, SyncResult};

/// İlk eşitlemede indirilecek en yeni posta sayısı.
pub const INITIAL_FETCH: u32 = 300;
/// Bu boyutun üstündeki postaların yalnız başlığı indirilir.
pub const MAX_INLINE_BODY: u32 = 512 * 1024;
/// Bayrakları yenilenen en yeni posta sayısı.
pub const FLAG_REFRESH: usize = 500;
const CHUNK: usize = 50;

/// "Daha eski postalar" isteğinde kutu başına indirilen posta sayısı.
pub const OLDER_BATCH: usize = 300;

/// `fetch_older` sonucu.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OlderOutcome {
    /// Bu çağrıda yerele yeni yazılan posta sayısı.
    pub fetched: u32,
    /// Sunucuda hâlâ daha eski posta var mı.
    pub more: bool,
}

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

/// FETCH yanıtından ayrıştırma için gereken sahipli veriler; ayrı iş
/// parçacığına taşınabilir.
struct RawFetch {
    uid: u32,
    internal: Option<Millis>,
    raw: Vec<u8>,
    headers_only: bool,
    unread: bool,
    starred: bool,
    size: Option<u32>,
}

impl RawFetch {
    fn of(f: &Fetch) -> Option<Self> {
        let uid = f.uid?;
        let (raw, headers_only) = match f.body() {
            Some(b) => (b.to_vec(), false),
            None => (f.header()?.to_vec(), true),
        };
        let (unread, starred) = flags_of(f);
        Some(Self {
            uid,
            internal: f.internal_date().map(|d| d.timestamp_millis()),
            raw,
            headers_only,
            unread,
            starred,
            size: f.size,
        })
    }
}

fn to_message(
    f: RawFetch,
    account_id: &str,
    mailbox_id: &str,
    uidvalidity: Option<u32>,
    now: Millis,
) -> Option<NewMessage> {
    let fallback = f.internal.unwrap_or(now);
    let mut m = NewMessage::from_raw(f.raw, f.headers_only, account_id, mailbox_id, fallback)?;
    // Gelecekten gelen (bozuk) tarihlerde sunucunun alış zamanı esas alınır.
    if m.date > now + 86_400_000 {
        m.date = fallback;
    }
    m.uid = Some(f.uid);
    m.uidvalidity = uidvalidity;
    m.unread = f.unread;
    m.starred = f.starred;
    if let Some(size) = f.size {
        m.size = u64::from(size);
    }
    Some(m)
}

/// Postaları ayrı iş parçacığında ayrıştırır: HTML → metin işlemciye
/// bağlıdır; büyük ya da kötü niyetli bir posta eşzamansız eşitleme görevini
/// (ve aynı iş parçacığındaki diğer görevleri) bloklamasın.
async fn parse_all(
    items: Vec<RawFetch>,
    info: &MailboxInfo,
    uidvalidity: Option<u32>,
    now: Millis,
) -> SyncResult<Vec<(u32, Option<NewMessage>)>> {
    let account = info.mailbox.account_id.clone();
    let mailbox = info.mailbox.id.clone();
    tokio::task::spawn_blocking(move || {
        items
            .into_iter()
            .map(|f| {
                let uid = f.uid;
                (uid, to_message(f, &account, &mailbox, uidvalidity, now))
            })
            .collect()
    })
    .await
    .map_err(|_| {
        SyncError::App(AppError::new(
            ErrorCode::Internal,
            "Postalar ayrıştırılamadı.",
        ))
    })
}

/// İndirilecek UID'ler: yerelde olmayanlar, en yeniler önce (liste hızla
/// dolsun).
fn to_fetch(mut server: Vec<u32>, have: &HashSet<u32>) -> Vec<u32> {
    server.retain(|u| !have.contains(u));
    server.sort_unstable_by(|a, b| b.cmp(a));
    server.dedup();
    server
}

async fn fetch_uids(session: &mut ImapSession, seq: bool, set: &str) -> SyncResult<Vec<u32>> {
    let fetches: Vec<Fetch> = if seq {
        session.fetch(set, "(UID)").await?.try_collect().await?
    } else {
        session.uid_fetch(set, "(UID)").await?.try_collect().await?
    };
    Ok(fetches.iter().filter_map(|f| f.uid).collect())
}

/// Verilen UID'leri (seçili kutudan) indirip depoya yazar: küçük postalar
/// gövdesiyle, büyükler yalnız başlığıyla. Olağan alım yoludur (ayrıştırma,
/// önizleme, kategori, kod ve istenmeyen puanı). Yazılan her posta için
/// `stored` çağrılır (sonuç, okunmamış mı).
async fn download(
    session: &mut ImapSession,
    store: &Store,
    info: &MailboxInfo,
    uidvalidity: Option<u32>,
    uids: &[u32],
    mut stored: impl FnMut(UpsertOutcome, bool),
) -> SyncResult<()> {
    let now = now_ms();
    for chunk in uids.chunks(CHUNK) {
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
            let items = {
                let fetches: Vec<Fetch> = session
                    .uid_fetch(uid_set(&uids), query)
                    .await?
                    .try_collect()
                    .await?;
                let mut items = Vec::with_capacity(fetches.len());
                for f in fetches
                    .iter()
                    .filter(|f| f.uid.is_some_and(|u| uids.contains(&u)))
                {
                    match RawFetch::of(f) {
                        Some(r) => items.push(r),
                        None => tracing::warn!(uid = ?f.uid, "posta okunamadı, atlandı"),
                    }
                }
                items
            };
            for (uid, m) in parse_all(items, info, uidvalidity, now).await? {
                let Some(m) = m else {
                    tracing::warn!(uid, "posta ayrıştırılamadı, atlandı");
                    continue;
                };
                let unread = m.unread;
                stored(store.upsert_message(&m)?, unread);
            }
        }
    }
    Ok(())
}

/// Sunucuda olup yerelde olmayan UID'lerden indirilecek parti (yeniden
/// eskiye, en çok `limit`) ve ondan sonra daha eskisinin kalıp kalmadığı.
/// `skip`: yerelde bilerek bulunmayan UID'ler (kalıcı silinmesi sırada
/// bekleyenler).
fn older_batch(
    server: impl IntoIterator<Item = u32>,
    have: &HashSet<u32>,
    skip: &HashSet<u32>,
    limit: usize,
) -> (Vec<u32>, bool) {
    let mut missing: Vec<u32> = server
        .into_iter()
        .filter(|u| !have.contains(u) && !skip.contains(u))
        .collect();
    missing.sort_unstable_by(|a, b| b.cmp(a));
    missing.dedup();
    let more = missing.len() > limit;
    missing.truncate(limit);
    (missing, more)
}

/// Kutunun bir sonraki eski partisini indirir (`messages_older`).
///
/// İlk eşitleme yalnız en yeni postaları indirir; burada sunucunun filigrana
/// (görülen en büyük UID) kadarki UID'lerinden yerelde olmayanların en yeni
/// `OLDER_BATCH` tanesi alınır. Sınır "yereldeki en küçük UID" değil küme
/// farkıdır: yarıda kalan bir indirmenin bıraktığı boşluk da, yerelde hiç
/// postası kalmamış kutu da aynı yoldan tamamlanır. Filigranın üstündeki
/// (yeni gelen) postalara dokunulmaz; onları olağan eşitleme bildirimle
/// indirir. Bu yoldan inen posta bildirim, kural ya da elek tetiklemez.
///
/// Kutu henüz eşitlenmediyse ya da UIDVALIDITY değiştiyse hiçbir şey
/// indirilmez (`more` doğru döner): kutuyu önce olağan eşitleme kurar.
pub async fn fetch_older(
    session: &mut ImapSession,
    store: &Store,
    info: &MailboxInfo,
    skip: &HashSet<u32>,
) -> SyncResult<OlderOutcome> {
    let id = info.mailbox.id.as_str();
    let mb = session.examine(&info.mailbox.path).await?;
    if mb.exists == 0 {
        return Ok(OlderOutcome::default());
    }
    if info.uidvalidity.is_none() || info.uidvalidity != mb.uid_validity || info.highest_uid == 0 {
        return Ok(OlderOutcome {
            fetched: 0,
            more: true,
        });
    }
    let server: HashSet<u32> =
        crate::checked::uid_search(session, &format!("UID 1:{}", info.highest_uid)).await?;
    let have: HashSet<u32> = store
        .local_uids(id)?
        .into_iter()
        .map(|(_, uid, ..)| uid)
        .collect();
    let (batch, more) = older_batch(
        server.into_iter().filter(|u| *u <= info.highest_uid),
        &have,
        skip,
        OLDER_BATCH,
    );
    let mut fetched = 0u32;
    download(session, store, info, mb.uid_validity, &batch, |r, _| {
        if r.inserted {
            fetched += 1;
        }
    })
    .await?;
    if !more {
        // Parti eksiksiz indi ve gerisi yok: kutunun başına ulaşıldı.
        store.set_mailbox_older_done(id, true)?;
    }
    Ok(OlderOutcome { fetched, more })
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
    // UIDVALIDITY hemen yazılır (filigran değişmeden): yarıda kalan ilk
    // eşitlemeden sonra da kutunun değiştiği fark edilsin.
    if info.uidvalidity != mb.uid_validity {
        store.set_mailbox_sync_state(id, mb.uid_validity, info.uidnext, highest)?;
    }

    // 1) Yeni UID'ler
    let server_uids = if mb.exists == 0 {
        Vec::new()
    } else if highest == 0 {
        let start = mb.exists.saturating_sub(INITIAL_FETCH - 1).max(1);
        fetch_uids(session, true, &format!("{start}:*")).await?
    } else {
        let mut v = fetch_uids(session, false, &format!("{}:*", highest + 1)).await?;
        v.retain(|u| *u > highest);
        v
    };
    // Filigran (görülen en büyük UID) yalnız aralığın tamamı indirildikten
    // sonra ilerler: yarıda kalan eşitleme (kapanış, kopan bağlantı, zaman
    // aşımı) eksik postaları bir sonrakinde yeniden ister. Önceki denemede
    // inenler yeniden indirilmez.
    let watermark = server_uids.iter().copied().fold(highest, u32::max);
    let have: HashSet<u32> = store
        .local_uids(id)?
        .into_iter()
        .map(|(_, uid, ..)| uid)
        .collect();
    let new_uids = to_fetch(server_uids, &have);

    download(
        session,
        store,
        info,
        mb.uid_validity,
        &new_uids,
        |r, unread| {
            out.changed = true;
            if r.inserted && unread && notify && !initial {
                out.new_unread.push(r.id);
            }
        },
    )
    .await?;
    store.set_mailbox_sync_state(id, mb.uid_validity, mb.uid_next, watermark)?;

    // 2) Sunucuda silinenler ve bayrak değişiklikleri
    let local = store.local_uids(id)?;
    if let Some(&(_, min_uid, _, _)) = local.first() {
        // Durumu denetlenen arama: sunucu aramayı reddederse boş küme "hepsi
        // silinmiş" diye okunmaz, eşitleme hata verir (yerel postalar kalır).
        let existing: HashSet<u32> = if mb.exists == 0 {
            HashSet::new()
        } else {
            crate::checked::uid_search(session, &format!("UID {min_uid}:*")).await?
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
    fn indirilecekler_yeniden_eskiye_yereldekiler_haric() {
        let have: HashSet<u32> = [9, 4].into_iter().collect();
        assert_eq!(to_fetch(vec![3, 9, 5, 3, 4, 12], &have), vec![12, 5, 3]);
        assert!(to_fetch(vec![], &have).is_empty());
    }

    #[test]
    fn daha_eski_parti_yeniden_eskiye_ve_gerisi() {
        let set = |v: &[u32]| v.iter().copied().collect::<HashSet<u32>>();
        let have = set(&[8, 9, 10]);
        let none = HashSet::new();
        // Yerelde olmayanların en yenileri; gerisi kaldıysa `more`.
        assert_eq!(
            older_batch([1, 2, 3, 5, 8, 9, 10], &have, &none, 3),
            (vec![5, 3, 2], true)
        );
        assert_eq!(
            older_batch([1, 2, 3, 5, 8, 9, 10], &have, &none, 4),
            (vec![5, 3, 2, 1], false),
            "parti tam dolunca gerisi yok"
        );
        // Aradaki boşluk (yarıda kalmış indirme) da tamamlanır.
        assert_eq!(
            older_batch([7, 8, 9, 10], &have, &none, 300),
            (vec![7], false)
        );
        // Kalıcı silinmesi bekleyen UID yeniden indirilmez.
        assert_eq!(
            older_batch([4, 5, 6, 8], &have, &set(&[5]), 300),
            (vec![6, 4], false)
        );
        // Hepsi yerelde ya da sunucu boş.
        assert_eq!(older_batch([8, 9], &have, &none, 300), (vec![], false));
        assert_eq!(older_batch([], &have, &none, 300), (vec![], false));
        // Yerelde hiç posta yok: en yeni parti.
        assert_eq!(older_batch(1..=5, &none, &none, 2), (vec![5, 4], true));
    }

    #[test]
    fn uid_kumesi() {
        assert_eq!(uid_set(&[5, 1, 2, 3, 9, 12, 13, 3]), "1:3,5,9,12:13");
        assert_eq!(uid_set(&[7]), "7");
        assert_eq!(uid_set(&[]), "");
    }
}
