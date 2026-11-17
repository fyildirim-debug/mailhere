//! İstenmeyen koruması: engellenen göndericiler ve şüpheli postalar.
//!
//! Yeni posta akışının ilk adımıdır ([`filter_new`]): kurallardan, elekten ve
//! bildirim kararından ÖNCE çalışır. Pencereli (`sink.rs`) ve penceresiz
//! (`headless.rs`) çekirdek aynı kodu kullanır.
//!
//! - **Engellenen gönderici** (`Settings::blocked`): posta İstenmeyen'e
//!   (kutu yoksa Çöp'e) taşınır; kural çalışmaz, bildirim gösterilmez.
//!   `spam_filter` kapalı olsa da çalışır.
//! - **Şüpheli posta** (puan eşiğin üstünde, `MessageSummary::spam`):
//!   `move` kipinde İstenmeyen'e taşınır; `warn` kipinde (ve `move` kipinde
//!   İstenmeyen kutusu yoksa) yerinde kalır, kurallar ve elek çalışır ama kart
//!   gösterilmez ([`notifiable`], bildirim kararında); `off` kipinde olağan
//!   akış.
//!
//! Kendi adreslerimiz ve bu hesapta kabul edilmiş göndericiler ("Güvenli",
//! elekten kabul) engellenmez: son açık karar geçerlidir. Günlüğe yalnız
//! posta kimliği ve karar yazılır; adres, konu ya da içerik yazılmaz.

use std::collections::HashSet;

use mh_core::{
    events, Account, AppError, AppResult, BlockResult, ErrorCode, MailboxRole, MessageId,
    MessageSummary, MoveTarget, Settings, SpamFilter, BLOCKED_MAX,
};
use tauri::{AppHandle, Emitter, State};

use crate::state::{now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Hesapların kendi adresleri (karşılaştırma biçiminde).
fn own_addresses(state: &AppState) -> HashSet<String> {
    state
        .store
        .accounts()
        .unwrap_or_default()
        .iter()
        .map(|a| mh_store::normalize_email(&a.email))
        .collect()
}

/// Postaları hesabın İstenmeyen kutusuna, kutu yoksa Çöp'e taşır; içeriği
/// değişen kutuları döner.
fn move_to_junk(state: &AppState, account_id: &str, ids: &[MessageId]) -> AppResult<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    match state.store.mailbox_by_role(account_id, MailboxRole::Junk)? {
        Some(junk) => crate::rules::Engines::of(state).move_to_mailbox(ids, &junk.mailbox.id),
        None => crate::cmd_jmap::move_messages(state, ids, MoveTarget::Trash),
    }
}

/// Yeni postanın süzgeç kararı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Olağan akış (kurallar, elek, bildirim).
    Keep,
    /// Engellenen gönderici: İstenmeyen'e (yoksa Çöp'e) taşınır.
    Blocked,
    /// Şüpheli ve kip `move`: İstenmeyen'e taşınır (kutu varsa).
    Suspicious,
}

/// Süzgeç kararı (saf): `blocked` gönderen engellenenler listesinde ve muaf
/// değil; `spam` puan eşiğin üstünde.
pub fn verdict(filter: SpamFilter, blocked: bool, spam: bool) -> Verdict {
    if blocked {
        Verdict::Blocked
    } else if spam && filter == SpamFilter::Move {
        Verdict::Suspicious
    } else {
        Verdict::Keep
    }
}

/// Şüpheli posta için kart gösterilir mi? `off` kipinde evet; `warn` ve
/// (İstenmeyen kutusu olmadığı için taşınamayan) `move` kipinde hayır.
pub fn notifiable(settings: &Settings, message: &MessageSummary) -> bool {
    !message.spam || settings.spam_filter == SpamFilter::Off
}

