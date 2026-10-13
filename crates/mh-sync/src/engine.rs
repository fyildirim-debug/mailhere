//! Hesap başına eşitleme motoru.
//!
//! Her IMAP hesabı için iki görev çalışır: komut bağlantısı (eşitleme ve
//! sunucu işlemleri, sırayla) ve INBOX üzerinde IDLE bekleyen ayrı bir
//! bağlantı. Bağlantı koparsa üstel bekleme ile yeniden denenir.

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
use mh_store::{MailboxInfo, Store};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Instant};

use crate::conn::ImapSession;
use crate::credentials::{connect_account, TokenProvider};
use crate::error::{SyncError, SyncResult};
use crate::mailbox::{sync_mailbox, uid_set};
use crate::roles;
use crate::secrets::SecretStore;
use crate::EventSink;

const MIN_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// Tüm kutuların düzenli eşitlenmesi (aynı zamanda bağlantıyı canlı tutar).
const PERIODIC: Duration = Duration::from_secs(10 * 60);
/// IDLE en geç bu sürede yenilenir (RFC 2177: 29 dakikadan kısa).
const IDLE_RENEW: Duration = Duration::from_secs(25 * 60);
/// IDLE desteklemeyen sunucuda gelen kutusu yoklama aralığı.
const POLL: Duration = Duration::from_secs(120);
const FULL_SYNC_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const ACTION_TIMEOUT: Duration = Duration::from_secs(120);

/// Eşitlenen kutu rolleri.
const SYNC_ROLES: [MailboxRole; 5] = [
    MailboxRole::Inbox,
    MailboxRole::Sent,
    MailboxRole::Drafts,
    MailboxRole::Archive,
    MailboxRole::Trash,
];

/// Sunucudaki bir postanın yeri.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    path: String,
    uid: u32,
    uidvalidity: Option<u32>,
}

enum Command {
    SyncAll,
    SyncInbox,
    Flags {
        /// Yerel kimlikler: sunucu onayından sonra yerel değer yeniden yazılır
        /// (arada çalışan bayrak yenilemesi eski değeri geri getirmesin).
        ids: Vec<MessageId>,
        items: Vec<Target>,
        unread: Option<bool>,
        starred: Option<bool>,
    },
    Move {
        items: Vec<Target>,
        dest: String,
    },
    Append {
        dest: String,
        raw: Vec<u8>,
    },
}

impl Command {
    fn is_action(&self) -> bool {
        matches!(
            self,
            Self::Flags { .. } | Self::Move { .. } | Self::Append { .. }
        )
    }
}

struct AccountCtx {
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
}

/// Hesaba göre gruplanmış (posta, sunucudaki yeri).
type AccountTargets = HashMap<String, Vec<(MessageId, Option<Target>)>>;

