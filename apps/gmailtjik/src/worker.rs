use crate::{
    google::Google,
    model::{Contact, Label, Message, Page},
    store::Store,
};
use anyhow::Result;
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

pub enum Command {
    Filters(crate::filters::Request),
    OpenMail(crate::links::MailLink),
    Connect(PathBuf),
    Sync,
    PushSync(mpsc::Sender<bool>),
    WakeActions,
    Thread(String),
    Load {
        ticket: u64,
        label: String,
        query: String,
        page: String,
        force: bool,
    },
    ContactPhotos(Vec<crate::contacts::PhotoRequest>),
    Contacts {
        query: String,
    },
    Images {
        generation: u64,
        message: Message,
        remote: bool,
    },
}
pub enum Event {
    Filters(Result<crate::filters::Outcome, String>),
    QueueChanged,
    OpenMail(Message),
    SyncComplete,
    ContactBook(Vec<Contact>),
    ContactPhoto {
        url: String,
        bytes: Vec<u8>,
    },
    Thread {
        id: String,
        messages: Vec<Message>,
    },
    Connected,
    ConnectionFailed(String),
    Inbox(Page),
    SyncError(String),
    Ready {
        labels: Vec<Label>,
        email: String,
        counts_observed_at: u64,
    },
    Page {
        ticket: u64,
        cached: bool,
        complete: bool,
        page: Page,
    },
    Loaded {
        ticket: u64,
    },
    LoadError {
        ticket: u64,
        error: String,
    },
    Error(String),
    Contacts {
        query: String,
        rows: Vec<Contact>,
    },
    Images {
        generation: u64,
        remote: bool,
        id: String,
        images: Vec<(String, Vec<u8>)>,
    },
}
pub fn key(label: &str, query: &str, page: &str) -> String {
    serde_json::to_string(&(label, query, page)).unwrap()
}

fn check_cooldown(store: &Store) -> Result<()> {
    let remaining = store.rate_limit_until()?.saturating_sub(crate::auth::now());
    anyhow::ensure!(
        remaining == 0,
        "Google is limiting requests. Cached mail remains available; refresh can retry in {remaining} seconds."
    );
    Ok(())
}

fn filter_request(
    google: &mut Google,
    store: &Store,
    request: crate::filters::Request,
) -> Result<crate::filters::Outcome> {
    use crate::filters::{Outcome, Request};
    let mut outcome = Outcome {
        saved: None,
        deleted: None,
        filters: None,
        note: String::new(),
    };
    match request {
        Request::List => {
            let filters = google.filters()?;
            store.put("gmail_filters", &filters)?;
            outcome.filters = Some(filters);
            outcome.note = "Filters loaded from Gmail".into();
        }
        Request::Delete(id) => {
            google.delete_filter(&id)?;
            outcome.deleted = Some(id);
            outcome.note = "Filter deleted; existing mail is unchanged".into();
        }
        Request::Save { filter, apply } => {
            filter.validate(apply)?;
            let saved = google.create_filter(&filter)?;
            outcome.saved = Some(saved.clone());
            outcome.note = "Filter saved in Gmail".into();
            if !filter.id.is_empty() {
                if let Err(error) = google.delete_filter(&filter.id) {
                    remember_rate_limit(store, &error);
                    outcome.note = format!(
                        "New filter saved, but the old filter still exists: {error}. Refresh the list before deleting the old one."
                    );
                    return Ok(outcome);
                }
                outcome.deleted = Some(filter.id);
            }
            if apply {
                outcome.note = match google.apply_filter(store, &saved) {
                    Ok(count) => format!("Filter saved · applied to {count} matching messages"),
                    Err(error) => {
                        remember_rate_limit(store, &error);
                        format!("Filter saved. Existing mail: {error:#}")
                    }
                };
            }
        }
        Request::Apply(filter) => {
            let count = google.apply_filter(store, &filter)?;
            outcome.note = format!("Applied to {count} matching messages");
        }
    }
    Ok(outcome)
}

fn remember_rate_limit(store: &Store, error: &anyhow::Error) {
    if let Some(until) = crate::google::rate_limit_until(error) {
        let _ = store.defer_requests(until);
    }
}

struct LoadRequest {
    ticket: u64,
    label: String,
    query: String,
    page: String,
    force: bool,
}

struct ReadRequest {
    ticket: u64,
    id: String,
    open: Option<crate::links::MailLink>,
}

#[derive(Default)]
struct ReadGate(Mutex<(u64, String)>);

impl ReadGate {
    fn select(&self, id: &str) -> u64 {
        let mut state = self.0.lock().unwrap();
        state.0 += 1;
        state.1 = id.into();
        state.0
    }

    fn current(&self, request: &ReadRequest) -> bool {
        let state = self.0.lock().unwrap();
        if request.open.is_some() {
            state.0 == request.ticket
        } else {
            state.1 == request.id
        }
    }
}

struct ImageRequest {
    generation: u64,
    message: Message,
    remote: bool,
}

#[derive(Default)]
struct Foreground {
    changed_at: AtomicU64,
    active: AtomicU64,
    label: Mutex<String>,
}

impl Foreground {
    fn touch(&self) {
        self.changed_at.store(now_ms(), Ordering::SeqCst);
    }

    fn busy(&self) -> bool {
        self.active.load(Ordering::SeqCst) != 0
            || now_ms().saturating_sub(self.changed_at.load(Ordering::SeqCst)) < 5000
    }

    fn work(self: &Arc<Self>) -> ForegroundWork {
        self.active.fetch_add(1, Ordering::SeqCst);
        ForegroundWork(self.clone())
    }
}

struct ForegroundWork(Arc<Foreground>);
impl Drop for ForegroundWork {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
        self.0.touch();
    }
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

#[derive(Default)]
struct LoadState {
    latest: Option<u64>,
    // Odd while a local mailbox mutation is running; even when reads may start.
    epoch: u64,
    closed: bool,
}

#[derive(Default)]
struct LoadGate {
    state: Mutex<LoadState>,
    wake: Condvar,
}

impl LoadGate {
    fn select(&self, ticket: u64) {
        self.state.lock().unwrap().latest = Some(ticket);
        self.wake.notify_all();
    }

    fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.wake.notify_all();
    }

    fn epoch(&self, ticket: u64) -> Option<u64> {
        let mut state = self.state.lock().unwrap();
        while !state.closed && state.latest == Some(ticket) && state.epoch % 2 != 0 {
            state = self.wake.wait(state).unwrap();
        }
        (!state.closed && state.latest == Some(ticket)).then_some(state.epoch)
    }

    // Only bounded local cache work and event delivery belong under this lock.
    // Network requests run outside it so mutations can invalidate their results.
    fn current<T>(&self, ticket: u64, epoch: u64, apply: impl FnOnce() -> T) -> Option<T> {
        let state = self.state.lock().unwrap();
        if state.closed || state.latest != Some(ticket) || state.epoch != epoch {
            return None;
        }
        Some(apply())
    }

    fn mutation(self: &Arc<Self>) -> Mutation {
        self.state.lock().unwrap().epoch += 1;
        Mutation(self.clone())
    }

    fn invalidate(&self) {
        self.state.lock().unwrap().epoch += 2;
        self.wake.notify_all();
    }
}

struct Mutation(Arc<LoadGate>);

impl Drop for Mutation {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().epoch += 1;
        self.0.wake.notify_all();
    }
}