/// Yeni posta akışının ilk adımı: engellenen göndericilerin ve (`move`
/// kipinde) şüpheli postaların gelen kutusundan çıkarılması. Akışa devam
/// edecek postaları ve içeriği değişen kutuları döner.
pub(crate) fn filter_new(
    state: &AppState,
    account: &Account,
    messages: Vec<MessageSummary>,
) -> (Vec<MessageSummary>, Vec<String>) {
    if messages.is_empty() {
        return (messages, Vec::new());
    }
    let Ok(settings) = state.store.settings() else {
        return (messages, Vec::new());
    };
    let any_spam = settings.spam_filter == SpamFilter::Move && messages.iter().any(|m| m.spam);
    if settings.blocked.is_empty() && !any_spam {
        return (messages, Vec::new());
    }
    let own = own_addresses(state);
    let mut kept = Vec::with_capacity(messages.len());
    let mut touched: Vec<String> = Vec::new();
    for m in messages {
        let email = mh_store::normalize_email(&m.from.email);
        let blocked = mh_core::is_blocked(&settings.blocked, &email)
            && !own.contains(&email)
            && !state
                .store
                .sender_accepted(&account.id, &email)
                .unwrap_or(false);
        let decision = verdict(settings.spam_filter, blocked, m.spam);
        if decision == Verdict::Keep {
            kept.push(m);
            continue;
        }
        // Yalnız gelen kutusundaki posta taşınır (başka kutuya düşen ya da bu
        // arada taşınan postaya dokunulmaz).
        let in_inbox = state
            .store
            .location(m.id)
            .ok()
            .flatten()
            .is_some_and(|l| l.role == MailboxRole::Inbox);
        if !in_inbox {
            kept.push(m);
            continue;
        }
        // Şüpheli posta yalnız İstenmeyen'e taşınır; kutu yoksa yerinde kalır
        // (uyarıyla) ve bildirim kararında kartsız geçer.
        if decision == Verdict::Suspicious
            && !matches!(
                state.store.mailbox_by_role(&account.id, MailboxRole::Junk),
                Ok(Some(_))
            )
        {
            kept.push(m);
            continue;
        }
        match move_to_junk(state, &account.id, &[m.id]) {
            Ok(t) => {
                tracing::info!(
                    id = m.id,
                    engelli = decision == Verdict::Blocked,
                    "yeni posta gelen kutusundan çıkarıldı (istenmeyen koruması)"
                );
                touched.extend(t);
            }
            Err(e) => {
                // Taşınamadı: posta yerinde kalır; engelli gönderici yine
                // bildirim almaz (aşağıda listeye eklenmez), şüpheli posta
                // uyarıyla kalır.
                tracing::warn!(id = m.id, code = %e.code, "istenmeyen posta taşınamadı");
                if decision == Verdict::Suspicious {
                    kept.push(m);
                }
            }
        }
    }
    touched.sort();
    touched.dedup();
    (kept, touched)
}

fn not_found() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Posta bulunamadı; silinmiş ya da taşınmış olabilir. Listeyi yenileyip yeniden dene.",
    )
}

/// `sender_block`in kendisi (pencere bilmez): sonucu, yeni ayarları ve
/// içeriği değişen kutuları döner.
pub fn block_sender(
    state: &AppState,
    id: MessageId,
) -> AppResult<(BlockResult, Settings, Vec<String>)> {
    let summary = state.store.summary(id)?.ok_or_else(not_found)?;
    let address = mh_core::normalize_blocked_entry(&summary.from.email)
        .filter(|a| !a.starts_with('@'))
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Invalid,
                "Gönderenin adresi eksik ya da bozuk; engellenemedi. Bu postayı silebilir ya da gönderen adına göre bir kural oluşturabilirsin.",
            )
        })?;
    if state.store.is_own_address(&address)? {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Kendi adresini engelleyemezsin. Bu postadan kurtulmak için onu sil ya da arşivle.",
        ));
    }
    let mut settings = state.store.settings()?;
    if !settings.blocked.contains(&address) {
        if settings.blocked.len() >= BLOCKED_MAX {
            return Err(AppError::new(
                ErrorCode::Invalid,
                format!(
                    "Engellenenler listesi dolu (en çok {BLOCKED_MAX}). Ayarlar › Gizlilik ve güvenlik'ten kullanmadığın girdileri kaldırıp yeniden dene."
                ),
            ));
        }
        settings.blocked.push(address.clone());
        settings.blocked = mh_core::normalize_blocked(settings.blocked);
        state.store.set_settings(&settings)?;
    }
    let account = &summary.account_id;
    // Son açık karar geçerlidir: önceki "Güvenli" / elekten kabul geri alınır,
    // elekte bekleyen postaları elekten çıkar.
    state.store.untrust_sender(account, &address)?;
    state
        .store
        .screener_decide(account, &address, false, now_ms())?;
    let ids = state.store.inbox_messages_from(account, &address)?;
    let mut touched = move_to_junk(state, account, &ids)?;
    touched.sort();
    touched.dedup();
    tracing::info!(id, tasinan = ids.len(), "gönderici engellendi");
    Ok((
        BlockResult {
            address,
            moved: ids.len() as u32,
            ids,
        },
        settings,
        touched,
    ))
}

