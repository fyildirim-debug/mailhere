//! Arayüzün çağırdığı komutlar (`app/src/lib/api/types.ts` → `Commands`).

use std::path::{Path, PathBuf};

use mh_core::{
    events, Account, AccountKind, AppError, AppResult, ComposePrefill, Counts, DiscoveredConfig,
    ErrorCode, ListQuery, Mailbox, MessageDetail, MessageId, MessageSummary, Millis, MoveTarget,
    NewAccount, OutboxItem, OutgoingMessage, Page, Settings, SyncState, SyncStatus,
};
use tauri::{AppHandle, Emitter, State};

use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

/// Yeni hesaplara sırayla verilen renkler.
pub(crate) const PALETTE: [&str; 6] = [
    "#4C6FD8", "#2E9A61", "#D08A1E", "#B4508C", "#2A8C9A", "#7A5BC8",
];

// ------------------------------------------------------------------ hesaplar

#[tauri::command]
pub fn accounts_list(state: State<'_, AppState>) -> Res<Vec<Account>> {
    state.store.accounts()
}

#[tauri::command]
pub async fn account_discover(email: String) -> Res<DiscoveredConfig> {
    mh_sync::discover(&email).await
}

pub(crate) fn validate_server(cfg: &mh_core::ServerConfig, what: &str) -> AppResult<()> {
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
        signatures: Vec::new(),
        signature_new: None,
        signature_reply: None,
        created_at: now_ms(),
        auth: Default::default(),
        notify: true,
    };
    state.store.insert_account(&account)?;
    state.sync.start(account.clone());
    crate::sink::changed(&app, Vec::new());
    Ok(account)
}

#[tauri::command]
pub async fn account_remove(app: AppHandle, state: State<'_, AppState>, id: String) -> Res<()> {
    state.sync.stop(&id);
    state.jmap.stop(&id);
    state.sync.tokens().forget(&id);
    let removed = state.store.remove_account(&id)?;
    // Açılmış eklerin kopyaları da gider (gerekirse yeniden yazılır).
    let _ = std::fs::remove_dir_all(state.cache_dir.join("ekler"));
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
    if crate::cmd_jmap::is_jmap_account(state, &loc.account_id)? {
        return state.jmap.fetch_body(id).await;
    }
    state.sync.fetch_body(id).await
}

/// Postayı okuma alanı için döner. Okundu işaretlemez; arayüz ayrıca
/// `messages_flag` çağırır.
#[tauri::command]
pub async fn message_get(state: State<'_, AppState>, id: MessageId) -> Res<MessageDetail> {
    ensure_body(&state, id).await?;
    let allow = state.store.settings()?.remote_allow;
    // HTML temizleme ağır olabilir (kötü niyetli posta): çalışma iş
    // parçacıklarını tutmasın.
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || store.detail(id, &allow))
        .await
        .map_err(internal)?
}

#[tauri::command]
pub fn messages_flag(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<MessageId>,
    unread: Option<bool>,
    starred: Option<bool>,
) -> Res<()> {
    let touched = crate::cmd_jmap::set_flags(&state, &ids, unread, starred)?;
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
    let touched = crate::cmd_jmap::move_messages(&state, &ids, target)?;
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
    // Yalnız tam adres yazılır: `From: <com>` "com" olarak çözülür ve alan adı
    // sanılırsa tüm .com göndericilerine izin verirdi.
    if !mh_store::is_full_address(&email) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Gönderenin adresi eksik ya da bozuk; bu postada uzak görsellere izin verilemez.",
        ));
    }
    let mut settings = state.store.settings()?;
    if !settings
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
    const MAX: usize = 120;
    let cleaned: String = name
        .chars()
        .filter(|c| !is_invisible(*c))
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = trim_edges(&cleaned);
    // Uzantı korunur, kısaltma gövdeden yapılır: "….exe .pdf" gibi bir ad
    // kesilince ".exe " ile bitip çalıştırılabilir hale gelemez.
    let (stem, ext) = match trimmed.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 16
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            (stem, Some(ext))
        }
        _ => (trimmed, None),
    };
    let room = MAX - ext.map_or(0, |e| e.len() + 1);
    let stem: String = stem.chars().take(room).collect();
    let stem = trim_edges(&stem);
    let mut out = match (stem.is_empty(), ext) {
        (true, Some(ext)) => format!("ek.{ext}"),
        (true, None) => "ek".to_owned(),
        (false, Some(ext)) => format!("{stem}.{ext}"),
        (false, None) => stem.to_owned(),
    };
    let first = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL"];
    let numbered = (first.starts_with("COM") || first.starts_with("LPT"))
        && first.len() == 4
        && first.as_bytes()[3].is_ascii_digit();
    if RESERVED.contains(&first.as_str()) || numbered {
        out.insert(0, '_');
    }
    out
}

