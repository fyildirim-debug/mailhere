//! Teklif sunucusu (bilgisayar tarafı).
//!
//! [`start`] paketi alır, yerel bir kapı açar, kodu ve QR'ı üretir ve arka
//! planda bir görev başlatır. Görev:
//! - yalnız yerel ağ eşlerini kabul eder ([`crate::addr::peer_allowed`]);
//! - geçerli ilk selama paketi şifreli gönderir (**tek teslim**); sonraki
//!   selamlar ve her hatalı selam yanıtsız kapanır;
//! - `max_bad_attempts` hatalı denemede `failed` ile kapanır;
//! - `ttl` içinde teslim olmazsa `expired`; teslimden sonra `confirm_ttl`
//!   içinde onay gelmezse yine `expired`;
//! - onay gelince `applied` (ya da telefon başarısız bildirdiyse `failed`);
//! - [`OfferHandle::cancel`] ya da tutamaç düşünce `cancelled`.
//!
//! Durumlar `events` kanalından `TransferState` olarak gelir; ilk durum
//! `waiting`'dir ve kanal son durumdan sonra kapanır. Aynı anda tek teklif
//! kuralını çağıran (mh-app) uygular: yeni teklif açmadan önce eskisini
//! iptal eder.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mh_core::{Millis, TransferOffer, TransferState, TransferStateKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::{timeout, Instant};
use zeroize::Zeroizing;

use crate::addr::{local_hosts, peer_allowed};
use crate::code::Code;
use crate::crypto::{self, Keys, Token, CONFIRM_LEN, HELLO_LEN, MAX_PACKAGE};
use crate::error::{TransferError, TransferResult};
use crate::qr;
use crate::ApplyStatus;

/// Aynı anda işlenen en çok bağlantı; fazlası hemen kapatılır.
pub const MAX_CONNECTIONS: usize = 8;

/// Teklif ayarları. Varsayılanlar üretim değerleridir; testler kısaltır ve
/// loopback açar.
#[derive(Clone, Debug)]
pub struct OfferOptions {
    /// Teklifin ömrü (telefon bu sürede bağlanmalı): 180 s.
    pub ttl: Duration,
    /// Teslimden sonra onay için beklenen süre: 120 s.
    pub confirm_ttl: Duration,
    /// Bir bağlantıdan selam/onay okuma süresi: 10 s.
    pub read_timeout: Duration,
    /// Paketi yazma süresi: 30 s.
    pub write_timeout: Duration,
    /// Bu kadar hatalı selam/onay sonrası teklif kapanır: 5.
    pub max_bad_attempts: u32,
    /// Loopback eşleri ve adresleri kabul et (yalnız testler).
    pub allow_loopback: bool,
    /// Kodda duyurulacak adresler; `None` ise bu bilgisayarın yerel adresleri.
    pub hosts: Option<Vec<IpAddr>>,
    /// Dinlenecek adres; `None` ise tüm arayüzler (IPv6 çift yığın + IPv4).
    pub bind: Option<IpAddr>,
    /// Kapı; 0 ise işletim sistemi seçer.
    pub port: u16,
}

impl Default for OfferOptions {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(180),
            confirm_ttl: Duration::from_secs(120),
            read_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(30),
            max_bad_attempts: 5,
            allow_loopback: false,
            hosts: None,
            bind: None,
            port: 0,
        }
    }
}

impl OfferOptions {
    /// Yalnız 127.0.0.1'de dinleyen ve onu duyuran yapılandırma (testler).
    pub fn loopback() -> Self {
        Self {
            allow_loopback: true,
            hosts: Some(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
            bind: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            ..Self::default()
        }
    }
}

/// Teklifi iptal etme tutamacı. Kopyalanabilir; son kopya düşünce de teklif
/// iptal edilir (`cancelled`).
#[derive(Clone)]
pub struct OfferHandle(Arc<watch::Sender<bool>>);

impl OfferHandle {
    /// Teklifi kapatır; `cancelled` durumu yayımlanır. Bitmiş teklifte etkisiz.
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
}

impl fmt::Debug for OfferHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OfferHandle")
    }
}

/// Başlatılmış teklif.
pub struct Offer {
    /// Arayüze giden kod, QR ve süre bilgisi.
    pub info: TransferOffer,
    /// Durum akışı: `waiting` → `connected` → `delivered` → `applied`;
    /// ya da `failed` / `expired` / `cancelled`. Son durumdan sonra kapanır.
    pub events: mpsc::UnboundedReceiver<TransferState>,
    pub handle: OfferHandle,
}