/// `spam_trust`in kendisi (pencere bilmez): yeni ayarları ve içeriği değişen
/// kutuları döner.
pub fn trust_sender(state: &AppState, id: MessageId) -> AppResult<(Settings, Vec<String>)> {
    let summary = state.store.summary(id)?.ok_or_else(not_found)?;
    let email = mh_store::normalize_email(&summary.from.email);
    if !mh_store::is_full_address(&email) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Gönderenin adresi eksik ya da bozuk; güvenli sayılamadı. Postayı gelen kutusuna elle taşıyabilirsin.",
        ));
    }
    let mut touched = state
        .store
        .trust_sender(&summary.account_id, &email, now_ms())?;
    // Adres engellenenlerden çıkar; alan adı girdisine dokunulmaz (kabul
    // edilmiş gönderici alan adı engelinden zaten muaftır).
    let mut settings = state.store.settings()?;
    let before = settings.blocked.len();
    settings.blocked.retain(|e| *e != email);
    if settings.blocked.len() != before {
        state.store.set_settings(&settings)?;
    }
    let in_junk = state
        .store
        .location(id)?
        .is_some_and(|l| l.role == MailboxRole::Junk);
    if in_junk {
        touched.extend(crate::cmd_jmap::move_messages(
            state,
            &[id],
            MoveTarget::Inbox,
        )?);
    }
    // Okuma alanı ve liste satırı yenilensin (uyarı kalktı).
    touched.push(summary.mailbox_id);
    touched.sort();
    touched.dedup();
    tracing::info!(id, tasindi = in_junk, "gönderici güvenli sayıldı");
    Ok((settings, touched))
}

fn emit_settings(app: &AppHandle, settings: &Settings) {
    if let Err(e) = app.emit(events::SETTINGS, settings) {
        tracing::warn!(error = %e, "ayar olayı gönderilemedi");
    }
}

/// Postanın göndericisini engeller: adres `Settings::blocked` listesine
/// girer, bu hesabın gelen kutusundaki postaları İstenmeyen'e (kutu yoksa
/// Çöp'e) taşınır. `mh:settings` ve `mh:changed` yayar.
#[tauri::command]
pub fn sender_block(app: AppHandle, state: State<'_, AppState>, id: MessageId) -> Res<BlockResult> {
    let (result, settings, touched) = block_sender(&state, id)?;
    emit_settings(&app, &settings);
    crate::sink::changed(&app, touched);
    Ok(result)
}

/// "Güvenli / İstenmeyen değil": gönderici bu hesapta kabul edilmiş sayılır,
/// postalarının istenmeyen puanı sıfırlanır, adresi engellenenlerden çıkar;
/// posta İstenmeyen kutusundaysa gelen kutusuna döner. `mh:settings` ve
/// `mh:changed` yayar.
#[tauri::command]
pub fn spam_trust(app: AppHandle, state: State<'_, AppState>, id: MessageId) -> Res<()> {
    let (settings, touched) = trust_sender(&state, id)?;
    emit_settings(&app, &settings);
    crate::sink::changed(&app, touched);
    Ok(())
}

