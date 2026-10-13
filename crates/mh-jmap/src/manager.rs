//! Hesap başına JMAP eşitleme görevleri.
//!
//! Her JMAP hesabı için iki görev çalışır: sunucu işlemlerini ve eşitlemeyi
//! sırayla yürüten komut görevi ve durum değişikliği akışını (EventSource)
//! dinleyen görev. Akış yokken ya da koptuğunda iki dakikada bir yoklanır.
//! Bağlantı koparsa üstel bekleme ile yeniden denenir.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mh_core::{
    Account, AccountId, AccountKind, AppError, AppResult, ErrorCode, MailboxRole, MessageId,
    Millis, MoveTarget, SyncState, SyncStatus,
};
use mh_store::{NewMessage, Store};
use mh_sync::{EventSink, SecretStore};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Instant};

use crate::client::Client;
use crate::error::{JmapError, JmapResult};
use crate::{is_syncable, ops, push, send, sync};

/// Zamanlamalar (testlerde kısaltılabilir).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Durum akışı yokken yoklama aralığı.
    pub poll: Duration,
    /// Akış çalışsa da düzenli eşitleme.
    pub periodic: Duration,
    /// Sunucudan istenen canlılık sinyali aralığı (saniye).
    pub ping_secs: u32,
    pub min_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(120),
            periodic: Duration::from_secs(10 * 60),
            ping_secs: 60,
            min_backoff: Duration::from_secs(2),
            max_backoff: Duration::from_secs(300),
        }
    }
}

const ACTION_TIMEOUT: Duration = Duration::from_secs(120);
const SYNC_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const BODY_TIMEOUT: Duration = Duration::from_secs(90);

/// Taşıma hedefi.
enum Dest {
    /// Yerelde bilinen kutu (JMAP kutu kimliği).
    Path(String),
    /// Sunucuda henüz olmayabilir; gerekirse oluşturulur.
    Role(MailboxRole),
}

enum Command {
    /// Artımlı eşitleme.
    Sync,
    Flags {
        /// Yerel kimlikler: sunucu onayından sonra yerel değer yeniden yazılır.
        ids: Vec<MessageId>,
        jmap_ids: Vec<String>,
        unread: Option<bool>,
        starred: Option<bool>,
    },
    Move {
        /// (sunucu kimliği, kaynak kutu kimliği)
        items: Vec<(String, Option<String>)>,
        dest: Dest,
    },
}

impl Command {
    fn is_action(&self) -> bool {
        !matches!(self, Self::Sync)
    }
}

/// Sıradaki komut: önce sunucu işlemleri (yerel değişiklik eşitlemeyle
/// ezilmesin); yinelenen eşitleme istekleri birleştirilir.
fn next_command(pending: &mut VecDeque<Command>) -> Option<Command> {
    let idx = pending.iter().position(Command::is_action).unwrap_or(0);
    let cmd = pending.remove(idx)?;
    if matches!(cmd, Command::Sync) {
        pending.retain(Command::is_action);
    }
    Some(cmd)
}

fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as Millis)
        .unwrap_or(0)
}

fn not_found() -> AppError {
    AppError::new(
        ErrorCode::NotFound,
        "Posta bulunamadı; silinmiş ya da taşınmış olabilir.",
    )
}

struct Shared {
    store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
    sink: Arc<dyn EventSink>,
    /// Hesap başına açık oturum (komut, akış ve gövde indirme paylaşır).
    clients: Mutex<HashMap<AccountId, Arc<Client>>>,
    timing: Timing,
}

impl Shared {
    fn clients(&self) -> std::sync::MutexGuard<'_, HashMap<AccountId, Arc<Client>>> {
        self.clients.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn client(&self, account: &Account) -> JmapResult<Arc<Client>> {
        let cached = self
            .clients()
            .get(&account.id)
            .filter(|c| !c.is_stale())
            .cloned();
        if let Some(c) = cached {
            return Ok(c);
        }
        let c = Arc::new(crate::connect(&self.store, &self.secrets, account).await?);
        self.clients().insert(account.id.clone(), c.clone());
        Ok(c)
    }

    fn forget(&self, account_id: &str) {
        self.clients().remove(account_id);
    }

    fn changed(&self, mut mailboxes: Vec<String>) {
        mailboxes.sort();
        mailboxes.dedup();
        if !mailboxes.is_empty() {
            self.sink.changed(mailboxes);
        }
    }
}