struct Handle {
    tx: UnboundedSender<Command>,
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

/// Bu hesap gerçek bir IMAP sunucusuyla eşitlenir mi? (Tanıtım hesapları ve
/// henüz desteklenmeyen JMAP hesapları yalnız yereldir.)
pub fn is_syncable(account: &Account) -> bool {
    account.kind == AccountKind::Imap
        && account
            .imap
            .as_ref()
            .is_some_and(|c| !c.host.trim().to_ascii_lowercase().ends_with(".invalid"))
}

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
        if !is_syncable(&account) {
            self.sink.sync_status(SyncStatus {
                account_id: account.id.clone(),
                state: SyncState::Online,
            });
            return;
        }
        let (tx, rx) = unbounded_channel();
        let ctx = Arc::new(AccountCtx {
            account: account.clone(),
            store: self.store.clone(),
            secrets: self.secrets.clone(),
            tokens: self.tokens.clone(),
            sink: self.sink.clone(),
        });
        let tasks = vec![
            self.rt.spawn(account_loop(ctx.clone(), rx)),
            self.rt.spawn(idle_loop(ctx, tx.clone())),
        ];
        self.handles().insert(account.id, Handle { tx, tasks });
    }

    /// Hesabın görevlerini durdurur.
    pub fn stop(&self, account_id: &str) {
        if let Some(h) = self.handles().remove(account_id) {
            for t in h.tasks {
                t.abort();
            }
        }
    }

    fn send(&self, account_id: &str, cmd: Command) -> bool {
        self.handles()
            .get(account_id)
            .is_some_and(|h| h.tx.send(cmd).is_ok())
    }

    /// Tüm hesapları hemen eşitler (bekleyen yeniden bağlanmayı da hızlandırır).
    pub fn sync_now(&self) {
        for h in self.handles().values() {
            let _ = h.tx.send(Command::SyncAll);
        }
    }

    fn targets_by_account(&self, ids: &[MessageId]) -> AppResult<AccountTargets> {
        let mut out = AccountTargets::new();
        for loc in self.store.locations(ids)? {
            let target = loc.uid.map(|uid| Target {
                path: loc.mailbox_path.clone(),
                uid,
                uidvalidity: loc.uidvalidity,
            });
            out.entry(loc.account_id)
                .or_default()
                .push((loc.id, target));
        }
        Ok(out)
    }

    /// Bayrakları önce yerelde değiştirir, sonra sunucuya iletir. Etkilenen
    /// kutuları döner.
    pub fn set_flags(
        &self,
        ids: &[MessageId],
        unread: Option<bool>,
        starred: Option<bool>,
    ) -> AppResult<Vec<String>> {
        let groups = self.targets_by_account(ids)?;
        let touched = self.store.set_flags(ids, unread, starred)?;
        for (account, list) in groups {
            let ids: Vec<MessageId> = list.iter().map(|(id, _)| *id).collect();
            let items: Vec<Target> = list.into_iter().filter_map(|(_, t)| t).collect();
            if !items.is_empty() {
                self.send(
                    &account,
                    Command::Flags {
                        ids,
                        items,
                        unread,
                        starred,
                    },
                );
            }
        }
        Ok(touched)
    }

    /// Postaları önce yerelde taşır, sonra sunucuya iletir.
    pub fn move_messages(&self, ids: &[MessageId], target: MoveTarget) -> AppResult<Vec<String>> {
        let role = match target {
            MoveTarget::Inbox => MailboxRole::Inbox,
            MoveTarget::Archive => MailboxRole::Archive,
            MoveTarget::Trash => MailboxRole::Trash,
        };
        let mut touched = Vec::new();
        for (account, list) in self.targets_by_account(ids)? {
            let dest = self.destination(&account, role)?;
            let ids: Vec<MessageId> = list.iter().map(|(id, _)| *id).collect();
            let items: Vec<Target> = list
                .into_iter()
                .filter_map(|(_, t)| t)
                .filter(|t| t.path != dest.mailbox.path)
                .collect();
            touched.extend(self.store.move_local(&ids, &dest.mailbox.id)?);
            if !items.is_empty() {
                self.send(
                    &account,
                    Command::Move {
                        items,
                        dest: dest.mailbox.path.clone(),
                    },
                );
            }
        }
        touched.sort();
        touched.dedup();
        Ok(touched)
    }

    /// Roldeki kutu; yoksa yerelde oluşturulur (sunucuda ilk kullanımda açılır).
    fn destination(&self, account: &str, role: MailboxRole) -> AppResult<MailboxInfo> {
        if let Some(mb) = self.store.mailbox_by_role(account, role)? {
            return Ok(mb);
        }
        let path = match role {
            MailboxRole::Inbox => "INBOX",
            MailboxRole::Archive => "Archive",
            MailboxRole::Trash => "Trash",
            MailboxRole::Sent => "Sent",
            MailboxRole::Drafts => "Drafts",
            MailboxRole::Junk => "Junk",
            MailboxRole::Other => "Mailhere",
        };
        let id = self.store.upsert_mailbox(account, path, path, role)?;
        self.store
            .mailbox(&id)?
            .ok_or_else(|| AppError::new(ErrorCode::Internal, "Kutu oluşturulamadı."))
    }

    /// Ham postayı hesabın verilen roldeki kutusuna ekler (ör. Gönderilenler).
    /// Komut kuyruğa alındıysa `true`.
    pub fn append(&self, account_id: &str, role: MailboxRole, raw: Vec<u8>) -> AppResult<bool> {
        let dest = self.destination(account_id, role)?;
        Ok(self.send(
            account_id,
            Command::Append {
                dest: dest.mailbox.path,
                raw,
            },
        ))
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
    let uid = loc.uid.ok_or_else(|| {
        AppError::new(
            ErrorCode::Network,
            "Posta henüz sunucuyla eşitlenmedi. Birazdan yeniden dene.",
        )
    })?;
    let cfg = account
        .imap
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::Invalid, "Hesabın IMAP ayarı yok."))?;
    let work = async {
        let mut session = connect_account(&cfg, secrets, tokens, &account).await?;
        let mb = session.examine(&loc.mailbox_path).await?;
        if loc.uidvalidity.is_some() && mb.uid_validity != loc.uidvalidity {
            return Ok(None);
        }
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
    store.store_body(id, &raw)
}