#[cfg(test)]
mod tests {
    use mh_core::{SpamReason, SpamVerdict};

    use super::*;
    use crate::testutil::{deliver, deliver_to, mailbox_of, raw_mail, state};

    /// Alıcı sunucunun istenmeyen diye işaretlediği posta (puan 60).
    fn flagged(from: &str, mid: &str) -> Vec<u8> {
        raw_mail(
            from,
            "Hesabınız askıya alındı",
            mid,
            "X-Spam-Flag: YES\r\n",
            "Hemen tıklayın.",
        )
    }

    fn plain(from: &str, mid: &str) -> Vec<u8> {
        raw_mail(from, "Selam", mid, "", "Yarın görüşelim mi?")
    }

    fn set(state: &AppState, f: impl FnOnce(&mut Settings)) {
        let mut s = state.store.settings().unwrap();
        f(&mut s);
        state.store.set_settings(&s).unwrap();
    }

    fn account(state: &AppState, id: &str) -> Account {
        state.store.account(id).unwrap().unwrap()
    }

    #[test]
    fn suzgec_karari() {
        use SpamFilter::*;
        // Engellenen gönderici her kipte taşınır.
        for filter in [Off, Warn, Move] {
            assert_eq!(verdict(filter, true, false), Verdict::Blocked);
            assert_eq!(verdict(filter, true, true), Verdict::Blocked);
            assert_eq!(verdict(filter, false, false), Verdict::Keep);
        }
        // Şüpheli posta yalnız `move` kipinde taşınır.
        assert_eq!(verdict(Move, false, true), Verdict::Suspicious);
        assert_eq!(verdict(Warn, false, true), Verdict::Keep);
        assert_eq!(verdict(Off, false, true), Verdict::Keep);
    }

    #[tokio::test]
    async fn supheli_posta_kartsiz_gecer() {
        let st = state();
        let clean = deliver(&st, "a", plain("Ayşe <ayse@eposta.com>", "t1@x"), 1);
        let spam = deliver(&st, "a", flagged("Banka <x@kotu.example>", "t2@x"), 2);
        assert!(spam.spam && !clean.spam);
        let mut s = Settings::default();
        for (filter, card) in [
            (SpamFilter::Warn, false),
            (SpamFilter::Move, false),
            (SpamFilter::Off, true),
        ] {
            s.spam_filter = filter;
            assert_eq!(notifiable(&s, &spam), card, "{filter:?}");
            assert!(notifiable(&s, &clean), "{filter:?}");
        }
    }

