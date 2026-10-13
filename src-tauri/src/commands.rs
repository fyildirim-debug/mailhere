//! Arayüzün çağırdığı komutlar (`app/src/lib/api/types.ts` → `Commands`).

use std::path::{Path, PathBuf};

use mh_core::{
    events, Account, AccountKind, AppError, AppResult, Category, ComposePrefill, Counts,
    DiscoveredConfig, ErrorCode, ListQuery, Mailbox, MessageDetail, MessageId, MessageSummary,
    Millis, MoveTarget, NewAccount, NotifyEvent, OutboxItem, OutgoingMessage, Page, Settings,
    SyncState, SyncStatus,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Yeni hesaplara sırayla verilen renkler.
pub(crate) const PALETTE: [&str; 6] = [
    "#4C6FD8", "#2E9A61", "#D08A1E", "#B4508C", "#2A8C9A", "#7A5BC8",
];

fn win(e: tauri::Error) -> AppError {
    internal(e)
}

// ------------------------------------------------------------------ hesaplar

#[tauri::command]
pub fn accounts_list(state: State<'_, AppState>) -> Res<Vec<Account>> {
    state.store.accounts()
}

#[tauri::command]
pub async fn account_discover(email: String) -> Res<DiscoveredConfig> {
    mh_sync::discover(&email).await
}

fn validate_server(cfg: &mh_core::ServerConfig, what: &str) -> AppResult<()> {
    if cfg.host.trim().is_empty() || cfg.port == 0 || cfg.username.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::Invalid,
            format!("{what} sunucu adı, kapı ve kullanıcı adı boş olamaz."),
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn account_add(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewAccount,
) -> Res<Account> {
    let (email, _) = mh_sync::discover::split_email(&input.email)?;
    validate_server(&input.imap, "Gelen posta")?;
    validate_server(&input.smtp, "Giden posta")?;
    if input.password.is_empty() {
        return Err(AppError::new(ErrorCode::Invalid, "Parola boş olamaz."));
    }
    mh_sync::test_login(&input.imap, &input.password).await?;

    let existing = state.store.accounts()?;
    let id = uuid::Uuid::new_v4().to_string();
    let secrets = state.secrets.clone();
    let (sid, password) = (id.clone(), input.password);
    tokio::task::spawn_blocking(move || secrets.set(&sid, &password))
        .await
        .map_err(internal)??;

    let name = match input.name.trim() {
        "" => email.split('@').nth(1).unwrap_or(&email).to_owned(),
        n => n.to_owned(),
    };
    let account = Account {
        id,
        name,
        display_name: match input.display_name.trim() {
            "" => email.split('@').next().unwrap_or("").to_owned(),
            d => d.to_owned(),
        },
        email,
        color: input
            .color
            .filter(|c| c.len() == 7 && c.starts_with('#'))
            .unwrap_or_else(|| PALETTE[existing.len() % PALETTE.len()].to_owned()),
        kind: AccountKind::Imap,
        imap: Some(input.imap),
        smtp: Some(input.smtp),
        jmap_url: None,
        signature: None,
        created_at: now_ms(),
        auth: Default::default(),
    };
    state.store.insert_account(&account)?;
    state.sync.start(account.clone());
    crate::sink::changed(&app, Vec::new());
    Ok(account)
}

#[tauri::command]
pub async fn account_remove(app: AppHandle, state: State<'_, AppState>, id: String) -> Res<()> {
    state.sync.stop(&id);
    state.sync.tokens().forget(&id);
    let removed = state.store.remove_account(&id)?;
    let secrets = state.secrets.clone();
    let sid = id.clone();
    // Parola ve (OAuth hesabıysa) yenileme jetonu.
    let delete = move || {
        let password = secrets.delete(&sid);
        let oauth = secrets.delete(&mh_sync::oauth_secret_key(&sid));
        password.and(oauth)
    };
    match tokio::task::spawn_blocking(delete).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!(error = %e.message, "hesabın parolası anahtar zincirinden silinemedi")
        }
        Err(e) => tracing::warn!(error = %e, "parola silme görevi başarısız"),
    }
    if !removed {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "Hesap bulunamadı; zaten kaldırılmış olabilir.",
        ));
    }
    crate::sink::changed(&app, Vec::new());
    Ok(())
}