fn route_commands(
    rx: mpsc::Receiver<Command>,
    work: mpsc::Sender<Command>,
    loads: mpsc::Sender<LoadRequest>,
    reads: mpsc::Sender<ReadRequest>,
    images: mpsc::Sender<ImageRequest>,
    actions: mpsc::Sender<()>,
    gate: Arc<LoadGate>,
    read_generation: Arc<ReadGate>,
    image_generation: Arc<AtomicU64>,
    foreground: Arc<Foreground>,
) {
    while let Ok(command) = rx.recv() {
        if !matches!(
            command,
            Command::Sync | Command::PushSync(_) | Command::ContactPhotos(_)
        ) {
            foreground.touch();
        }
        let closed = match command {
            Command::Load {
                ticket,
                label,
                query,
                page,
                force,
            } => {
                *foreground.label.lock().unwrap() = label.clone();
                gate.select(ticket);
                loads
                    .send(LoadRequest {
                        ticket,
                        label,
                        query,
                        page,
                        force,
                    })
                    .is_err()
            }
            Command::Thread(id) => {
                let ticket = read_generation.select(&id);
                reads
                    .send(ReadRequest {
                        ticket,
                        id,
                        open: None,
                    })
                    .is_err()
            }
            Command::OpenMail(link) => {
                let ticket = read_generation.select(&link.thread);
                reads
                    .send(ReadRequest {
                        ticket,
                        id: link.thread.clone(),
                        open: Some(link),
                    })
                    .is_err()
            }
            Command::Images {
                generation,
                message,
                remote,
            } => {
                image_generation.fetch_max(generation, Ordering::SeqCst);
                images
                    .send(ImageRequest {
                        generation,
                        message,
                        remote,
                    })
                    .is_err()
            }
            Command::WakeActions => {
                let _ = actions.send(());
                false
            }
            command => work.send(command).is_err(),
        };
        if closed {
            break;
        }
    }
    gate.close();
}

fn load_page(
    request: &LoadRequest,
    epoch: u64,
    dir: &Path,
    store: &Store,
    connection: &mut Option<Google>,
    gate: &LoadGate,
    events: &mpsc::Sender<Event>,
) -> Result<Option<()>> {
    if !request.label.is_empty() && request.query.is_empty() && request.page.is_empty() {
        let mut recent = store
            .get::<Vec<String>>("recent_folders")?
            .unwrap_or_default();
        recent.retain(|label| label != &request.label);
        recent.insert(0, request.label.clone());
        recent.truncate(8);
        store.put("recent_folders", &recent)?;
    }
    let cache_key = key(&request.label, &request.query, &request.page);
    let cached = store.cached_page(&request.label, &request.query, &request.page)?;
    let had_cache = cached.is_some();
    let fresh = had_cache
        && !request.force
        && store.page_is_fresh(&request.label, &request.query, &request.page, 300)?;
    if gate
        .current(request.ticket, epoch, || {
            if let Some(page) = cached {
                let _ = events.send(Event::Page {
                    ticket: request.ticket,
                    cached: true,
                    complete: fresh,
                    page,
                });
            }
        })
        .is_none()
    {
        return Ok(None);
    }
    if fresh {
        return gate
            .current(request.ticket, epoch, || {
                let _ = events.send(Event::Loaded {
                    ticket: request.ticket,
                });
                Ok(())
            })
            .transpose();
    }
    check_cooldown(store)?;
    if connection.is_none() {
        let mut google = Google::open(dir)?;
        let email = google.email()?;
        if store
            .get::<String>("email")?
            .is_some_and(|old| old != email)
        {
            anyhow::bail!(
                "This cache belongs to another account. Use a separate GMAILTJIK_DATA_DIR"
            );
        }
        *connection = Some(google);
    }
    if gate.current(request.ticket, epoch, || ()).is_none() {
        return Ok(None);
    }
    let mut cache_error = None;
    let read_started = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let page_result = connection.as_mut().unwrap().thread_summaries(
        &request.label,
        &request.query,
        &request.page,
        |page| {
            match gate.current(request.ticket, epoch, || -> Result<()> {
                // Keep every useful batch across revisits and restarts, even
                // when a later request is rejected. Complete pages stay intact.
                let mut page = page.clone();
                store.observe_messages(&mut page.messages, read_started)?;
                store.save_partial_page(&cache_key, &page)?;
                store.confirm_actions(&page.messages, read_started)?;
                // Keep a selected cached conversation stable until all 25 rows
                // are ready; first visits can render each arriving batch.
                if !had_cache {
                    store.project_page(&request.label, &request.query, &request.page, &mut page)?;
                    // Older must not skip rows that have not arrived if a later
                    // batch fails. Only the completed page owns its next cursor.
                    page.next.clear();
                    let _ = events.send(Event::Page {
                        ticket: request.ticket,
                        cached: false,
                        complete: false,
                        page,
                    });
                }
                Ok(())
            }) {
                Some(Ok(())) => true,
                Some(Err(error)) => {
                    cache_error = Some(error);
                    false
                }
                None => false,
            }
        },
    );
    if let Some(error) = cache_error {
        return Err(error);
    }
    let page = page_result?;
    let Some(mut page) = page else {
        return Ok(None);
    };
    gate.current(request.ticket, epoch, || -> Result<()> {
        // Metadata stays in the page cache. Full messages and reader threads
        // are populated only by their existing body-fetch paths.
        store.observe_messages(&mut page.messages, read_started)?;
        store.save_page(&cache_key, &page)?;
        store.confirm_actions(&page.messages, read_started)?;
        store.confirm_folder_snapshot(&cache_key, read_started)?;
        store.project_page(&request.label, &request.query, &request.page, &mut page)?;
        let _ = events.send(Event::Page {
            ticket: request.ticket,
            cached: false,
            complete: true,
            page,
        });
        let _ = events.send(Event::Loaded {
            ticket: request.ticket,
        });
        Ok(())
    })
    .transpose()
}

fn load_worker(
    dir: PathBuf,
    rx: mpsc::Receiver<LoadRequest>,
    events: mpsc::Sender<Event>,
    gate: Arc<LoadGate>,
    foreground: Arc<Foreground>,
) {
    let mut store = None;
    let mut connection = None;
    while let Ok(mut request) = rx.recv() {
        // Pending navigation is replaceable; never fetch folders already left.
        for newer in rx.try_iter() {
            request = newer;
        }
        while let Some(epoch) = gate.epoch(request.ticket) {
            let result = (|| -> Result<Option<()>> {
                let _foreground = foreground.work();
                if store.is_none() {
                    store = Some(Store::open(Some(&dir))?);
                }
                load_page(
                    &request,
                    epoch,
                    &dir,
                    store.as_ref().unwrap(),
                    &mut connection,
                    &gate,
                    &events,
                )
            })();
            match result {
                Ok(Some(())) => break,
                // A mutation invalidates successful and failed in-flight reads.
                // Retry only if this folder is still requested after it completes.
                Ok(None) => continue,
                Err(error) => {
                    // Throttling applies to the account even if navigation has
                    // moved on since this particular request was started.
                    if let Some(store) = store.as_ref() {
                        remember_rate_limit(store, &error);
                    }
                    if gate
                        .current(request.ticket, epoch, || {
                            let _ = events.send(Event::LoadError {
                                ticket: request.ticket,
                                error: error.to_string(),
                            });
                            let _ = events.send(Event::Loaded {
                                ticket: request.ticket,
                            });
                        })
                        .is_some()
                    {
                        break;
                    }
                }
            }
        }
    }
}