// ------------------------------------------------------------ komut bağlantısı

struct Conn {
    session: ImapSession,
    has_move: bool,
    has_uidplus: bool,
    /// Son LIST yanıtındaki kutular.
    server_paths: HashSet<String>,
    /// Eşitlenmeyen kutular (Gmail "Tüm Postalar" gibi).
    skip: HashSet<String>,
}

async fn account_loop(ctx: Arc<AccountCtx>, mut rx: UnboundedReceiver<Command>) {
    let mut pending: VecDeque<Command> = VecDeque::from([Command::SyncAll]);
    let mut backoff = MIN_BACKOFF;
    loop {
        let err = match run_session(&ctx, &mut rx, &mut pending, &mut backoff).await {
            Ok(()) => return,
            Err(e) => e,
        };
        let app = err.to_app();
        tracing::warn!(account = %ctx.account.id, error = %err, "eşitleme bağlantısı kesildi");
        ctx.status(match app.code {
            ErrorCode::Network => SyncState::Offline,
            _ => SyncState::Error {
                message: app.message,
            },
        });
        let sleep = tokio::time::sleep(backoff);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                cmd = rx.recv() => match cmd {
                    None => return,
                    Some(Command::SyncAll) => break,
                    Some(c) => pending.push_back(c),
                },
            }
        }
        if !pending.iter().any(|c| matches!(c, Command::SyncAll)) {
            pending.push_front(Command::SyncAll);
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Sıradaki komut: önce sunucu işlemleri (yerel değişiklik eşitlemeyle ezilmesin),
/// yinelenen eşitleme istekleri birleştirilir.
fn next_command(pending: &mut VecDeque<Command>) -> Option<Command> {
    let idx = pending.iter().position(Command::is_action).unwrap_or(0);
    let cmd = pending.remove(idx)?;
    match cmd {
        Command::SyncAll | Command::SyncInbox
            if pending.iter().any(|c| matches!(c, Command::SyncAll)) =>
        {
            next_command(pending)
        }
        Command::SyncInbox if pending.iter().any(|c| matches!(c, Command::SyncInbox)) => {
            next_command(pending)
        }
        c => Some(c),
    }
}

async fn run_session(
    ctx: &AccountCtx,
    rx: &mut UnboundedReceiver<Command>,
    pending: &mut VecDeque<Command>,
    backoff: &mut Duration,
) -> SyncResult<()> {
    if pending.iter().any(|c| matches!(c, Command::SyncAll)) {
        ctx.status(SyncState::Syncing { done: 0, total: 0 });
    }
    let mut session = ctx.connect().await?;
    let caps = timeout(ACTION_TIMEOUT, session.capabilities()).await??;
    let mut conn = Conn {
        has_move: caps.has_str("MOVE"),
        has_uidplus: caps.has_str("UIDPLUS"),
        session,
        server_paths: HashSet::new(),
        skip: HashSet::new(),
    };
    *backoff = MIN_BACKOFF;
    let mut periodic = tokio::time::interval_at(Instant::now() + PERIODIC, PERIODIC);
    periodic.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        while let Ok(c) = rx.try_recv() {
            pending.push_back(c);
        }
        while let Some(cmd) = next_command(pending) {
            let limit = if cmd.is_action() {
                ACTION_TIMEOUT
            } else {
                FULL_SYNC_TIMEOUT
            };
            let res = match timeout(limit, execute(ctx, &mut conn, &cmd)).await {
                Ok(r) => r,
                Err(_) => Err(SyncError::Timeout),
            };
            if let Err(e) = res {
                if e.is_connection() {
                    pending.push_front(cmd);
                    return Err(e);
                }
                tracing::warn!(account = %ctx.account.id, error = %e, "sunucu işlemi başarısız");
                ctx.status(SyncState::Error {
                    message: e.to_app().message,
                });
            }
            while let Ok(c) = rx.try_recv() {
                pending.push_back(c);
            }
        }
        tokio::select! {
            cmd = rx.recv() => match cmd {
                None => {
                    let _ = timeout(Duration::from_secs(5), conn.session.logout()).await;
                    return Ok(());
                }
                Some(c) => pending.push_back(c),
            },
            _ = periodic.tick() => pending.push_back(Command::SyncAll),
        }
    }
}