struct Ctx {
    account: Account,
    shared: Arc<Shared>,
    /// Durum akışı bağlı mı (bağlı değilse yoklanır)?
    push_ok: AtomicBool,
    /// Son bildirilen durum "çevrimiçi" mi (gereksiz olay gönderilmesin)?
    online: AtomicBool,
}

impl Ctx {
    fn status(&self, state: SyncState) {
        let online = state == SyncState::Online;
        if online && self.online.swap(true, Ordering::SeqCst) {
            return;
        }
        if !online {
            self.online.store(false, Ordering::SeqCst);
        }
        self.shared.sink.sync_status(SyncStatus {
            account_id: self.account.id.clone(),
            state,
        });
    }

    fn emit_new(&self, ids: &[MessageId]) {
        if ids.is_empty() {
            return;
        }
        match self.shared.store.summaries(ids) {
            Ok(list) if !list.is_empty() => self.shared.sink.new_mail(&self.account, list),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "yeni posta özetleri okunamadı"),
        }
    }
}

struct Handle {
    tx: UnboundedSender<Command>,
    tasks: Vec<JoinHandle<()>>,
}

/// Tüm JMAP hesaplarının eşitlemesini yönetir.
pub struct JmapManager {
    shared: Arc<Shared>,
    rt: tokio::runtime::Handle,
    accounts: Mutex<HashMap<AccountId, Handle>>,
}

impl JmapManager {
    pub fn new(
        store: Arc<Store>,
        secrets: Arc<dyn SecretStore>,
        sink: Arc<dyn EventSink>,
        rt: tokio::runtime::Handle,
    ) -> Self {
        Self::with_timing(store, secrets, sink, rt, Timing::default())
    }

