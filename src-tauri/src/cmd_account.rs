//! Hesap düzenleme: `account_update`, `account_update_server`,
//! `account_set_password`.
//!
//! Parola yalnız bağlantı denemesine ve anahtar zincirine gider; günlüğe, hata
//! mesajına ya da veritabanına yazılmaz. Sunucu adresi, kapısı ya da
//! kullanıcı adı değişince kayıtlı parola yeni sunucuya kendiliğinden
//! gönderilmez: kullanıcı yeniden yazar.

use mh_core::{
    Account, AccountKind, AccountUpdate, AppError, AppResult, AuthKind, ErrorCode, ServerConfig,
    ServerUpdate,
};
use tauri::{AppHandle, State};

use crate::state::{internal, AppState};

type Res<T> = Result<T, AppError>;

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::Invalid, message)
}

fn not_found() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Hesap bulunamadı; kaldırılmış olabilir.",
    )
}

/// Sunucu ayarı yalnız parolalı IMAP hesabında değiştirilebilir.
pub const SERVER_NOT_EDITABLE: &str = "Bu hesabın sunucu ayarları buradan değiştirilemez. Google, Microsoft ve JMAP hesaplarında sunucu değişecekse hesabı kaldırıp yeniden ekle.";
/// Sunucu değişti, parola verilmedi.
pub const PASSWORD_REQUIRED: &str = "Sunucu değişince parolayı yeniden yaz.";
/// OAuth hesabına parola yazılmak istendi.
pub const OAUTH_HAS_NO_PASSWORD: &str = "Google ve Microsoft hesaplarında parola yok. Giriş sorunu varsa hesabı kaldırıp yeniden giriş yap.";

/// "#RRGGBB" biçiminde mi?
fn is_hex_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Düzenlemeyi denetler ve sadeleştirir: adlar kırpılır, renk büyük harfe
/// çevrilir, boş imza kaldırılır.
pub fn normalize_update(u: AccountUpdate) -> AppResult<AccountUpdate> {
    let name = mh_send::clean_name(&u.name);
    let name = name.as_str();
    if name.is_empty() {
        return Err(invalid("Hesap adı boş olamaz."));
    }
    let display_name = mh_send::clean_name(&u.display_name);
    let display_name = display_name.as_str();
    if display_name.is_empty() {
        return Err(invalid("Gönderen adı boş olamaz."));
    }
    let color = u.color.trim();
    if !is_hex_color(color) {
        return Err(invalid("Renk #RRGGBB biçiminde olmalı (ör. #4C6FD8)."));
    }
    let signature = u
        .signature
        .map(|s| s.trim_end().to_owned())
        .filter(|s| !s.trim().is_empty());
    Ok(AccountUpdate {
        name: name.to_owned(),
        display_name: display_name.to_owned(),
        color: color.to_ascii_uppercase(),
        signature,
        notify: u.notify,
    })
}

/// Hesabın adını, gönderen adını, rengini, imzasını ve bildirim tercihini
/// değiştirir.
#[tauri::command]
pub fn account_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    update: AccountUpdate,
) -> Res<Account> {
    let update = normalize_update(update)?;
    let account = state
        .store
        .update_account(&id, &update)?
        .ok_or_else(not_found)?;
    crate::sink::changed(&app, Vec::new());
    Ok(account)
}

/// Sunucu adı ve kullanıcı adı kırpılır.
fn clean_server(cfg: ServerConfig) -> ServerConfig {
    ServerConfig {
        host: cfg.host.trim().to_owned(),
        username: cfg.username.trim().to_owned(),
        ..cfg
    }
}

/// Parolanın gideceği yer değişti mi (sunucu adı, kapı ya da kullanıcı adı)?
/// Güvenlik türü tek başına sayılmaz: şifresiz bağlantı yalnız bu makineye
/// açılabilir.
fn endpoint_changed(old: Option<&ServerConfig>, new: &ServerConfig) -> bool {
    let host = |h: &str| h.trim().trim_end_matches('.').to_ascii_lowercase();
    old.is_none_or(|o| {
        host(&o.host) != host(&new.host)
            || o.port != new.port
            || o.username.trim() != new.username.trim()
    })
}

/// Sunucu düzenlemesini denetler: yalnız parolalı IMAP hesabı; alanlar dolu;
/// sunucu değişiyorsa parola verilmiş. (gelen, giden, verilen parola) döner.
pub fn check_server_update(
    account: &Account,
    update: ServerUpdate,
) -> AppResult<(ServerConfig, ServerConfig, Option<String>)> {
    if account.kind != AccountKind::Imap || account.auth != AuthKind::Password {
        return Err(invalid(SERVER_NOT_EDITABLE));
    }
    let imap = clean_server(update.imap);
    let smtp = clean_server(update.smtp);
    crate::commands::validate_server(&imap, "Gelen posta")?;
    crate::commands::validate_server(&smtp, "Giden posta")?;
    let password = update.password.filter(|p| !p.is_empty());
    let moved = endpoint_changed(account.imap.as_ref(), &imap)
        || endpoint_changed(account.smtp.as_ref(), &smtp);
    if moved && password.is_none() {
        return Err(invalid(PASSWORD_REQUIRED));
    }
    Ok((imap, smtp, password))
}