// ------------------------------------------------------------------ kutular

#[tauri::command]
pub fn mailboxes_list(state: State<'_, AppState>) -> Res<Vec<Mailbox>> {
    state.store.mailboxes()
}

#[tauri::command]
pub fn counts(state: State<'_, AppState>) -> Res<Counts> {
    state.store.counts(now_ms())
}

// ------------------------------------------------------------------ postalar

#[tauri::command]
pub fn messages_list(state: State<'_, AppState>, query: ListQuery) -> Res<Page<MessageSummary>> {
    state.store.list(&query, now_ms())
}

/// Gövde yerelde yoksa sunucudan indirir.
pub(crate) async fn ensure_body(state: &AppState, id: MessageId) -> AppResult<()> {
    let loc = state.store.location(id)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Posta bulunamadı; silinmiş ya da taşınmış olabilir.",
        )
    })?;
    if loc.has_raw {
        return Ok(());
    }
    state.sync.fetch_body(id).await
}

/// Postayı okuma alanı için döner. Okundu işaretlemez; arayüz ayrıca
/// `messages_flag` çağırır.
#[tauri::command]
pub async fn message_get(state: State<'_, AppState>, id: MessageId) -> Res<MessageDetail> {
    ensure_body(&state, id).await?;
    let allow = state.store.settings()?.remote_allow;
    state.store.detail(id, &allow)
}

#[tauri::command]
pub fn messages_flag(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<MessageId>,
    unread: Option<bool>,
    starred: Option<bool>,
) -> Res<()> {
    let touched = state.sync.set_flags(&ids, unread, starred)?;
    crate::sink::changed(&app, touched);
    Ok(())
}

#[tauri::command]
pub fn messages_move(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<MessageId>,
    target: MoveTarget,
) -> Res<()> {
    let touched = state.sync.move_messages(&ids, target)?;
    crate::sink::changed(&app, touched);
    Ok(())
}

#[tauri::command]
pub fn messages_snooze(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<MessageId>,
    until: Option<Millis>,
) -> Res<()> {
    let touched = state.store.snooze(&ids, until)?;
    crate::sink::changed(&app, touched);
    Ok(())
}

#[tauri::command]
pub fn search(state: State<'_, AppState>, q: String, limit: u32) -> Res<Vec<MessageSummary>> {
    state.store.search(&q, limit)
}

/// Göndericinin uzak görsellerine izin verir (ayara kalıcı yazılır) ve
/// postayı görsellerle döner.
#[tauri::command]
pub fn remote_allow(
    app: AppHandle,
    state: State<'_, AppState>,
    id: MessageId,
) -> Res<MessageDetail> {
    let summary = state
        .store
        .summary(id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Posta bulunamadı."))?;
    let email = summary.from.email.trim().to_ascii_lowercase();
    let mut settings = state.store.settings()?;
    if !email.is_empty()
        && !settings
            .remote_allow
            .iter()
            .any(|e| e.eq_ignore_ascii_case(&email))
    {
        settings.remote_allow.push(email);
        state.store.set_settings(&settings)?;
        let _ = app.emit(events::SETTINGS, &settings);
    }
    state.store.detail(id, &settings.remote_allow)
}

/// Dosya adını tüm işletim sistemlerinde güvenli hale getirir.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let mut out: String = trimmed.chars().take(120).collect();
    if out.is_empty() {
        out = "ek".into();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && stem.as_bytes()[3].is_ascii_digit();
    if RESERVED.contains(&stem.as_str()) || numbered {
        out.insert(0, '_');
    }
    out
}

/// Çalıştırılabilir türler doğrudan açılmaz; klasörde gösterilir.
pub fn is_risky(name: &str) -> bool {
    const RISKY: &[&str] = &[
        "exe", "com", "bat", "cmd", "scr", "pif", "msi", "msix", "msp", "ps1", "psm1", "vbs",
        "vbe", "js", "jse", "wsf", "wsh", "hta", "cpl", "msc", "jar", "lnk", "reg", "inf", "dll",
        "sys", "appx", "sh", "bash", "run", "bin", "desktop", "appimage", "deb", "rpm", "py", "pl",
        "iso", "img", "vhd", "vhdx",
    ];
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| RISKY.contains(&e.to_ascii_lowercase().as_str()))
}