    pub fn with_timing(
        store: Arc<Store>,
        secrets: Arc<dyn SecretStore>,
        sink: Arc<dyn EventSink>,
        rt: tokio::runtime::Handle,
        timing: Timing,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                store,
                secrets,
                sink,
                clients: Mutex::default(),
                timing,
            }),
            rt,
            accounts: Mutex::default(),
        }
    }

    fn handles(&self) -> std::sync::MutexGuard<'_, HashMap<AccountId, Handle>> {
        self.accounts.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Kayıtlı tüm JMAP hesaplarını başlatır.
    pub fn start_all(&self) -> AppResult<()> {
        for a in self.shared.store.accounts()? {
            if a.kind == AccountKind::Jmap {
                self.start(a);
            }
        }
        Ok(())
    }

    /// Hesabın eşitleme görevlerini başlatır (zaten çalışıyorsa yeniden).
    /// Tanıtım hesabı ağa çıkmaz; yalnız "çevrimiçi" bildirilir.
    pub fn start(&self, account: Account) {
        self.stop(&account.id);
        if !is_syncable(&account) {
            self.shared.sink.sync_status(SyncStatus {
                account_id: account.id.clone(),
                state: SyncState::Online,
            });
            return;
        }
        let (tx, rx) = unbounded_channel();
        let ctx = Arc::new(Ctx {
            account: account.clone(),
            shared: self.shared.clone(),
            push_ok: AtomicBool::new(false),
            online: AtomicBool::new(false),
        });
        let tasks = vec![
            self.rt.spawn(account_loop(ctx.clone(), rx)),
            self.rt.spawn(push_loop(ctx, tx.clone())),
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
        self.shared.forget(account_id);
    }

    /// Hesap için çalışan görev var mı?
    pub fn is_running(&self, account_id: &str) -> bool {
        self.handles().contains_key(account_id)
    }

    fn send(&self, account_id: &str, cmd: Command) -> bool {
        self.handles()
            .get(account_id)
            .is_some_and(|h| h.tx.send(cmd).is_ok())
    }

    /// Tüm JMAP hesaplarını hemen eşitler.
    pub fn sync_now(&self) {
        for h in self.handles().values() {
            let _ = h.tx.send(Command::Sync);
        }
    }

    /// Bayrakları önce yerelde değiştirir, sonra sunucuya iletir. Etkilenen
    /// kutuları döner.
    pub fn set_flags(
        &self,
        ids: &[MessageId],
        unread: Option<bool>,
        starred: Option<bool>,
    ) -> AppResult<Vec<String>> {
        let store = &self.shared.store;
        let locations = store.jmap_locations(ids)?;
        let touched = store.set_flags(ids, unread, starred)?;
        let mut groups: HashMap<String, (Vec<MessageId>, Vec<String>)> = HashMap::new();
        for loc in locations {
            let g = groups.entry(loc.account_id).or_default();
            g.0.push(loc.id);
            if let Some(j) = loc.jmap_id {
                g.1.push(j);
            }
        }
        for (account, (ids, jmap_ids)) in groups {
            if !jmap_ids.is_empty() {
                self.send(
                    &account,
                    Command::Flags {
                        ids,
                        jmap_ids,
                        unread,
                        starred,
                    },
                );
            }
        }
        Ok(touched)
    }

    /// Postaları önce yerelde taşır, sonra sunucuya iletir. Hedef rolde kutu
    /// yerelde yoksa (sunucuda ilk kez oluşturulacak) yerel taşıma eşitlemeye
    /// kalır.
    pub fn move_messages(&self, ids: &[MessageId], target: MoveTarget) -> AppResult<Vec<String>> {
        let role = match target {
            MoveTarget::Inbox => MailboxRole::Inbox,
            MoveTarget::Archive => MailboxRole::Archive,
            MoveTarget::Trash => MailboxRole::Trash,
        };
        let store = &self.shared.store;
        let mut groups: HashMap<String, Vec<mh_store::JmapLocation>> = HashMap::new();
        for loc in store.jmap_locations(ids)? {
            groups.entry(loc.account_id.clone()).or_default().push(loc);
        }
        let mut touched = Vec::new();
        for (account, list) in groups {
            let dest = store.mailbox_by_role(&account, role)?;
            let items: Vec<(String, Option<String>)> = list
                .iter()
                .filter(|l| {
                    dest.as_ref()
                        .is_none_or(|d| d.mailbox.path != l.mailbox_path)
                })
                .filter_map(|l| Some((l.jmap_id.clone()?, Some(l.mailbox_path.clone()))))
                .collect();
            let local: Vec<MessageId> = list.iter().map(|l| l.id).collect();
            let dest = match dest {
                Some(d) => {
                    touched.extend(store.move_local(&local, &d.mailbox.id)?);
                    Dest::Path(d.mailbox.path)
                }
                None => Dest::Role(role),
            };
            if !items.is_empty() {
                self.send(&account, Command::Move { items, dest });
            }
        }
        touched.sort();
        touched.dedup();
        Ok(touched)
    }

    /// Yalnız başlığı eşitlenmiş postanın gövdesini indirir.
    pub async fn fetch_body(&self, id: MessageId) -> AppResult<()> {
        let store = &self.shared.store;
        let loc = store
            .jmap_locations(&[id])?
            .into_iter()
            .next()
            .ok_or_else(not_found)?;
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
        let jmap_id = loc.jmap_id.ok_or_else(|| {
            AppError::new(
                ErrorCode::Network,
                "Posta henüz sunucuyla eşitlenmedi. Birazdan yeniden dene.",
            )
        })?;
        let work = async {
            let client = self.shared.client(&account).await?;
            sync::fetch_body(&client, store, id, &jmap_id, loc.blob_id.as_deref()).await
        };
        let res = match timeout(BODY_TIMEOUT, work).await {
            Ok(r) => r,
            Err(_) => Err(JmapError::Timeout),
        };
        res.map_err(|e| {
            if e.is_connection() || matches!(e, JmapError::Auth) {
                self.shared.forget(&account.id);
            }
            e.to_app()
        })
    }

    /// Taslağı kaydeder: sunucudaki Taslaklar kutusuna `$draft` ile eklenir,
    /// `replace` verilirse önceki taslak sunucudan ve yerelden silinir.
    /// Tanıtım hesabında yalnız yereldir. Yeni taslağın yerel kimliğini döner.
    pub async fn save_draft(
        &self,
        account_id: &str,
        raw: Vec<u8>,
        replace: Option<MessageId>,
    ) -> AppResult<MessageId> {
        let store = &self.shared.store;
        let account = store
            .account(account_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Hesap bulunamadı."))?;
        let old = match replace {
            Some(id) => store
                .jmap_locations(&[id])?
                .into_iter()
                .next()
                .filter(|l| l.account_id == account.id),
            None => None,
        };
        let mut client = None;
        let mut saved = None;
        if is_syncable(&account) {
            let c = self.shared.client(&account).await.map_err(|e| e.to_app())?;
            let s = send::save_draft(&c, &raw, old.as_ref().and_then(|l| l.jmap_id.as_deref()))
                .await
                .map_err(|e| e.to_app())?;
            client = Some(c);
            saved = Some(s);
        }
        let drafts = match store.mailbox_by_role(&account.id, MailboxRole::Drafts)? {
            Some(m) => m.mailbox.id,
            None => match &client {
                Some(c) => {
                    sync::sync_mailboxes(c, store, &account.id)
                        .await
                        .map_err(|e| e.to_app())?;
                    store
                        .mailbox_by_role(&account.id, MailboxRole::Drafts)?
                        .map(|m| m.mailbox.id)
                        .ok_or_else(|| {
                            AppError::new(ErrorCode::Internal, "Taslaklar kutusu bulunamadı.")
                        })?
                }
                None => {
                    store.upsert_mailbox(&account.id, "Drafts", "Drafts", MailboxRole::Drafts)?
                }
            },
        };
        let mut m = NewMessage::from_raw(raw, false, &account.id, &drafts, now_ms())
            .ok_or_else(|| AppError::new(ErrorCode::Invalid, "Taslak ayrıştırılamadı."))?;
        m.unread = false;
        let id = match &saved {
            Some(s) => store.jmap_upsert(&m, &s.id, s.blob_id.as_deref(), None)?.id,
            None => store.upsert_message(&m)?.id,
        };
        let mut touched = vec![drafts];
        if let Some(old) = old.filter(|o| o.id != id) {
            store.delete_messages(&[old.id])?;
            touched.push(old.mailbox_id);
        }
        self.shared.changed(touched);
        Ok(id)
    }

    /// Taslağı sunucudan ve yerelden siler.
    pub async fn delete_draft(&self, id: MessageId) -> AppResult<()> {
        let store = &self.shared.store;
        let loc = store
            .jmap_locations(&[id])?
            .into_iter()
            .next()
            .ok_or_else(not_found)?;
        let account = store
            .account(&loc.account_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Postanın hesabı bulunamadı."))?;
        if let (true, Some(jmap_id)) = (is_syncable(&account), loc.jmap_id.as_deref()) {
            let c = self.shared.client(&account).await.map_err(|e| e.to_app())?;
            send::delete_draft(&c, jmap_id)
                .await
                .map_err(|e| e.to_app())?;
        }
        store.delete_messages(&[id])?;
        self.shared.changed(vec![loc.mailbox_id]);
        Ok(())
    }
}

// ------------------------------------------------------------ komut görevi

async fn account_loop(ctx: Arc<Ctx>, mut rx: UnboundedReceiver<Command>) {
    let timing = ctx.shared.timing;
    let mut pending: VecDeque<Command> = VecDeque::from([Command::Sync]);
    let mut backoff = timing.min_backoff;
    loop {
        let err = match run_session(&ctx, &mut rx, &mut pending, &mut backoff).await {
            Ok(()) => return,
            Err(e) => e,
        };
        ctx.shared.forget(&ctx.account.id);
        let app = err.to_app();
        tracing::warn!(account = %ctx.account.id, error = %err, "JMAP eşitlemesi kesildi");
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
                    Some(Command::Sync) => break,
                    Some(c) => pending.push_back(c),
                },
            }
        }
        if !pending.iter().any(|c| matches!(c, Command::Sync)) {
            pending.push_front(Command::Sync);
        }
        backoff = (backoff * 2).min(timing.max_backoff);
    }
}

