//! Hesap başına eşitleme motoru.
//!
//! Her IMAP hesabı için iki görev çalışır: komut bağlantısı (eşitleme ve
//! sunucu işlemleri, sırayla) ve INBOX üzerinde IDLE bekleyen ayrı bir
//! bağlantı. Bağlantı koparsa üstel bekleme ile yeniden denenir.
//!
//! Sunucu işlemleri (bayrak, taşıma, ekleme, silme) depodaki kalıcı kuyrukta
//! (`mh_store::Action`) bekler; uygulama kapanıp açılsa da kaybolmaz. Bağlanınca
//! önce kutular listelenir (LIST), sonra kuyruk baştan sona sırayla uygulanır,
//! ardından eşitleme çalışır. Bir postanın sunucudaki yeri işlem çalışırken
//! çözülür: yerel taşıma sunucu onaylayana kadar kaynak yeri (kutu, UID)
//! saklar, geri alınan taşıma sunucuya hiç gitmez.

mod drafts;
mod moves;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_imap::extensions::idle::IdleResponse;
use async_imap::types::{Fetch, Name};
use futures::TryStreamExt;
use mh_core::{
    Account, AccountId, AccountKind, AppError, AppResult, ErrorCode, MailboxRole, MessageId,
    MoveTarget, SyncState, SyncStatus,
};
use mh_store::{
    Action, MailboxInfo, MessageLocation, Namespace, PendingMove, QueuedAction, ServerFlag,
    ServerRef, Store,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Instant};

use crate::checked::{command_ok, uid_search};
use crate::conn::ImapSession;
use crate::credentials::{connect_account, TokenProvider};
use crate::error::{SyncError, SyncResult};
use crate::mailbox::{sync_mailbox, uid_set, OlderOutcome};
use crate::roles;
use crate::secrets::SecretStore;
use crate::EventSink;

const MIN_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// Bu kadar ayakta kalan oturum sağlıklı sayılır; kopunca bekleme baştan
/// başlar. Giriş yapılır yapılmaz kopan oturum (bağlantı sınırında BYE,
/// IDLE'ı saniyeler içinde kesen ara cihaz) beklemeyi büyütmeye devam eder;
/// yoksa sunucu 2 saniyede bir yoklanırdı.
const STABLE_SESSION: Duration = Duration::from_secs(60);
/// Tüm kutuların düzenli eşitlenmesi (aynı zamanda bağlantıyı canlı tutar).
/// Gelen kutusu dışındaki klasörlere yeni posta en geç bu kadar sonra gelir.
const PERIODIC: Duration = Duration::from_secs(5 * 60);
/// IDLE en geç bu sürede yenilenir (RFC 2177: 29 dakikadan kısa). Kısa
/// tutulur: sessizce kopan bağlantı en geç bu kadar sonra fark edilir (TCP
/// canlılık yoklaması işletim sisteminde kapalıysa da).
const IDLE_RENEW: Duration = Duration::from_secs(10 * 60);
/// Gelen kutusu yoklama aralığı. IDLE'siz sunucuda tek kaynak; IDLE varken
/// güvenlik ağı: sessizce kopan (ya da yeni postayı bildirmeyen) IDLE
/// bağlantısı yeni postayı en çok bu kadar geciktirir.
const POLL: Duration = Duration::from_secs(120);
const FULL_SYNC_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Tek kutu (gelen kutusu, işlem sonrası kutu) eşitlemesinin süre sınırı.
/// Sessizce ölmüş bağlantıda takılan yoklama yeni postayı tam eşitlemenin
/// 20 dakikasına kadar bekletmesin: aşılınca bağlantı baştan kurulur.
const QUICK_SYNC_TIMEOUT: Duration = Duration::from_secs(90);
const ACTION_TIMEOUT: Duration = Duration::from_secs(120);
/// Kalıcı silme ve kutu boşaltma binlerce postayı parça parça siler.
const PURGE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// "Daha eski postalar" isteğinin (kutu başına bir parti) süre sınırı.
const OLDER_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Tek komutta kalıcı silinen en fazla posta (komut satırı kısa kalsın).
const PURGE_CHUNK: usize = 500;
/// Bağlantı kopması yüzünden bu kadar kez yarıda kalan işlemden vazgeçilir
/// (kuyruğu sonsuza dek tıkamasın).
const ACTION_MAX_ATTEMPTS: u32 = 5;

/// Gizli olsa da eşitlenen kutu rolleri.
const SYNC_ROLES: [MailboxRole; 5] = [
    MailboxRole::Inbox,
    MailboxRole::Sent,
    MailboxRole::Drafts,
    MailboxRole::Archive,
    MailboxRole::Trash,
];

/// Kutu eşitlenir mi? Rol kutuları her zaman; Gereksiz ve diğer klasörler
/// kenar çubuğunda görünüyorsa. Eşitlenmeyen kutu (Gmail "Tüm Postalar")
/// hiçbir zaman. Bildirim yalnız gelen kutusundan çıkar.
fn syncs(info: &MailboxInfo) -> bool {
    !info.nosync && (SYNC_ROLES.contains(&info.mailbox.role) || !info.mailbox.hidden)
}

/// Tam eşitlemede kutu sırası: gelen kutusu, rol kutuları, sonra diğerleri.
fn sync_order(role: MailboxRole) -> u8 {
    match role {
        MailboxRole::Inbox => 0,
        r if SYNC_ROLES.contains(&r) => 1,
        _ => 2,
    }
}

/// Bellekteki komutlar. Sunucu işlemleri kalıcı kuyruktadır; `Wake` yalnız
/// kuyruğa yeni işlem yazıldığını haber verir.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    SyncAll,
    SyncInbox,
    /// Tek kutunun eşitlenmesi (taşıma ya da ekleme sonrası).
    SyncPath(String),
    Wake,
}

/// Komutu bekleyenlere ekler (`Wake` yalnız uyandırır).
fn enqueue(pending: &mut VecDeque<Command>, c: Command) {
    if c != Command::Wake {
        pending.push_back(c);
    }
}

struct AccountCtx {
    /// "Postaları al" (SyncAll) gelince IDLE döngüsünün beklemesini keser.
    wake: tokio::sync::Notify,
    account: Account,
    store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
    tokens: Arc<TokenProvider>,
    sink: Arc<dyn EventSink>,
}

impl AccountCtx {
    fn status(&self, state: SyncState) {
        self.sink.sync_status(SyncStatus {
            account_id: self.account.id.clone(),
            state,
        });
    }

    fn id(&self) -> &str {
        &self.account.id
    }

    /// Gelen posta sunucusu (sertifika onayı için).
    fn imap_endpoint(&self) -> (String, u16) {
        self.account
            .imap
            .as_ref()
            .map(|c| (c.host.trim().to_owned(), c.port))
            .unwrap_or_default()
    }

    async fn connect(&self) -> SyncResult<ImapSession> {
        let cfg = self
            .account
            .imap
            .clone()
            .ok_or_else(|| AppError::new(ErrorCode::Invalid, "Hesabın IMAP ayarı yok."))?;
        connect_account(&cfg, &self.secrets, &self.tokens, &self.account).await
    }

    fn emit_new(&self, ids: &[MessageId]) {
        if ids.is_empty() {
            return;
        }
        match self.store.summaries(ids) {
            Ok(list) if !list.is_empty() => self.sink.new_mail(&self.account, list),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "yeni posta özetleri okunamadı"),
        }
    }

    fn changed(&self, mut ids: Vec<String>) {
        ids.sort();
        ids.dedup();
        if !ids.is_empty() {
            self.sink.changed(ids);
        }
    }
}

struct Handle {
    tx: UnboundedSender<Command>,
    ctx: Arc<AccountCtx>,
    tasks: Vec<JoinHandle<()>>,
}

/// Tüm hesapların eşitlemesini yönetir.
pub struct SyncManager {
    store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
    /// OAuth hesaplarının erişim jetonu önbelleği (giden kutusu da kullanır).
    tokens: Arc<TokenProvider>,
    sink: Arc<dyn EventSink>,
    rt: tokio::runtime::Handle,
    accounts: Mutex<HashMap<AccountId, Handle>>,
}