impl Offer {
    pub fn cancel(&self) {
        self.handle.cancel();
    }

    /// Parçalara ayırır: bilgi, iptal tutamacı ve durum akışı.
    pub fn into_parts(
        self,
    ) -> (
        TransferOffer,
        OfferHandle,
        mpsc::UnboundedReceiver<TransferState>,
    ) {
        (self.info, self.handle, self.events)
    }
}

impl fmt::Debug for Offer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Offer").field("info", &self.info).finish()
    }
}

fn now_millis() -> Millis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}

/// Teklifi başlatır. Bir tokio çalışma zamanı içinde çağrılmalıdır (arka
/// plan görevi `tokio::spawn` ile başlar). `package` düz paket JSON'udur;
/// en çok 8 MiB. Dönüşte `waiting` durumu kanalda hazırdır.
pub async fn start(package: Zeroizing<Vec<u8>>, opts: OfferOptions) -> TransferResult<Offer> {
    if package.len() > MAX_PACKAGE {
        return Err(TransferError::Internal(format!(
            "Aktarım paketi çok büyük ({} KB; üst sınır {} KB). Çok sayıda imza ya da şablon varsa azaltıp yeniden dene.",
            package.len() / 1024,
            MAX_PACKAGE / 1024
        )));
    }
    let hosts = match &opts.hosts {
        Some(h) => h.clone(),
        None => local_hosts(opts.allow_loopback),
    };
    let (listeners, port) = bind(&opts).await?;
    let code = Code::generate(hosts, port, opts.allow_loopback)?;
    let text = code.to_text();
    let qr = qr::matrix(&text)?;
    let info = TransferOffer {
        text: text.to_string(),
        qr,
        hosts: code.hosts.iter().map(|h| h.to_string()).collect(),
        port,
        expires_at: now_millis() + opts.ttl.as_millis() as Millis,
    };

    let (ev_tx, ev_rx) = mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (in_tx, in_rx) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared {
        keys: code.keys(),
        t: code.t,
        phase: Mutex::new(Phase::Waiting),
        package: Mutex::new(Some(package)),
        bad: AtomicU32::new(0),
        read_timeout: opts.read_timeout,
        write_timeout: opts.write_timeout,
        max_bad: opts.max_bad_attempts.max(1),
        allow_loopback: opts.allow_loopback,
        tx: in_tx,
    });
    let _ = ev_tx.send(TransferState::new(TransferStateKind::Waiting));
    tracing::debug!(port, hosts = ?code.hosts, "aktarım teklifi açıldı");
    tokio::spawn(run(listeners, shared, in_rx, cancel_rx, ev_tx, opts));
    Ok(Offer {
        info,
        events: ev_rx,
        handle: OfferHandle(Arc::new(cancel_tx)),
    })
}

/// Dinleyicileri açar. `bind` verilmemişse önce `[::]` (Linux/macOS'ta çift
/// yığın: IPv4 eşleri eşlenmiş adresle gelir), sonra aynı kapıda `0.0.0.0`
/// (Windows'ta `[::]` yalnız IPv6'dır; çift yığın sistemlerde bu bağlama
/// "kullanımda" hatası verir ve atlanır).
async fn bind(opts: &OfferOptions) -> TransferResult<(Vec<TcpListener>, u16)> {
    let fail = |e: std::io::Error| {
        TransferError::Internal(format!(
            "Aktarım için yerel kapı açılamadı ({e}). Güvenlik duvarı ya da başka bir uygulama engelliyor olabilir."
        ))
    };
    if let Some(ip) = opts.bind {
        let l = TcpListener::bind((ip, opts.port)).await.map_err(fail)?;
        let port = l.local_addr().map_err(fail)?.port();
        return Ok((vec![l], port));
    }
    let mut listeners = Vec::new();
    let mut port = opts.port;
    match TcpListener::bind((Ipv6Addr::UNSPECIFIED, port)).await {
        Ok(l6) => {
            port = l6.local_addr().map_err(fail)?.port();
            listeners.push(l6);
        }
        Err(e) => tracing::debug!(error = %e, "IPv6 dinleyici açılamadı; yalnız IPv4"),
    }
    match TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await {
        Ok(l4) => {
            port = l4.local_addr().map_err(fail)?.port();
            listeners.push(l4);
        }
        Err(e) if listeners.is_empty() => return Err(fail(e)),
        Err(e) => {
            tracing::debug!(error = %e, "IPv4 dinleyici açılamadı; IPv6 çift yığın kullanılır")
        }
    }
    Ok((listeners, port))
}