async fn run_session(
    ctx: &Ctx,
    rx: &mut UnboundedReceiver<Command>,
    pending: &mut VecDeque<Command>,
    backoff: &mut Duration,
) -> JmapResult<()> {
    let timing = ctx.shared.timing;
    let mut client = ctx.shared.client(&ctx.account).await?;
    *backoff = timing.min_backoff;
    let mut poll = tokio::time::interval_at(Instant::now() + timing.poll, timing.poll);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut periodic = tokio::time::interval_at(Instant::now() + timing.periodic, timing.periodic);
    periodic.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        while let Ok(c) = rx.try_recv() {
            pending.push_back(c);
        }
        while let Some(cmd) = next_command(pending) {
            if client.is_stale() {
                ctx.shared.forget(&ctx.account.id);
                client = ctx.shared.client(&ctx.account).await?;
            }
            let limit = if cmd.is_action() {
                ACTION_TIMEOUT
            } else {
                SYNC_TIMEOUT
            };
            let res = match timeout(limit, execute(ctx, &client, &cmd)).await {
                Ok(r) => r,
                Err(_) => Err(JmapError::Timeout),
            };
            if let Err(e) = res {
                if e.is_connection() || matches!(e, JmapError::Auth) {
                    pending.push_front(cmd);
                    return Err(e);
                }
                tracing::warn!(account = %ctx.account.id, error = %e, "JMAP işlemi başarısız");
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
                None => return Ok(()),
                Some(c) => pending.push_back(c),
            },
            _ = poll.tick() => {
                if !ctx.push_ok.load(Ordering::SeqCst) {
                    pending.push_back(Command::Sync);
                }
            }
            _ = periodic.tick() => pending.push_back(Command::Sync),
        }
    }
}

