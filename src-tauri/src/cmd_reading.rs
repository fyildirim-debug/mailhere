//! Okuma özellikleri: konuşma görünümü, kişi kartı, .eml kaydetme ve takvim
//! daveti (`thread_get`, `contact_get`, `message_save_eml`, `invite_reply`,
//! `invite_open`).

use std::path::PathBuf;

use mh_core::{
    Address, AppError, ContactInfo, ErrorCode, InviteMethod, InviteResponse, MessageDetail,
    MessageId,
};
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::commands::{ensure_body, private_dir, sanitize_filename, write_private};
use crate::state::{internal, now_ms, AppState};

type Res<T> = Result<T, AppError>;

fn not_found() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Posta bulunamadı; silinmiş ya da taşınmış olabilir.",
    )
}

fn no_body() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Postanın içeriği yerelde yok. Eşitlemenin bitmesini bekleyip yeniden dene.",
    )
}

fn no_invite() -> AppError {
    AppError::new(ErrorCode::NotFound, "Bu postada takvim daveti yok.")
}

/// Postanın konuşmasındaki tüm iletiler (aynı hesap, eskiden yeniye, en çok
/// 100). Açık gelecek iletilerin (okunmamış, en yeni ve seçili) gövdesi
/// gerekirse indirilir; diğerleri yerelde varsa gövdeli, yoksa gövdesiz
/// gelir ve arayüz açılınca `message_get` ile indirir.
#[tauri::command]
pub async fn thread_get(state: State<'_, AppState>, id: MessageId) -> Res<Vec<MessageDetail>> {
    let list = state.store.thread_messages(id, mh_store::THREAD_LIMIT)?;
    if list.is_empty() {
        return Err(not_found());
    }
    let newest = list.last().map(|m| m.id);
    for m in &list {
        if m.unread || Some(m.id) == newest || m.id == id {
            if let Err(e) = ensure_body(&state, m.id).await {
                tracing::warn!(id = m.id, error = %e.message, "konuşmadaki iletinin gövdesi indirilemedi");
            }
        }
    }
    let allow = state.store.settings()?.remote_allow;
    let store = state.store.clone();
    let ids: Vec<MessageId> = list.iter().map(|m| m.id).collect();
    // HTML temizleme ağır olabilir: çalışma iş parçacıklarını tutmasın.
    tokio::task::spawn_blocking(move || {
        let mut out = Vec::with_capacity(ids.len());
        for mid in ids {
            match store.detail(mid, &allow) {
                Ok(d) => out.push(d),
                // Bu arada silinen ileti sessizce düşer.
                Err(e) if e.code == ErrorCode::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    })
    .await
    .map_err(internal)?
}

/// Bir kişiyle yazışmanın özeti (kişi kartı).
#[tauri::command]
pub fn contact_get(state: State<'_, AppState>, email: String) -> Res<ContactInfo> {
    let email = email.trim();
    if !mh_store::is_full_address(email) {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Adres eksik ya da bozuk; kişi bilgisi gösterilemez.",
        ));
    }
    state.store.contact(email, mh_store::CONTACT_RECENT)
}

/// Kaydet penceresinde önerilen dosya adı: temizlenmiş konu + `.eml`.
pub fn eml_name(subject: &str) -> String {
    let subject = subject.trim();
    if subject.is_empty() {
        return "posta.eml".to_owned();
    }
    let base = sanitize_filename(subject);
    if base.to_ascii_lowercase().ends_with(".eml") {
        base
    } else {
        format!("{base}.eml")
    }
}

/// Kullanıcının seçtiği yol `.eml` ile bitmiyorsa uzantı eklenir (dosya
/// başka bir türle açılmasın).
pub fn with_eml_extension(path: PathBuf) -> PathBuf {
    let has = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("eml"));
    if has {
        path
    } else {
        let mut name = path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "posta".into());
        name.push(".eml");
        path.with_file_name(name)
    }
}