/// Bu hesap gerçek bir IMAP sunucusuyla eşitlenir mi? (Tanıtım hesapları
/// yalnız yereldir; JMAP hesaplarını `mh-jmap` eşitler.)
pub fn is_syncable(account: &Account) -> bool {
    account.kind == AccountKind::Imap
        && account
            .imap
            .as_ref()
            .is_some_and(|c| !c.host.trim().to_ascii_lowercase().ends_with(".invalid"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Yeniden bağlanma beklemesine %0–25 rastgele pay ekler: ağ geri gelince
/// birden çok hesabın (ve aynı sunucuya giden komut + IDLE bağlantılarının)
/// hepsi aynı anda yüklenmesin. Harici kitaplık gerekmesin diye kaynak
/// saatin nanosaniyesidir; `max` aşılmaz.
fn jittered(backoff: Duration, max: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let pay = u64::from(nanos % 256);
    let extra = backoff.as_millis() as u64 * pay / 1024;
    (backoff + Duration::from_millis(extra)).min(max)
}

/// Kalıcı silme yalnız Çöp ve İstenmeyen kutularında yapılır.
fn is_purgeable(role: MailboxRole) -> bool {
    matches!(role, MailboxRole::Trash | MailboxRole::Junk)
}

fn not_purgeable() -> AppError {
    AppError::new(
        ErrorCode::Invalid,
        "Yalnız Çöp ya da İstenmeyen kutusundaki postalar kalıcı silinebilir. Postayı önce Çöp'e taşı.",
    )
}

/// Hesaba göre gruplanmış postalar: (hesap, eşitleniyor mu, yerler).
type ByAccount = Vec<(String, bool, Vec<MessageLocation>)>;

impl SyncManager {
    pub fn new(
        store: Arc<Store>,
        secrets: Arc<dyn SecretStore>,
        sink: Arc<dyn EventSink>,
        rt: tokio::runtime::Handle,
    ) -> Self {
        Self {
            store,
            secrets,
            tokens: Arc::new(TokenProvider::default()),
            sink,
            rt,
            accounts: Mutex::default(),
        }
    }

    /// Jeton sağlayıcısını değiştirir (testlerde sahte OAuth sunucusu).
    pub fn with_tokens(mut self, tokens: Arc<TokenProvider>) -> Self {
        self.tokens = tokens;
        self
    }

    /// OAuth erişim jetonu önbelleği.
    pub fn tokens(&self) -> &Arc<TokenProvider> {
        &self.tokens
    }

    fn handles(&self) -> std::sync::MutexGuard<'_, HashMap<AccountId, Handle>> {
        self.accounts.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Kayıtlı tüm hesapları başlatır.
    pub fn start_all(&self) -> AppResult<()> {
        for a in self.store.accounts()? {
            self.start(a);
        }
        Ok(())
    }

    /// Hesabın eşitleme görevlerini başlatır (zaten çalışıyorsa yeniden).
    pub fn start(&self, account: Account) {
        self.stop(&account.id);
        // JMAP hesabının durumunu JMAP yöneticisi bildirir; burada "çevrimiçi"
        // yayılsaydı yeniden başlatmada bir an yanlış durum görünürdü.
        if account.kind == AccountKind::Jmap {
            return;
        }
        if !is_syncable(&account) {
            self.sink.sync_status(SyncStatus {
                account_id: account.id.clone(),
                state: SyncState::Online,
            });
            return;
        }
        let (tx, rx) = unbounded_channel();
        let ctx = Arc::new(AccountCtx {
            wake: tokio::sync::Notify::new(),
            account: account.clone(),
            store: self.store.clone(),
            secrets: self.secrets.clone(),
            tokens: self.tokens.clone(),
            sink: self.sink.clone(),
        });
        let tasks = vec![
            self.rt.spawn(account_loop(ctx.clone(), rx)),
            self.rt.spawn(idle_loop(ctx.clone(), tx.clone())),
        ];
        self.handles().insert(account.id, Handle { tx, ctx, tasks });
    }

    /// Hesabın görevlerini durdurur. Kuyruktaki sunucu işlemleri kalır;
    /// hesap yeniden başlayınca uygulanır.
    pub fn stop(&self, account_id: &str) {
        if let Some(h) = self.handles().remove(account_id) {
            for t in h.tasks {
                t.abort();
            }
        }
    }

    /// Gelen posta sunucusu `host` olan hesapları yeniden başlatır (sertifika
    /// onaylandıktan sonra). Yeniden başlatılan hesap sayısını döner.
    pub fn restart_host(&self, host: &str) -> AppResult<usize> {
        let host = host.trim();
        let mut n = 0;
        for a in self.store.accounts()? {
            let uses = a
                .imap
                .as_ref()
                .is_some_and(|c| c.host.trim().eq_ignore_ascii_case(host));
            if uses {
                self.start(a);
                n += 1;
            }
        }
        Ok(n)
    }

    fn send(&self, account_id: &str, cmd: Command) -> bool {
        self.handles().get(account_id).is_some_and(|h| {
            // Elle eşitleme bekleyen IDLE bağlantısını da hemen yeniden kurar.
            if cmd == Command::SyncAll {
                h.ctx.wake.notify_one();
            }
            h.tx.send(cmd).is_ok()
        })
    }

    /// Tüm hesapları hemen eşitler (bekleyen yeniden bağlanmayı ve onay
    /// bekleyen sertifika durumunu da yeniden dener).
    pub fn sync_now(&self) {
        for h in self.handles().values() {
            let _ = h.tx.send(Command::SyncAll);
        }
    }

    /// Tek hesabı hemen eşitler (kutular yeniden listelenir). Hesap
    /// çalışmıyorsa `false`.
    pub fn sync_account(&self, account_id: &str) -> bool {
        self.send(account_id, Command::SyncAll)
    }

    /// Hesabın gelen kutusunu hemen eşitler (sunucu "yeni posta teslim
    /// edildi" dedi). Hesap çalışmıyorsa `false`.
    pub fn sync_inbox(&self, account_id: &str) -> bool {
        self.send(account_id, Command::SyncInbox)
    }

    /// Hesabın tek kutusunu hemen eşitler (ör. görünür yapılan klasör).
    /// Hesap çalışmıyorsa `false`.
    pub fn sync_path(&self, account_id: &str, path: &str) -> bool {
        self.send(account_id, Command::SyncPath(path.to_owned()))
    }

    /// İşlemi kalıcı kuyruğa yazar ve hesabın motorunu uyandırır.
    fn queue(&self, account_id: &str, action: &Action, raw: Option<&[u8]>) -> AppResult<()> {
        self.store.action_push(account_id, action, raw, now_ms())?;
        self.send(account_id, Command::Wake);
        Ok(())
    }

    /// Hesap sunucuyla eşitleniyor mu (tanıtım hesabı değil)?
    fn syncable(&self, account_id: &str) -> AppResult<bool> {
        Ok(self
            .store
            .account(account_id)?
            .is_some_and(|a| is_syncable(&a)))
    }

    fn by_account(&self, ids: &[MessageId]) -> AppResult<ByAccount> {
        let mut out: ByAccount = Vec::new();
        let mut syncable: HashMap<String, bool> = HashMap::new();
        for loc in self.store.locations(ids)? {
            let s = match syncable.get(&loc.account_id) {
                Some(s) => *s,
                None => {
                    let s = self
                        .store
                        .account(&loc.account_id)?
                        .is_some_and(|a| is_syncable(&a));
                    syncable.insert(loc.account_id.clone(), s);
                    s
                }
            };
            match out.iter_mut().find(|(a, ..)| *a == loc.account_id) {
                Some((_, _, list)) => list.push(loc),
                None => out.push((loc.account_id.clone(), s, vec![loc])),
            }
        }
        Ok(out)
    }

    /// Bayrakları önce yerelde değiştirir, sonra sunucu kuyruğuna yazar.
    /// Etkilenen kutuları döner.
    pub fn set_flags(
        &self,
        ids: &[MessageId],
        unread: Option<bool>,
        starred: Option<bool>,
    ) -> AppResult<Vec<String>> {
        let groups = self.by_account(ids)?;
        let touched = self.store.set_flags(ids, unread, starred)?;
        for (account, syncable, list) in groups {
            if !syncable {
                continue;
            }
            let ids: Vec<MessageId> = list.iter().map(|l| l.id).collect();
            for (flag, value) in [
                (ServerFlag::Seen, unread.map(|u| !u)),
                (ServerFlag::Flagged, starred),
            ] {
                if let Some(value) = value {
                    let action = Action::SetFlag {
                        ids: ids.clone(),
                        flag,
                        value,
                    };
                    self.queue(&account, &action, None)?;
                }
            }
        }
        Ok(touched)
    }

    /// Postaları önce yerelde taşır, sonra sunucu kuyruğuna yazar. Zaten
    /// hedefte olan postalara dokunulmaz; geri alma sunucuya gitmemiş taşımayı
    /// iptal eder.
    pub fn move_messages(&self, ids: &[MessageId], target: MoveTarget) -> AppResult<Vec<String>> {
        let role = match target {
            MoveTarget::Inbox => MailboxRole::Inbox,
            MoveTarget::Archive => MailboxRole::Archive,
            MoveTarget::Trash => MailboxRole::Trash,
        };
        let mut touched = Vec::new();
        for (account, syncable, list) in self.by_account(ids)? {
            let dest = self.destination(&account, role)?;
            let ids: Vec<MessageId> = list.iter().map(|l| l.id).collect();
            if syncable {
                touched.extend(self.store.move_local(&ids, &dest.mailbox.id)?);
                self.queue(&account, &Action::Moves, None)?;
            } else {
                touched.extend(self.store.move_local_untracked(&ids, &dest.mailbox.id)?);
            }
        }
        touched.sort();
        touched.dedup();
        Ok(touched)
    }

    /// Postaları **kalıcı** siler: yerelden hemen, sunucudan kalıcı kuyruk
    /// üzerinden (çevrimdışıyken de kaybolmaz; bağlanınca uygulanır). Yalnız
    /// Çöp ya da İstenmeyen kutusunda görünen postalar silinir: başka kutuda
    /// bir posta varsa hiçbirine dokunulmaz (`Invalid`). Artık var olmayan
    /// kimlikler sessizce atlanır. Etkilenen kutuları döner.
    ///
    /// Taşıması sunucuya henüz gitmemiş posta (ör. az önce silinip Çöp'e
    /// düşen) sunucuda hâlâ kaynak kutusundadır; orada yalnız kendi UID'siyle
    /// silinir (`apply_delete`).
    pub fn purge_messages(&self, ids: &[MessageId]) -> AppResult<Vec<String>> {
        let groups = self.by_account(ids)?;
        if groups
            .iter()
            .flat_map(|(_, _, list)| list)
            .any(|l| !is_purgeable(l.role))
        {
            return Err(not_purgeable());
        }
        let mut touched = Vec::new();
        for (account, syncable, list) in groups {
            if syncable {
                let items: Vec<ServerRef> = list
                    .iter()
                    .filter(|l| l.server_uid.is_some() || l.message_id_header.is_some())
                    .map(|l| ServerRef {
                        path: l.server_path.clone(),
                        uid: l.server_uid,
                        uidvalidity: l.server_uidvalidity,
                        message_id: l.message_id_header.clone(),
                        fallback_path: l.pending.then(|| l.mailbox_path.clone()),
                    })
                    .collect();
                // Önce kuyruk, sonra yerel silme: araya hata girerse posta
                // yerelde kalır (sunucudan silinecek ama listede duran posta,
                // listeden kalkıp sunucuda kalan postadan iyidir).
                for chunk in items.chunks(PURGE_CHUNK) {
                    let action = Action::Delete {
                        items: chunk.to_vec(),
                    };
                    self.store.action_push(&account, &action, None, now_ms())?;
                }
            }
            let ids: Vec<MessageId> = list.iter().map(|l| l.id).collect();
            self.store.delete_messages(&ids)?;
            if syncable {
                self.send(&account, Command::Wake);
            }
            touched.extend(list.into_iter().map(|l| l.mailbox_id));
        }
        touched.sort();
        touched.dedup();
        Ok(touched)
    }

    /// Çöp ya da İstenmeyen kutusunu boşaltır: yerelde görünen tüm postalar
    /// kalıcı silinir ve sunucuda kutunun yerelde hiç indirilmemiş eski
    /// postaları da (istek anındaki filigrana kadar) silinmek üzere kuyruğa
    /// yazılır. Filigranın üstündeki (son eşitlemeden sonra gelen, kullanıcının
    /// hiç görmediği) postaya dokunulmaz. Başka roldeki kutu
    /// reddedilir (`Invalid`). (yerelden silinen posta sayısı, etkilenen
    /// kutular) döner.
    pub fn empty_mailbox(&self, mailbox_id: &str) -> AppResult<(u32, Vec<String>)> {
        let info = self.store.mailbox(mailbox_id)?.ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "Klasör bulunamadı; sunucuda silinmiş ya da yeniden adlandırılmış olabilir.",
            )
        })?;
        if !is_purgeable(info.mailbox.role) {
            return Err(not_purgeable());
        }
        let ids = self.store.message_ids_in(mailbox_id)?;
        let mut touched = self.purge_messages(&ids)?;
        let account = &info.mailbox.account_id;
        // Kutu sunucuyla hiç eşitlenmediyse (yerelde yeni oluşturulmuş rol
        // kutusu) sunucuda silinecek bilinen bir aralık yoktur.
        if self.syncable(account)? && info.uidvalidity.is_some() && info.highest_uid > 0 {
            let action = Action::Empty {
                path: info.mailbox.path.clone(),
                upto: info.highest_uid,
                uidvalidity: info.uidvalidity,
                since: None,
            };
            self.queue(account, &action, None)?;
            if !touched.iter().any(|t| t == mailbox_id) {
                touched.push(mailbox_id.to_owned());
            }
        }
        Ok((ids.len() as u32, touched))
    }

    /// Kutunun bir sonraki eski partisini ayrı bir bağlantıyla indirir
    /// (`messages_older`; `crate::mailbox::fetch_older`). Eşitlenmeyen kutuda
    /// (tanıtım hesabı, gizli klasör, Gmail "Tüm Postalar") ve başına
    /// ulaşılmış kutuda sunucuya gidilmez. Bağlantı kurulamazsa `Network`.
    pub async fn fetch_older(&self, mailbox_id: &str) -> AppResult<OlderOutcome> {
        let info = self.store.mailbox(mailbox_id)?.ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "Klasör bulunamadı; sunucuda silinmiş ya da yeniden adlandırılmış olabilir.",
            )
        })?;
        if info.older_done || !syncs(&info) {
            return Ok(OlderOutcome::default());
        }
        let Some(account) = self
            .store
            .account(&info.mailbox.account_id)?
            .filter(is_syncable)
        else {
            return Ok(OlderOutcome::default());
        };
        // Kalıcı silinmesi sırada bekleyen postalar yeniden indirilmez; kutu
        // boşaltılıyorsa hiçbir şey indirilmez.
        let mut skip: HashSet<u32> = HashSet::new();
        for a in self.store.actions(&account.id)? {
            match a.action {
                Action::Delete { items } => skip.extend(
                    items
                        .into_iter()
                        .filter(|r| r.path == info.mailbox.path)
                        .filter_map(|r| r.uid),
                ),
                Action::Empty { path, .. } if path == info.mailbox.path => {
                    return Ok(OlderOutcome::default())
                }
                _ => {}
            }
        }
        let cfg = account
            .imap
            .clone()
            .ok_or_else(|| AppError::new(ErrorCode::Invalid, "Hesabın IMAP ayarı yok."))?;
        let work = async {
            let mut session = connect_account(&cfg, &self.secrets, &self.tokens, &account).await?;
            let out = crate::mailbox::fetch_older(&mut session, &self.store, &info, &skip).await;
            let _ = timeout(Duration::from_secs(5), session.logout()).await;
            out
        };
        let out = match timeout(OLDER_TIMEOUT, work).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(e.to_app()),
            Err(_) => return Err(SyncError::Timeout.to_app()),
        };
        if out.fetched == 0 && out.more {
            // Kutu henüz eşitlenmemiş (ya da UIDVALIDITY değişmiş): önce
            // olağan eşitleme kursun.
            self.sync_path(&account.id, &info.mailbox.path);
        }
        Ok(out)
    }

    /// Roldeki kutu; yoksa yerelde oluşturulur (sunucuda ilk kullanımda
    /// açılır). Yeni kutu sunucunun kişisel ad alanına konur ("INBOX.Archive").
    pub fn destination(&self, account: &str, role: MailboxRole) -> AppResult<MailboxInfo> {
        if let Some(mb) = self.store.mailbox_by_role(account, role)? {
            return Ok(mb);
        }
        let name = match role {
            MailboxRole::Inbox => "INBOX",
            MailboxRole::Archive => "Archive",
            MailboxRole::Trash => "Trash",
            MailboxRole::Sent => "Sent",
            MailboxRole::Drafts => "Drafts",
            MailboxRole::Junk => "Junk",
            MailboxRole::Other => "Mailhere",
        };
        let path = match role {
            MailboxRole::Inbox => name.to_owned(),
            _ => {
                let ns = self.store.namespace(account)?.unwrap_or_default();
                format!("{}{name}", ns.prefix)
            }
        };
        let id = self
            .store
            .upsert_mailbox(account, &path, name, role, None, false)?;
        self.store
            .mailbox(&id)?
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "Kutu oluşturulamadı."))
    }

    /// Ham postayı hesabın verilen roldeki kutusuna ekler (ör. Gönderilenler).
    /// Kalıcı kuyruğa yazıldıysa `true` (eşitlenmeyen hesapta `false`).
    pub fn append(&self, account_id: &str, role: MailboxRole, raw: Vec<u8>) -> AppResult<bool> {
        if !self.syncable(account_id)? {
            return Ok(false);
        }
        let dest = self.destination(account_id, role)?;
        let message_id = mh_mime::parse(&raw, true).and_then(|p| p.message_id);
        let action = Action::Append {
            path: dest.mailbox.path,
            message_id,
        };
        self.queue(account_id, &action, Some(&raw))?;
        Ok(true)
    }

    /// Yalnız başlığı indirilmiş postanın gövdesini ayrı bir bağlantıyla indirir.
    pub async fn fetch_body(&self, id: MessageId) -> AppResult<()> {
        fetch_body(&self.store, &self.secrets, &self.tokens, id).await
    }
}

