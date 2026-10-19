//! IMAP hesapları için anlık akış: hesabın sunucusu JMAP de konuşuyorsa
//! (Stalwart gibi) sunucunun durum akışına (RFC 8620 §7.3, EventSource)
//! bağlanılır; "yeni posta teslim edildi" olayı gelince IMAP eşitlemesi hemen
//! başlar. IMAP IDLE'ın kopup geç fark edildiği ya da yalnız tek klasörü
//! izlediği durumlarda posta saniyeler içinde gelir. Aktarıcı ve Firebase
//! gerekmez; uygulama (masaüstünde tepside, Android'de ön plan hizmetinde)
//! çalıştıkça bağlantı açık kalır.
//!
//! Adres ve kimlik `push::yan_istemci` ile aynıdır: IMAP sunucusunun kendi
//! adındaki `/.well-known/jmap`, IMAP kullanıcı adı ve parolası. JMAP'i
//! olmayan sunucu [`JMAPSIZ_BEKLE`] boyunca yeniden denenmez.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use mh_core::Account;
use mh_store::Store;
use mh_sync::{SecretStore, SyncManager};
use tokio::task::JoinHandle;

use crate::push::{imap_jmap_adresi, jmap_yok, yan_istemci};
use crate::state::AppState;

/// Hesap listesi bu aralıkla gözden geçirilir (eklenen, silinen, sunucusu değişen).
const GOZDEN_GECIR: Duration = Duration::from_secs(60);
/// Sunucudan istenen canlılık sinyali (saniye); üç katı sessizlikte akış yenilenir.
const PING_SN: u32 = 60;
const EN_AZ_BEKLE: Duration = Duration::from_secs(2);
const EN_COK_BEKLE: Duration = Duration::from_secs(300);
/// JMAP'i olmayan ya da bu kimliği kabul etmeyen sunucu bu süre denenmez.
const JMAPSIZ_BEKLE: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone)]
struct Parcalar {
    store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
    sync: Arc<SyncManager>,
}

/// İzlenecek hesaplar: anahtar hesap kimliği, değer JMAP adresi (adres
/// değişirse dinleyici yeniden kurulur).
pub fn izlenecekler(hesaplar: &[Account]) -> BTreeMap<String, String> {
    hesaplar
        .iter()
        .filter_map(|a| imap_jmap_adresi(a).map(|u| (a.id.clone(), u)))
        .collect()
}

/// Gözetmeni başlatır: hesap başına bir dinleyici, dakikada bir uzlaştırma.
pub fn start(state: &AppState, rt: &tokio::runtime::Handle) -> JoinHandle<()> {
    let p = Parcalar {
        store: state.store.clone(),
        secrets: state.secrets.clone(),
        sync: state.sync.clone(),
    };
    let rt2 = rt.clone();
    rt.spawn(async move {
        let mut calisan: BTreeMap<String, (String, JoinHandle<()>)> = BTreeMap::new();
        loop {
            let hesaplar = {
                let store = p.store.clone();
                tokio::task::spawn_blocking(move || store.accounts())
                    .await
                    .ok()
                    .and_then(Result::ok)
            };
            if let Some(hesaplar) = hesaplar {
                let istenen = izlenecekler(&hesaplar);
                calisan.retain(|id, (url, h)| {
                    let kalsin = istenen.get(id) == Some(url) && !h.is_finished();
                    if !kalsin {
                        h.abort();
                    }
                    kalsin
                });
                for (id, url) in istenen {
                    if calisan.contains_key(&id) {
                        continue;
                    }
                    let Some(acc) = hesaplar.iter().find(|a| a.id == id).cloned() else {
                        continue;
                    };
                    let h = rt2.spawn(dinle(p.clone(), acc));
                    calisan.insert(id, (url, h));
                }
            }
            tokio::time::sleep(GOZDEN_GECIR).await;
        }
    })
}

/// Tek hesabın akışı; kopunca artan beklemeyle yeniden bağlanır.
async fn dinle(p: Parcalar, account: Account) {
    let idle = Duration::from_secs(u64::from(PING_SN) * 3);
    let mut bekle = EN_AZ_BEKLE;
    // Sunucu `EmailDelivery` türünü akışta tanımıyorsa posta değişikliklerine düşülür.
    let mut turler = "EmailDelivery";
    loop {
        let mut baglandi = false;
        let sonuc = async {
            let client = yan_istemci(&p.secrets, &account).await?;
            mh_jmap::listen(
                &client,
                turler,
                PING_SN,
                idle,
                || {
                    baglandi = true;
                    tracing::info!("JMAP durum akışı bağlandı (IMAP hesabı)");
                },
                || {
                    if !p.sync.sync_account(&account.id) {
                        p.sync.sync_now();
                    }
                },
            )
            .await
        }
        .await;
        let Err(e) = sonuc else { continue };
        if !baglandi && turler == "EmailDelivery" && matches!(e, mh_jmap::JmapError::Status(400)) {
            turler = "Email";
            continue;
        }
        if !baglandi && jmap_yok(&e) {
            tracing::info!(error = %e, "IMAP sunucusunda JMAP durum akışı yok; sonra yeniden denenecek");
            tokio::time::sleep(JMAPSIZ_BEKLE).await;
            continue;
        }
        tracing::debug!(error = %e, "JMAP durum akışı kesildi");
        if baglandi {
            bekle = EN_AZ_BEKLE;
        }
        tokio::time::sleep(bekle).await;
        bekle = (bekle * 2).min(EN_COK_BEKLE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_core::{AccountKind, AuthKind, Security, ServerConfig};

    fn hesap(id: &str, kind: AccountKind, host: &str, auth: AuthKind) -> Account {
        Account {
            id: id.into(),
            name: "İş".into(),
            email: "furkan@ornek.com".into(),
            display_name: "Furkan".into(),
            color: "#4C6FD8".into(),
            kind,
            imap: Some(ServerConfig {
                host: host.into(),
                port: 993,
                security: Security::Tls,
                username: "furkan".into(),
            }),
            smtp: None,
            jmap_url: None,
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth,
            notify: true,
        }
    }

    #[test]
    fn yalniz_parolali_imap_hesaplari_izlenir() {
        let liste = izlenecekler(&[
            hesap("a", AccountKind::Imap, "mail.ornek.com", AuthKind::Password),
            hesap("b", AccountKind::Imap, "imap.gmail.com", AuthKind::Google),
            hesap("c", AccountKind::Jmap, "mail.ornek.com", AuthKind::Password),
            hesap("d", AccountKind::Imap, "demo.invalid", AuthKind::Password),
        ]);
        assert_eq!(
            liste.into_iter().collect::<Vec<_>>(),
            vec![(
                "a".to_owned(),
                "https://mail.ornek.com/.well-known/jmap".to_owned()
            )]
        );
    }
}