    #[tokio::test]
    async fn engellenen_gonderici_gelen_kutusundan_cikar() {
        let st = state();
        set(&st, |s| {
            s.blocked = vec!["reklam@firma.com".into(), "@kotu.example".into()];
            // Koruma kapalıyken de engellenenler çalışır.
            s.spam_filter = SpamFilter::Off;
        });
        let a = account(&st, "a");
        let blocked = deliver(&st, "a", plain("Reklam <Reklam@Firma.com>", "e1@x"), 1);
        let domain = deliver(&st, "a", plain("Kötü <x@posta.kotu.example>", "e2@x"), 2);
        let other = deliver(&st, "a", plain("Ayşe <ayse@eposta.com>", "e3@x"), 3);
        let (kept, touched) = filter_new(
            &st,
            &a,
            vec![blocked.clone(), domain.clone(), other.clone()],
        );
        assert_eq!(kept.iter().map(|m| m.id).collect::<Vec<_>>(), [other.id]);
        assert_eq!(touched, ["a:INBOX", "a:Junk"]);
        assert_eq!(mailbox_of(&st, blocked.id), "a:Junk");
        assert_eq!(mailbox_of(&st, domain.id), "a:Junk");
        assert_eq!(mailbox_of(&st, other.id), "a:INBOX");

        // İstenmeyen kutusu olmayan hesapta Çöp'e gider.
        let b = account(&st, "b");
        let trashed = deliver(&st, "b", plain("Reklam <reklam@firma.com>", "e4@x"), 4);
        let (kept, touched) = filter_new(&st, &b, vec![trashed.clone()]);
        assert!(kept.is_empty());
        assert_eq!(touched, ["b:INBOX", "b:Trash"]);
        assert_eq!(mailbox_of(&st, trashed.id), "b:Trash");

        // Bu hesapta kabul edilmiş gönderici alan adı engelinden muaftır;
        // başka hesapta muaf değildir.
        st.store.trust_sender("a", "dost@kotu.example", 5).unwrap();
        let friend = deliver(&st, "a", plain("Dost <dost@kotu.example>", "e5@x"), 5);
        let friend_b = deliver(&st, "b", plain("Dost <dost@kotu.example>", "e6@x"), 6);
        assert_eq!(filter_new(&st, &a, vec![friend.clone()]).0.len(), 1);
        assert_eq!(mailbox_of(&st, friend.id), "a:INBOX");
        assert!(filter_new(&st, &b, vec![friend_b.clone()]).0.is_empty());
        assert_eq!(mailbox_of(&st, friend_b.id), "b:Trash");

        // Kendi adresimiz (alan adı engellense de) engellenmez.
        set(&st, |s| s.blocked.push("@ornek.com.tr".into()));
        let own = deliver(&st, "a", plain("Furkan <furkan@ornek.com.tr>", "e7@x"), 7);
        assert_eq!(filter_new(&st, &a, vec![own.clone()]).0.len(), 1);
        assert_eq!(mailbox_of(&st, own.id), "a:INBOX");

        // Gelen kutusu dışındaki postaya dokunulmaz.
        let archived = deliver_to(
            &st,
            "a",
            "Archive",
            plain("Reklam <reklam@firma.com>", "e8@x"),
            8,
        );
        let (kept, touched) = filter_new(&st, &a, vec![archived.clone()]);
        assert_eq!((kept.len(), touched.len()), (1, 0));
        assert_eq!(mailbox_of(&st, archived.id), "a:Archive");
        assert!(filter_new(&st, &a, Vec::new()).0.is_empty());
    }

    #[tokio::test]
    async fn supheli_posta_kipe_gore_tasinir() {
        let st = state();
        let a = account(&st, "a");
        let b = account(&st, "b");
        let spam = |account: &str, mid: &str, date| {
            deliver(&st, account, flagged("Banka <x@kotu.example>", mid), date)
        };

        // Uyar (varsayılan): yerinde kalır, akışa devam eder (kartı bildirim
        // kararı keser).
        let warn = spam("a", "s1@x", 1);
        let (kept, touched) = filter_new(&st, &a, vec![warn.clone()]);
        assert_eq!((kept.len(), touched.len()), (1, 0));
        assert_eq!(mailbox_of(&st, warn.id), "a:INBOX");
        assert!(!notifiable(&st.store.settings().unwrap(), &kept[0]));

        // Kapalı: olağan akış.
        set(&st, |s| s.spam_filter = SpamFilter::Off);
        let off = spam("a", "s2@x", 2);
        let (kept, _) = filter_new(&st, &a, vec![off.clone()]);
        assert_eq!(kept.len(), 1);
        assert!(notifiable(&st.store.settings().unwrap(), &kept[0]));

        // Taşı: İstenmeyen'e; temiz posta kalır.
        set(&st, |s| s.spam_filter = SpamFilter::Move);
        let moved = spam("a", "s3@x", 3);
        let clean = deliver(&st, "a", plain("Ayşe <ayse@eposta.com>", "s4@x"), 4);
        let (kept, touched) = filter_new(&st, &a, vec![moved.clone(), clean.clone()]);
        assert_eq!(kept.iter().map(|m| m.id).collect::<Vec<_>>(), [clean.id]);
        assert_eq!(touched, ["a:INBOX", "a:Junk"]);
        assert_eq!(mailbox_of(&st, moved.id), "a:Junk");

        // Taşı ama İstenmeyen kutusu yok: uyar gibi (yerinde, kartsız).
        let stay = spam("b", "s5@x", 5);
        let (kept, touched) = filter_new(&st, &b, vec![stay.clone()]);
        assert_eq!((kept.len(), touched.len()), (1, 0));
        assert_eq!(mailbox_of(&st, stay.id), "b:INBOX");
        assert!(!notifiable(&st.store.settings().unwrap(), &kept[0]));
    }

