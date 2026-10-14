//! İstemci (telefon tarafı): paketi alma ve onay gönderme.
//!
//! [`fetch`] koddaki tüm adreslere eşzamanlı bağlanmayı dener (her biri
//! `connect_timeout`), ilk kurulan bağlantıya selamı gönderir ve paketi
//! okur; diğer denemeler iptal edilir. Tüm işlem `total_timeout` içinde
//! biter. [`confirm`] ikinci bir bağlantıyla onay mesajını yollar.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::time::timeout;
use zeroize::Zeroizing;

use crate::code::Code;
use crate::crypto::{self, FRAME_OVERHEAD, MAX_PACKAGE, NONCE_C_LEN};
use crate::error::{TransferError, TransferResult};
use crate::ApplyStatus;

/// İstemci ayarları. Varsayılanlar üretim değerleridir.
#[derive(Clone, Debug)]
pub struct ClientOptions {
    /// Her adrese bağlanma süresi: 3 s.
    pub connect_timeout: Duration,
    /// Paketi almanın toplam süresi: 30 s.
    pub total_timeout: Duration,
    /// Onayı göndermenin toplam süresi: 10 s.
    pub confirm_timeout: Duration,
    /// Koddaki loopback adreslere bağlan (yalnız testler).
    pub allow_loopback: bool,
    /// Kabul edilen en büyük düz paket (8 MiB).
    pub max_package: usize,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(3),
            total_timeout: Duration::from_secs(30),
            confirm_timeout: Duration::from_secs(10),
            allow_loopback: false,
            max_package: MAX_PACKAGE,
        }
    }
}

fn net(msg: &str) -> TransferError {
    TransferError::Network(msg.into())
}

fn hosts_for(code: &Code, opts: &ClientOptions) -> TransferResult<Vec<IpAddr>> {
    let hosts: Vec<IpAddr> = code
        .hosts
        .iter()
        .copied()
        .filter(|h| opts.allow_loopback || !h.is_loopback())
        .collect();
    if hosts.is_empty() {
        return Err(TransferError::InvalidCode(
            "Aktarım kodunda bu cihazın ulaşabileceği bir adres yok. Bilgisayarda yeni kod oluştur."
                .into(),
        ));
    }
    Ok(hosts)
}

/// Tüm adreslere eşzamanlı bağlanır; ilk kurulanı döner, kalanları iptal eder.
async fn connect_first(
    hosts: &[IpAddr],
    port: u16,
    connect_timeout: Duration,
) -> TransferResult<TcpStream> {
    let mut set = JoinSet::new();
    for ip in hosts {
        let addr = SocketAddr::new(*ip, port);
        set.spawn(async move {
            match timeout(connect_timeout, TcpStream::connect(addr)).await {
                Ok(Ok(s)) => Some((s, addr)),
                _ => None,
            }
        });
    }
    while let Some(r) = set.join_next().await {
        if let Ok(Some((stream, addr))) = r {
            tracing::debug!(%addr, "aktarım bağlantısı kuruldu");
            set.abort_all();
            return Ok(stream);
        }
    }
    Err(net(
        "Bilgisayara bağlanılamadı. İki cihaz aynı Wi-Fi ağında mı, bilgisayarda aktarım ekranı hâlâ açık mı? Güvenlik duvarı Mailhere'ye izin vermeli.",
    ))
}

/// Paketi alır. Dönen düz paket (JSON) `Zeroizing`'dir; çağıran ayrıştırır.
pub async fn fetch(code: &Code, opts: &ClientOptions) -> TransferResult<Zeroizing<Vec<u8>>> {
    let hosts = hosts_for(code, opts)?;
    let keys = code.keys();
    let work = async {
        let mut stream = connect_first(&hosts, code.port, opts.connect_timeout).await?;
        let nonce_c = *crypto::random::<NONCE_C_LEN>()?;
        let hello = crypto::build_hello(&keys, &code.t, &nonce_c);
        stream
            .write_all(&hello)
            .await
            .map_err(|_| net("Bilgisayara istek gönderilemedi; bağlantı koptu. Yeniden dene."))?;
        let mut len = [0u8; 4];
        // Sunucu geçersiz selamı yanıtsız kapatır: EOF ya da (okunmamış bayt
        // kaldıysa) sıfırlama. İkisi de "kabul edilmedi" demektir.
        stream.read_exact(&mut len).await.map_err(|e| {
            use std::io::ErrorKind::{ConnectionAborted, ConnectionReset, UnexpectedEof};
            if matches!(e.kind(), UnexpectedEof | ConnectionReset | ConnectionAborted) {
                TransferError::Protocol(
                    "Bilgisayar isteği kabul etmedi: kod eşleşmiyor, süresi dolmuş ya da daha önce kullanılmış olabilir. Bilgisayarda yeni kod oluştur."
                        .into(),
                )
            } else {
                net("Bilgisayardan yanıt alınamadı; bağlantı koptu. Yeniden dene.")
            }
        })?;
        let len = u32::from_be_bytes(len) as usize;
        if len < FRAME_OVERHEAD || len > FRAME_OVERHEAD + opts.max_package {
            return Err(TransferError::Protocol(
                "Bilgisayardan gelen paket beklenen boyutta değil; aktarım durduruldu. İki cihazdaki Mailhere sürümleri uyumlu mu?"
                    .into(),
            ));
        }
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.map_err(|_| {
            net("Paket alınırken bağlantı koptu. İki cihaz aynı ağda kalsın; yeni kodla yeniden dene.")
        })?;
        crypto::open_package(&keys, &code.t, &nonce_c, body)
    };
    timeout(opts.total_timeout, work).await.map_err(|_| {
        TransferError::Timeout(format!(
            "Bilgisayar {} saniyede yanıt vermedi. İki cihaz aynı ağda mı? Yeni kodla yeniden dene.",
            opts.total_timeout.as_secs()
        ))
    })?
}