#[tauri::command]
pub async fn attachment_open(
    app: AppHandle,
    state: State<'_, AppState>,
    id: MessageId,
    index: u32,
) -> Res<String> {
    ensure_body(&state, id).await?;
    let raw = state
        .store
        .raw(id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Postanın içeriği yerelde yok."))?;
    let att = mh_mime::attachment(&raw, index).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Ek bulunamadı; posta değişmiş olabilir.",
        )
    })?;
    let dir: PathBuf = state.cache_dir.join("ekler").join(id.to_string());
    std::fs::create_dir_all(&dir).map_err(internal)?;
    let name = sanitize_filename(&att.name);
    let path = dir.join(&name);
    std::fs::write(&path, &att.bytes).map_err(|e| {
        tracing::warn!(error = %e, "ek yazılamadı");
        AppError::new(
            ErrorCode::Storage,
            "Ek diske yazılamadı. Boş alanı denetle.",
        )
    })?;
    let path_str = path.to_string_lossy().into_owned();
    let opener = app.opener();
    let res = if is_risky(&name) {
        opener.reveal_item_in_dir(&path)
    } else {
        opener.open_path(path_str.clone(), None::<&str>)
    };
    res.map_err(|e| {
        tracing::warn!(error = %e, "ek açılamadı");
        AppError::new(
            ErrorCode::Internal,
            format!("Ek açılamadı. Dosya şurada: {path_str}"),
        )
    })?;
    Ok(path_str)
}

// ------------------------------------------------------------------ gönderim

/// Postayı giden kutusuna alır. İletilen postanın (`forward_from`) ekleri
/// kopyalanacağı için gövdesi önce indirilir; `draft_id` taslağı başarılı
/// gönderimden sonra silinir.
#[tauri::command]
pub async fn send(state: State<'_, AppState>, msg: OutgoingMessage) -> Res<OutboxItem> {
    if let Some(id) = msg.forward_from {
        ensure_body(&state, id).await?;
    }
    state.outbox.enqueue(&msg, now_ms())
}

#[tauri::command]
pub fn send_cancel(state: State<'_, AppState>, id: String) -> Res<bool> {
    state.outbox.cancel(&id)
}

// -------------------------------------------------------------------- ayarlar

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Res<Settings> {
    state.store.settings()
}