/// Gövdeyi indirip depoya yazar. Zaten varsa hiçbir şey yapmaz.
pub async fn fetch_body(
    store: &Store,
    secrets: &Arc<dyn SecretStore>,
    tokens: &TokenProvider,
    id: MessageId,
) -> AppResult<()> {
    let loc = store.location(id)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Posta bulunamadı; silinmiş ya da taşınmış olabilir.",
        )
    })?;
    if loc.has_raw {
        return Ok(());
    }
    let account = store
        .account(&loc.account_id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Postanın hesabı bulunamadı."))?;
    if !is_syncable(&account) {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "Bu postanın içeriği yerelde yok.",
        ));
    }
    if loc.server_uid.is_none() && loc.message_id_header.is_none() {
        return Err(AppError::new(
            ErrorCode::Network,
            "Posta henüz sunucuyla eşitlenmedi. Birazdan yeniden dene.",
        ));
    }
    let cfg = account
        .imap
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::Invalid, "Hesabın IMAP ayarı yok."))?;
    let work = async {
        let mut session = connect_account(&cfg, secrets, tokens, &account).await?;
        // Taşıması bekleyen posta sunucuda hâlâ kaynak kutudadır.
        let mb = session.examine(&loc.server_path).await?;
        let uid = match loc.server_uid {
            Some(uid)
                if loc.server_uidvalidity.is_none()
                    || mb.uid_validity == loc.server_uidvalidity =>
            {
                Some(uid)
            }
            _ => match loc.message_id_header.as_deref() {
                Some(mid) => search_message_id(&mut session, mid).await?,
                None => None,
            },
        };
        let Some(uid) = uid else {
            return Ok(None);
        };
        let fetches: Vec<Fetch> = session
            .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
            .await?
            .try_collect()
            .await?;
        let raw = fetches
            .iter()
            .find(|f| f.uid == Some(uid))
            .and_then(|f| f.body())
            .map(<[u8]>::to_vec);
        let _ = timeout(Duration::from_secs(5), session.logout()).await;
        Ok::<_, SyncError>(raw)
    };
    let raw = match timeout(Duration::from_secs(90), work).await {
        Ok(Ok(Some(raw))) => raw,
        Ok(Ok(None)) => {
            return Err(AppError::new(
                ErrorCode::NotFound,
                "Posta sunucuda bulunamadı; başka bir yerden silinmiş olabilir.",
            ))
        }
        Ok(Err(e)) => return Err(e.to_app()),
        Err(_) => return Err(SyncError::Timeout.to_app()),
    };
    // Ayrıştırma (HTML → metin) işlemciye bağlıdır; büyük ya da kötü niyetli
    // bir posta eşzamansız iş parçacığını bloklamasın.
    let (raw, parsed) = tokio::task::spawn_blocking(move || {
        let parsed = mh_mime::parse(&raw, false);
        (raw, parsed)
    })
    .await
    .map_err(|_| AppError::new(ErrorCode::Internal, "Posta ayrıştırılamadı."))?;
    store.store_parsed_body(id, &raw, parsed)
}