    #[tokio::test]
    async fn gonderici_engelleme() {
        let st = state();
        let m1 = deliver(&st, "a", plain("Reklam <Reklam@Firma.com>", "b1@x"), 1);
        let m2 = deliver(&st, "a", plain("Reklam <reklam@firma.com>", "b2@x"), 2);
        let archived = deliver_to(
            &st,
            "a",
            "Archive",
            plain("Reklam <reklam@firma.com>", "b3@x"),
            3,
        );
        let other = deliver(&st, "a", plain("Ayşe <ayse@eposta.com>", "b4@x"), 4);
        let other_account = deliver(&st, "b", plain("Reklam <reklam@firma.com>", "b5@x"), 5);
        // Önceden "Güvenli" denmiş ve elekte bekleyen postası var.
        st.store.trust_sender("a", "reklam@firma.com", 1).unwrap();
        st.store
            .screen_message(m1.id, "a", "reklam@firma.com", 1)
            .unwrap();

        let (result, settings, touched) = block_sender(&st, m1.id).unwrap();
        assert_eq!(result.address, "reklam@firma.com");
        assert_eq!(result.moved, 2);
        assert_eq!(result.ids, [m2.id, m1.id]);
        assert_eq!(settings.blocked, ["reklam@firma.com"]);
        assert_eq!(st.store.settings().unwrap().blocked, ["reklam@firma.com"]);
        assert_eq!(touched, ["a:INBOX", "a:Junk"]);
        for id in [m1.id, m2.id] {
            assert_eq!(mailbox_of(&st, id), "a:Junk");
        }
        // Yalnız bu hesabın gelen kutusu: arşiv, başka gönderici ve başka
        // hesap yerinde.
        assert_eq!(mailbox_of(&st, archived.id), "a:Archive");
        assert_eq!(mailbox_of(&st, other.id), "a:INBOX");
        assert_eq!(mailbox_of(&st, other_account.id), "b:INBOX");
        // Son karar geçerli: kabul geri alındı, elekten çıktı.
        assert!(!st.store.sender_accepted("a", "reklam@firma.com").unwrap());
        assert!(st.store.screened_senders().unwrap().is_empty());

        // İkinci kez: liste yinelenmez, taşınacak posta kalmamıştır.
        let (again, settings, _) = block_sender(&st, m1.id).unwrap();
        assert_eq!((again.moved, again.ids.len()), (0, 0));
        assert_eq!(settings.blocked, ["reklam@firma.com"]);

        // İstenmeyen kutusu olmayan hesapta Çöp'e taşınır.
        let (result, settings, touched) = block_sender(&st, other_account.id).unwrap();
        assert_eq!(result.moved, 1);
        assert_eq!(touched, ["b:INBOX", "b:Trash"]);
        assert_eq!(settings.blocked, ["reklam@firma.com"]);
        assert_eq!(mailbox_of(&st, other_account.id), "b:Trash");
    }