/// Baştaki ve sondaki boşluk ve noktalar atılır (Windows sondakileri
/// sessizce siler; ad başka bir türe dönüşmesin).
fn trim_edges(s: &str) -> &str {
    s.trim_matches(|c: char| c == '.' || c.is_whitespace())
}

/// Görünmez biçim karakterleri: yön değiştirenler (`fatura\u{202E}fdp.exe`
/// "fatura.pdf" gibi görünür), sıfır genişlikli boşluklar, BOM.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// Doğrudan (varsayılan uygulamasıyla) açılabilen ek türleri. İzin listesi:
/// burada olmayan her tür (çalıştırılabilir, betik, kısayol, makrolu ya da
/// eski Office biçimi, HTML/SVG, arşiv, disk görüntüsü...) yalnız klasöründe
/// gösterilir; kullanıcı ne yaptığını bilerek açar.
pub fn opens_directly(name: &str) -> bool {
    const SAFE: &[&str] = &[
        // belgeler (makrosuz Office Open XML ve OpenDocument)
        "pdf", "txt", "csv", "log", "docx", "xlsx", "pptx", "odt", "ods", "odp",
        // görseller
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "avif",
        // ses ve görüntü
        "mp3", "m4a", "wav", "ogg", "oga", "flac", "mp4", "m4v", "webm", "mov", "ogv",
        // takvim ve kişi
        "ics", "vcf",
    ];
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|e| SAFE.contains(&e.as_str()))
}

/// Windows: dosyayı "İnternet'ten geldi" olarak işaretler (Zone.Identifier
/// akışı, ZoneId=3). Böylece Office korumalı görünümde açar, SmartScreen ve
/// Mark-of-the-Web denetimleri çalışır. Başarısızlık açmayı engellemez.
#[cfg(windows)]
fn mark_from_internet(path: &Path) {
    let mut ads = path.as_os_str().to_owned();
    ads.push(":Zone.Identifier");
    if let Err(e) = std::fs::write(&ads, b"[ZoneTransfer]\r\nZoneId=3\r\n") {
        tracing::debug!(error = %e, "Zone.Identifier yazılamadı");
    }
}

#[cfg(not(windows))]
fn mark_from_internet(_path: &Path) {}