/// IMAP SEARCH için tırnaklı dize; yalnız yazdırılabilir ASCII kabul edilir.
fn imap_quote(s: &str) -> Option<String> {
    if s.is_empty() || s.len() > 900 || !s.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        return None;
    }
    Some(format!(
        "\"{}\"",
        s.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Seçili kutuda Message-ID başlığıyla arar.
async fn search_message_id(session: &mut ImapSession, message_id: &str) -> SyncResult<Option<u32>> {
    let mid = message_id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>');
    let Some(q) = imap_quote(&format!("<{mid}>")) else {
        return Ok(None);
    };
    let found = session.uid_search(format!("HEADER Message-ID {q}")).await?;
    Ok(found.into_iter().min())
}

// ------------------------------------------------------------ komut bağlantısı

struct Conn {
    session: ImapSession,
    has_move: bool,
    has_uidplus: bool,
    /// Son LIST yanıtındaki kutular.
    server_paths: HashSet<String>,
    /// Bağlantı açılırken LIST yapıldı; ilk tam eşitleme yeniden listelemez.
    listed: bool,
}

async fn account_loop(ctx: Arc<AccountCtx>, mut rx: UnboundedReceiver<Command>) {
    let mut pending: VecDeque<Command> = VecDeque::from([Command::SyncAll]);
    let mut backoff = MIN_BACKOFF;
    loop {
        let opened = Instant::now();
        let err = match run_session(&ctx, &mut rx, &mut pending).await {
            Ok(()) => return,
            Err(e) => e,
        };
        let app = err.to_app();
        tracing::warn!(account = %ctx.account.id, error = %err, "eşitleme bağlantısı kesildi");
        // Güvenilmeyen sertifika: kullanıcı onaylayana ya da elle
        // eşitleyene kadar yeniden denenmez.
        let wait_user = app.code == ErrorCode::Untrusted;
        ctx.status(match app.code {
            ErrorCode::Network => SyncState::Offline,
            ErrorCode::Untrusted => {
                let (host, port) = ctx.imap_endpoint();
                SyncState::Untrusted { host, port }
            }
            _ => SyncState::Error {
                message: app.message,
            },
        });
        if opened.elapsed() >= STABLE_SESSION {
            backoff = MIN_BACKOFF;
        }
        let sleep = tokio::time::sleep(jittered(backoff, MAX_BACKOFF));
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep, if !wait_user => break,
                cmd = rx.recv() => match cmd {
                    None => return,
                    Some(Command::SyncAll) => break,
                    Some(c) => enqueue(&mut pending, c),
                },
            }
        }
        if !pending.iter().any(|c| *c == Command::SyncAll) {
            pending.push_front(Command::SyncAll);
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Sıradaki eşitleme: önce işlem sonrası tek kutular, sonra tam eşitleme
/// (gelen kutusu isteklerini de kapsar), sonra gelen kutusu. Yinelenenler
/// birleştirilir.
fn next_sync(pending: &mut VecDeque<Command>) -> Option<Command> {
    if let Some(i) = pending
        .iter()
        .position(|c| matches!(c, Command::SyncPath(_)))
    {
        let cmd = pending.remove(i)?;
        pending.retain(|c| *c != cmd);
        return Some(cmd);
    }
    let pick = [Command::SyncAll, Command::SyncInbox]
        .into_iter()
        .find(|c| pending.contains(c));
    match pick {
        Some(Command::SyncAll) => {
            pending.retain(|c| !matches!(c, Command::SyncAll | Command::SyncInbox));
            Some(Command::SyncAll)
        }
        Some(c) => {
            pending.retain(|x| *x != c);
            Some(c)
        }
        None => {
            pending.clear();
            None
        }
    }
}

async fn run_session(
    ctx: &AccountCtx,
    rx: &mut UnboundedReceiver<Command>,
    pending: &mut VecDeque<Command>,
) -> SyncResult<()> {
    if pending.contains(&Command::SyncAll) {
        ctx.status(SyncState::Syncing { done: 0, total: 0 });
    }
    let mut session = ctx.connect().await?;
    let caps = timeout(ACTION_TIMEOUT, session.capabilities()).await??;
    let mut conn = Conn {
        has_move: caps.has_str("MOVE"),
        has_uidplus: caps.has_str("UIDPLUS"),
        session,
        server_paths: HashSet::new(),
        listed: false,
    };
    // Kutular işlemlerden önce: hedef kutular, roller ve eşitlenmeyen kutular
    // (Gmail "Tüm Postalar") bilinsin.
    timeout(ACTION_TIMEOUT, refresh_mailboxes(ctx, &mut conn)).await??;
    // Kuyruğu düşmüş ama sunucuya gitmemiş taşımalar (güvenlik ağı).
    if ctx.store.has_pending_moves(ctx.id())? {
        ctx.store
            .action_push(ctx.id(), &Action::Moves, None, now_ms())?;
    }
    let mut periodic = tokio::time::interval_at(Instant::now() + PERIODIC, PERIODIC);
    periodic.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut poll = tokio::time::interval_at(Instant::now() + POLL, POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        while let Ok(c) = rx.try_recv() {
            enqueue(pending, c);
        }
        run_actions(ctx, &mut conn, rx, pending).await?;
        if let Some(cmd) = next_sync(pending) {
            let limit = if cmd == Command::SyncAll {
                FULL_SYNC_TIMEOUT
            } else {
                QUICK_SYNC_TIMEOUT
            };
            let res = match timeout(limit, execute_sync(ctx, &mut conn, &cmd)).await {
                Ok(r) => r,
                Err(_) => Err(SyncError::Timeout),
            };
            if let Err(e) = res {
                if e.is_connection() {
                    pending.push_front(cmd);
                    return Err(e);
                }
                tracing::warn!(account = %ctx.account.id, error = %e, "eşitleme başarısız");
                ctx.status(SyncState::Error {
                    message: e.to_app().message,
                });
            }
            continue;
        }
        tokio::select! {
            cmd = rx.recv() => match cmd {
                None => {
                    let _ = timeout(Duration::from_secs(5), conn.session.logout()).await;
                    return Ok(());
                }
                Some(c) => enqueue(pending, c),
            },
            _ = periodic.tick() => pending.push_back(Command::SyncAll),
            // Gelen kutusu IDLE'den bağımsız da yoklanır: IDLE kurulamasa,
            // sessizce kopsa ya da sunucu desteklemese de yeni posta beklemesin.
            _ = poll.tick() => enqueue(pending, Command::SyncInbox),
        }
    }
}

/// İşlemin süre sınırı: ekleme boyutla uzar (yavaş bağlantıda büyük ek).
fn action_timeout(a: &QueuedAction) -> Duration {
    if matches!(a.action, Action::Delete { .. } | Action::Empty { .. }) {
        return PURGE_TIMEOUT;
    }
    let size = a.raw.as_ref().map_or(0, Vec::len) as u64;
    ACTION_TIMEOUT + Duration::from_secs(size / 20_000)
}

/// Kalıcı kuyruğu baştan sona uygular. Her işlem sunucu onayından sonra
/// silinir; bağlantı koparsa kalır ve yeniden bağlanınca tekrar denenir.
async fn run_actions(
    ctx: &AccountCtx,
    conn: &mut Conn,
    rx: &mut UnboundedReceiver<Command>,
    pending: &mut VecDeque<Command>,
) -> SyncResult<()> {
    while let Some(qa) = ctx.store.action_head(ctx.id())? {
        // Ekleme başlamadan deneme sayılır: görev ortada iptal edilirse
        // (uyandırma, yeniden bağlanma) sonraki oturum sunucuda Message-ID ile
        // arar, posta Gönderilenler'e iki kez yazılmaz.
        let started = if matches!(qa.action, Action::Append { .. }) {
            Some(ctx.store.action_attempt(qa.id)?)
        } else {
            None
        };
        let res = match timeout(action_timeout(&qa), run_action(ctx, conn, &qa)).await {
            Ok(r) => r,
            Err(_) => Err(SyncError::Timeout),
        };
        match res {
            Ok(follow) => {
                ctx.store.action_done(qa.id)?;
                // Kutu eşitlemesi ayrı bir adımdır: zaman aşımı işlemi
                // (ör. eklemeyi) yinelemez.
                for path in follow {
                    let cmd = Command::SyncPath(path);
                    if !pending.contains(&cmd) {
                        pending.push_front(cmd);
                    }
                }
            }
            Err(e) if e.is_connection() => {
                let attempts = match started {
                    Some(n) => n,
                    None => ctx.store.action_attempt(qa.id)?,
                };
                if attempts >= ACTION_MAX_ATTEMPTS {
                    tracing::warn!(account = %ctx.account.id, action = action_kind(&qa.action), "sunucu işleminden vazgeçildi");
                    ctx.store.action_done(qa.id)?;
                    for path in purge_abandoned(ctx, &qa.action)? {
                        enqueue(pending, Command::SyncPath(path));
                    }
                }
                return Err(e);
            }
            Err(e) => {
                tracing::warn!(account = %ctx.account.id, error = %e, "sunucu işlemi başarısız");
                ctx.store.action_done(qa.id)?;
                for path in purge_abandoned(ctx, &qa.action)? {
                    let cmd = Command::SyncPath(path);
                    if !pending.contains(&cmd) {
                        pending.push_front(cmd);
                    }
                }
                ctx.status(SyncState::Error {
                    message: e.to_app().message,
                });
            }
        }
        while let Ok(c) = rx.try_recv() {
            enqueue(pending, c);
        }
    }
    Ok(())
}

/// Günlük için işlemin türü: içeriği (kutu yolu, Message-ID, yüzlerce posta
/// yeri) yazılmaz.
fn action_kind(action: &Action) -> &'static str {
    match action {
        Action::SetFlag { .. } => "bayrak",
        Action::Moves => "taşıma",
        Action::Append { .. } => "ekleme",
        Action::Delete { .. } => "kalıcı silme",
        Action::Draft { .. } => "taslak",
        Action::Empty { .. } => "kutu boşaltma",
        Action::Purge { .. } => "jmap silme",
    }
}

/// Kalıcı silme ya da kutu boşaltma sunucuda uygulanamadı (sunucu reddetti
/// ya da bağlantı art arda koptu) ve işlemden vazgeçildi: postalar yerelden
/// silinmişti ama sunucuda duruyor. Sessizce kaybolmasınlar diye ilgili Çöp /
/// İstenmeyen kutuları baştan (ilk eşitleme gibi, bildirimsiz) eşitlenir ve
/// "daha eskisi yok" işaretleri kalkar: sunucuda kalan postalar listeye geri
/// döner. Yeniden eşitlenecek kutu yollarını döner.
fn purge_abandoned(ctx: &AccountCtx, action: &Action) -> SyncResult<Vec<String>> {
    let mut paths: Vec<&str> = match action {
        Action::Delete { items } => items
            .iter()
            .flat_map(|r| [Some(r.path.as_str()), r.fallback_path.as_deref()])
            .flatten()
            .collect(),
        Action::Empty { path, .. } => vec![path.as_str()],
        _ => return Ok(Vec::new()),
    };
    paths.sort_unstable();
    paths.dedup();
    let mut resync = Vec::new();
    for path in paths {
        let id = mh_store::mailbox_id(ctx.id(), path);
        let Some(info) = ctx.store.mailbox(&id)? else {
            continue;
        };
        ctx.store.set_mailbox_older_done(&id, false)?;
        // Gelen kutusu gibi bir kaynak kutunun filigranı sıfırlanmaz: o an
        // gelen yeni postanın bildirimi kaybolmasın.
        if is_purgeable(info.mailbox.role) {
            ctx.store
                .set_mailbox_sync_state(&id, info.uidvalidity, info.uidnext, 0)?;
            resync.push(path.to_owned());
        }
    }
    Ok(resync)
}

/// Tek işlemi uygular; ardından eşitlenecek kutuları döner.
async fn run_action(
    ctx: &AccountCtx,
    conn: &mut Conn,
    qa: &QueuedAction,
) -> SyncResult<Vec<String>> {
    match &qa.action {
        Action::SetFlag { ids, flag, value } => {
            apply_flag(ctx, conn, qa.id, ids, *flag, *value).await?;
            Ok(Vec::new())
        }
        Action::Moves => apply_moves(ctx, conn).await,
        Action::Append { path, message_id } => {
            let Some(raw) = qa.raw.as_deref() else {
                tracing::warn!("eklenecek postanın içeriği yok, atlandı");
                return Ok(Vec::new());
            };
            // Yarıda kalan denemeden sonra: sunucu postayı almışsa yinelenmez.
            if qa.attempts > 0 && conn.server_paths.contains(path) {
                if let Some(mid) = message_id {
                    if try_select(conn, path).await?.is_some()
                        && search_message_id(&mut conn.session, mid).await?.is_some()
                    {
                        return Ok(vec![path.clone()]);
                    }
                }
            }
            ensure_mailbox(conn, path).await?;
            conn.session
                .append(path, Some("(\\Seen)"), None, raw)
                .await?;
            Ok(vec![path.clone()])
        }
        Action::Delete { items } => apply_delete(ctx, conn, items).await,
        Action::Empty {
            path,
            upto,
            uidvalidity,
            ..
        } => apply_empty(ctx, conn, path, *upto, *uidvalidity).await,
        Action::Draft { path, remove } => {
            drafts::apply(conn, path, qa.raw.as_deref(), remove).await
        }
        // JMAP hesaplarının işlemi; IMAP kuyruğunda işi yoktur.
        Action::Purge { .. } => {
            tracing::warn!(account = %ctx.account.id, "JMAP silme işlemi IMAP kuyruğunda, atlandı");
            Ok(Vec::new())
        }
    }
}

async fn execute_sync(ctx: &AccountCtx, conn: &mut Conn, cmd: &Command) -> SyncResult<()> {
    match cmd {
        Command::SyncAll => full_sync(ctx, conn).await,
        Command::SyncInbox => {
            if let Some(info) = ctx.store.mailbox_by_role(ctx.id(), MailboxRole::Inbox)? {
                sync_one(ctx, conn, &info).await?;
            }
            Ok(())
        }
        Command::SyncPath(path) => sync_path(ctx, conn, path).await,
        Command::Wake => Ok(()),
    }
}

/// LIST sonucundan bir kutu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub path: String,
    pub delimiter: Option<String>,
    pub attr_role: Option<MailboxRole>,
    pub all: bool,
    pub noselect: bool,
    /// Başka kutuların kopyası (`\Flagged`, `\Important`).
    pub copy: bool,
}

impl Listed {
    fn from_name(n: &Name) -> Self {
        Self {
            path: n.name().to_owned(),
            delimiter: n.delimiter().map(str::to_owned),
            attr_role: roles::from_attributes(n.attributes()),
            all: roles::is_all(n.attributes()),
            noselect: roles::is_noselect(n.attributes()),
            copy: roles::is_copy(n.attributes()),
        }
    }
}

/// LIST sonucundan yerel kutu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assigned {
    pub path: String,
    /// Görünen ad: yaprak ad ya da seçilemeyen atalarla "Projeler / 2024".
    pub name: String,
    /// Sunucudan bulunan (otomatik) rol.
    pub role: MailboxRole,
    /// Hedef olarak kullanılır ama eşitlenmez (Gmail "Tüm Postalar").
    pub nosync: bool,
    /// Seçilebilir en yakın üst klasörün yolu; kökteyse ya da rol kutusuysa `None`.
    pub parent: Option<String>,
    /// Yalnız ilk eklemede uygulanan varsayılan gizlilik.
    pub hidden: bool,
}

/// Kutuların rolleri, ağaçtaki yerleri ve varsayılan gizliliği. Seçilemeyen
/// kutular atlanır.
///
/// Varsayılan gizli: kopya kutular (`\Flagged`, `\Important`), arşiv olarak
/// kullanılmayan `\All` ve eşitlenmeyen her kutu. Diğer klasörler görünür.
pub fn assign_roles(listed: &[Listed]) -> Vec<Assigned> {
    let selectable: Vec<&Listed> = listed.iter().filter(|l| !l.noselect).collect();
    let mut role_of: HashMap<&str, (MailboxRole, bool)> = HashMap::new();
    let mut taken: HashSet<MailboxRole> = HashSet::new();
    // 1) INBOX ve SPECIAL-USE öznitelikleri
    for l in &selectable {
        let role = if l.path.eq_ignore_ascii_case("INBOX") {
            Some(MailboxRole::Inbox)
        } else {
            l.attr_role
        };
        if let Some(r) = role.filter(|r| !taken.contains(r)) {
            taken.insert(r);
            role_of.insert(&l.path, (r, false));
        }
    }
    // 2) Arşivi olmayan sunucuda \All (Gmail): arşiv hedefi, eşitlenmez
    if !taken.contains(&MailboxRole::Archive) {
        if let Some(l) = selectable
            .iter()
            .find(|l| l.all && !role_of.contains_key(l.path.as_str()))
        {
            taken.insert(MailboxRole::Archive);
            role_of.insert(&l.path, (MailboxRole::Archive, true));
        }
    }
    // 3) Ad tahmini
    for l in &selectable {
        if role_of.contains_key(l.path.as_str()) {
            continue;
        }
        if let Some(r) =
            roles::from_name(&l.path, l.delimiter.as_deref()).filter(|r| !taken.contains(r))
        {
            taken.insert(r);
            role_of.insert(&l.path, (r, false));
        }
    }
    // 4) Ağaç: kişisel ad alanı "INBOX." ise altındaki klasörler köktedir.
    let prefix = personal_namespace(listed).prefix;
    let paths: HashSet<&str> = selectable.iter().map(|l| l.path.as_str()).collect();
    selectable
        .iter()
        .map(|l| {
            let (role, nosync) = role_of
                .get(l.path.as_str())
                .copied()
                .unwrap_or((MailboxRole::Other, false));
            let (parent, name) = if role == MailboxRole::Other {
                roles::place(&l.path, l.delimiter.as_deref(), &prefix, |p| {
                    paths.contains(p)
                })
            } else {
                (None, roles::leaf(&l.path, l.delimiter.as_deref()))
            };
            Assigned {
                path: l.path.clone(),
                name,
                role,
                nosync,
                parent,
                hidden: nosync || l.copy || (l.all && role != MailboxRole::Archive),
            }
        })
        .collect()
}