fn open_mail_connection(dir: &Path, store: &Store) -> Result<(Google, String)> {
    let mut google = Google::open(dir)?;
    let email = google.email()?;
    if store
        .get::<String>("email")?
        .is_some_and(|old| old != email)
    {
        anyhow::bail!("This cache belongs to another account. Use a separate GMAILTJIK_DATA_DIR");
    }
    store.put("email", &email)?;
    Ok((google, email))
}

#[derive(Default)]
struct LabelSnapshot {
    labels: Vec<Label>,
    counts_observed_at: u64,
}

// A receipt is written only after notification scanning succeeds. Advancing the
// history cursor alone must never suppress a retry of an interrupted sync.
fn unchanged_sync_page(store: &Store) -> Result<Option<Page>> {
    let history = store.get::<String>("history")?;
    if history.is_none()
        || history != store.get::<String>("completed_sync_history")?
        || !store.page_is_fresh("INBOX", "", "", 900)?
    {
        return Ok(None);
    }
    store.cached_page("INBOX", "", "")
}

fn cached_labels(store: &Store) -> Result<Option<LabelSnapshot>> {
    let transaction = store.db.unchecked_transaction()?;
    let labels = store.get("labels")?;
    let counts_observed_at = store.get("labels_counts_observed_at")?.unwrap_or(0);
    transaction.commit()?;
    Ok(labels.map(|labels| LabelSnapshot {
        labels,
        counts_observed_at,
    }))
}

fn sync_labels(google: &mut Google, store: &Store) -> Result<LabelSnapshot> {
    sync_labels_with(store, || google.labels())
}

fn sync_labels_with(
    store: &Store,
    fetch: impl FnOnce() -> Result<Vec<Label>>,
) -> Result<LabelSnapshot> {
    let now = crate::auth::now();
    if store
        .get::<u64>("labels_checked_at")?
        .is_some_and(|last| now.saturating_sub(last) < 300)
        && let Some(snapshot) = cached_labels(store)?
    {
        return Ok(snapshot);
    }
    // Counts cannot confirm a mutation completed after this request began.
    // Cache reads must preserve this timestamp instead of appearing newer.
    let counts_observed_at = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let labels = fetch()?;
    let transaction = store.db.unchecked_transaction()?;
    store.put("labels", &labels)?;
    store.put("labels_counts_observed_at", &counts_observed_at)?;
    store.put("labels_checked_at", &now)?;
    transaction.commit()?;
    Ok(LabelSnapshot {
        labels,
        counts_observed_at,
    })
}

fn emit_thread(
    request: &ReadRequest,
    messages: Vec<Message>,
    events: &mpsc::Sender<Event>,
) -> Result<()> {
    let event = if let Some(link) = &request.open {
        Event::OpenMail(link.select(messages)?)
    } else {
        Event::Thread {
            id: request.id.clone(),
            messages,
        }
    };
    let _ = events.send(event);
    Ok(())
}