/// Bağlantı görevlerinin ana döngüye bildirdikleri.
enum Internal {
    Connected,
    Delivered,
    Failed(String),
    Applied(u8),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Geçerli selam bekleniyor.
    Waiting,
    /// Paket bir istemciye verildi (gönderiliyor ya da gönderildi); artık
    /// yalnız onay okunur.
    Claimed,
    /// Teklif bitti.
    Closed,
}

struct Shared {
    keys: Keys,
    t: Token,
    phase: Mutex<Phase>,
    package: Mutex<Option<Zeroizing<Vec<u8>>>>,
    bad: AtomicU32,
    read_timeout: Duration,
    write_timeout: Duration,
    max_bad: u32,
    allow_loopback: bool,
    tx: mpsc::UnboundedSender<Internal>,
}

impl Shared {
    fn phase(&self) -> Phase {
        *self.phase.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set_phase(&self, p: Phase) {
        *self.phase.lock().unwrap_or_else(|p| p.into_inner()) = p;
    }

    /// Paketi yalnız `Waiting` evresinde ve tek bir bağlantıya verir.
    fn claim(&self) -> Option<Zeroizing<Vec<u8>>> {
        let mut phase = self.phase.lock().unwrap_or_else(|p| p.into_inner());
        if *phase != Phase::Waiting {
            return None;
        }
        *phase = Phase::Claimed;
        self.package
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }

    fn close(&self) {
        self.set_phase(Phase::Closed);
        self.package
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }

    fn bad_attempt(&self, peer: IpAddr) {
        let n = self.bad.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::debug!(%peer, n, "hatalı aktarım denemesi");
        if n >= self.max_bad {
            let _ = self.tx.send(Internal::Failed(format!(
                "Ağdan {n} hatalı bağlantı denemesi geldi; güvenlik için teklif kapatıldı. Bilgisayarda yeni kod oluşturup yeniden dene."
            )));
        }
    }
}

async fn accept_any(listeners: &[TcpListener]) -> std::io::Result<(TcpStream, SocketAddr)> {
    match listeners {
        [a] => a.accept().await,
        [a, b, ..] => tokio::select! {
            r = a.accept() => r,
            r = b.accept() => r,
        },
        [] => std::future::pending().await,
    }
}

fn applied_state(status: u8) -> TransferState {
    match ApplyStatus::from_u8(status) {
        Some(ApplyStatus::Applied) => TransferState::new(TransferStateKind::Applied),
        Some(ApplyStatus::Partial) => TransferState::with_message(
            TransferStateKind::Applied,
            "Telefon ayarları uyguladı; bazı öğeler aktarılamadı, ayrıntı telefondaki raporda.",
        ),
        Some(ApplyStatus::Failed) => TransferState::with_message(
            TransferStateKind::Failed,
            "Telefon paketi uygulayamadı. Telefondaki hatayı denetleyip yeniden dene.",
        ),
        None => TransferState::with_message(
            TransferStateKind::Failed,
            "Telefon bilinmeyen bir sonuç bildirdi. Telefondaki durumu denetle.",
        ),
    }
}

async fn run(
    listeners: Vec<TcpListener>,
    shared: Arc<Shared>,
    mut in_rx: mpsc::UnboundedReceiver<Internal>,
    mut cancel_rx: watch::Receiver<bool>,
    ev_tx: mpsc::UnboundedSender<TransferState>,
    opts: OfferOptions,
) {
    let emit = |s: TransferState| {
        let _ = ev_tx.send(s);
    };
    let mut deadline = Instant::now() + opts.ttl;
    let mut delivered = false;
    let mut conns: JoinSet<()> = JoinSet::new();
    let last = loop {
        tokio::select! {
            biased;
            // Ok: iptal istendi; Err: tutamaç düştü. İkisi de iptaldir.
            _ = cancel_rx.changed() => {
                break TransferState::new(TransferStateKind::Cancelled);
            }
            Some(msg) = in_rx.recv() => match msg {
                Internal::Connected => emit(TransferState::new(TransferStateKind::Connected)),
                Internal::Delivered => {
                    if !delivered {
                        delivered = true;
                        deadline = Instant::now() + opts.confirm_ttl;
                        emit(TransferState::new(TransferStateKind::Delivered));
                    }
                }
                Internal::Failed(m) => break TransferState::with_message(TransferStateKind::Failed, m),
                Internal::Applied(status) => {
                    // Onay teslimi de kanıtlar; sıra bozulmasın.
                    if !delivered {
                        emit(TransferState::new(TransferStateKind::Delivered));
                    }
                    break applied_state(status);
                }
            },
            _ = tokio::time::sleep_until(deadline) => {
                break if delivered {
                    TransferState::with_message(
                        TransferStateKind::Expired,
                        "Paket telefona ulaştı ama onay gelmedi. Telefondaki sonucu denetle; gerekirse yeni kodla yeniden dene.",
                    )
                } else {
                    TransferState::with_message(
                        TransferStateKind::Expired,
                        "Kodun süresi doldu; telefon bağlanmadı. Yeni kod oluşturup yeniden dene.",
                    )
                };
            }
            r = accept_any(&listeners) => match r {
                Ok((stream, peer)) => {
                    while conns.try_join_next().is_some() {}
                    if conns.len() >= MAX_CONNECTIONS {
                        tracing::debug!(%peer, "çok fazla eşzamanlı bağlantı; kapatıldı");
                        drop(stream);
                    } else {
                        conns.spawn(handle(stream, peer, shared.clone()));
                    }
                }
                Err(e) => {
                    tracing::debug!(error = %e, "bağlantı kabul edilemedi");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            Some(_) = conns.join_next(), if !conns.is_empty() => {}
        }
    };
    shared.close();
    conns.abort_all();
    drop(listeners);
    tracing::debug!(state = ?last.state, "aktarım teklifi kapandı");
    emit(last);
}

/// Tek bağlantı: eş denetimi, selam → paket ya da onay.
async fn handle(mut stream: TcpStream, peer: SocketAddr, shared: Arc<Shared>) {
    let peer_ip = peer.ip().to_canonical();
    if !peer_allowed(peer_ip, shared.allow_loopback) {
        tracing::debug!(%peer_ip, "yerel ağ dışından bağlantı; kapatıldı");
        return;
    }
    match shared.phase() {
        Phase::Waiting => {
            let mut hello = [0u8; HELLO_LEN];
            if timeout(shared.read_timeout, stream.read_exact(&mut hello))
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_none()
            {
                shared.bad_attempt(peer_ip);
                return;
            }
            let Ok(nonce_c) = crypto::verify_hello(&shared.keys, &shared.t, &hello) else {
                shared.bad_attempt(peer_ip);
                return;
            };
            // Geçerli selam ama paket zaten verildi: sessiz kapanış, sayılmaz.
            let Some(package) = shared.claim() else {
                tracing::debug!(%peer_ip, "ikinci geçerli selam; paket zaten verildi");
                return;
            };
            let _ = shared.tx.send(Internal::Connected);
            let frame = match crypto::seal_package(&shared.keys, &shared.t, &nonce_c, &package) {
                Ok(f) => f,
                Err(e) => {
                    let _ = shared.tx.send(Internal::Failed(e.to_string()));
                    return;
                }
            };
            drop(package);
            let written = timeout(shared.write_timeout, async {
                stream.write_all(&frame).await?;
                stream.shutdown().await
            })
            .await;
            match written {
                Ok(Ok(())) => {
                    let _ = shared.tx.send(Internal::Delivered);
                }
                _ => {
                    let _ = shared.tx.send(Internal::Failed(
                        "Paket telefona gönderilirken bağlantı koptu. İki cihaz aynı ağda kalsın; yeni kodla yeniden dene."
                            .into(),
                    ));
                }
            }
        }
        Phase::Claimed => {
            let mut msg = [0u8; CONFIRM_LEN];
            if timeout(shared.read_timeout, stream.read_exact(&mut msg))
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_none()
            {
                shared.bad_attempt(peer_ip);
                return;
            }
            match crypto::open_confirm(&shared.keys, &shared.t, &msg) {
                Ok(status) => {
                    let _ = shared.tx.send(Internal::Applied(status));
                    // Telefon kapanışı bekleyebilir; okuduğumuzu belli edelim.
                    let _ = stream.shutdown().await;
                }
                Err(crypto::Rejected) => shared.bad_attempt(peer_ip),
            }
        }
        Phase::Closed => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{self, ClientOptions};
    use crate::crypto::{build_hello, NONCE_C_LEN};
    use tokio::io::AsyncWriteExt;

    fn paket() -> Zeroizing<Vec<u8>> {
        Zeroizing::new(br#"{"v":1,"app":"mailhere","accounts":[]}"#.to_vec())
    }

    fn opts() -> OfferOptions {
        OfferOptions {
            read_timeout: Duration::from_millis(500),
            ..OfferOptions::loopback()
        }
    }

    fn copts() -> ClientOptions {
        ClientOptions {
            allow_loopback: true,
            ..ClientOptions::default()
        }
    }

    async fn next(offer: &mut Offer) -> TransferStateKind {
        timeout(Duration::from_secs(5), offer.events.recv())
            .await
            .expect("durum zamanında gelmeli")
            .expect("kanal açık olmalı")
            .state
    }

    #[tokio::test]
    async fn uctan_uca_loopback() {
        let mut offer = start(paket(), opts()).await.unwrap();
        assert_eq!(offer.info.hosts, vec!["127.0.0.1"]);
        assert!(offer.info.port > 0);
        assert!(offer
            .info
            .text
            .starts_with("mailhere://aktar?v=1&h=127.0.0.1&p="));
        assert!(offer.info.qr.size >= 21);
        assert!(offer.info.expires_at > now_millis());
        assert_eq!(next(&mut offer).await, TransferStateKind::Waiting);

        let code = Code::parse(&offer.info.text).unwrap();
        let got = client::fetch(&code, &copts()).await.unwrap();
        assert_eq!(&got[..], &paket()[..]);
        assert_eq!(next(&mut offer).await, TransferStateKind::Connected);
        assert_eq!(next(&mut offer).await, TransferStateKind::Delivered);

        client::confirm(&code, ApplyStatus::Applied, &copts())
            .await
            .unwrap();
        let last = timeout(Duration::from_secs(5), offer.events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(last.state, TransferStateKind::Applied);
        assert!(last.message.is_none());
        // Kanal kapanır; kapı artık dinlemez.
        assert!(timeout(Duration::from_secs(5), offer.events.recv())
            .await
            .unwrap()
            .is_none());
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), offer.info.port);
        assert!(TcpStream::connect(addr).await.is_err());
    }

    #[tokio::test]
    async fn kismi_ve_basarisiz_onay() {
        for (status, kind, has_msg) in [
            (ApplyStatus::Partial, TransferStateKind::Applied, true),
            (ApplyStatus::Failed, TransferStateKind::Failed, true),
        ] {
            let mut offer = start(paket(), opts()).await.unwrap();
            let code = Code::parse(&offer.info.text).unwrap();
            client::fetch(&code, &copts()).await.unwrap();
            client::confirm(&code, status, &copts()).await.unwrap();
            let mut last = None;
            while let Some(s) = timeout(Duration::from_secs(5), offer.events.recv())
                .await
                .unwrap()
            {
                last = Some(s);
            }
            let last = last.unwrap();
            assert_eq!(last.state, kind);
            assert_eq!(last.message.is_some(), has_msg);
        }
    }

    #[tokio::test]
    async fn ikinci_istek_reddedilir() {
        let mut offer = start(paket(), opts()).await.unwrap();
        let code = Code::parse(&offer.info.text).unwrap();
        client::fetch(&code, &copts()).await.unwrap();
        // Aynı geçerli kodla ikinci istek: yanıtsız kapanış.
        let e = client::fetch(&code, &copts()).await.unwrap_err();
        assert!(matches!(e, TransferError::Protocol(_)), "{e:?}");
        assert!(e.to_string().contains("kabul etmedi"), "{e}");
        // Teklif hâlâ açık (onay bekliyor): durumlar waiting, connected, delivered.
        assert_eq!(next(&mut offer).await, TransferStateKind::Waiting);
        assert_eq!(next(&mut offer).await, TransferStateKind::Connected);
        assert_eq!(next(&mut offer).await, TransferStateKind::Delivered);
        client::confirm(&code, ApplyStatus::Applied, &copts())
            .await
            .unwrap();
        assert_eq!(next(&mut offer).await, TransferStateKind::Applied);
    }

    #[tokio::test]
    async fn yanlis_kod_sunucuda_reddedilir() {
        let mut offer = start(paket(), opts()).await.unwrap();
        let code = Code::parse(&offer.info.text).unwrap();
        // Yanlış K: aynı T ve kapı, başka anahtar.
        let mut wrong_k = Code::parse(&offer.info.text).unwrap();
        wrong_k.k[0] ^= 0x55;
        let e = client::fetch(&wrong_k, &copts()).await.unwrap_err();
        assert!(matches!(e, TransferError::Protocol(_)), "{e:?}");
        // Yanlış T: aynı K, başka T.
        let mut wrong_t = Code::parse(&offer.info.text).unwrap();
        wrong_t.t[5] ^= 0x55;
        let e = client::fetch(&wrong_t, &copts()).await.unwrap_err();
        assert!(matches!(e, TransferError::Protocol(_)), "{e:?}");
        // Teklif hâlâ bekliyor; doğru kod çalışır.
        assert_eq!(next(&mut offer).await, TransferStateKind::Waiting);
        let got = client::fetch(&code, &copts()).await.unwrap();
        assert_eq!(&got[..], &paket()[..]);
        assert_eq!(next(&mut offer).await, TransferStateKind::Connected);
        assert_eq!(next(&mut offer).await, TransferStateKind::Delivered);
        offer.cancel();
        assert_eq!(next(&mut offer).await, TransferStateKind::Cancelled);
    }

    #[tokio::test]
    async fn bes_hatali_deneme_kapatir() {
        let mut offer = start(paket(), opts()).await.unwrap();
        assert_eq!(next(&mut offer).await, TransferStateKind::Waiting);
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), offer.info.port);
        let code = Code::parse(&offer.info.text).unwrap();
        let mut wrong = Code::parse(&offer.info.text).unwrap();
        wrong.k[3] ^= 1;
        let keys = wrong.keys();
        for i in 0..5 {
            let mut s = TcpStream::connect(addr).await.unwrap();
            if i % 2 == 0 {
                // Yanlış anahtarla üretilmiş, biçimi doğru selam.
                let hello = build_hello(&keys, &code.t, &[i as u8; NONCE_C_LEN]);
                s.write_all(&hello).await.unwrap();
            } else {
                // Rastgele çöp.
                s.write_all(&[0x41u8; HELLO_LEN]).await.unwrap();
            }
            // Sunucu yanıt vermeden kapatır.
            let mut buf = [0u8; 1];
            let n = timeout(Duration::from_secs(5), s.read(&mut buf))
                .await
                .unwrap()
                .unwrap_or(0);
            assert_eq!(n, 0, "deneme {i}: yanıt olmamalı");
        }
        let last = timeout(Duration::from_secs(5), offer.events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(last.state, TransferStateKind::Failed);
        assert!(last.message.unwrap().contains("5 hatalı"));
        assert!(timeout(Duration::from_secs(5), offer.events.recv())
            .await
            .unwrap()
            .is_none());
        // Doğru kod artık işe yaramaz: kapı kapalı.
        assert!(client::fetch(&code, &copts()).await.is_err());
    }

    #[tokio::test]
    async fn sessiz_baglanti_telefonu_engellemez() {
        // Selam göndermeyen bağlantılar okuma zaman aşımında hatalı sayılır;
        // bu sırada gerçek telefon paketini alabilir (eşzamanlı işleme).
        let mut offer = start(paket(), opts()).await.unwrap();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), offer.info.port);
        let mut idle = Vec::new();
        for _ in 0..4 {
            idle.push(TcpStream::connect(addr).await.unwrap());
        }
        let code = Code::parse(&offer.info.text).unwrap();
        let got = client::fetch(&code, &copts()).await.unwrap();
        assert_eq!(&got[..], &paket()[..]);
        assert_eq!(next(&mut offer).await, TransferStateKind::Waiting);
        assert_eq!(next(&mut offer).await, TransferStateKind::Connected);
        assert_eq!(next(&mut offer).await, TransferStateKind::Delivered);
        // Dört sessiz bağlantı zaman aşımında sayıldı (4); beşinci hatalı
        // deneme teklifi kapatır.
        tokio::time::sleep(Duration::from_millis(800)).await;
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(&[0u8; CONFIRM_LEN]).await.unwrap();
        let last = timeout(Duration::from_secs(5), offer.events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(last.state, TransferStateKind::Failed);
        assert!(last.message.unwrap().contains("5 hatalı"));
        drop(idle);
    }

    #[tokio::test(start_paused = true)]
    async fn sure_dolunca_kapanir() {
        let mut offer = start(
            paket(),
            OfferOptions {
                ttl: Duration::from_secs(180),
                ..opts()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            offer.events.recv().await.unwrap().state,
            TransferStateKind::Waiting
        );
        // Zaman durdurulmuş; kimse bağlanmıyor → çalışma zamanı 180 s'yi atlar.
        let last = offer.events.recv().await.unwrap();
        assert_eq!(last.state, TransferStateKind::Expired);
        assert!(last.message.unwrap().contains("süresi doldu"));
        assert!(offer.events.recv().await.is_none());
    }

    #[tokio::test]
    async fn onay_gelmezse_sure_dolar() {
        let mut offer = start(
            paket(),
            OfferOptions {
                confirm_ttl: Duration::from_millis(300),
                ..opts()
            },
        )
        .await
        .unwrap();
        let code = Code::parse(&offer.info.text).unwrap();
        client::fetch(&code, &copts()).await.unwrap();
        assert_eq!(next(&mut offer).await, TransferStateKind::Waiting);
        assert_eq!(next(&mut offer).await, TransferStateKind::Connected);
        assert_eq!(next(&mut offer).await, TransferStateKind::Delivered);
        let last = timeout(Duration::from_secs(5), offer.events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(last.state, TransferStateKind::Expired);
        assert!(last.message.unwrap().contains("onay gelmedi"));
        // Geç onay artık reddedilir (kapı kapalı).
        assert!(client::confirm(&code, ApplyStatus::Applied, &copts())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn iptal_ve_tutamac_dusmesi() {
        let mut offer = start(paket(), opts()).await.unwrap();
        assert_eq!(next(&mut offer).await, TransferStateKind::Waiting);
        offer.cancel();
        assert_eq!(next(&mut offer).await, TransferStateKind::Cancelled);
        assert!(timeout(Duration::from_secs(5), offer.events.recv())
            .await
            .unwrap()
            .is_none());

        // Tutamaç düşünce de iptal.
        let offer = start(paket(), opts()).await.unwrap();
        let (info, handle, mut events) = offer.into_parts();
        assert_eq!(
            events.recv().await.unwrap().state,
            TransferStateKind::Waiting
        );
        drop(handle);
        assert_eq!(
            timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap()
                .state,
            TransferStateKind::Cancelled
        );
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), info.port);
        assert!(TcpStream::connect(addr).await.is_err());
    }

    #[tokio::test]
    async fn buyuk_paket_reddedilir() {
        let big = Zeroizing::new(vec![0u8; MAX_PACKAGE + 1]);
        let e = start(big, opts()).await.unwrap_err();
        assert!(matches!(e, TransferError::Internal(_)));
        assert!(e.to_string().contains("çok büyük"));
    }

    #[tokio::test]
    async fn genel_adres_duyurulmaz() {
        let e = start(
            paket(),
            OfferOptions {
                hosts: Some(vec!["8.8.8.8".parse().unwrap()]),
                ..opts()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(e, TransferError::Internal(_)));
        // Üretimde loopback duyurulmaz.
        let e = start(
            paket(),
            OfferOptions {
                hosts: Some(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
                allow_loopback: false,
                ..opts()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(e, TransferError::Internal(_)));
        // Hiç adres yoksa ağ hatası.
        let e = start(
            paket(),
            OfferOptions {
                hosts: Some(vec![]),
                ..opts()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(e, TransferError::Network(_)));
    }

    #[tokio::test]
    async fn debug_ciktilari_gizli_deger_icermez() {
        let offer = start(paket(), opts()).await.unwrap();
        let k = offer.info.text.rsplit("&k=").next().unwrap().to_string();
        let dbg = format!("{offer:?} {:?}", offer.handle);
        assert!(!dbg.contains(&k), "{dbg}");
        assert!(dbg.contains("<gizli>"));
        offer.cancel();
    }
}