async fn execute(ctx: &AccountCtx, conn: &mut Conn, cmd: &Command) -> SyncResult<()> {
    match cmd {
        Command::SyncAll => full_sync(ctx, conn).await,
        Command::SyncInbox => {
            if let Some(info) = ctx
                .store
                .mailbox_by_role(&ctx.account.id, MailboxRole::Inbox)?
            {
                sync_one(ctx, conn, &info).await?;
            }
            Ok(())
        }
        Command::Flags {
            ids,
            items,
            unread,
            starred,
        } => {
            apply_flags(conn, items, *unread, *starred).await?;
            let touched = ctx.store.set_flags(ids, *unread, *starred)?;
            if !touched.is_empty() {
                ctx.sink.changed(touched);
            }
            Ok(())
        }
        Command::Move { items, dest } => {
            apply_move(conn, items, dest).await?;
            sync_path(ctx, conn, dest).await
        }
        Command::Append { dest, raw } => {
            ensure_mailbox(conn, dest).await?;
            conn.session
                .append(dest, Some("(\\Seen)"), None, raw)
                .await?;
            sync_path(ctx, conn, dest).await
        }
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
}

impl Listed {
    fn from_name(n: &Name) -> Self {
        Self {
            path: n.name().to_owned(),
            delimiter: n.delimiter().map(str::to_owned),
            attr_role: roles::from_attributes(n.attributes()),
            all: roles::is_all(n.attributes()),
            noselect: roles::is_noselect(n.attributes()),
        }
    }
}

/// Kutu atanmış rolü: (yol, görünen ad, rol, eşitlenmesin mi).
pub fn assign_roles(listed: &[Listed]) -> Vec<(String, String, MailboxRole, bool)> {
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
    selectable
        .iter()
        .map(|l| {
            let (role, skip) = role_of
                .get(l.path.as_str())
                .copied()
                .unwrap_or((MailboxRole::Other, false));
            (
                l.path.clone(),
                roles::leaf(&l.path, l.delimiter.as_deref()),
                role,
                skip,
            )
        })
        .collect()
}

async fn full_sync(ctx: &AccountCtx, conn: &mut Conn) -> SyncResult<()> {
    let account = &ctx.account.id;
    let names: Vec<Name> = conn
        .session
        .list(Some(""), Some("*"))
        .await?
        .try_collect()
        .await?;
    let listed: Vec<Listed> = names.iter().map(Listed::from_name).collect();
    let assigned = assign_roles(&listed);
    conn.server_paths = listed.iter().map(|l| l.path.clone()).collect();
    conn.skip.clear();
    let mut changed: Vec<String> = Vec::new();
    let mut keep = Vec::new();
    for (path, name, role, skip) in &assigned {
        ctx.store.upsert_mailbox(account, path, name, *role)?;
        keep.push(path.clone());
        if *skip {
            conn.skip.insert(path.clone());
        }
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
    changed.extend(ctx.store.remove_mailboxes_except(account, &keep)?);

    let targets: Vec<MailboxInfo> = ctx
        .store
        .mailboxes_for(account)?
        .into_iter()
        .filter(|m| SYNC_ROLES.contains(&m.mailbox.role))
        .filter(|m| {
            conn.server_paths.contains(&m.mailbox.path) && !conn.skip.contains(&m.mailbox.path)
        })
        .collect();
    let total = targets.len() as u32;
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
    ctx.status(SyncState::Online);
    if !changed.is_empty() {
        ctx.sink.changed(changed);
    }
    ctx.emit_new(&new_unread);
    Ok(())
}

async fn sync_one(ctx: &AccountCtx, conn: &mut Conn, info: &MailboxInfo) -> SyncResult<()> {
    if conn.skip.contains(&info.mailbox.path) || !SYNC_ROLES.contains(&info.mailbox.role) {
        return Ok(());
    }
    let notify = info.mailbox.role == MailboxRole::Inbox;
    let out = sync_mailbox(&mut conn.session, &ctx.store, info, notify).await?;
    if out.changed {
        ctx.sink.changed(vec![info.mailbox.id.clone()]);
    }
    ctx.emit_new(&out.new_unread);
    Ok(())
}

async fn sync_path(ctx: &AccountCtx, conn: &mut Conn, path: &str) -> SyncResult<()> {
    let id = mh_store::mailbox_id(&ctx.account.id, path);
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
        // Zaten varsa sunucu NO döner; sorun değil.
        Err(async_imap::error::Error::No(m)) => {
            tracing::debug!(path, reply = %m, "kutu oluşturulamadı ya da zaten var")
        }
        Err(e) => return Err(e.into()),
    }
    conn.server_paths.insert(path.to_owned());
    Ok(())
}

fn group_by_path(items: &[Target]) -> Vec<(String, Option<u32>, Vec<u32>)> {
    let mut groups: Vec<(String, Option<u32>, Vec<u32>)> = Vec::new();
    for t in items {
        match groups
            .iter_mut()
            .find(|(p, v, _)| *p == t.path && *v == t.uidvalidity)
        {
            Some((_, _, uids)) => uids.push(t.uid),
            None => groups.push((t.path.clone(), t.uidvalidity, vec![t.uid])),
        }
    }
    groups
}

/// Kutuyu yazılabilir seçer; UIDVALIDITY değiştiyse `false` (UID'ler geçersiz).
async fn select(conn: &mut Conn, path: &str, uidvalidity: Option<u32>) -> SyncResult<bool> {
    let mb = conn.session.select(path).await?;
    Ok(uidvalidity.is_none() || mb.uid_validity == uidvalidity)
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

async fn apply_flags(
    conn: &mut Conn,
    items: &[Target],
    unread: Option<bool>,
    starred: Option<bool>,
) -> SyncResult<()> {
    for (path, validity, uids) in group_by_path(items) {
        if !select(conn, &path, validity).await? {
            continue;
        }
        let set = uid_set(&uids);
        if let Some(u) = unread {
            store_flags(
                conn,
                &set,
                if u {
                    "-FLAGS.SILENT (\\Seen)"
                } else {
                    "+FLAGS.SILENT (\\Seen)"
                },
            )
            .await?;
        }
        if let Some(s) = starred {
            store_flags(
                conn,
                &set,
                if s {
                    "+FLAGS.SILENT (\\Flagged)"
                } else {
                    "-FLAGS.SILENT (\\Flagged)"
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn apply_move(conn: &mut Conn, items: &[Target], dest: &str) -> SyncResult<()> {
    ensure_mailbox(conn, dest).await?;
    for (path, validity, uids) in group_by_path(items) {
        if path == dest || !select(conn, &path, validity).await? {
            continue;
        }
        let set = uid_set(&uids);
        if conn.has_move {
            conn.session.uid_mv(&set, dest).await?;
        } else {
            conn.session.uid_copy(&set, dest).await?;
            store_flags(conn, &set, "+FLAGS.SILENT (\\Deleted)").await?;
            if conn.has_uidplus {
                let _: Vec<u32> = conn.session.uid_expunge(&set).await?.try_collect().await?;
            } else {
                let _: Vec<u32> = conn.session.expunge().await?.try_collect().await?;
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------- IDLE

async fn idle_loop(ctx: Arc<AccountCtx>, tx: UnboundedSender<Command>) {
    let mut backoff = MIN_BACKOFF;
    loop {
        match idle_session(&ctx, &tx, &mut backoff).await {
            Ok(()) => return,
            Err(e) => {
                tracing::debug!(account = %ctx.account.id, error = %e, "IDLE bağlantısı kesildi");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

async fn idle_session(
    ctx: &AccountCtx,
    tx: &UnboundedSender<Command>,
    backoff: &mut Duration,
) -> SyncResult<()> {
    let mut session = ctx.connect().await?;
    let caps = timeout(ACTION_TIMEOUT, session.capabilities()).await??;
    *backoff = MIN_BACKOFF;
    if !caps.has_str("IDLE") {
        let _ = timeout(Duration::from_secs(5), session.logout()).await;
        loop {
            tokio::time::sleep(POLL).await;
            if tx.send(Command::SyncInbox).is_err() {
                return Ok(());
            }
        }
    }
    let inbox = ctx
        .store
        .mailbox_by_role(&ctx.account.id, MailboxRole::Inbox)?
        .map(|m| m.mailbox.path)
        .unwrap_or_else(|| "INBOX".into());
    timeout(ACTION_TIMEOUT, session.examine(&inbox)).await??;
    loop {
        let mut idle = session.idle();
        timeout(ACTION_TIMEOUT, idle.init()).await??;
        let (wait, stop) = idle.wait_with_timeout(IDLE_RENEW);
        let res = wait.await;
        drop(stop);
        let res = res?;
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

    fn l(path: &str, attr_role: Option<MailboxRole>, all: bool) -> Listed {
        Listed {
            path: path.into(),
            delimiter: Some("/".into()),
            attr_role,
            all,
            noselect: false,
        }
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
            l("Archive", None, false),
        ];
        let roles = assign_roles(&listed);
        let get = |p: &str| roles.iter().find(|r| r.0 == p).map(|r| (r.2, r.3));
        assert_eq!(get("[Gmail]"), None, "seçilemeyen kutu atlanır");
        assert_eq!(get("INBOX"), Some((MailboxRole::Inbox, false)));
        assert_eq!(get("[Gmail]/All Mail"), Some((MailboxRole::Archive, true)));
        assert_eq!(
            get("Archive"),
            Some((MailboxRole::Other, false)),
            "rol bir kez atanır"
        );
        assert_eq!(get("[Gmail]/Sent Mail"), Some((MailboxRole::Sent, false)));
        assert_eq!(
            roles.iter().find(|r| r.0 == "[Gmail]/Sent Mail").unwrap().1,
            "Sent Mail"
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
        let get = |p: &str| roles.iter().find(|r| r.0 == p).map(|r| (r.2, r.3)).unwrap();
        assert_eq!(get("Gonderilen-Ozel"), (MailboxRole::Sent, false));
        assert_eq!(get("Sent"), (MailboxRole::Other, false));
        assert_eq!(get("All"), (MailboxRole::Other, false));
        assert_eq!(get("Spam"), (MailboxRole::Junk, false));
    }

    #[test]
    fn komut_sirasi_ve_birlestirme() {
        let t = || Target {
            path: "INBOX".into(),
            uid: 1,
            uidvalidity: None,
        };
        let mut q: VecDeque<Command> = VecDeque::from([
            Command::SyncInbox,
            Command::SyncInbox,
            Command::Flags {
                ids: vec![1],
                items: vec![t()],
                unread: Some(false),
                starred: None,
            },
            Command::SyncAll,
            Command::SyncInbox,
        ]);
        assert!(matches!(next_command(&mut q), Some(Command::Flags { .. })));
        assert!(matches!(next_command(&mut q), Some(Command::SyncAll)));
        assert!(matches!(next_command(&mut q), Some(Command::SyncInbox)));
        assert!(next_command(&mut q).is_none());
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
            signature: None,
            created_at: 0,
            auth: Default::default(),
        };
        assert!(!is_syncable(&a));
        a.imap.as_mut().unwrap().host = "imap.ornek.com".into();
        assert!(is_syncable(&a));
        a.kind = AccountKind::Jmap;
        assert!(!is_syncable(&a));
    }
}