/// Açılan eklerin önbellekte kalma süresi.
const ATTACHMENT_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Yalnız kullanıcının erişebildiği klasör (Unix'te 0700; Windows'ta kullanıcı
/// profilinin varsayılan izinleri).
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Dosyayı yalnız kullanıcının okuyabileceği biçimde yazar (Unix'te 0600).
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        opts.mode(0o600);
        // Önceden var olan dosyanın izinleri de daraltılır.
        if path.exists() {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    opts.open(path)?.write_all(bytes)
}

/// Süresi geçmiş açılmış ekleri siler (en iyi çaba).
fn prune_attachments(root: &Path, ttl: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > ttl);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
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
    let root = state.cache_dir.join("ekler");
    prune_attachments(&root, ATTACHMENT_TTL);
    let dir: PathBuf = root.join(id.to_string());
    private_dir(&dir).map_err(internal)?;
    let name = sanitize_filename(&att.name);
    let path = dir.join(&name);
    write_private(&path, &att.bytes).map_err(|e| {
        tracing::warn!(error = %e, "ek yazılamadı");
        AppError::new(
            ErrorCode::Storage,
            "Ek diske yazılamadı. Boş alanı denetle.",
        )
    })?;
    mark_from_internet(&path);
    let path_str = path.to_string_lossy().into_owned();
    state
        .platform
        .open_file(&app, &path, &att.mime, opens_directly(&name))
        .map_err(|e| {
            tracing::warn!(error = %e.message, "ek açılamadı");
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
pub async fn send(
    state: State<'_, AppState>,
    picked: State<'_, crate::files::Picked>,
    msg: OutgoingMessage,
) -> Res<OutboxItem> {
    picked.ensure(&msg.attachments)?;
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
    // Tek parçalı alan adı ("com") gibi her şeye izin veren öğeler atılır.
    s.remote_allow = clean(s.remote_allow)
        .into_iter()
        .filter(|e| mh_store::is_remote_entry(e))
        .collect();
    s.trusted_certs = crate::cmd_cert::normalize_trusted(s.trusted_certs);
    // Boş liste geçerlidir (kullanıcı tümünü sildi); `None` dile göre varsayılan.
    s.quick_replies = s.quick_replies.map(clean_quick_replies);
    s.templates = crate::signatures::normalize_templates(s.templates);
    s.saved_searches = crate::cmd_list::normalize_saved_searches(s.saved_searches);
    s
}

/// Hızlı yanıtları sadeleştirir: her biri tek satır (satır sonu boşluğa,
/// denetim ve yön karakterleri atılır, boşluklar teke iner), en çok
/// `QUICK_REPLY_CHARS` karakter; boşlar ve büyük/küçük harf farkıyla
/// yinelenenler atılır (ilk gelen kalır, sıra korunur); en çok
/// `QUICK_REPLIES_MAX` öğe.
pub fn clean_quick_replies(list: Vec<String>) -> Vec<String> {
    // Türkçe I/ı/İ/i aynı sayılır ("İyi" ile "iyi" yinelenen).
    let key = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| *c != '\u{0307}')
            .map(|c| if c == 'ı' { 'i' } else { c })
            .collect()
    };
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for reply in list {
        let line = mh_mime::clean_line(&reply);
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let cut: String = line.chars().take(mh_core::QUICK_REPLY_CHARS).collect();
        let cut = cut.trim_end();
        if cut.is_empty() || !seen.insert(key(cut)) {
            continue;
        }
        out.push(cut.to_owned());
        if out.len() == mh_core::QUICK_REPLIES_MAX {
            break;
        }
    }
    out
}

/// İstenen onay listesinden yalnız zaten onaylı olanları bırakır.
pub fn keep_known_certs(
    wanted: Vec<mh_core::TrustedCert>,
    known: &[mh_core::TrustedCert],
) -> Vec<mh_core::TrustedCert> {
    wanted.into_iter().filter(|c| known.contains(c)).collect()
}

#[tauri::command]
pub fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Res<Settings> {
    let current = state.store.settings()?;
    let mut settings = normalize_settings(settings);
    // Onaylı sertifika buradan yalnız kaldırılabilir; eklemek `cert_trust`
    // (inceleme + işletim sistemi onayı) ile olur.
    settings.trusted_certs = keep_known_certs(settings.trusted_certs, &current.trusted_certs);
    let trust_changed = current.trusted_certs != settings.trusted_certs;
    state.store.set_settings(&settings)?;
    if trust_changed {
        // Onay beklerken duran hesaplar yeni listeyle yeniden denenir.
        mh_sync::tls::set_trusted(&settings.trusted_certs);
        state.sync.sync_now();
    }
    if let Err(e) = app.emit(events::SETTINGS, &settings) {
        tracing::warn!(error = %e, "ayar olayı gönderilemedi");
    }
    Ok(settings)
}

/// Yalnız hızlı yanıtları değiştirir (hızlı okuma penceresi `settings_set`e
/// erişemez). `None` dile göre varsayılana döner, boş liste hiç düğme
/// göstermez. Diğer ayarlara dokunulmaz; tüm pencerelere `mh:settings` gider.
#[tauri::command]
pub fn quick_replies_set(
    app: AppHandle,
    state: State<'_, AppState>,
    replies: Option<Vec<String>>,
) -> Res<Settings> {
    let mut settings = state.store.settings()?;
    settings.quick_replies = replies.map(clean_quick_replies);
    state.store.set_settings(&settings)?;
    if let Err(e) = app.emit(events::SETTINGS, &settings) {
        tracing::warn!(error = %e, "ayar olayı gönderilemedi");
    }
    Ok(settings)
}

#[tauri::command]
pub fn sync_now(app: AppHandle, state: State<'_, AppState>) -> Res<()> {
    state.sync.sync_now();
    state.jmap.sync_now();
    // Yerel (tanıtım) hesaplar için hemen "çevrimiçi" bildir.
    for a in state.store.accounts()? {
        if !mh_sync::is_syncable(&a) && !mh_jmap::is_syncable(&a) {
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
// Windows'ta (WebView2) kilitlenmeye yol açar. Pencerenin kendisi kabuğundur
// (`Platform`); hızlı okuma ve bildirim kartı komutları yalnız masaüstündedir.

#[tauri::command]
pub async fn open_main(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<MessageId>,
) -> Res<()> {
    state.platform.open_main(&app, id);
    Ok(())
}

#[tauri::command]
pub async fn open_compose(
    app: AppHandle,
    state: State<'_, AppState>,
    prefill: ComposePrefill,
) -> Res<()> {
    state.platform.open_compose(&app, &prefill)
}

#[tauri::command]
pub async fn notify_snooze(app: AppHandle, state: State<'_, AppState>, minutes: u32) -> Res<()> {
    state.mute.mute_for(now_ms(), minutes);
    state.platform.muted(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ayarlar_onayli_sertifika_ekleyemez_yalniz_kaldirir() {
        let c = |h: &str| mh_core::TrustedCert {
            host: h.into(),
            port: 993,
            sha256: "AA".into(),
        };
        let known = vec![c("a.ornek"), c("b.ornek")];
        assert_eq!(
            keep_known_certs(vec![c("a.ornek"), c("imap.gmail.com")], &known),
            vec![c("a.ornek")]
        );
        assert!(keep_known_certs(vec![], &known).is_empty());
    }

    #[test]
    fn dosya_adi_temizlenir() {
        assert_eq!(sanitize_filename("Sözleşme v3.pdf"), "Sözleşme v3.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize_filename("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize_filename("  ...  "), "ek");
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename("com1"), "_com1");
        assert_eq!(sanitize_filename("COMPUTER.txt"), "COMPUTER.txt");
        // Kısaltma uzantıyı korur; sonda boşluk/nokta kalmaz.
        let tricky = format!("Fatura_2026_{}.exe .pdf", "A".repeat(103));
        let out = sanitize_filename(&tricky);
        assert!(out.ends_with(".pdf"), "{out}");
        assert!(out.chars().count() <= 120);
        assert!(!opens_directly(&sanitize_filename(&format!(
            "{}.exe .",
            "B".repeat(130)
        ))));
        let dotted = sanitize_filename(&format!("{}.exe . . ", "C".repeat(200)));
        assert!(!dotted.ends_with(' ') && !dotted.ends_with('.'), "{dotted}");
        // Yön değiştiren karakter atılır: gerçek uzantı görünür.
        assert_eq!(sanitize_filename("fatura\u{202E}fdp.exe"), "faturafdp.exe");
        assert!(!opens_directly(&sanitize_filename("fatura\u{202E}fdp.exe")));
        assert_eq!(sanitize_filename(&"x".repeat(300)).chars().count(), 120);
    }

    #[cfg(unix)]
    #[test]
    fn ekler_yalniz_kullaniciya_acik_yazilir_ve_eskiler_silinir() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("mh-ek-{}", std::process::id()));
        let dir = root.join("42");
        private_dir(&dir).unwrap();
        let file = dir.join("fatura.pdf");
        write_private(&file, b"%PDF").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&file), 0o600);
        prune_attachments(&root, std::time::Duration::from_secs(3600));
        assert!(file.exists(), "yeni ek kalır");
        prune_attachments(&root, std::time::Duration::ZERO);
        assert!(!dir.exists(), "süresi geçen ek silinir");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn yalniz_guvenli_turler_dogrudan_acilir() {
        for n in [
            "fatura.PDF",
            "teklif.docx",
            "tablo.xlsx",
            "foto.JPG",
            "not.txt",
            "toplanti.ics",
        ] {
            assert!(opens_directly(n), "{n}");
        }
        for n in [
            "fatura.PDF.exe",
            "kur.sh",
            "kisayol.url",
            "yardim.chm",
            "uygulama.appref-ms",
            "eklenti.xll",
            "teklif.docm",
            "tablo.XLSM",
            "eski.xls",
            "eski.doc",
            "zengin.rtf",
            "betik.pyw",
            "paket.pyz",
            "sayfa.html",
            "sayfa.mht",
            "cizim.svg",
            "arama.search-ms",
            "uzak.rdp",
            "arsiv.zip",
            "disk.iso",
            "README",
        ] {
            assert!(!opens_directly(n), "{n}");
        }
    }

    #[test]
    fn uzak_icerik_listesinde_tek_parcali_alan_adi_olmaz() {
        let s = normalize_settings(Settings {
            remote_allow: vec![
                "com".into(),
                "@com".into(),
                " Ayse@Eposta.com ".into(),
                "bulten.co".into(),
                "x@com".into(),
            ],
            ..Settings::default()
        });
        assert_eq!(
            s.remote_allow,
            vec!["ayse@eposta.com".to_string(), "bulten.co".into()]
        );
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

    #[test]
    fn hizli_yanitlar_sadelestirilir() {
        let s = normalize_settings(Settings {
            quick_replies: Some(vec![
                "  Teşekkürler!  ".into(),
                "Tamam,\r\nyarın\tkonuşalım".into(),
                "teşekkürler!".into(),
                " \n ".into(),
                "İyi akşamlar".into(),
                "iyi AKŞAMLAR".into(),
                "fatura\u{202E}fdp\u{0007}".into(),
                "x".repeat(200),
            ]),
            ..Settings::default()
        });
        let got = s.quick_replies.unwrap();
        assert_eq!(
            got[..4],
            [
                "Teşekkürler!".to_string(),
                "Tamam, yarın konuşalım".into(),
                "İyi akşamlar".into(),
                "faturafdp".into(),
            ]
        );
        assert_eq!(got.len(), 5);
        assert_eq!(got[4].chars().count(), mh_core::QUICK_REPLY_CHARS);

        // Karakter (bayt değil) sınırı ve öğe sınırı.
        let long: String = "ğ".repeat(500);
        let out = clean_quick_replies(vec![long]);
        assert_eq!(out[0].chars().count(), mh_core::QUICK_REPLY_CHARS);
        let many: Vec<String> = (0..20).map(|i| format!("Yanıt {i}")).collect();
        let out = clean_quick_replies(many);
        assert_eq!(out.len(), mh_core::QUICK_REPLIES_MAX);
        assert_eq!(out[0], "Yanıt 0");
        assert_eq!(out[7], "Yanıt 7");

        // Boş liste geçerli ve korunur; yokluk varsayılandır.
        let s = normalize_settings(Settings {
            quick_replies: Some(vec![" ".into()]),
            ..Settings::default()
        });
        assert_eq!(s.quick_replies, Some(Vec::new()));
        assert_eq!(normalize_settings(Settings::default()).quick_replies, None);
    }
}