/// Değerleri makul sınırlara çeker.
pub fn normalize_settings(mut s: Settings) -> Settings {
    s.card_seconds = s.card_seconds.clamp(3, 60);
    s.undo_send_seconds = s.undo_send_seconds.min(60);
    let valid = |t: &Option<String>| t.as_deref().and_then(mh_notify::parse_hhmm).is_some();
    if !valid(&s.quiet_start) || !valid(&s.quiet_end) {
        s.quiet_start = None;
        s.quiet_end = None;
    }
    let clean = |v: Vec<String>| {
        let mut out: Vec<String> = v
            .into_iter()
            .map(|e| e.trim().to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        out.sort();
        out.dedup();
        out
    };
    s.vip = clean(s.vip);
    s.remote_allow = clean(s.remote_allow);
    s
}

#[tauri::command]
pub fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Res<Settings> {
    let settings = normalize_settings(settings);
    state.store.set_settings(&settings)?;
    if let Err(e) = app.emit(events::SETTINGS, &settings) {
        tracing::warn!(error = %e, "ayar olayı gönderilemedi");
    }
    Ok(settings)
}

#[tauri::command]
pub fn sync_now(app: AppHandle, state: State<'_, AppState>) -> Res<()> {
    state.sync.sync_now();
    // Yerel (tanıtım) hesaplar için hemen "çevrimiçi" bildir.
    for a in state.store.accounts()? {
        if !mh_sync::is_syncable(&a) {
            let _ = app.emit(
                events::SYNC,
                SyncStatus {
                    account_id: a.id,
                    state: SyncState::Online,
                },
            );
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ pencereler
//
// Pencere açan komutlar `async`tır: eşzamanlı komutta pencere oluşturmak
// Windows'ta (WebView2) kilitlenmeye yol açar.

#[tauri::command]
pub async fn open_main(app: AppHandle, id: Option<MessageId>) -> Res<()> {
    crate::windows::open_main(&app, id);
    Ok(())
}

#[tauri::command]
pub async fn open_quick(app: AppHandle, id: MessageId) -> Res<()> {
    crate::windows::open_quick(&app, id).map_err(win)
}

#[tauri::command]
pub async fn open_compose(app: AppHandle, prefill: ComposePrefill) -> Res<()> {
    crate::windows::open_compose(&app, &prefill).map_err(win)
}

#[tauri::command]
pub async fn notify_layout(app: AppHandle, height: f64) -> Res<()> {
    crate::windows::notify_layout(&app, height).map_err(win)
}

#[tauri::command]
pub async fn notify_hide(app: AppHandle) -> Res<()> {
    crate::windows::notify_hide(&app).map_err(win)
}

#[tauri::command]
pub fn notify_pending(app: AppHandle) -> Res<Vec<NotifyEvent>> {
    Ok(crate::notify::take_pending(&app))
}

#[tauri::command]
pub async fn notify_snooze(app: AppHandle, state: State<'_, AppState>, minutes: u32) -> Res<()> {
    state.mute.mute_for(now_ms(), minutes);
    crate::windows::notify_hide(&app).map_err(win)
}

/// Ekran görüntüsü için: "Cumartesi kahvaltı?" postasıyla bir kart açar.
#[tauri::command]
pub async fn demo_notify(app: AppHandle) -> Res<()> {
    show_demo_card(&app)
}

pub fn show_demo_card(app: &AppHandle) -> AppResult<()> {
    const SUBJECT: &str = "Cumartesi kahvaltı?";
    let state = app.state::<AppState>();
    let message = match state.store.find_by_subject(SUBJECT)? {
        Some(m) => m,
        None => MessageSummary {
            id: 0,
            account_id: String::new(),
            mailbox_id: String::new(),
            thread_id: String::new(),
            from: mh_core::Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com"),
            to: vec![],
            subject: SUBJECT.into(),
            snippet: "Moda'daki yeni yere gidelim mi? Saat 10 gibi buluşursak kalabalığa kalmayız."
                .into(),
            date: now_ms(),
            unread: true,
            starred: false,
            has_attachments: false,
            category: Category::People,
            snoozed_until: None,
        },
    };
    let account = state.store.account(&message.account_id)?;
    let ev = NotifyEvent {
        account_name: account
            .as_ref()
            .map_or_else(|| "Kişisel".into(), |a| a.name.clone()),
        account_color: account.map_or_else(|| "#2E9A61".into(), |a| a.color),
        message,
    };
    crate::notify::show_card(app, ev, false);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dosya_adi_temizlenir() {
        assert_eq!(sanitize_filename("Sözleşme v3.pdf"), "Sözleşme v3.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize_filename("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize_filename("  ...  "), "ek");
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename("com1"), "_com1");
        assert_eq!(sanitize_filename("COMPUTER.txt"), "COMPUTER.txt");
        assert_eq!(sanitize_filename(&"x".repeat(300)).chars().count(), 120);
    }

    #[test]
    fn riskli_turler() {
        assert!(is_risky("fatura.PDF.exe"));
        assert!(is_risky("kur.sh"));
        assert!(!is_risky("fatura.pdf"));
        assert!(!is_risky("README"));
    }

    #[test]
    fn ayarlar_sinirlanir() {
        let s = normalize_settings(Settings {
            card_seconds: 0,
            undo_send_seconds: 600,
            quiet_start: Some("25:00".into()),
            vip: vec![
                " Ayse@Eposta.com".into(),
                "ayse@eposta.com".into(),
                " ".into(),
            ],
            ..Settings::default()
        });
        assert_eq!(s.card_seconds, 3);
        assert_eq!(s.undo_send_seconds, 60);
        assert_eq!((s.quiet_start, s.quiet_end), (None, None));
        assert_eq!(s.vip, vec!["ayse@eposta.com".to_string()]);
        let d = normalize_settings(Settings::default());
        assert_eq!(d, Settings::default());
    }
}