/// Yeni kutuların açılacağı kişisel ad alanı. INBOX dışındaki tüm kutular
/// "INBOX<ayraç>" altındaysa (Courier, Cyrus, "INBOX." önekli Dovecot) önek
/// odur; değilse kök. (`async-imap` NAMESPACE komutunu bilmez; LIST yeterli.)
pub fn personal_namespace(listed: &[Listed]) -> Namespace {
    let delimiter = listed
        .iter()
        .find(|l| l.path.eq_ignore_ascii_case("INBOX"))
        .and_then(|l| l.delimiter.clone())
        .or_else(|| listed.iter().find_map(|l| l.delimiter.clone()))
        .filter(|d| !d.is_empty());
    let prefix = match delimiter.as_deref() {
        Some(d) => {
            let under = format!("INBOX{d}");
            let mut others = listed
                .iter()
                .filter(|l| !l.path.eq_ignore_ascii_case("INBOX"))
                .peekable();
            let nested = others.peek().is_some()
                && others.all(|l| {
                    l.path.len() > under.len()
                        && l.path
                            .get(..under.len())
                            .is_some_and(|p| p.eq_ignore_ascii_case(&under))
                });
            if nested {
                under
            } else {
                String::new()
            }
        }
        None => String::new(),
    };
    Namespace { prefix, delimiter }
}

/// LIST: kutuları, rolleri, eşitlenmeyen kutuları ve ad alanını depoya yazar.
async fn refresh_mailboxes(ctx: &AccountCtx, conn: &mut Conn) -> SyncResult<()> {
    let account = ctx.id();
    let names: Vec<Name> = conn
        .session
        .list(Some(""), Some("*"))
        .await?
        .try_collect()
        .await?;
    let listed: Vec<Listed> = names.iter().map(Listed::from_name).collect();
    let assigned = assign_roles(&listed);
    conn.server_paths = listed.iter().map(|l| l.path.clone()).collect();
    // Adı, rolü ya da üst klasörü değişen (ya da yeni) kutular arayüze bildirilir.
    let shape = |m: &MailboxInfo| {
        (
            m.mailbox.id.clone(),
            m.mailbox.name.clone(),
            m.mailbox.role,
            m.mailbox.parent_id.clone(),
        )
    };
    let before: HashSet<_> = ctx
        .store
        .mailboxes_for(account)?
        .iter()
        .map(shape)
        .collect();
    let mut keep = Vec::new();
    for a in &assigned {
        let parent = a
            .parent
            .as_deref()
            .map(|p| mh_store::mailbox_id(account, p));
        let id = ctx.store.upsert_mailbox(
            account,
            &a.path,
            &a.name,
            a.role,
            parent.as_deref(),
            a.hidden,
        )?;
        ctx.store.set_mailbox_nosync(&id, a.nosync)?;
        keep.push(a.path.clone());
    }
    // Yerelde oluşturulup sunucuda henüz açılmamış rol kutularını koru.
    for info in ctx.store.mailboxes_for(account)? {
        if !keep.contains(&info.mailbox.path)
            && info.mailbox.total > 0
            && info.mailbox.role != MailboxRole::Other
        {
            keep.push(info.mailbox.path);
        }
    }
    let mut changed = ctx.store.remove_mailboxes_except(account, &keep)?;
    for m in ctx.store.mailboxes_for(account)? {
        if !before.contains(&shape(&m)) {
            changed.push(m.mailbox.id);
        }
    }
    ctx.store
        .set_namespace(account, &personal_namespace(&listed))?;
    ctx.changed(changed);
    conn.listed = true;
    Ok(())
}

async fn full_sync(ctx: &AccountCtx, conn: &mut Conn) -> SyncResult<()> {
    if !std::mem::take(&mut conn.listed) {
        refresh_mailboxes(ctx, conn).await?;
        conn.listed = false;
    }
    let mut targets: Vec<MailboxInfo> = ctx
        .store
        .mailboxes_for(ctx.id())?
        .into_iter()
        .filter(syncs)
        .filter(|m| conn.server_paths.contains(&m.mailbox.path))
        .collect();
    // Önce gelen kutusu, sonra rol kutuları, sonra klasörler (yol sırasıyla).
    targets.sort_by_key(|m| sync_order(m.mailbox.role));
    let total = targets.len() as u32;
    let started = Instant::now();
    let mut changed: Vec<String> = Vec::new();
    let mut new_unread = Vec::new();
    for (i, info) in targets.iter().enumerate() {
        ctx.status(SyncState::Syncing {
            done: i as u32,
            total,
        });
        let notify = info.mailbox.role == MailboxRole::Inbox;
        // Tek kutunun hatası (ör. izin) diğerlerini durdurmasın; kopan bağlantı durdurur.
        let out = match sync_mailbox(&mut conn.session, &ctx.store, info, notify).await {
            Ok(out) => out,
            Err(e) if !e.is_connection() => {
                tracing::warn!(mailbox = %info.mailbox.path, error = %e, "kutu eşitlenemedi");
                continue;
            }
            Err(e) => return Err(e),
        };
        if out.changed {
            changed.push(info.mailbox.id.clone());
        }
        new_unread.extend(out.new_unread);
    }
    // Günlükte süre: yavaş sunucu ya da büyük kutu şikayetinde ilk bakılacak yer.
    tracing::info!(
        account = %ctx.account.id,
        mailboxes = total,
        changed = changed.len(),
        new = new_unread.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "tam eşitleme bitti"
    );
    ctx.status(SyncState::Online);
    ctx.changed(changed);
    ctx.emit_new(&new_unread);
    Ok(())
}

async fn sync_one(ctx: &AccountCtx, conn: &mut Conn, info: &MailboxInfo) -> SyncResult<()> {
    if !syncs(info) || !conn.server_paths.contains(&info.mailbox.path) {
        return Ok(());
    }
    let notify = info.mailbox.role == MailboxRole::Inbox;
    let out = sync_mailbox(&mut conn.session, &ctx.store, info, notify).await?;
    if out.changed {
        ctx.changed(vec![info.mailbox.id.clone()]);
    }
    ctx.emit_new(&out.new_unread);
    Ok(())
}

async fn sync_path(ctx: &AccountCtx, conn: &mut Conn, path: &str) -> SyncResult<()> {
    let id = mh_store::mailbox_id(ctx.id(), path);
    if let Some(info) = ctx.store.mailbox(&id)? {
        sync_one(ctx, conn, &info).await?;
    }
    Ok(())
}

async fn ensure_mailbox(conn: &mut Conn, path: &str) -> SyncResult<()> {
    if conn.server_paths.contains(path) {
        return Ok(());
    }
    match conn.session.create(path).await {
        Ok(()) => {}
        // Zaten varsa sunucu NO döner; sorun değil. Açılamadıysa taşıma
        // TRYCREATE ile reddedilir ve geri alınır.
        Err(async_imap::error::Error::No(m)) => {
            tracing::debug!(path, reply = %m, "kutu oluşturulamadı ya da zaten var")
        }
        Err(e) => return Err(e.into()),
    }
    conn.server_paths.insert(path.to_owned());
    Ok(())
}

/// Kutuyu yazılabilir seçer; UIDVALIDITY değerini döner.
async fn select(conn: &mut Conn, path: &str) -> SyncResult<Option<u32>> {
    Ok(conn.session.select(path).await?.uid_validity)
}

/// Seçimi dener; sunucu reddederse (kutu yok, izin) `None`, bağlantı
/// koptuysa hata.
async fn try_select(conn: &mut Conn, path: &str) -> SyncResult<Option<Option<u32>>> {
    match select(conn, path).await {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_connection() => Err(e),
        Err(e) => {
            tracing::warn!(path, error = %e, "kutu seçilemedi");
            Ok(None)
        }
    }
}

async fn store_flags(conn: &mut Conn, set: &str, query: &str) -> SyncResult<()> {
    let _: Vec<Fetch> = conn
        .session
        .uid_store(set, query)
        .await?
        .try_collect()
        .await?;
    Ok(())
}

/// Seçili kutuda aranan posta.
struct Want {
    uid: Option<u32>,
    uidvalidity: Option<u32>,
    message_id: Option<String>,
}

/// Seçili kutuda postaların UID'lerini çözer (sıra korunur; bulunamayan
/// `None`). UID biliniyor ve UIDVALIDITY tutuyorsa kullanılır (`verify` ise
/// varlığı da denetlenir); yoksa Message-ID ile aranır.
async fn resolve(
    conn: &mut Conn,
    validity: Option<u32>,
    wants: &[Want],
    verify: bool,
) -> SyncResult<Vec<Option<u32>>> {
    let usable = |w: &Want| {
        w.uid
            .filter(|_| w.uidvalidity.is_none() || validity.is_none() || w.uidvalidity == validity)
    };
    let known: Vec<u32> = wants.iter().filter_map(usable).collect();
    let existing: HashSet<u32> = if verify && !known.is_empty() {
        conn.session
            .uid_search(format!("UID {}", uid_set(&known)))
            .await?
    } else {
        known.iter().copied().collect()
    };
    let mut out = Vec::with_capacity(wants.len());
    for w in wants {
        let uid = match usable(w).filter(|u| existing.contains(u)) {
            Some(u) => Some(u),
            None => match w.message_id.as_deref() {
                Some(mid) => search_message_id(&mut conn.session, mid).await?,
                None => None,
            },
        };
        out.push(uid);
    }
    Ok(out)
}