fn read_worker(
    dir: PathBuf,
    rx: Arc<Mutex<mpsc::Receiver<ReadRequest>>>,
    events: mpsc::Sender<Event>,
    gate: Arc<ReadGate>,
    in_flight: Arc<Mutex<std::collections::HashSet<String>>>,
    foreground: Arc<Foreground>,
) {
    let result = (|| -> Result<()> {
        let store = Store::open(Some(&dir))?;
        let mut connection = None;
        loop {
            let request = {
                let rx = rx.lock().unwrap();
                let Ok(mut request) = rx.recv() else { break };
                for newer in rx.try_iter() {
                    request = newer;
                }
                request
            };
            if !gate.current(&request) {
                continue;
            }
            let result = (|| -> Result<()> {
                let _foreground = foreground.work();
                if let Some(link) = &request.open {
                    link.check_account(&store.get::<String>("email")?.unwrap_or_default())?;
                }
                if let Some(messages) = store.cached_thread(&request.id)? {
                    emit_thread(&request, messages, &events)?;
                    if store.thread_is_fresh(&request.id, 900)? {
                        return Ok(());
                    }
                }
                check_cooldown(&store)?;
                if !in_flight.lock().unwrap().insert(request.id.clone()) {
                    return Ok(());
                }
                let fetched = (|| -> Result<()> {
                    if connection.is_none() {
                        connection = Some(open_mail_connection(&dir, &store)?);
                    }
                    let (google, email) = connection.as_mut().unwrap();
                    if let Some(link) = &request.open {
                        link.check_account(email)?;
                    }
                    if !gate.current(&request) {
                        return Ok(());
                    }
                    let started = chrono::Utc::now().timestamp_millis().max(0) as u64;
                    let mut messages = google.thread(&store, &request.id)?;
                    store.confirm_actions(&messages, started)?;
                    store.project_messages(&mut messages)?;
                    if gate.current(&request) {
                        emit_thread(&request, messages, &events)?;
                    }
                    Ok(())
                })();
                in_flight.lock().unwrap().remove(&request.id);
                fetched
            })();
            if let Err(error) = result {
                remember_rate_limit(&store, &error);
                if gate.current(&request) {
                    let _ = events.send(Event::Error(error.to_string()));
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = events.send(Event::Error(error.to_string()));
    }
}

fn image_worker(
    dir: PathBuf,
    rx: Arc<Mutex<mpsc::Receiver<ImageRequest>>>,
    events: mpsc::Sender<Event>,
    generation: Arc<AtomicU64>,
    completed: Arc<Mutex<std::collections::HashSet<(u64, String, bool)>>>,
) {
    let result = (|| -> Result<()> {
        let store = Store::open(Some(&dir))?;
        let mut connection = None;
        loop {
            let request = {
                let Ok(request) = rx.lock().unwrap().recv() else {
                    break;
                };
                request
            };
            if request.generation < generation.load(Ordering::SeqCst) {
                continue;
            }
            let key = (
                request.generation,
                request.message.id.clone(),
                request.remote,
            );
            {
                let mut completed = completed.lock().unwrap();
                completed.retain(|(old, _, _)| *old >= request.generation);
                if !completed.insert(key.clone()) {
                    continue;
                }
            }
            let result = (|| -> Result<()> {
                check_cooldown(&store)?;
                if connection.is_none() {
                    connection = Some(Google::open(&dir)?);
                }
                let images = connection
                    .as_mut()
                    .unwrap()
                    .images(&request.message, request.remote)?;
                if request.generation == generation.load(Ordering::SeqCst) {
                    let _ = events.send(Event::Images {
                        generation: request.generation,
                        remote: request.remote,
                        id: request.message.id,
                        images,
                    });
                }
                Ok(())
            })();
            if let Err(error) = result {
                completed.lock().unwrap().remove(&key);
                remember_rate_limit(&store, &error);
                if request.generation == generation.load(Ordering::SeqCst) {
                    let _ = events.send(Event::Error(error.to_string()));
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = events.send(Event::Error(error.to_string()));
    }
}

fn action_lock(dir: &Path) -> Result<Option<File>> {
    account_lock(dir, "actions.lock")
}

fn account_lock(dir: &Path, name: &str) -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(dir.join(name))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn claim_prefetch(dir: &Path, store: &Store, now: u64) -> Result<Option<File>> {
    let Some(lock) = account_lock(dir, "prefetch.lock")? else {
        return Ok(None);
    };
    if store.get::<u64>("prefetch_not_before")?.unwrap_or(0) > now {
        return Ok(None);
    }
    store.put("prefetch_not_before", &now.saturating_add(300))?;
    Ok(Some(lock))
}

pub(crate) fn execute_action(
    record: &crate::actions::QueuedAction,
    google: &mut Google,
    email: &str,
    store: &Store,
) -> Result<(Vec<String>, crate::actions::ActionResult)> {
    use crate::actions::{Action, ActionResult};
    let mut result = ActionResult::default();
    let ids = match &record.action {
        Action::Labels {
            id,
            thread,
            add,
            remove,
        } => {
            if thread.is_empty() {
                google.modify(id, add, remove)?;
                vec![id.clone()]
            } else {
                google.modify_thread(thread, add, remove, false)?
            }
        }
        Action::Trash { id, thread } => {
            if thread.is_empty() {
                google.trash(id)?;
                vec![id.clone()]
            } else {
                google.modify_thread(thread, &["TRASH".into()], &["INBOX".into()], true)?
            }
        }
        Action::MarkAllRead { label, query } => {
            let ids = if let Some(ids) = &record.target_ids {
                ids.clone()
            } else {
                let ids = google.unread_ids(label, query)?;
                store.prepare_action_targets(record.id, &ids)?
            };
            let remaining: Vec<_> = ids
                .iter()
                .filter(|id| !record.confirmed_ids.contains(id))
                .cloned()
                .collect();
            google.mark_messages_read(&remaining, |ids| {
                store.record_action_progress(record.id, ids)
            })?;
            ids
        }
        Action::EmptyFolder { label } => {
            result.count = google.empty_folder(store, label)?;
            vec![]
        }
        Action::SaveDraft { draft } => {
            result.draft_id = google.save_draft(draft, email)?;
            vec![]
        }
        Action::Send { draft } => {
            google.send(draft, email)?;
            vec![]
        }
    };
    if !ids.is_empty() {
        result.count = ids.len();
    }
    Ok((ids, result))
}

/// One durable attempt. Retrying the head preserves the user's action order.
fn deliver_action(
    store: &Store,
    now: u64,
    mut execute: impl FnMut(
        &crate::actions::QueuedAction,
        &mut bool,
    ) -> Result<(Vec<String>, crate::actions::ActionResult)>,
) -> Result<bool> {
    if store.rate_limit_until()? > now {
        return Ok(false);
    }
    let Some(record) = store.claim_action(now)? else {
        return Ok(false);
    };
    let mut prepared = false;
    match execute(&record, &mut prepared) {
        Ok((ids, result)) => store.complete_action_result(record.id, &ids, &result)?,
        Err(error) => {
            remember_rate_limit(store, &error);
            if (record.action.retry_safe() || !prepared || crate::google::before_write(&error))
                && crate::google::transient_failure(&error)
            {
                let until = crate::google::rate_limit_until(&error).unwrap_or_else(|| {
                    now.saturating_add(2_u64.pow(record.attempts.min(8)).min(300))
                });
                store.defer_action(record.id, until, &format!("{error:#}"))?;
            } else {
                store.fail_action(record.id, &format!("{error:#}"))?;
            }
        }
    }
    Ok(true)
}

fn action_worker(
    dir: PathBuf,
    wake: mpsc::Receiver<()>,
    events: mpsc::Sender<Event>,
    gate: Arc<LoadGate>,
) {
    let result = (|| -> Result<()> {
        let store = Store::open(Some(&dir))?;
        let mut ownership = None;
        let mut connection = None;
        loop {
            if ownership.is_none() {
                ownership = action_lock(&dir)?;
                if ownership.is_some() {
                    store.recover_actions()?;
                    let _ = events.send(Event::QueueChanged);
                }
            }
            let attempted = if ownership.is_some() {
                deliver_action(&store, crate::auth::now(), |record, prepared| {
                    gate.invalidate();
                    let _ = events.send(Event::QueueChanged);
                    if connection.is_none() {
                        connection = Some(open_mail_connection(&dir, &store)?);
                    }
                    let (google, email) = connection.as_mut().unwrap();
                    *prepared = true;
                    execute_action(record, google, email, &store)
                })?
            } else {
                false
            };
            if attempted {
                gate.invalidate();
                let _ = events.send(Event::QueueChanged);
                continue;
            }
            match wake.recv_timeout(Duration::from_millis(500)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = events.send(Event::Error(format!(
            "Queued changes could not be processed: {error}"
        )));
    }
}

fn may_prefetch(store: &Store, foreground: &Foreground, gate: &LoadGate) -> Result<bool> {
    Ok(!gate.state.lock().unwrap().closed
        && !foreground.busy()
        && store.rate_limit_until()? <= crate::auth::now()
        && !store.actions()?.iter().any(|action| {
            matches!(
                action.state,
                crate::actions::ActionState::Queued | crate::actions::ActionState::Running
            )
        }))
}

fn prefetch_target(store: &Store, current: &str) -> Result<Option<String>> {
    let labels = store.get::<Vec<Label>>("labels")?.unwrap_or_default();
    let mut candidates = store
        .get::<Vec<String>>("recent_folders")?
        .unwrap_or_default();
    candidates.truncate(8);
    candidates.extend(["INBOX", "STARRED", "SENT", "TRASH", "SPAM"].map(str::to_owned));
    let mut seen = std::collections::HashSet::new();
    for label in candidates {
        if label == current
            || !seen.insert(label.clone())
            || !labels.iter().any(|known| known.id == label)
        {
            continue;
        }
        if !store.page_is_fresh(&label, "", "", 1800)? {
            return Ok(Some(label));
        }
    }
    Ok(None)
}

fn prefetch_worker(dir: PathBuf, foreground: Arc<Foreground>, gate: Arc<LoadGate>) {
    let Ok(store) = Store::open(Some(&dir)) else {
        return;
    };
    let mut connection = None;
    let mut next_attempt = 0;
    while !gate.state.lock().unwrap().closed {
        if crate::auth::now() >= next_attempt
            && may_prefetch(&store, &foreground, &gate).unwrap_or(false)
        {
            let current = foreground.label.lock().unwrap().clone();
            if let Ok(Some(label)) = prefetch_target(&store, &current) {
                // A separate lane means even a stalled background request cannot
                // hold navigation. At most one folder is warmed every five minutes.
                next_attempt = crate::auth::now().saturating_add(300);
                let Ok(Some(_ownership)) = claim_prefetch(&dir, &store, crate::auth::now()) else {
                    continue;
                };
                let result = (|| -> Result<()> {
                    if connection.is_none() {
                        connection = Some(open_mail_connection(&dir, &store)?.0);
                    }
                    if !may_prefetch(&store, &foreground, &gate)? {
                        return Ok(());
                    }
                    let started = now_ms();
                    let cache_key = key(&label, "", "");
                    let page =
                        connection
                            .as_mut()
                            .unwrap()
                            .thread_summaries(&label, "", "", |page| {
                                if !may_prefetch(&store, &foreground, &gate).unwrap_or(false) {
                                    return false;
                                }
                                let mut page = page.clone();
                                store
                                    .observe_messages(&mut page.messages, started)
                                    .and_then(|()| store.save_partial_page(&cache_key, &page))
                                    .is_ok()
                            })?;
                    if let Some(mut page) = page
                        && may_prefetch(&store, &foreground, &gate)?
                    {
                        store.observe_messages(&mut page.messages, started)?;
                        store.save_page(&cache_key, &page)?;
                        store.confirm_actions(&page.messages, started)?;
                        store.confirm_folder_snapshot(&cache_key, started)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    remember_rate_limit(&store, &error);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub fn start(dir: PathBuf, headless: bool) -> (mpsc::Sender<Command>, mpsc::Receiver<Event>) {
    let (tx, requests) = mpsc::channel();
    let (work, rx) = mpsc::channel();
    let (loads, load_requests) = mpsc::channel();
    let (reads, read_requests) = mpsc::channel();
    let (images, image_requests) = mpsc::channel();
    let (actions, action_wake) = mpsc::channel();
    let (events, out) = mpsc::channel();
    let gate = Arc::new(LoadGate::default());
    let read_generation = Arc::new(ReadGate::default());
    let image_generation = Arc::new(AtomicU64::new(0));
    let foreground = Arc::new(Foreground::default());
    {
        let gate = gate.clone();
        let read_generation = read_generation.clone();
        let image_generation = image_generation.clone();
        let foreground = foreground.clone();
        std::thread::spawn(move || {
            route_commands(
                requests,
                work,
                loads,
                reads,
                images,
                actions,
                gate,
                read_generation,
                image_generation,
                foreground,
            )
        });
    }
    {
        let dir = dir.clone();
        let events = events.clone();
        let gate = gate.clone();
        let foreground = foreground.clone();
        std::thread::spawn(move || load_worker(dir, load_requests, events, gate, foreground));
    }
    {
        let dir = dir.clone();
        let events = events.clone();
        let gate = gate.clone();
        std::thread::spawn(move || action_worker(dir, action_wake, events, gate));
    }
    {
        let dir = dir.clone();
        let foreground = foreground.clone();
        let gate = gate.clone();
        std::thread::spawn(move || prefetch_worker(dir, foreground, gate));
    }
    let read_requests = Arc::new(Mutex::new(read_requests));
    let image_requests = Arc::new(Mutex::new(image_requests));
    let in_flight = Arc::new(Mutex::new(std::collections::HashSet::new()));
    let completed_images = Arc::new(Mutex::new(std::collections::HashSet::new()));
    for _ in 0..2 {
        let dir = dir.clone();
        let events = events.clone();
        let rx = read_requests.clone();
        let gate = read_generation.clone();
        let in_flight = in_flight.clone();
        let foreground = foreground.clone();
        std::thread::spawn(move || read_worker(dir, rx, events, gate, in_flight, foreground));
    }
    for _ in 0..2 {
        let dir = dir.clone();
        let events = events.clone();
        let rx = image_requests.clone();
        let generation = image_generation.clone();
        let completed = completed_images.clone();
        std::thread::spawn(move || image_worker(dir, rx, events, generation, completed));
    }
    let photos = crate::contacts::photo_worker(events.clone());
    let favicons = crate::contacts::photo_worker(events.clone());
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let store = Store::open(Some(&dir))?;
            let mut connection: Option<(Google, String)> = None;
            let mut contacts_synced = None::<std::time::Instant>;
            let mut deadline = std::time::Instant::now();
            loop {
                let command = if std::time::Instant::now() >= deadline {
                    deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                    Command::Sync
                } else {
                    match rx
                        .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    {
                        Ok(command) => command,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                };
                if let Command::ContactPhotos(urls) = &command {
                    let (icons, portraits): (Vec<_>, Vec<_>) = urls
                        .iter()
                        .cloned()
                        .partition(|request| request.key.starts_with("favicon:"));
                    if !icons.is_empty() {
                        let _ = favicons.send(icons);
                    }
                    if !portraits.is_empty() {
                        let _ = photos.send(portraits);
                    }
                    continue;
                }
                if let Command::Filters(request) = command {
                    let _mutation = (!matches!(request, crate::filters::Request::List))
                        .then(|| gate.mutation());
                    let result = (|| -> Result<crate::filters::Outcome> {
                        check_cooldown(&store)?;
                        let mut google = open_mail_connection(&dir, &store)?.0;
                        filter_request(&mut google, &store, request)
                    })();
                    if let Err(error) = &result {
                        remember_rate_limit(&store, error);
                    }
                    let _ =
                        events.send(Event::Filters(result.map_err(|error| format!("{error:#}"))));
                    continue;
                }
                let _mutation = matches!(&command, Command::Connect(_)).then(|| gate.mutation());
                if let Command::Connect(path) = &command {
                    match crate::auth::connect(&dir, path, true) {
                        Ok(()) => {
                            connection = None;
                            contacts_synced = None;
                            deadline = std::time::Instant::now();
                            let _ = events.send(Event::Connected);
                        }
                        Err(error) => {
                            let _ = events.send(Event::ConnectionFailed(error.to_string()));
                        }
                    }
                    continue;
                }
                let syncing = matches!(command, Command::Sync | Command::PushSync(_));
                let push_reply = if let Command::PushSync(reply) = &command {
                    Some(reply.clone())
                } else {
                    None
                };
                let mut sync_success = false;
                let result = (|| -> Result<()> {
                    let _sync_guard = if syncing && !headless {
                        match crate::background::lock(&dir)? {
                            Some(guard) => Some(guard),
                            None => {
                                let LabelSnapshot {
                                    labels,
                                    counts_observed_at,
                                } = cached_labels(&store)?.unwrap_or_default();
                                let email = store.get("email")?.unwrap_or_default();
                                let _ = events.send(Event::Ready {
                                    labels,
                                    email,
                                    counts_observed_at,
                                });
                                if let Some(page) = store.cached_page("INBOX", "", "")? {
                                    let _ = events.send(Event::Inbox(page));
                                }
                                if let Some(rows) = store.get("contacts")? {
                                    let _ = events.send(Event::ContactBook(rows));
                                }
                                return Ok(());
                            }
                        }
                    } else {
                        None
                    };
                    check_cooldown(&store)?;
                    if connection.is_none() {
                        let mut google = Google::open(&dir)?;
                        let email = google.email()?;
                        if store
                            .get::<String>("email")?
                            .is_some_and(|old| old != email)
                        {
                            anyhow::bail!(
                                "This cache belongs to another account. Use a separate GMAILTJIK_DATA_DIR"
                            );
                        }
                        let LabelSnapshot {
                            labels,
                            counts_observed_at,
                        } = sync_labels(&mut google, &store)?;
                        store.put("email", &email)?;
                        let _ = events.send(Event::Ready {
                            labels,
                            email: email.clone(),
                            counts_observed_at,
                        });
                        connection = Some((google, email));
                    }
                    let (google, email) = connection.as_mut().unwrap();
                    match command {
                        Command::Filters(_)
                        | Command::Connect(_)
                        | Command::ContactPhotos(_)
                        | Command::Load { .. }
                        | Command::WakeActions
                        | Command::Thread(_)
                        | Command::OpenMail(_)
                        | Command::Images { .. } => {
                            unreachable!()
                        }
                        Command::Sync | Command::PushSync(_) => {
                            let sync_started = chrono::Utc::now().timestamp_millis();
                            google.changes(&store)?;
                            if contacts_synced
                                .is_some_and(|last| last.elapsed() < Duration::from_secs(3600))
                                && let Some(page) = unchanged_sync_page(&store)?
                            {
                                let _ = events.send(Event::Inbox(page));
                                store.put("background_last_sync", &sync_started)?;
                                sync_success = true;
                                let _ = events.send(Event::SyncComplete);
                                return Ok(());
                            }
                            let baseline = match store.get::<i64>("notification_baseline")? {
                                Some(value) => value,
                                None => {
                                    let value = chrono::Utc::now().timestamp_millis();
                                    store.put("notification_baseline", &value)?;
                                    value
                                }
                            };
                            let LabelSnapshot {
                                labels,
                                counts_observed_at,
                            } = sync_labels(google, &store)?;
                            let _ = events.send(Event::Ready {
                                labels: labels.clone(),
                                email: email.clone(),
                                counts_observed_at,
                            });
                            let started = chrono::Utc::now().timestamp_millis().max(0) as u64;
                            let mut page = google
                                .thread_summaries("INBOX", "", "", |_| true)?
                                .unwrap_or_default();
                            store.observe_messages(&mut page.messages, started)?;
                            store.save_page(&key("INBOX", "", ""), &page)?;
                            store.confirm_actions(&page.messages, started)?;
                            store.confirm_folder_snapshot(&key("INBOX", "", ""), started)?;
                            store.project_page("INBOX", "", "", &mut page)?;
                            let _ = events.send(Event::Inbox(page.clone()));
                            let rules = crate::rules::load(&store)?;
                            let mut notifications_ok = true;
                            // Include filtered AUTH mail outside Inbox, but keep normal mail
                            // notifications scoped to Inbox. Fetching does not mark mail read.
                            let notification_query = crate::google::exclude_snoozed(&format!(
                                "is:unread after:{}",
                                baseline / 1000
                            ));
                            let mut notification_page =
                                google.page(&store, "", &notification_query, "")?;
                            loop {
                                for message in &notification_page.messages {
                                    let outcome = crate::rules::evaluate(&rules, message, &labels);
                                    let Some(notice) =
                                        crate::notifications::rule_notice(email, message, outcome)
                                    else {
                                        continue;
                                    };
                                    if let Err(error) = crate::notifications::deliver(
                                        &store,
                                        message,
                                        baseline,
                                        |_| {
                                            let mut notice = notice;
                                            crate::notifications::sender_image(
                                                &store,
                                                message,
                                                &mut notice,
                                            );
                                            crate::notifications::send(notice)
                                        },
                                    ) {
                                        notifications_ok = false;
                                        let _ = events
                                            .send(Event::SyncError(format!("Notjik: {error}")));
                                        break;
                                    }
                                }
                                if !notifications_ok
                                    || notification_page.next.is_empty()
                                    || notification_page.messages.iter().any(|m| {
                                        m.internal_date.parse::<i64>().unwrap_or(0) <= baseline
                                    })
                                {
                                    break;
                                }
                                notification_page = google.page(
                                    &store,
                                    "",
                                    &notification_query,
                                    &notification_page.next,
                                )?;
                            }
                            if notifications_ok {
                                store.put("notification_baseline", &sync_started)?;
                                store.put("background_last_sync", &sync_started)?;
                                if let Some(history) = store.get::<String>("history")? {
                                    store.put("completed_sync_history", &history)?;
                                }
                            }
                            if contacts_synced.is_none_or(|last| {
                                last.elapsed() >= std::time::Duration::from_secs(3600)
                            }) {
                                contacts_synced = Some(std::time::Instant::now());
                                match google.all_contacts() {
                                    Ok(rows) => {
                                        store.put("contacts", &rows)?;
                                        let _ = events.send(Event::ContactBook(rows));
                                    }
                                    Err(error) => {
                                        remember_rate_limit(&store, &error);
                                        let _ = events
                                            .send(Event::SyncError(format!("Contacts: {error}")));
                                    }
                                }
                            }
                            if notifications_ok {
                                sync_success = true;
                                let _ = events.send(Event::SyncComplete);
                            }
                        }
                        Command::Contacts { query } => {
                            let contacts =
                                store.get::<Vec<Contact>>("contacts")?.unwrap_or_default();
                            let rows = crate::contacts::suggestions(&contacts, &query);
                            let rows = if rows.is_empty() {
                                google.contacts(&query)?
                            } else {
                                rows
                            };
                            let _ = events.send(Event::Contacts { query, rows });
                        }
                    }
                    Ok(())
                })();
                if let Some(reply) = push_reply {
                    let _ = reply.send(sync_success && result.is_ok());
                }
                if let Err(error) = result {
                    remember_rate_limit(&store, &error);
                    let _ = events.send(if syncing {
                        Event::SyncError(error.to_string())
                    } else {
                        Event::Error(error.to_string())
                    });
                }
                if syncing {
                    deadline = std::time::Instant::now()
                        + std::time::Duration::from_secs(if headless {
                            crate::push::interval(&store)
                        } else {
                            30
                        });
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = events.send(Event::Error(error.to_string()));
        }
    });
    (tx, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn request(ticket: u64) -> Command {
        Command::Load {
            ticket,
            label: format!("folder-{ticket}"),
            query: String::new(),
            page: String::new(),
            force: false,
        }
    }

    fn synthetic_action() -> crate::actions::Action {
        crate::actions::Action::Labels {
            id: "demo-0".into(),
            thread: "thread-0".into(),
            add: vec![],
            remove: vec!["INBOX".into()],
        }
    }

    #[test]
    fn cached_label_counts_keep_their_original_observation_time() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let labels = crate::model::demo().0;
        {
            let store = Store::open(Some(dir.path())).unwrap();
            store.put("labels", &labels).unwrap();
            store.put("labels_checked_at", &crate::auth::now()).unwrap();
            store.put("labels_counts_observed_at", &123_u64).unwrap();
        }
        let reopened = Store::open(Some(dir.path())).unwrap();
        let reused = sync_labels_with(&reopened, || panic!("fresh cache must skip Gmail")).unwrap();
        assert_eq!(reused.labels.len(), labels.len());
        assert_eq!(reused.counts_observed_at, 123);
        // The background-lock fallback reads the same original snapshot.
        let fallback = cached_labels(&reopened).unwrap().unwrap();
        assert_eq!(fallback.counts_observed_at, 123);
        assert_eq!(
            reopened.get::<u64>("labels_counts_observed_at").unwrap(),
            Some(123)
        );

        let legacy = Store::open(None).unwrap();
        legacy.put("labels", &labels).unwrap();
        legacy
            .put("labels_checked_at", &crate::auth::now())
            .unwrap();
        assert_eq!(
            sync_labels_with(&legacy, || panic!("legacy cache is still reusable"))
                .unwrap()
                .counts_observed_at,
            0
        );
    }

    #[test]
    fn fetched_label_counts_are_observed_before_fetch_and_persisted_together() {
        let store = Store::open(None).unwrap();
        let entered_fetch = std::cell::Cell::new(0_u64);
        let snapshot = sync_labels_with(&store, || {
            entered_fetch.set(chrono::Utc::now().timestamp_millis().max(0) as u64);
            Ok(crate::model::demo().0)
        })
        .unwrap();
        assert!(snapshot.counts_observed_at > 0);
        assert!(snapshot.counts_observed_at <= entered_fetch.get());
        let saved = cached_labels(&store).unwrap().unwrap();
        assert_eq!(saved.counts_observed_at, snapshot.counts_observed_at);
        assert_eq!(saved.labels.len(), snapshot.labels.len());
        assert_eq!(
            sync_labels_with(&store, || panic!("new snapshot is cached"))
                .unwrap()
                .counts_observed_at,
            snapshot.counts_observed_at
        );
    }

    #[test]
    fn queue_delivery_keeps_fifo_and_persists_terminal_failures() {
        use crate::actions::{ActionResult, ActionState};
        let store = Store::open(None).unwrap();
        let seeds = crate::model::demo().1.messages;
        let first = store.enqueue_action(synthetic_action(), &seeds).unwrap();
        let second = store.enqueue_action(synthetic_action(), &seeds).unwrap();
        assert!(
            deliver_action(&store, 10, |row, prepared| {
                assert_eq!(row.id, first);
                *prepared = true;
                anyhow::bail!("Synthetic permission denial")
            })
            .unwrap()
        );
        assert_eq!(
            store.action(first).unwrap().unwrap().state,
            ActionState::Failed
        );
        assert!(
            deliver_action(&store, 10, |row, prepared| {
                assert_eq!(row.id, second);
                *prepared = true;
                Ok((vec!["demo-0".into()], ActionResult::default()))
            })
            .unwrap()
        );
        assert_eq!(
            store.action(second).unwrap().unwrap().state,
            ActionState::Succeeded
        );
        assert!(
            store
                .action(first)
                .unwrap()
                .unwrap()
                .error
                .contains("Synthetic permission denial")
        );
    }

    #[test]
    fn account_cooldown_leaves_queue_unclaimed_and_survives_restart() {
        use crate::actions::ActionState;
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let id = {
            let store = Store::open(Some(dir.path())).unwrap();
            let id = store.enqueue_action(synthetic_action(), &[]).unwrap();
            store.defer_requests(120).unwrap();
            assert!(
                !deliver_action(&store, 100, |_, _| panic!("cooldown must not dispatch")).unwrap()
            );
            assert_eq!(store.action(id).unwrap().unwrap().attempts, 0);
            id
        };
        let reopened = Store::open(Some(dir.path())).unwrap();
        assert_eq!(
            reopened.action(id).unwrap().unwrap().state,
            ActionState::Queued
        );
        assert!(
            deliver_action(&reopened, 120, |_, prepared| {
                *prepared = true;
                Ok((vec![], Default::default()))
            })
            .unwrap()
        );
    }

    #[test]
    fn delivery_lock_excludes_other_processes_and_releases_on_exit() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let first = action_lock(dir.path()).unwrap().unwrap();
        assert!(action_lock(dir.path()).unwrap().is_none());
        drop(first);
        assert!(action_lock(dir.path()).unwrap().is_some());
    }

    #[test]
    fn prefetch_budget_is_shared_between_processes_and_survives_restart() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let first = Store::open(Some(dir.path())).unwrap();
        let second = Store::open(Some(dir.path())).unwrap();
        let owner = claim_prefetch(dir.path(), &first, 100).unwrap().unwrap();
        assert!(claim_prefetch(dir.path(), &second, 100).unwrap().is_none());
        drop(owner);
        assert!(claim_prefetch(dir.path(), &second, 399).unwrap().is_none());
        assert!(claim_prefetch(dir.path(), &second, 400).unwrap().is_some());
    }

    #[test]
    fn offline_prepare_defers_send_but_uncertain_delivery_never_replays() {
        use crate::actions::{Action, ActionState};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);
        let failed_connection = || {
            reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(1))
                .build()
                .unwrap()
                .get(&url)
                .send()
                .unwrap_err()
                .into()
        };
        let store = Store::open(None).unwrap();
        let id = store
            .enqueue_action(
                Action::Send {
                    draft: Default::default(),
                },
                &[],
            )
            .unwrap();
        deliver_action(&store, 100, |_, _| Err(failed_connection())).unwrap();
        let deferred = store.action(id).unwrap().unwrap();
        assert_eq!(deferred.state, ActionState::Queued);
        assert!(deferred.next_attempt > 100);
        assert!(!deliver_action(&store, 100, |_, _| panic!("retry must honor backoff")).unwrap());
        deliver_action(&store, deferred.next_attempt, |_, prepared| {
            *prepared = true; // The mailbox request may have reached Gmail.
            Err(failed_connection())
        })
        .unwrap();
        assert_eq!(
            store.action(id).unwrap().unwrap().state,
            ActionState::Failed
        );
        assert!(
            !deliver_action(&store, 1000, |_, _| panic!(
                "uncertain send must not replay"
            ))
            .unwrap()
        );
    }

    #[test]
    fn unchanged_history_reuses_only_a_completed_valid_sync() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        store
            .save_page(&key("INBOX", "", ""), &crate::model::demo().1)
            .unwrap();
        store.put("history", &"cursor-1").unwrap();
        assert!(unchanged_sync_page(&store).unwrap().is_none());
        store.put("completed_sync_history", &"cursor-1").unwrap();
        assert!(unchanged_sync_page(&store).unwrap().is_some());
        store.put("history", &"cursor-2").unwrap();
        assert!(unchanged_sync_page(&store).unwrap().is_none());
        store.put("completed_sync_history", &"cursor-2").unwrap();
        store.invalidate_all().unwrap();
        assert!(unchanged_sync_page(&store).unwrap().is_none());
    }

    #[test]
    fn recently_cached_folder_needs_no_connection_and_force_revalidates() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        store
            .save_page(&key("INBOX", "", ""), &crate::model::demo().1)
            .unwrap();
        let gate = LoadGate::default();
        gate.select(1);
        let (events, received) = mpsc::channel();
        let mut connection = None;
        let mut request = LoadRequest {
            ticket: 1,
            label: "INBOX".into(),
            query: String::new(),
            page: String::new(),
            force: false,
        };
        assert_eq!(
            load_page(
                &request,
                0,
                dir.path(),
                &store,
                &mut connection,
                &gate,
                &events
            )
            .unwrap(),
            Some(())
        );
        assert!(connection.is_none());
        assert!(matches!(
            received.try_recv().unwrap(),
            Event::Page {
                cached: true,
                complete: true,
                ..
            }
        ));
        assert!(matches!(
            received.try_recv().unwrap(),
            Event::Loaded { ticket: 1 }
        ));
        request.force = true;
        assert!(
            load_page(
                &request,
                0,
                dir.path(),
                &store,
                &mut connection,
                &gate,
                &events
            )
            .unwrap_err()
            .to_string()
            .contains("Connect Gmail first")
        );
    }

    #[test]
    fn cached_reader_bypasses_unavailable_google_and_obsolete_selection() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        let messages = vec![crate::model::demo().1.messages[0].clone()];
        store.put("thread:thread-0", &messages).unwrap();
        store.defer_requests(crate::auth::now() + 120).unwrap();
        let (requests, rx) = mpsc::channel();
        let (events, received) = mpsc::channel();
        let gate = Arc::new(ReadGate::default());
        let old = gate.select("old");
        let latest = gate.select("thread-0");
        requests
            .send(ReadRequest {
                ticket: old,
                id: "old".into(),
                open: None,
            })
            .unwrap();
        requests
            .send(ReadRequest {
                ticket: latest,
                id: "thread-0".into(),
                open: None,
            })
            .unwrap();
        drop(requests);
        read_worker(
            dir.path().to_owned(),
            Arc::new(Mutex::new(rx)),
            events,
            gate,
            Arc::new(Mutex::new(Default::default())),
            Arc::new(Foreground::default()),
        );
        let Event::Thread { id, messages } = received.try_recv().unwrap() else {
            panic!("body must be first")
        };
        assert_eq!(id, "thread-0");
        assert!(!messages[0].text().is_empty());
        assert!(!dir.path().join("account.json").exists());
    }

    #[test]
    fn prefetch_is_bounded_and_yields_to_navigation_actions_and_cooldown() {
        let store = Store::open(None).unwrap();
        let (labels, page) = crate::model::demo();
        store.put("labels", &labels).unwrap();
        let gate = LoadGate::default();
        let foreground = Arc::new(Foreground::default());
        assert!(may_prefetch(&store, &foreground, &gate).unwrap());
        assert_eq!(
            prefetch_target(&store, "INBOX").unwrap().unwrap(),
            "STARRED"
        );
        store.save_page(&key("STARRED", "", ""), &page).unwrap();
        assert_eq!(prefetch_target(&store, "INBOX").unwrap().unwrap(), "SENT");
        foreground.touch();
        assert!(!may_prefetch(&store, &foreground, &gate).unwrap());
        foreground.changed_at.store(0, Ordering::SeqCst);
        let active = foreground.work();
        assert!(!may_prefetch(&store, &foreground, &gate).unwrap());
        drop(active);
        foreground.changed_at.store(0, Ordering::SeqCst);
        let action = store.enqueue_action(synthetic_action(), &[]).unwrap();
        assert!(!may_prefetch(&store, &foreground, &gate).unwrap());
        store.fail_action(action, "Synthetic rejection").unwrap();
        assert!(may_prefetch(&store, &foreground, &gate).unwrap());
        store.defer_requests(crate::auth::now() + 120).unwrap();
        assert!(!may_prefetch(&store, &foreground, &gate).unwrap());
    }

    #[test]
    fn cached_folder_is_delivered_before_cooldown_or_account_initialization() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let store = Store::open(Some(dir.path())).unwrap();
        let page = crate::model::demo().1;
        store
            .save_partial_page(&key("INBOX", "", ""), &page)
            .unwrap();
        store.defer_requests(crate::auth::now() + 120).unwrap();
        let gate = Arc::new(LoadGate::default());
        gate.select(1);
        let (events, received) = mpsc::channel();
        let mut connection = None;
        let result = load_page(
            &LoadRequest {
                ticket: 1,
                label: "INBOX".into(),
                query: String::new(),
                page: String::new(),
                force: false,
            },
            0,
            dir.path(),
            &store,
            &mut connection,
            &gate,
            &events,
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cached mail remains available")
        );
        assert!(connection.is_none());
        assert!(!dir.path().join("account.json").exists());
        let Event::Page {
            page: cached,
            cached: true,
            ..
        } = received.try_recv().unwrap()
        else {
            panic!("cached rows must be delivered first")
        };
        assert_eq!(cached.conversations().len(), page.conversations().len());
        assert!(cached.next.is_empty());
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn folder_requests_bypass_busy_sync_and_newer_navigation_wins() {
        let (commands, rx) = mpsc::channel();
        let (work, busy_sync) = mpsc::channel();
        let (loads, folder_requests) = mpsc::channel();
        let gate = Arc::new(LoadGate::default());
        let (reads, read_requests) = mpsc::channel();
        let (images, image_requests) = mpsc::channel();
        let (actions, action_wake) = mpsc::channel();
        let router = {
            let gate = gate.clone();
            std::thread::spawn(move || {
                route_commands(
                    rx,
                    work,
                    loads,
                    reads,
                    images,
                    actions,
                    gate,
                    Arc::new(ReadGate::default()),
                    Arc::new(AtomicU64::new(0)),
                    Arc::new(Foreground::default()),
                )
            })
        };
        // Sync stays unprocessed while both folder requests reach their own lane.
        commands.send(Command::Sync).unwrap();
        commands.send(request(1)).unwrap();
        commands.send(request(2)).unwrap();
        commands.send(Command::Thread("thread-0".into())).unwrap();
        commands
            .send(Command::Images {
                generation: 1,
                message: crate::model::demo().1.messages[0].clone(),
                remote: false,
            })
            .unwrap();
        commands.send(Command::WakeActions).unwrap();
        let first = folder_requests
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        let second = folder_requests
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!((first.ticket, second.ticket), (1, 2));
        assert_eq!(second.label, "folder-2");
        assert_eq!(gate.epoch(first.ticket), None);
        assert_eq!(gate.epoch(second.ticket), Some(0));
        assert!(matches!(busy_sync.try_recv(), Ok(Command::Sync)));
        assert_eq!(
            read_requests
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .id,
            "thread-0"
        );
        assert_eq!(
            image_requests
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .generation,
            1
        );
        action_wake.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(commands);
        router.join().unwrap();
    }

    #[test]
    fn in_flight_previews_cannot_restore_mail_after_an_archive() {
        let store = Store::open(None).unwrap();
        let page = crate::model::demo().1;
        let id = page.messages[0].id.clone();
        let cache_key = key("INBOX", "", "");
        store.save_page(&cache_key, &page).unwrap();
        let gate = Arc::new(LoadGate::default());
        gate.select(1);
        let before = gate.epoch(1).unwrap();
        {
            let _mutation = gate.mutation();
            store.apply_labels(&id, &[], &["INBOX".into()]).unwrap();
            assert!(gate.current(1, before, || ()).is_none());
        }
        // The network request started before archive and finishes afterward.
        assert!(
            gate.current(1, before, || store.save_page(&cache_key, &page))
                .is_none()
        );
        let cached = store.page(&cache_key).unwrap().unwrap();
        assert!(!cached.messages.iter().any(|message| message.id == id));
        assert_eq!(gate.epoch(1), Some(before + 2));
    }

    #[test]
    fn obsolete_results_and_errors_are_dropped() {
        let gate = Arc::new(LoadGate::default());
        let (events, received) = mpsc::channel();
        gate.select(1);
        let epoch = gate.epoch(1).unwrap();
        gate.select(2);
        assert!(
            gate.current(1, epoch, || {
                events.send(Event::Page {
                    ticket: 1,
                    cached: false,
                    complete: true,
                    page: Page::default(),
                })
            })
            .is_none()
        );
        assert!(
            gate.current(1, epoch, || {
                events.send(Event::LoadError {
                    ticket: 1,
                    error: "Old folder request failed".into(),
                })
            })
            .is_none()
        );
        assert!(matches!(
            received.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn navigation_cancels_a_load_waiting_for_a_mailbox_mutation() {
        let gate = Arc::new(LoadGate::default());
        gate.select(1);
        let mutation = gate.mutation();
        let (finished, result) = mpsc::channel();
        let waiter = {
            let gate = gate.clone();
            std::thread::spawn(move || finished.send(gate.epoch(1)).unwrap())
        };
        // New navigation must wake a cancelled request even before the mutation
        // finishes. Waiting cannot monopolise the gate or the router.
        gate.select(2);
        assert_eq!(result.recv_timeout(Duration::from_secs(1)).unwrap(), None);
        waiter.join().unwrap();
        drop(mutation);
        assert_eq!(gate.epoch(2), Some(2));
    }
}