/// Ham postayı işletim sisteminin kaydet penceresiyle `.eml` olarak yazar
/// (yalnız kullanıcıya okunur); vazgeçilirse `None`.
#[tauri::command]
pub async fn message_save_eml(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: MessageId,
) -> Res<Option<String>> {
    ensure_body(&state, id).await?;
    let summary = state.store.summary(id)?.ok_or_else(not_found)?;
    let raw = state.store.raw(id)?.ok_or_else(no_body)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_file_name(eml_name(&summary.subject))
        .add_filter("E-posta (.eml)", &["eml"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(picked) = rx.await.map_err(internal)? else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|_| {
        AppError::new(
            ErrorCode::Invalid,
            "Seçilen konum bir dosya yolu değil. Bilgisayarında bir klasör seç.",
        )
    })?;
    let path = with_eml_extension(path);
    let written = tokio::task::spawn_blocking(move || write_private(&path, &raw).map(|_| path))
        .await
        .map_err(internal)?
        .map_err(|e| {
            tracing::warn!(error = %e, ".eml yazılamadı");
            AppError::new(
                ErrorCode::Storage,
                "Dosya yazılamadı. Klasörün yazılabilir olduğunu ve boş alanı denetle.",
            )
        })?;
    Ok(Some(written.to_string_lossy().into_owned()))
}

/// Davetin ham takvimi ve özeti.
fn calendar_of(raw: &[u8]) -> Res<(mh_mime::Calendar, mh_core::Invite)> {
    let text = mh_mime::calendar_text(raw).ok_or_else(no_invite)?;
    let cal = mh_mime::parse_calendar(&text).ok_or_else(no_invite)?;
    let invite = mh_mime::to_invite(&cal).ok_or_else(no_invite)?;
    Ok((cal, invite))
}

/// Davete yanıt: düzenleyene, postayı alan hesaptan `METHOD:REPLY` postası.
/// Yalnız METHOD:REQUEST davetlere; düzenleyen adresi yoksa gönderilmez.
#[tauri::command]
pub async fn invite_reply(
    state: State<'_, AppState>,
    id: MessageId,
    response: InviteResponse,
) -> Res<()> {
    ensure_body(&state, id).await?;
    let summary = state.store.summary(id)?.ok_or_else(not_found)?;
    let raw = state.store.raw(id)?.ok_or_else(no_body)?;
    let (cal, invite) = calendar_of(&raw)?;
    if invite.method != InviteMethod::Request {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Bu etkinlik iptal edildi; yanıt gönderilmez.",
        ));
    }
    let organizer = invite.organizer.clone().ok_or_else(|| {
        AppError::new(
            ErrorCode::Invalid,
            "Davette düzenleyenin adresi yok; yanıt gönderilemez. Gönderene doğrudan yazabilirsin.",
        )
    })?;
    let account = state
        .store
        .account(&summary.account_id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Postayı alan hesap bulunamadı."))?;
    let me = Address::new(Some(&account.display_name), &account.email);
    let ics = mh_mime::build_reply(&cal, &me, response, now_ms()).ok_or_else(no_invite)?;
    let lang = state.store.settings()?.language;
    let msg =
        mh_send::invite_reply_message(&account, &organizer, id, &invite.summary, response, lang);
    state.outbox.send_invite_reply(&msg, &ics).await
}

/// Davetin `.ics` dosyasının adı (temizlenmiş etkinlik adı).
pub fn ics_name(summary: &str) -> String {
    let s = summary.trim();
    if s.is_empty() {
        return "davet.ics".to_owned();
    }
    let base = sanitize_filename(s);
    if base.to_ascii_lowercase().ends_with(".ics") {
        base
    } else {
        format!("{base}.ics")
    }
}

/// Davetin takvim parçasını geçici klasöre yazar ve varsayılan takvim
/// uygulamasıyla açar ("Takvime ekle"); dosya yolunu döner.
#[tauri::command]
pub async fn invite_open(app: AppHandle, state: State<'_, AppState>, id: MessageId) -> Res<String> {
    ensure_body(&state, id).await?;
    let raw = state.store.raw(id)?.ok_or_else(no_body)?;
    let text = mh_mime::calendar_text(&raw).ok_or_else(no_invite)?;
    let summary = mh_mime::parse_calendar(&text)
        .and_then(|c| c.event.map(|e| e.summary))
        .unwrap_or_default();
    let dir: PathBuf = state.cache_dir.join("ekler").join(id.to_string());
    private_dir(&dir).map_err(internal)?;
    let path = dir.join(ics_name(&summary));
    write_private(&path, text.as_bytes()).map_err(|e| {
        tracing::warn!(error = %e, "davet dosyası yazılamadı");
        AppError::new(
            ErrorCode::Storage,
            "Davet dosyası diske yazılamadı. Boş alanı denetle.",
        )
    })?;
    let path_str = path.to_string_lossy().into_owned();
    app.opener()
        .open_path(path_str.clone(), None::<&str>)
        .map_err(|e| {
            tracing::warn!(error = %e, "davet dosyası açılamadı");
            AppError::new(
                ErrorCode::Internal,
                format!("Takvim uygulaması açılamadı. Dosya şurada: {path_str}"),
            )
        })?;
    Ok(path_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eml_ve_ics_adlari() {
        assert_eq!(eml_name("Re: Q4 bütçe taslağı"), "Re_ Q4 bütçe taslağı.eml");
        assert_eq!(eml_name("   "), "posta.eml");
        assert_eq!(eml_name("../../etc/passwd"), "_.._etc_passwd.eml");
        assert_eq!(eml_name("yedek.EML"), "yedek.EML");
        assert_eq!(
            ics_name("Sprint planlama, 4. çeyrek"),
            "Sprint planlama, 4. çeyrek.ics"
        );
        assert_eq!(ics_name(""), "davet.ics");
        assert_eq!(ics_name("fatura\u{202E}sci.exe"), "faturasci.exe.ics");
        assert!(eml_name(&"a".repeat(300)).chars().count() <= 124);
    }

    #[test]
    fn eml_uzantisi_eklenir() {
        assert_eq!(
            with_eml_extension(PathBuf::from("/ev/furkan/posta")),
            PathBuf::from("/ev/furkan/posta.eml")
        );
        assert_eq!(
            with_eml_extension(PathBuf::from("/ev/furkan/posta.txt")),
            PathBuf::from("/ev/furkan/posta.txt.eml")
        );
        assert_eq!(
            with_eml_extension(PathBuf::from("/ev/furkan/Posta.EML")),
            PathBuf::from("/ev/furkan/Posta.EML")
        );
    }
}