/// Konumları sunucudaki kutuya göre gruplar.
fn group_by<T, K: PartialEq + Clone>(items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<(K, Vec<T>)> {
    let mut groups: Vec<(K, Vec<T>)> = Vec::new();
    for item in items {
        let k = key(&item);
        match groups.iter_mut().find(|(g, _)| *g == k) {
            Some((_, list)) => list.push(item),
            None => groups.push((k, vec![item])),
        }
    }
    groups
}

/// Bayrağı sunucuya yazar, sonra yerel değeri yeniden yazar (arada çalışan
/// bayrak yenilemesi eski değeri geri getirmiş olabilir). Kuyrukta aynı
/// bayrak için daha yeni bir istek bekleyen postalara dokunulmaz.
async fn apply_flag(
    ctx: &AccountCtx,
    conn: &mut Conn,
    action_id: i64,
    ids: &[MessageId],
    flag: ServerFlag,
    value: bool,
) -> SyncResult<()> {
    let name = match flag {
        ServerFlag::Seen => "\\Seen",
        ServerFlag::Flagged => "\\Flagged",
    };
    let query = format!("{}FLAGS.SILENT ({name})", if value { "+" } else { "-" });
    let locs = ctx.store.locations(ids)?;
    for (path, group) in group_by(locs, |l| l.server_path.clone()) {
        let Some(validity) = try_select(conn, &path).await? else {
            continue;
        };
        let wants: Vec<Want> = group
            .iter()
            .map(|l| Want {
                uid: l.server_uid,
                uidvalidity: l.server_uidvalidity,
                message_id: l.message_id_header.clone(),
            })
            .collect();
        let uids: Vec<u32> = resolve(conn, validity, &wants, false)
            .await?
            .into_iter()
            .flatten()
            .collect();
        if !uids.is_empty() {
            store_flags(conn, &uid_set(&uids), &query).await?;
        }
    }
    let newer: HashSet<MessageId> = ctx
        .store
        .actions(ctx.id())?
        .into_iter()
        .filter(|a| a.id > action_id)
        .filter_map(|a| match a.action {
            Action::SetFlag { ids, flag: f, .. } if f == flag => Some(ids),
            _ => None,
        })
        .flatten()
        .collect();
    let settle: Vec<MessageId> = ids.iter().copied().filter(|i| !newer.contains(i)).collect();
    let (unread, starred) = match flag {
        ServerFlag::Seen => (Some(!value), None),
        ServerFlag::Flagged => (None, Some(value)),
    };
    ctx.changed(ctx.store.set_flags(&settle, unread, starred)?);
    Ok(())
}

/// UID'leri taşır (MOVE; yoksa COPY + \Deleted + EXPUNGE).
async fn move_uids(conn: &mut Conn, uids: &[u32], dest: &str) -> SyncResult<()> {
    let set = uid_set(uids);
    if conn.has_move {
        conn.session.uid_mv(&set, dest).await?;
    } else {
        conn.session.uid_copy(&set, dest).await?;
        expunge_uids(conn, uids).await?;
    }
    Ok(())
}

/// Seçili kutudan **yalnız** verilen UID'leri kalıcı siler (\Deleted +
/// EXPUNGE), parça parça.
///
/// UIDPLUS varsa `UID EXPUNGE` yalnız bu UID'leri siler. Yoksa düz EXPUNGE
/// kutudaki \Deleted işaretli HER postayı siler: başka bir istemcinin
/// işaretleyip bıraktığı (henüz silmediği) postalar da giderdi. Bu yüzden
/// önce onların işareti kaldırılır, yalnız bizimkiler işaretlenip silinir,
/// sonra işaretleri geri konur. Araya bağlantı kopması girerse en kötü sonuç
/// işaretin geri konmamasıdır: posta silinmez, yalnız "silinecek" işareti
/// kalkar. Geriye kalan tek açık, arama ile EXPUNGE arasındaki birkaç
/// milisaniyede başka bir istemcinin yeni bir postayı işaretlemesidir;
/// UIDPLUS'sız sunucuda bundan kaçınmanın yolu yoktur.
///
/// Komutların durumu denetlenir (`crate::checked`): sunucu işaretlemeyi ya da
/// silmeyi reddederse (salt okunur kutu, izin yok) hata döner; "silindi"
/// sanılmaz.
async fn expunge_uids(conn: &mut Conn, uids: &[u32]) -> SyncResult<()> {
    for chunk in uids.chunks(PURGE_CHUNK) {
        let set = uid_set(chunk);
        let mark =
            |set: &str, sign: char| format!("UID STORE {set} {sign}FLAGS.SILENT (\\Deleted)");
        if conn.has_uidplus {
            command_ok(&mut conn.session, &mark(&set, '+')).await?;
            command_ok(&mut conn.session, &format!("UID EXPUNGE {set}")).await?;
            continue;
        }
        let ours: HashSet<u32> = chunk.iter().copied().collect();
        let others: Vec<u32> = uid_search(&mut conn.session, "DELETED")
            .await?
            .into_iter()
            .filter(|u| !ours.contains(u))
            .collect();
        for part in others.chunks(PURGE_CHUNK) {
            command_ok(&mut conn.session, &mark(&uid_set(part), '-')).await?;
        }
        let res = async {
            command_ok(&mut conn.session, &mark(&set, '+')).await?;
            command_ok(&mut conn.session, "EXPUNGE").await
        }
        .await;
        // Sunucu reddetse de başkalarının işareti geri konur.
        for part in others.chunks(PURGE_CHUNK) {
            command_ok(&mut conn.session, &mark(&uid_set(part), '+')).await?;
        }
        res?;
    }
    Ok(())
}

/// Sunucu onayı bekleyen taşımaları uygular. Postanın kaynaktaki UID'si
/// denetlenir (yoksa Message-ID ile aranır); sunucu reddederse yerel taşıma
/// geri alınır. Kaynakta bulunamayan posta hedefteyse taşınmış sayılır
/// (önceki deneme yarıda kalmış), hiçbir yerde yoksa yerelden de kalkar.
async fn apply_moves(ctx: &AccountCtx, conn: &mut Conn) -> SyncResult<Vec<String>> {
    let pending = ctx.store.pending_moves(ctx.id())?;
    let mut changed: Vec<String> = Vec::new();
    let mut follow: Vec<String> = Vec::new();
    let mut again = false;
    let groups = group_by(pending, |p| {
        (
            p.src_path.clone(),
            p.dest_path.clone(),
            p.dest_mailbox_id.clone(),
        )
    });
    for ((src, dest, dest_id), items) in groups {
        let res = move_group(ctx, conn, &src, &dest, &dest_id, &items).await;
        match res {
            Ok(outcome) => {
                again |= outcome.again;
                changed.extend(outcome.changed);
                if outcome.moved {
                    follow.push(dest);
                }
            }
            Err(e) if e.is_connection() => {
                ctx.changed(changed);
                return Err(e);
            }
            // Sunucu reddetti (NO / TRYCREATE / BAD): posta yerelde de geri döner.
            Err(e) => {
                tracing::warn!(account = %ctx.account.id, src, dest, error = %e, "taşıma reddedildi, geri alındı");
                for p in &items {
                    changed.extend(ctx.store.revert_move(p.id)?);
                }
                ctx.status(SyncState::Error {
                    message: format!(
                        "Postalar taşınamadı, eski yerlerine döndü. {}",
                        e.to_app().message
                    ),
                });
            }
        }
    }
    if again {
        ctx.store
            .action_push(ctx.id(), &Action::Moves, None, now_ms())?;
    }
    ctx.changed(changed);
    Ok(follow)
}

#[derive(Default)]
struct GroupOutcome {
    moved: bool,
    again: bool,
    changed: Vec<String>,
}

async fn move_group(
    ctx: &AccountCtx,
    conn: &mut Conn,
    src: &str,
    dest: &str,
    dest_id: &str,
    items: &[PendingMove],
) -> SyncResult<GroupOutcome> {
    let mut out = GroupOutcome::default();
    ensure_mailbox(conn, dest).await?;
    let validity = select(conn, src).await?;
    let wants: Vec<Want> = items
        .iter()
        .map(|p| Want {
            uid: p.src_uid,
            uidvalidity: p.src_uidvalidity,
            message_id: p.message_id_header.clone(),
        })
        .collect();
    let found = resolve(conn, validity, &wants, true).await?;
    let uids: Vec<u32> = found.iter().flatten().copied().collect();
    if !uids.is_empty() {
        move_uids(conn, &uids, dest).await?;
        out.moved = true;
        for (p, _) in items.iter().zip(&found).filter(|(_, f)| f.is_some()) {
            out.again |= ctx.store.confirm_move(p.id, dest_id, None, None)?;
            out.changed.push(dest_id.to_owned());
        }
    }
    let missing: Vec<&PendingMove> = items
        .iter()
        .zip(&found)
        .filter(|(_, f)| f.is_none())
        .map(|(p, _)| p)
        .collect();
    if missing.is_empty() {
        return Ok(out);
    }
    // Kaynakta yok: önceki deneme taşımış olabilir.
    let dest_validity = try_select(conn, dest).await?;
    for p in missing {
        let in_dest = match (dest_validity, p.message_id_header.as_deref()) {
            (Some(v), Some(mid)) => search_message_id(&mut conn.session, mid)
                .await?
                .map(|u| (u, v)),
            _ => None,
        };
        match in_dest {
            Some((uid, v)) => out.again |= ctx.store.confirm_move(p.id, dest_id, Some(uid), v)?,
            // Sunucuda bir UID'si vardı ama artık hiçbir yerde yok: başka
            // yerden silinmiş.
            None if p.src_uid.is_some() => ctx.store.delete_messages(&[p.id])?,
            // Sunucuda hiç olmamış yerel kopya.
            None => ctx.store.forget_server_copy(p.id)?,
        }
        out.changed.push(dest_id.to_owned());
    }
    Ok(out)
}

/// Kutu şu an Çöp ya da İstenmeyen rolünde mi (işlem çalışırken denetlenir)?
fn purgeable_path(ctx: &AccountCtx, path: &str) -> SyncResult<bool> {
    Ok(ctx
        .store
        .mailbox(&mh_store::mailbox_id(ctx.id(), path))?
        .is_some_and(|m| is_purgeable(m.mailbox.role)))
}

/// Kalıcı silinecek postanın seçili kutudaki UID'si, UIDVALIDITY tutuyorsa.
fn exact_uid(r: &ServerRef, validity: Option<u32>) -> Option<u32> {
    r.uid
        .filter(|_| r.uidvalidity.is_none() || validity.is_none() || r.uidvalidity == validity)
}

/// Postaları sunucudan kalıcı siler (`purge_messages`); ardından eşitlenecek
/// kutuları döner.
///
/// Her posta kayıtlı kutusunda kendi UID'siyle aranır (varlığı denetlenir).
/// Message-ID ile arama yalnız Çöp ve İstenmeyen kutularında yapılır: başka
/// bir kutuda (taşıması bekleyen postanın kaynağı) aynı Message-ID'yi taşıyan
/// bir kopya yanlışlıkla silinemez. Kaynağında bulunamayan posta (taşıması
/// tam bu sırada uygulanmış) `fallback_path` kutusunda Message-ID ile aranır.
async fn apply_delete(
    ctx: &AccountCtx,
    conn: &mut Conn,
    items: &[ServerRef],
) -> SyncResult<Vec<String>> {
    let mut touched: Vec<String> = Vec::new();
    let mut retry: Vec<ServerRef> = Vec::new();
    let has_fallback = |r: &ServerRef| r.fallback_path.is_some() && r.message_id.is_some();
    for (path, group) in group_by(items.to_vec(), |r| r.path.clone()) {
        let Some(validity) = try_select(conn, &path).await? else {
            retry.extend(group.into_iter().filter(has_fallback));
            continue;
        };
        let by_message_id = purgeable_path(ctx, &path)?;
        let mut uids: Vec<u32> = Vec::new();
        for chunk in group.chunks(PURGE_CHUNK) {
            let known: Vec<u32> = chunk
                .iter()
                .filter_map(|r| exact_uid(r, validity))
                .collect();
            let existing: HashSet<u32> = if known.is_empty() {
                HashSet::new()
            } else {
                uid_search(&mut conn.session, &format!("UID {}", uid_set(&known))).await?
            };
            for r in chunk {
                if let Some(uid) = exact_uid(r, validity).filter(|u| existing.contains(u)) {
                    uids.push(uid);
                    continue;
                }
                let found = match r.message_id.as_deref() {
                    Some(mid) if by_message_id => search_message_id(&mut conn.session, mid).await?,
                    _ => None,
                };
                match found {
                    Some(uid) => uids.push(uid),
                    None if has_fallback(r) => retry.push(r.clone()),
                    None => {}
                }
            }
        }
        if !uids.is_empty() {
            expunge_uids(conn, &uids).await?;
            touched.push(path);
        }
    }
    // Taşıması bu arada uygulanmış postalar: hedef kutuda (Çöp / İstenmeyen).
    for (path, group) in group_by(retry, |r| r.fallback_path.clone().unwrap_or_default()) {
        if !purgeable_path(ctx, &path)? || try_select(conn, &path).await?.is_none() {
            continue;
        }
        let mut uids: Vec<u32> = Vec::new();
        for r in &group {
            if let Some(mid) = r.message_id.as_deref() {
                uids.extend(search_message_id(&mut conn.session, mid).await?);
            }
        }
        if !uids.is_empty() {
            expunge_uids(conn, &uids).await?;
            if !touched.contains(&path) {
                touched.push(path);
            }
        }
    }
    Ok(touched)
}

/// Kutuyu sunucuda boşaltır (`empty_mailbox`): `upto` ve altındaki UID'lerin
/// tümü kalıcı silinir (yerelde hiç indirilmemiş eski postalar dahil). Kutu
/// artık Çöp ya da İstenmeyen değilse (rol bu arada değişti) ya da
/// UIDVALIDITY tutmuyorsa (UID'ler başka postaları gösteriyor olabilir)
/// hiçbir şey silinmez; kutu yeniden eşitlenir.
async fn apply_empty(
    ctx: &AccountCtx,
    conn: &mut Conn,
    path: &str,
    upto: u32,
    uidvalidity: Option<u32>,
) -> SyncResult<Vec<String>> {
    if upto == 0 || !conn.server_paths.contains(path) {
        return Ok(Vec::new());
    }
    if !purgeable_path(ctx, path)? {
        tracing::warn!(account = %ctx.account.id, "boşaltılacak kutu artık çöp ya da istenmeyen değil, atlandı");
        return Ok(Vec::new());
    }
    let Some(validity) = try_select(conn, path).await? else {
        return Ok(Vec::new());
    };
    if uidvalidity.is_none() || validity != uidvalidity {
        tracing::warn!(account = %ctx.account.id, "kutu boşaltılamadı: UIDVALIDITY değişmiş");
        return Ok(vec![path.to_owned()]);
    }
    let mut uids: Vec<u32> = uid_search(&mut conn.session, &format!("UID 1:{upto}"))
        .await?
        .into_iter()
        .filter(|u| *u <= upto)
        .collect();
    uids.sort_unstable();
    if !uids.is_empty() {
        expunge_uids(conn, &uids).await?;
    }
    Ok(vec![path.to_owned()])
}

// ------------------------------------------------------------------- IDLE

async fn idle_loop(ctx: Arc<AccountCtx>, tx: UnboundedSender<Command>) {
    let mut backoff = MIN_BACKOFF;
    loop {
        let opened = Instant::now();
        let res = idle_session(&ctx, &tx).await;
        match res {
            Ok(()) => return,
            Err(e) => {
                tracing::debug!(account = %ctx.account.id, error = %e, "IDLE bağlantısı kesildi");
                if opened.elapsed() >= STABLE_SESSION {
                    backoff = MIN_BACKOFF;
                }
                // Onay bekleyen sertifikada sunucu boşuna yoklanmasın.
                let wait = if e.to_app().code == ErrorCode::Untrusted {
                    MAX_BACKOFF
                } else {
                    jittered(backoff, MAX_BACKOFF)
                };
                // "Postaları al" beklemeyi keser: kullanıcı ağın döndüğünü
                // biliyorsa IDLE 5 dakikaya kadar beklemesin.
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = ctx.wake.notified() => {
                        tracing::debug!(account = %ctx.account.id, "IDLE elle eşitlemeyle uyandı");
                    }
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

async fn idle_session(ctx: &AccountCtx, tx: &UnboundedSender<Command>) -> SyncResult<()> {
    let mut session = ctx.connect().await?;
    let caps = timeout(ACTION_TIMEOUT, session.capabilities()).await??;
    if !caps.has_str("IDLE") {
        // IDLE'siz sunucu: gelen kutusunu komut bağlantısının yoklaması izler.
        let _ = timeout(Duration::from_secs(5), session.logout()).await;
        return Ok(());
    }
    let inbox = ctx
        .store
        .mailbox_by_role(&ctx.account.id, MailboxRole::Inbox)?
        .map(|m| m.mailbox.path)
        .unwrap_or_else(|| "INBOX".into());
    timeout(ACTION_TIMEOUT, session.examine(&inbox)).await??;
    let mut first = true;
    loop {
        let mut idle = session.idle();
        timeout(ACTION_TIMEOUT, idle.init()).await??;
        if first {
            first = false;
            // Bağlantı kopukken gelen postalar: IDLE her kurulduğunda gelen
            // kutusu bir kez eşitlenir (ilk açılışta tam eşitlemeyle birleşir).
            if tx.send(Command::SyncInbox).is_err() {
                return Ok(());
            }
        }
        let (wait, stop) = idle.wait_with_timeout(IDLE_RENEW);
        let res = wait.await;
        drop(stop);
        // Akış bitti: sunucu ya da aradaki cihaz bağlantıyı kapattı. async-imap
        // bunu "elle kesildi" diye bildirir; DONE yazılacak yer yok.
        let res = match res? {
            IdleResponse::ManualInterrupt => {
                return Err(SyncError::Io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "IDLE akışı kapandı",
                )))
            }
            r => r,
        };
        session = timeout(ACTION_TIMEOUT, idle.done()).await??;
        while session.unsolicited_responses.try_recv().is_ok() {}
        if let IdleResponse::NewData(_) = res {
            if tx.send(Command::SyncInbox).is_err() {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geri_cekilme_payi_sinirli() {
        for _ in 0..50 {
            let b = Duration::from_secs(8);
            let j = jittered(b, MAX_BACKOFF);
            assert!(j >= b && j <= b + b / 4, "{j:?}");
            assert_eq!(jittered(MAX_BACKOFF, MAX_BACKOFF), MAX_BACKOFF);
            assert_eq!(jittered(Duration::ZERO, MAX_BACKOFF), Duration::ZERO);
        }
    }

    fn l(path: &str, attr_role: Option<MailboxRole>, all: bool) -> Listed {
        Listed {
            path: path.into(),
            delimiter: Some("/".into()),
            attr_role,
            all,
            noselect: false,
            copy: false,
        }
    }

    fn find<'a>(roles: &'a [Assigned], path: &str) -> Option<&'a Assigned> {
        roles.iter().find(|r| r.path == path)
    }

    #[test]
    fn gmail_rolleri() {
        let listed = vec![
            l("INBOX", None, false),
            Listed {
                noselect: true,
                ..l("[Gmail]", None, false)
            },
            l("[Gmail]/All Mail", None, true),
            l("[Gmail]/Sent Mail", Some(MailboxRole::Sent), false),
            l("[Gmail]/Trash", Some(MailboxRole::Trash), false),
            l("[Gmail]/Drafts", Some(MailboxRole::Drafts), false),
            l("[Gmail]/Spam", Some(MailboxRole::Junk), false),
            Listed {
                copy: true,
                ..l("[Gmail]/Starred", None, false)
            },
            Listed {
                copy: true,
                ..l("[Gmail]/Important", None, false)
            },
            l("Archive", None, false),
        ];
        let roles = assign_roles(&listed);
        let get = |p: &str| find(&roles, p).map(|r| (r.role, r.nosync));
        assert_eq!(get("[Gmail]"), None, "seçilemeyen kutu atlanır");
        assert_eq!(get("INBOX"), Some((MailboxRole::Inbox, false)));
        assert_eq!(get("[Gmail]/All Mail"), Some((MailboxRole::Archive, true)));
        assert_eq!(
            get("Archive"),
            Some((MailboxRole::Other, false)),
            "rol bir kez atanır"
        );
        assert_eq!(get("[Gmail]/Sent Mail"), Some((MailboxRole::Sent, false)));
        assert_eq!(get("[Gmail]/Spam"), Some((MailboxRole::Junk, false)));
        let sent = find(&roles, "[Gmail]/Sent Mail").unwrap();
        assert_eq!(
            (sent.name.as_str(), sent.parent.as_deref()),
            ("Sent Mail", None)
        );
        assert_eq!(personal_namespace(&listed).prefix, "");

        // Varsayılan gizli: eşitlenmeyen "Tüm Postalar", kopya kutular.
        let hidden = |p: &str| find(&roles, p).unwrap().hidden;
        assert!(hidden("[Gmail]/All Mail"));
        assert!(hidden("[Gmail]/Starred"));
        assert!(hidden("[Gmail]/Important"));
        for p in ["INBOX", "[Gmail]/Sent Mail", "[Gmail]/Spam", "Archive"] {
            assert!(!hidden(p), "{p}");
        }
        // Seçilemeyen atanın adı görünen ada katılır.
        let starred = find(&roles, "[Gmail]/Starred").unwrap();
        assert_eq!(
            (starred.name.as_str(), starred.parent.as_deref()),
            ("[Gmail] / Starred", None)
        );
    }

    #[test]
    fn oznitelik_ad_tahmininden_once_gelir() {
        let listed = vec![
            l("Sent", None, false),
            l("Gonderilen-Ozel", Some(MailboxRole::Sent), false),
            l("Archive", Some(MailboxRole::Archive), false),
            l("All", None, true),
            l("Spam", None, false),
        ];
        let roles = assign_roles(&listed);
        let get = |p: &str| find(&roles, p).map(|r| (r.role, r.nosync)).unwrap();
        assert_eq!(get("Gonderilen-Ozel"), (MailboxRole::Sent, false));
        assert_eq!(get("Sent"), (MailboxRole::Other, false));
        assert_eq!(get("All"), (MailboxRole::Other, false));
        assert_eq!(get("Spam"), (MailboxRole::Junk, false));
        // Arşiv olarak kullanılmayan \All gizli ama eşitlenebilir (gösterilirse).
        assert!(find(&roles, "All").unwrap().hidden);
        assert!(!find(&roles, "Sent").unwrap().hidden);
    }

    #[test]
    fn klasor_agaci() {
        let listed = vec![
            l("INBOX", None, false),
            l("Projeler", None, false),
            l("Projeler/2024", None, false),
            l("Projeler/2024/Mart", None, false),
            Listed {
                noselect: true,
                ..l("Eski", None, false)
            },
            l("Eski/Faturalar", None, false),
            // Ara düğüm listede yok (sunucu yalnız yaprağı döndü).
            l("Musteriler/Acme/Teklifler", None, false),
            l("INBOX/Alt", None, false),
            l("Is", None, false),
            l("Is/Sent", None, false),
            l(
                &crate::mutf7::encode("İş Ortakları/Görüşmeler"),
                None,
                false,
            ),
        ];
        let roles = assign_roles(&listed);
        let at = |p: &str| {
            let a = find(&roles, p).unwrap();
            (a.parent.as_deref(), a.name.as_str())
        };
        assert_eq!(at("Projeler"), (None, "Projeler"));
        assert_eq!(at("Projeler/2024"), (Some("Projeler"), "2024"));
        assert_eq!(at("Projeler/2024/Mart"), (Some("Projeler/2024"), "Mart"));
        assert_eq!(at("Eski/Faturalar"), (None, "Eski / Faturalar"));
        assert_eq!(
            at("Musteriler/Acme/Teklifler"),
            (None, "Musteriler / Acme / Teklifler")
        );
        // Ad alanı önekli değilse INBOX'ın altı INBOX'a bağlanır.
        assert_eq!(at("INBOX/Alt"), (Some("INBOX"), "Alt"));
        // Rol kutusu her zaman köktedir.
        let sent = find(&roles, "Is/Sent").unwrap();
        assert_eq!(
            (sent.role, sent.parent.as_deref(), sent.name.as_str()),
            (MailboxRole::Sent, None, "Sent")
        );
        assert_eq!(
            at(&crate::mutf7::encode("İş Ortakları/Görüşmeler")),
            (None, "İş Ortakları / Görüşmeler")
        );
        assert!(roles.iter().all(|r| !r.hidden));
    }

    #[test]
    fn inbox_onekli_ad_alaninda_klasorler_koktedir() {
        let dot = |p: &str| Listed {
            delimiter: Some(".".into()),
            ..l(p, None, false)
        };
        let listed = vec![
            dot("INBOX"),
            dot("INBOX.Sent"),
            dot("INBOX.Trash"),
            dot("INBOX.Projeler"),
            dot("INBOX.Projeler.2024"),
            Listed {
                noselect: true,
                ..dot("INBOX.Eski")
            },
            dot("INBOX.Eski.Faturalar"),
        ];
        assert_eq!(personal_namespace(&listed).prefix, "INBOX.");
        let roles = assign_roles(&listed);
        let at = |p: &str| {
            let a = find(&roles, p).unwrap();
            (a.parent.as_deref(), a.name.as_str())
        };
        assert_eq!(at("INBOX.Projeler"), (None, "Projeler"));
        assert_eq!(at("INBOX.Projeler.2024"), (Some("INBOX.Projeler"), "2024"));
        assert_eq!(at("INBOX.Eski.Faturalar"), (None, "Eski / Faturalar"));
        assert_eq!(at("INBOX.Sent"), (None, "Sent"));
        assert_eq!(find(&roles, "INBOX.Sent").unwrap().role, MailboxRole::Sent);
        assert_eq!(at("INBOX"), (None, "INBOX"));
    }

    fn mb(role: MailboxRole, hidden: bool, nosync: bool) -> MailboxInfo {
        MailboxInfo {
            mailbox: mh_core::Mailbox {
                id: "a:x".into(),
                account_id: "a".into(),
                path: "x".into(),
                name: "x".into(),
                role,
                unread: 0,
                total: 0,
                parent_id: None,
                hidden,
            },
            uidvalidity: None,
            uidnext: None,
            highest_uid: 0,
            nosync,
            older_done: false,
        }
    }

    #[test]
    fn esitlenen_kutular() {
        // Rol kutuları gizli olsa da eşitlenir.
        for r in SYNC_ROLES {
            assert!(syncs(&mb(r, false, false)));
            assert!(syncs(&mb(r, true, false)), "{r:?}");
        }
        // Gereksiz ve klasörler yalnız görünürken.
        for r in [MailboxRole::Junk, MailboxRole::Other] {
            assert!(syncs(&mb(r, false, false)), "{r:?}");
            assert!(!syncs(&mb(r, true, false)), "{r:?}");
        }
        // Eşitlenmeyen kutu hiçbir zaman.
        assert!(!syncs(&mb(MailboxRole::Archive, false, true)));
        assert!(!syncs(&mb(MailboxRole::Other, false, true)));
        // Sıra: gelen kutusu, rol kutuları, diğerleri.
        let mut roles = [
            MailboxRole::Other,
            MailboxRole::Junk,
            MailboxRole::Trash,
            MailboxRole::Inbox,
        ];
        roles.sort_by_key(|r| sync_order(*r));
        assert_eq!(roles[0], MailboxRole::Inbox);
        assert_eq!(roles[1], MailboxRole::Trash);
    }

    #[test]
    fn ad_alani_inbox_oneki() {
        let dot = |p: &str| Listed {
            delimiter: Some(".".into()),
            ..l(p, None, false)
        };
        let ns = personal_namespace(&[dot("INBOX"), dot("INBOX.Sent"), dot("INBOX.Trash")]);
        assert_eq!(
            ns,
            Namespace {
                prefix: "INBOX.".into(),
                delimiter: Some(".".into())
            }
        );
        // Kökte kutu varsa önek yok; yalnız INBOX varsa da yok.
        assert_eq!(
            personal_namespace(&[dot("INBOX"), dot("INBOX.Sent"), dot("Trash")]).prefix,
            ""
        );
        assert_eq!(personal_namespace(&[dot("INBOX")]).prefix, "");
        assert_eq!(personal_namespace(&[]), Namespace::default());
    }

    #[test]
    fn esitleme_sirasi_ve_birlestirme() {
        let path = |p: &str| Command::SyncPath(p.into());
        let mut q: VecDeque<Command> = VecDeque::from([
            Command::SyncInbox,
            Command::SyncInbox,
            path("Archive"),
            Command::SyncAll,
            path("Sent"),
            path("Archive"),
            Command::SyncInbox,
        ]);
        assert_eq!(next_sync(&mut q), Some(path("Archive")));
        assert_eq!(next_sync(&mut q), Some(path("Sent")));
        assert_eq!(next_sync(&mut q), Some(Command::SyncAll));
        assert_eq!(
            next_sync(&mut q),
            None,
            "tam eşitleme gelen kutusunu kapsar"
        );
        let mut q = VecDeque::from([Command::SyncInbox, Command::SyncInbox]);
        assert_eq!(next_sync(&mut q), Some(Command::SyncInbox));
        assert!(q.is_empty());

        let mut p = VecDeque::new();
        enqueue(&mut p, Command::Wake);
        enqueue(&mut p, Command::SyncInbox);
        assert_eq!(p, VecDeque::from([Command::SyncInbox]));
    }

    #[test]
    fn imap_tirnak() {
        assert_eq!(imap_quote("<a@b>").as_deref(), Some("\"<a@b>\""));
        assert_eq!(imap_quote("a\"b\\c").as_deref(), Some("\"a\\\"b\\\\c\""));
        assert_eq!(imap_quote("satır\r\nX"), None);
        assert_eq!(imap_quote("ğ@ornek"), None);
        assert_eq!(imap_quote(""), None);
    }

    #[test]
    fn eklemenin_suresi_boyutla_uzar() {
        let qa = |raw: Option<Vec<u8>>| QueuedAction {
            id: 1,
            account_id: "a".into(),
            action: Action::Moves,
            raw,
            attempts: 0,
        };
        assert_eq!(action_timeout(&qa(None)), ACTION_TIMEOUT);
        assert_eq!(
            action_timeout(&qa(Some(vec![0; 2_000_000]))),
            ACTION_TIMEOUT + Duration::from_secs(100)
        );
    }

    #[test]
    fn tanitim_hesabi_eslenmez() {
        let mut a = Account {
            id: "a".into(),
            name: "İş".into(),
            email: "a@b.c".into(),
            display_name: "A".into(),
            color: "#000000".into(),
            kind: AccountKind::Imap,
            imap: Some(mh_core::ServerConfig {
                host: "demo.invalid".into(),
                port: 993,
                security: mh_core::Security::Tls,
                username: "a".into(),
            }),
            smtp: None,
            jmap_url: None,
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth: Default::default(),
            notify: true,
        };
        assert!(!is_syncable(&a));
        a.imap.as_mut().unwrap().host = "imap.ornek.com".into();
        assert!(is_syncable(&a));
        a.kind = AccountKind::Jmap;
        assert!(!is_syncable(&a));
    }

    struct Quiet;
    impl EventSink for Quiet {
        fn sync_status(&self, _: SyncStatus) {}
        fn changed(&self, _: Vec<String>) {}
        fn new_mail(&self, _: &Account, _: Vec<mh_core::MessageSummary>) {}
    }

    #[tokio::test]
    async fn islemler_kuyruga_yazilir_tanitim_hesabi_yerelde_kalir() {
        use crate::secrets::MemorySecrets;
        let store = Arc::new(Store::open_in_memory().unwrap());
        let mut a = Account {
            id: "a".into(),
            name: "İş".into(),
            email: "a@ornek.com".into(),
            display_name: "A".into(),
            color: "#000000".into(),
            kind: AccountKind::Imap,
            imap: Some(mh_core::ServerConfig {
                host: "imap.ornek.com".into(),
                port: 993,
                security: mh_core::Security::Tls,
                username: "a".into(),
            }),
            smtp: None,
            jmap_url: None,
            signatures: Vec::new(),
            signature_new: None,
            signature_reply: None,
            created_at: 0,
            auth: Default::default(),
            notify: true,
        };
        store.insert_account(&a).unwrap();
        a.id = "demo".into();
        a.imap.as_mut().unwrap().host = "demo.invalid".into();
        store.insert_account(&a).unwrap();
        for acc in ["a", "demo"] {
            store
                .upsert_mailbox(acc, "INBOX", "INBOX", MailboxRole::Inbox, None, false)
                .unwrap();
        }
        let mk = |acc: &str, uid: u32| {
            let raw = format!(
                "From: k@ornek.com\r\nSubject: s{uid}\r\nMessage-ID: <{acc}{uid}@ornek>\r\n\r\ngövde"
            );
            let mut m = mh_store::NewMessage::from_raw(
                raw.into_bytes(),
                false,
                acc,
                &format!("{acc}:INBOX"),
                1,
            )
            .unwrap();
            m.uid = Some(uid);
            store.upsert_message(&m).unwrap().id
        };
        let (x, y) = (mk("a", 1), mk("demo", 1));
        // Ad alanı biliniyor: yeni arşiv kutusu önekle açılır.
        store
            .set_namespace(
                "a",
                &Namespace {
                    prefix: "INBOX.".into(),
                    delimiter: Some(".".into()),
                },
            )
            .unwrap();
        let mgr = SyncManager::new(
            store.clone(),
            Arc::new(MemorySecrets::new()),
            Arc::new(Quiet),
            tokio::runtime::Handle::current(),
        );
        mgr.set_flags(&[x, y], Some(false), Some(true)).unwrap();
        mgr.move_messages(&[x, y], MoveTarget::Archive).unwrap();
        let kinds: Vec<Action> = store
            .actions("a")
            .unwrap()
            .into_iter()
            .map(|q| q.action)
            .collect();
        assert_eq!(
            kinds,
            vec![
                Action::SetFlag {
                    ids: vec![x],
                    flag: ServerFlag::Seen,
                    value: true
                },
                Action::SetFlag {
                    ids: vec![x],
                    flag: ServerFlag::Flagged,
                    value: true
                },
                Action::Moves,
            ]
        );
        assert!(store.actions("demo").unwrap().is_empty());
        let lx = store.location(x).unwrap().unwrap();
        assert_eq!(
            (lx.mailbox_path.as_str(), lx.pending, lx.server_uid),
            ("INBOX.Archive", true, Some(1))
        );
        let ly = store.location(y).unwrap().unwrap();
        assert_eq!((ly.mailbox_path.as_str(), ly.pending), ("Archive", false));

        // Geri al: sunucuya gitmemiş taşıma iptal; kuyruktaki taşıma boşa çalışır.
        mgr.move_messages(&[x], MoveTarget::Inbox).unwrap();
        assert!(!store.has_pending_moves("a").unwrap());

        assert!(mgr
            .append(
                "a",
                MailboxRole::Sent,
                b"Message-ID: <g@o>\r\n\r\nx".to_vec()
            )
            .unwrap());
        assert!(!mgr
            .append("demo", MailboxRole::Sent, b"x".to_vec())
            .unwrap());
        let last = store.actions("a").unwrap().pop().unwrap();
        assert_eq!(
            last.action,
            Action::Append {
                path: "INBOX.Sent".into(),
                message_id: Some("g@o".into())
            }
        );
    }
}