async fn run_sync(ctx: &Ctx, client: &Client) -> JmapResult<()> {
    let store = &ctx.shared.store;
    let account = &ctx.account.id;
    let first = store.jmap_account_state(account)?.email_state.is_none();
    if first {
        ctx.status(SyncState::Syncing { done: 0, total: 0 });
    }
    let progress = |done: u32, total: u32| {
        if first {
            ctx.status(SyncState::Syncing { done, total });
        }
    };
    let out = sync::incremental_sync(client, store, account, &progress).await?;
    ctx.status(SyncState::Online);
    ctx.shared.changed(out.changed);
    ctx.emit_new(&out.new_unread);
    if !out.prefetch.is_empty() {
        let done = sync::prefetch(client, store, &out.prefetch).await;
        if !done.is_empty() {
            let boxes = store
                .jmap_locations(&done)?
                .into_iter()
                .map(|l| l.mailbox_id)
                .collect();
            ctx.shared.changed(boxes);
        }
    }
    Ok(())
}

async fn execute(ctx: &Ctx, client: &Client, cmd: &Command) -> JmapResult<()> {
    match cmd {
        Command::Sync => run_sync(ctx, client).await,
        Command::Flags {
            ids,
            jmap_ids,
            unread,
            starred,
        } => {
            ops::set_keywords(client, jmap_ids, *unread, *starred).await?;
            // Arada çalışan eşitleme eski değeri yazmış olabilir.
            let touched = ctx.shared.store.set_flags(ids, *unread, *starred)?;
            ctx.shared.changed(touched);
            Ok(())
        }
        Command::Move { items, dest } => {
            let dest = match dest {
                Dest::Path(p) => p.clone(),
                Dest::Role(r) => ops::ensure_role_mailbox(client, *r).await?,
            };
            ops::move_emails(client, items, &dest).await?;
            run_sync(ctx, client).await
        }
    }
}

// ---------------------------------------------------------- durum akışı

async fn push_loop(ctx: Arc<Ctx>, tx: UnboundedSender<Command>) {
    let timing = ctx.shared.timing;
    let mut backoff = timing.min_backoff;
    let idle = Duration::from_secs(u64::from(timing.ping_secs.max(5)) * 3);
    loop {
        let res = async {
            let client = ctx.shared.client(&ctx.account).await?;
            push::listen(
                &client,
                timing.ping_secs,
                idle,
                || {
                    ctx.push_ok.store(true, Ordering::SeqCst);
                    // Kopukken kaçan değişiklikleri al.
                    let _ = tx.send(Command::Sync);
                },
                || {
                    let _ = tx.send(Command::Sync);
                },
            )
            .await
        }
        .await;
        let was_connected = ctx.push_ok.swap(false, Ordering::SeqCst);
        if tx.is_closed() {
            return;
        }
        if let Err(e) = res {
            tracing::debug!(account = %ctx.account.id, error = %e, "durum akışı kesildi");
            if matches!(e, JmapError::Auth) || e.is_connection() {
                ctx.shared.forget(&ctx.account.id);
            }
        }
        if was_connected {
            backoff = timing.min_backoff;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(timing.max_backoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn komut_sirasi_ve_birlestirme() {
        let mut q: VecDeque<Command> = VecDeque::from([
            Command::Sync,
            Command::Sync,
            Command::Flags {
                ids: vec![1],
                jmap_ids: vec!["M1".into()],
                unread: Some(false),
                starred: None,
            },
            Command::Sync,
        ]);
        assert!(matches!(next_command(&mut q), Some(Command::Flags { .. })));
        assert!(matches!(next_command(&mut q), Some(Command::Sync)));
        assert!(next_command(&mut q).is_none());
    }
}
