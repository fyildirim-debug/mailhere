//! Abonelikten çıkma: `unsubscribe`.
//!
//! Sıra: (1) gönderen tek tıkla çıkmayı (RFC 8058) destekliyorsa HTTPS POST;
//! (2) olmadıysa `mailto:` adresi varsa postayı alan hesaptan çıkma postası;
//! (3) o da yoksa arayüz çıkma sayfasını tarayıcıda açar.

use mh_core::{AppError, ErrorCode, MessageId, Unsubscribe, UnsubscribeResult};
use mh_send::OneClickError;
use tauri::State;

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

fn no_link() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Bu postada abonelikten çıkma bağlantısı yok.",
    )
}

/// Tek tık denemesinden sonraki adım.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// Bu `mailto:` adresine çıkma postası gönderilir.
    Mail(String),
    /// Arayüz `url` adresini tarayıcıda açar.
    Browser,
    Fail(AppError),
}

/// Tek tık isteği yapılmadıysa (`None`) ya da başarısız olduysa ne yapılır?
/// Sunucu isteği reddettiyse çıkma sayfası tarayıcıda tamamlanabilir; istek
/// hiç gönderilemediyse (güvenlik, bağlantı) hata gösterilir.
fn next_step(info: &Unsubscribe, one_click: Option<&OneClickError>) -> Step {
    if let Some(m) = &info.mailto {
        return Step::Mail(m.clone());
    }
    match (one_click, &info.url) {
        (Some(OneClickError::Failed(e)), _) => Step::Fail(e.clone()),
        (_, Some(_)) => Step::Browser,
        (Some(e), None) => Step::Fail(e.to_app()),
        (None, None) => Step::Fail(no_link()),
    }
}

/// Postanın `List-Unsubscribe` başlığıyla abonelikten çıkar.
#[tauri::command]
pub async fn unsubscribe(state: State<'_, AppState>, id: MessageId) -> Res<UnsubscribeResult> {
    crate::commands::ensure_body(&state, id).await?;
    let account_id = state
        .store
        .location(id)?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "Posta bulunamadı; silinmiş ya da taşınmış olabilir.",
            )
        })?
        .account_id;
    let raw = state.store.raw(id)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Postanın içeriği yerelde yok. Eşitlemenin bitmesini bekleyip yeniden dene.",
        )
    })?;
    let info = mh_mime::unsubscribe(&raw).ok_or_else(no_link)?;

    let mut failure = None;
    if let (true, Some(url)) = (info.one_click, info.url.as_deref()) {
        match mh_send::one_click(url).await {
            Ok(()) => return Ok(UnsubscribeResult::Done),
            Err(e) => failure = Some(e),
        }
    }
    match next_step(&info, failure.as_ref()) {
        Step::Mail(uri) => {
            let msg = mh_send::mailto_message(&account_id, &uri).ok_or_else(|| {
                AppError::new(
                    ErrorCode::Invalid,
                    "Abonelikten çıkma adresi geçersiz. Gönderenin sitesinden aboneliği kapatmayı dene.",
                )
            })?;
            state.outbox.send_immediately(&msg).await?;
            Ok(UnsubscribeResult::Done)
        }
        Step::Browser => Ok(UnsubscribeResult::Browser),
        Step::Fail(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(url: Option<&str>, mailto: Option<&str>, one_click: bool) -> Unsubscribe {
        Unsubscribe {
            url: url.map(str::to_owned),
            mailto: mailto.map(str::to_owned),
            one_click,
        }
    }

    #[test]
    fn tek_tiktan_sonraki_adim() {
        let blocked = OneClickError::Failed(AppError::new(ErrorCode::Invalid, "güvenlik"));
        let both = info(Some("https://a.com/c"), Some("mailto:c@a.com"), true);
        // Tek tık olmadı: mailto varsa posta.
        for f in [None, Some(&blocked), Some(&OneClickError::Refused(500))] {
            assert_eq!(next_step(&both, f), Step::Mail("mailto:c@a.com".into()));
        }
        let web = info(Some("https://a.com/c"), None, true);
        // Sunucu reddetti: tarayıcıda tamamlanır.
        assert_eq!(
            next_step(&web, Some(&OneClickError::Refused(403))),
            Step::Browser
        );
        // İstek gönderilemedi: hata.
        assert_eq!(
            next_step(&web, Some(&blocked)),
            Step::Fail(AppError::new(ErrorCode::Invalid, "güvenlik"))
        );
        // Tek tık yok, yalnız http adresi: tarayıcı.
        assert_eq!(
            next_step(&info(Some("http://a.com/c"), None, false), None),
            Step::Browser
        );
        assert_eq!(
            next_step(&info(None, None, false), None),
            Step::Fail(no_link())
        );
        assert_eq!(
            no_link().message,
            "Bu postada abonelikten çıkma bağlantısı yok."
        );
    }
}