/// Parolayı anahtar zincirine yazar (engelleyici çağrı ayrı iş parçacığında).
async fn save_password(state: &AppState, id: &str, password: String) -> AppResult<()> {
    let secrets = state.secrets.clone();
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || secrets.set(&id, &password))
        .await
        .map_err(internal)?
}

/// Parolalı IMAP hesabının gelen ve giden posta sunucularını değiştirir.
/// Kaydetmeden önce iki sunucuya da bağlanıp kimlik doğrulanır; sertifika
/// güvenilir değilse `untrusted` döner (arayüz onay penceresini açar).
/// Başarılıysa ayar ve (verildiyse) yeni parola yazılır, eşitleme yeniden
/// başlar.
#[tauri::command]
pub async fn account_update_server(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    update: ServerUpdate,
) -> Res<Account> {
    let account = state.store.account(&id)?.ok_or_else(not_found)?;
    let (imap, smtp, given) = check_server_update(&account, update)?;
    let password = match &given {
        Some(p) => p.clone(),
        None => mh_sync::password_for(&state.secrets, &id).await?,
    };
    mh_sync::test_login(&imap, &password).await?;
    mh_send::test_login(&smtp, &password).await?;
    // Önce parola: anahtar zinciri yazılamazsa sunucu ayarı da değişmez.
    if let Some(p) = given {
        save_password(&state, &id, p).await?;
    }
    let account = state
        .store
        .update_account_servers(&id, &imap, &smtp)?
        .ok_or_else(not_found)?;
    state.sync.start(account.clone());
    crate::sink::changed(&app, Vec::new());
    Ok(account)
}