    #[tokio::test]
    async fn engellenemeyen_gondericiler() {
        let st = state();
        // Kendi adresimiz (başka hesabımız da olsa).
        let own = deliver(&st, "a", plain("Furkan <Furkan@Eposta.com>", "k1@x"), 1);
        let e = block_sender(&st, own.id).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("Kendi adresini"));
        // Bozuk adres.
        let broken = deliver(&st, "a", plain("Bozuk <com>", "k2@x"), 2);
        let e = block_sender(&st, broken.id).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("eksik ya da bozuk"));
        assert_eq!(
            trust_sender(&st, broken.id).unwrap_err().code,
            ErrorCode::Invalid
        );
        // Olmayan posta.
        assert_eq!(
            block_sender(&st, 9_999).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(
            trust_sender(&st, 9_999).unwrap_err().code,
            ErrorCode::NotFound
        );
        // Hiçbiri listeye ya da kutulara dokunmadı.
        assert!(st.store.settings().unwrap().blocked.is_empty());
        assert_eq!(mailbox_of(&st, own.id), "a:INBOX");

        // Liste doluyken yeni gönderici eklenmez; ne yapılacağı söylenir.
        set(&st, |s| {
            s.blocked = (0..BLOCKED_MAX).map(|i| format!("a{i}@b.com")).collect();
        });
        let extra = deliver(&st, "a", plain("Yeni <yeni@firma.com>", "k3@x"), 3);
        let e = block_sender(&st, extra.id).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("dolu"));
        assert_eq!(mailbox_of(&st, extra.id), "a:INBOX");
        assert_eq!(st.store.settings().unwrap().blocked.len(), BLOCKED_MAX);
    }

    #[tokio::test]
    async fn guvenli_gonderici() {
        let st = state();
        set(&st, |s| {
            s.blocked = vec![
                "x@kotu.example".into(),
                "@kotu.example".into(),
                "baska@firma.com".into(),
            ];
        });
        let junk = deliver_to(
            &st,
            "a",
            "Junk",
            flagged("Banka <X@Kotu.example>", "g1@x"),
            1,
        );
        let inbox = deliver(&st, "a", flagged("Banka <x@kotu.example>", "g2@x"), 2);
        let other = deliver(&st, "a", flagged("Başka <y@kotu.example>", "g3@x"), 3);
        assert!(junk.spam && inbox.spam);
        assert_eq!(
            st.store.detail(junk.id, &[]).unwrap().spam_verdict,
            Some(SpamVerdict {
                score: 100,
                reasons: vec![SpamReason::Blocked, SpamReason::ServerFlagged],
            })
        );

        let (settings, touched) = trust_sender(&st, junk.id).unwrap();
        // Adres listeden çıkar; alan adı girdisi ve öteki adres kalır.
        assert_eq!(settings.blocked, ["@kotu.example", "baska@firma.com"]);
        assert_eq!(st.store.settings().unwrap().blocked, settings.blocked);
        assert_eq!(touched, ["a:INBOX", "a:Junk"]);
        // Posta gelen kutusuna döner; göndericinin postalarında uyarı kalkar.
        assert_eq!(mailbox_of(&st, junk.id), "a:INBOX");
        for id in [junk.id, inbox.id] {
            let d = st.store.detail(id, &[]).unwrap();
            assert_eq!((d.spam_verdict, d.summary.spam), (None, false), "{id}");
        }
        assert!(st.store.sender_accepted("a", "x@kotu.example").unwrap());
        // Aynı alan adındaki başka gönderici engelli ve şüpheli kalır.
        assert!(st
            .store
            .detail(other.id, &[])
            .unwrap()
            .spam_verdict
            .is_some());
        // Sonraki postası süzgeçten geçer (alan adı engeline rağmen).
        let next = deliver(&st, "a", flagged("Banka <x@kotu.example>", "g4@x"), 4);
        assert!(!next.spam);
        let a = account(&st, "a");
        assert_eq!(filter_new(&st, &a, vec![next.clone()]).0.len(), 1);

        // Gelen kutusundaki postada: taşınmaz, yine de liste yenilenir.
        let (settings, touched) = trust_sender(&st, inbox.id).unwrap();
        assert_eq!(settings.blocked, ["@kotu.example", "baska@firma.com"]);
        assert_eq!(touched, ["a:INBOX"]);
        assert_eq!(mailbox_of(&st, inbox.id), "a:INBOX");
    }
}