/// Uygulama sonucunu bilgisayara bildirir (ikinci bağlantı). Sunucu mesajı
/// okuyup kapatana dek bekler; böylece dönüşte bilgisayar durumu görmüştür.
pub async fn confirm(code: &Code, status: ApplyStatus, opts: &ClientOptions) -> TransferResult<()> {
    let hosts = hosts_for(code, opts)?;
    let keys = code.keys();
    let work = async {
        let mut stream = connect_first(&hosts, code.port, opts.connect_timeout).await?;
        let msg = crypto::build_confirm(&keys, &code.t, status.as_u8())?;
        stream.write_all(&msg).await.map_err(|_| {
            net("Onay bilgisayara gönderilemedi; bağlantı koptu. Ayarlar yine de bu cihazda uygulandı.")
        })?;
        let _ = stream.shutdown().await;
        let mut buf = [0u8; 1];
        let _ = stream.read(&mut buf).await;
        Ok(())
    };
    timeout(opts.confirm_timeout, work).await.map_err(|_| {
        TransferError::Timeout(
            "Onay bilgisayara zamanında ulaşmadı. Ayarlar yine de bu cihazda uygulandı; bilgisayardaki ekranı kapatabilirsin."
                .into(),
        )
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use tokio::net::TcpListener;

    fn lo_code(port: u16) -> Code {
        Code::generate(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)], port, true).unwrap()
    }

    fn copts() -> ClientOptions {
        ClientOptions {
            allow_loopback: true,
            connect_timeout: Duration::from_millis(500),
            total_timeout: Duration::from_secs(5),
            confirm_timeout: Duration::from_secs(2),
            ..ClientOptions::default()
        }
    }

    #[tokio::test]
    async fn loopback_uretimde_suzulur() {
        let code = lo_code(5000);
        let e = fetch(&code, &ClientOptions::default()).await.unwrap_err();
        assert!(matches!(e, TransferError::InvalidCode(_)), "{e:?}");
        let e = confirm(&code, ApplyStatus::Applied, &ClientOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(e, TransferError::InvalidCode(_)), "{e:?}");
    }

    #[tokio::test]
    async fn baglanamayinca_ag_hatasi() {
        // Dinleyen yok: bağlantı reddedilir.
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let e = fetch(&lo_code(port), &copts()).await.unwrap_err();
        assert!(matches!(e, TransferError::Network(_)), "{e:?}");
        assert!(e.to_string().contains("bağlanılamadı"));
    }

    #[tokio::test]
    async fn buyuk_paket_reddedilir() {
        // Sunucu sınırın üstünde bir uzunluk duyurur: gövde hiç okunmadan hata.
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut hello = [0u8; crypto::HELLO_LEN];
            s.read_exact(&mut hello).await.unwrap();
            let len = (FRAME_OVERHEAD + MAX_PACKAGE + 1) as u32;
            s.write_all(&len.to_be_bytes()).await.unwrap();
            // Gövdeyi hiç göndermeden bekle; istemci okumaya kalkışırsa takılır.
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let started = std::time::Instant::now();
        let e = fetch(&lo_code(port), &copts()).await.unwrap_err();
        assert!(matches!(e, TransferError::Protocol(_)), "{e:?}");
        assert!(e.to_string().contains("beklenen boyutta değil"));
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "hemen reddedilmeli"
        );
        // Çok küçük uzunluk da reddedilir.
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut hello = [0u8; crypto::HELLO_LEN];
            s.read_exact(&mut hello).await.unwrap();
            s.write_all(&(FRAME_OVERHEAD as u32 - 1).to_be_bytes())
                .await
                .unwrap();
        });
        let e = fetch(&lo_code(port), &copts()).await.unwrap_err();
        assert!(matches!(e, TransferError::Protocol(_)), "{e:?}");
    }

    #[tokio::test]
    async fn sunucu_susarsa_zaman_asimi() {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (_s, _) = l.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let e = fetch(
            &lo_code(port),
            &ClientOptions {
                total_timeout: Duration::from_millis(400),
                ..copts()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(e, TransferError::Timeout(_)), "{e:?}");
    }

    #[tokio::test]
    async fn ilk_kurulan_baglanti_kullanilir() {
        // Kodda ulaşılamayan adres + loopback: loopback kazanır.
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = l.local_addr().unwrap().port();
        let mut code = lo_code(port);
        code.hosts.insert(0, "10.255.255.1".parse().unwrap());
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut hello = [0u8; crypto::HELLO_LEN];
            s.read_exact(&mut hello).await.unwrap();
            // Yanıtsız kapat → istemci "kabul etmedi" der.
        });
        let e = fetch(&code, &copts()).await.unwrap_err();
        assert!(e.to_string().contains("kabul etmedi"), "{e}");
    }
}