/// Hesabın parolasını (ya da uygulama parolasını) değiştirir. Önce sunucuya
/// giriş denenir (IMAP: gelen posta sunucusu; JMAP: oturum), sonra anahtar
/// zincirine yazılır ve eşitleme yeniden başlar.
#[tauri::command]
pub async fn account_set_password(
    state: State<'_, AppState>,
    id: String,
    password: String,
) -> Res<()> {
    if password.is_empty() {
        return Err(invalid("Parola boş olamaz."));
    }
    let account = state.store.account(&id)?.ok_or_else(not_found)?;
    if account.auth != AuthKind::Password {
        return Err(invalid(OAUTH_HAS_NO_PASSWORD));
    }
    match account.kind {
        AccountKind::Imap => {
            let imap = account
                .imap
                .as_ref()
                .ok_or_else(|| invalid("Hesabın gelen posta sunucusu ayarı yok."))?;
            mh_sync::test_login(imap, &password).await?;
        }
        AccountKind::Jmap => {
            let url = account
                .jmap_url
                .as_deref()
                .ok_or_else(|| invalid("Hesabın JMAP oturum adresi yok."))?;
            let username = state
                .store
                .jmap_account_state(&id)?
                .username
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| account.email.clone());
            mh_jmap::test_login(url, &username, &password).await?;
        }
    }
    save_password(&state, &id, password).await?;
    match account.kind {
        AccountKind::Imap => state.sync.start(account),
        AccountKind::Jmap => state.jmap.start(account),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::Security;

    fn update(name: &str, display: &str, color: &str, sig: Option<&str>) -> AccountUpdate {
        AccountUpdate {
            name: name.into(),
            display_name: display.into(),
            color: color.into(),
            signature: sig.map(str::to_owned),
            notify: true,
        }
    }

    #[test]
    fn duzenleme_sadelestirilir() {
        let u = normalize_update(update(
            "  İş ",
            " Furkan Yıldırım ",
            "#4c6fd8",
            Some("-- \nFurkan\n\n"),
        ))
        .unwrap();
        assert_eq!(
            u,
            update("İş", "Furkan Yıldırım", "#4C6FD8", Some("-- \nFurkan"))
        );
        let u = normalize_update(update("İş", "F", "#2E9A61", Some(" \n "))).unwrap();
        assert_eq!(u.signature, None, "boş imza kaldırılır");
        let u = normalize_update(AccountUpdate {
            notify: false,
            ..update("İş", "F", "#2E9A61", None)
        })
        .unwrap();
        assert!(!u.notify, "bildirim tercihi korunur");
    }

    #[test]
    fn gecersiz_duzenleme() {
        let err = |u| normalize_update(u).unwrap_err();
        let e = err(update("  ", "F", "#2E9A61", None));
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("Hesap adı"));
        assert!(err(update("İş", "\t", "#2E9A61", None))
            .message
            .contains("Gönderen adı"));
        for color in ["2E9A61", "#2E9A6", "#2E9A611", "#GG9A61", "", "#2e9a6ı"] {
            assert!(
                err(update("İş", "F", color, None))
                    .message
                    .contains("#RRGGBB"),
                "{color}"
            );
        }
    }

    fn server(host: &str, port: u16, user: &str) -> ServerConfig {
        ServerConfig {
            host: host.into(),
            port,
            security: Security::Tls,
            username: user.into(),
        }
    }

    fn imap_account() -> Account {
        Account {
            id: "a".into(),
            name: "İş".into(),
            email: "furkan@ornek.com".into(),
            display_name: "Furkan".into(),
            color: "#4C6FD8".into(),
            kind: AccountKind::Imap,
            imap: Some(server("imap.ornek.com", 993, "furkan")),
            smtp: Some(server("smtp.ornek.com", 465, "furkan")),
            jmap_url: None,
            signature: None,
            created_at: 0,
            auth: AuthKind::Password,
            notify: true,
        }
    }

    fn su(imap: ServerConfig, smtp: ServerConfig, password: Option<&str>) -> ServerUpdate {
        ServerUpdate {
            imap,
            smtp,
            password: password.map(str::to_owned),
        }
    }

    #[test]
    fn sunucu_degisince_parola_istenir() {
        let a = imap_account();
        let same = || server("imap.ornek.com", 993, "furkan");
        let smtp = || server("smtp.ornek.com", 465, "furkan");

        // Yalnız güvenlik türü ya da yazım (boşluk, büyük harf) değişti: kayıtlı parola denenir.
        let (imap, _, pw) = check_server_update(
            &a,
            su(
                ServerConfig {
                    security: Security::Starttls,
                    ..server(" IMAP.ornek.com. ", 993, " furkan ")
                },
                smtp(),
                None,
            ),
        )
        .unwrap();
        assert_eq!(
            (imap.host.as_str(), imap.username.as_str()),
            ("IMAP.ornek.com.", "furkan")
        );
        assert_eq!(pw, None);

        // Sunucu adı, kapı ya da kullanıcı adı değişti: parola zorunlu.
        for (imap, smtp) in [
            (server("posta.baska.com", 993, "furkan"), smtp()),
            (server("imap.ornek.com", 143, "furkan"), smtp()),
            (server("imap.ornek.com", 993, "furkan2"), smtp()),
            (same(), server("smtp.baska.com", 465, "furkan")),
            (same(), server("smtp.ornek.com", 587, "furkan")),
        ] {
            let e = check_server_update(&a, su(imap.clone(), smtp.clone(), None)).unwrap_err();
            assert_eq!(
                (e.code, e.message.as_str()),
                (ErrorCode::Invalid, PASSWORD_REQUIRED)
            );
            // Boş parola verilmemiş sayılır.
            let e = check_server_update(&a, su(imap.clone(), smtp.clone(), Some(""))).unwrap_err();
            assert_eq!(e.message, PASSWORD_REQUIRED);
            let (_, _, pw) = check_server_update(&a, su(imap, smtp, Some("yeni"))).unwrap();
            assert_eq!(pw.as_deref(), Some("yeni"));
        }

        // Eksik alan.
        let e = check_server_update(&a, su(server(" ", 993, "f"), smtp(), Some("p"))).unwrap_err();
        assert!(e.message.starts_with("Gelen posta"), "{}", e.message);
        let e = check_server_update(&a, su(same(), server("s", 0, "f"), Some("p"))).unwrap_err();
        assert!(e.message.starts_with("Giden posta"), "{}", e.message);
    }

    #[test]
    fn oauth_ve_jmap_hesabinda_sunucu_degismez() {
        let mut a = imap_account();
        a.auth = AuthKind::Google;
        let up = || {
            su(
                server("imap.ornek.com", 993, "furkan"),
                server("smtp.ornek.com", 465, "furkan"),
                Some("p"),
            )
        };
        let e = check_server_update(&a, up()).unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str()),
            (ErrorCode::Invalid, SERVER_NOT_EDITABLE)
        );
        let mut a = imap_account();
        a.kind = AccountKind::Jmap;
        assert_eq!(
            check_server_update(&a, up()).unwrap_err().message,
            SERVER_NOT_EDITABLE
        );
        // Hata ve hata ayıklama çıktısında parola yok.
        let shown = format!("{:?}", up());
        assert!(!shown.contains("\"p\""), "{shown}");
    }
}
