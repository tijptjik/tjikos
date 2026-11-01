use crate::{
    google::Google,
    model::{Contact, Draft, Label, Message, Page},
    store::Store,
};
use anyhow::Result;
use std::{path::PathBuf, sync::mpsc};

pub enum Command {
    Connect(PathBuf),
    Sync,
    PushSync(mpsc::Sender<bool>),
    Thread(String),
    Load {
        ticket: u64,
        label: String,
        query: String,
        page: String,
    },
    Modify {
        id: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    ContactPhotos(Vec<String>),
    Contacts {
        query: String,
    },
    Images {
        generation: u64,
        message: Message,
        remote: bool,
    },
    Save(Draft),
    Send(Draft),
}
pub enum Event {
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
    },
    Page {
        ticket: u64,
        cached: bool,
        page: Page,
    },
    Loaded {
        ticket: u64,
    },
    Error(String),
    Modified {
        id: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
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
    Saved {
        original: Draft,
        id: String,
    },
    Sent,
    ComposeFailed(String),
}
pub fn key(label: &str, query: &str, page: &str) -> String {
    serde_json::to_string(&(label, query, page)).unwrap()
}

pub fn start(dir: PathBuf, headless: bool) -> (mpsc::Sender<Command>, mpsc::Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let (events, out) = mpsc::channel();
    let photos = crate::contacts::photo_worker(events.clone());
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
                    let _ = photos.send(urls.clone());
                    continue;
                }
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
                let compose = matches!(command, Command::Save(_) | Command::Send(_));
                let ticket = if let Command::Load { ticket, .. } = &command {
                    Some(*ticket)
                } else {
                    None
                };
                let result = (|| -> Result<()> {
                    let _sync_guard = if syncing && !headless {
                        match crate::background::lock(&dir)? {
                            Some(guard) => Some(guard),
                            None => {
                                let labels = store.get("labels")?.unwrap_or_default();
                                let email = store.get("email")?.unwrap_or_default();
                                let _ = events.send(Event::Ready { labels, email });
                                if let Some(page) = store.page(&key("INBOX", "", ""))? {
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
                        let labels = google.labels()?;
                        store.put("labels", &labels)?;
                        store.put("email", &email)?;
                        let _ = events.send(Event::Ready {
                            labels,
                            email: email.clone(),
                        });
                        connection = Some((google, email));
                    }
                    let (google, email) = connection.as_mut().unwrap();
                    match command {
                        Command::Connect(_) | Command::ContactPhotos(_) => unreachable!(),
                        Command::Sync | Command::PushSync(_) => {
                            let sync_started = chrono::Utc::now().timestamp_millis();
                            google.changes(&store)?;
                            let baseline = match store.get::<i64>("notification_baseline")? {
                                Some(value) => value,
                                None => {
                                    let value = chrono::Utc::now().timestamp_millis();
                                    store.put("notification_baseline", &value)?;
                                    value
                                }
                            };
                            let labels = google.labels()?;
                            store.put("labels", &labels)?;
                            let _ = events.send(Event::Ready {
                                labels: labels.clone(),
                                email: email.clone(),
                            });
                            let page = google.page(&store, "INBOX", "", "")?;
                            store.save_page(&key("INBOX", "", ""), &page)?;
                            let _ = events.send(Event::Inbox(page.clone()));
                            let rules = crate::rules::load(&store)?;
                            let mut notifications_ok = true;
                            // Include filtered AUTH mail outside Inbox, but keep normal mail
                            // notifications scoped to Inbox. Fetching does not mark mail read.
                            let notification_query = format!("is:unread after:{}", baseline / 1000);
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
                                        |_| crate::notifications::send(notice),
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
                        Command::Load {
                            ticket,
                            label,
                            query,
                            page,
                        } => {
                            let cache_key = key(&label, &query, &page);
                            if let Some(page) = store.page(&cache_key)? {
                                let _ = events.send(Event::Page {
                                    ticket,
                                    cached: true,
                                    page,
                                });
                            }
                            google.changes(&store)?;
                            let page = google.page(&store, &label, &query, &page)?;
                            store.save_page(&cache_key, &page)?;
                            let _ = events.send(Event::Page {
                                ticket,
                                cached: false,
                                page,
                            });
                        }
                        Command::Thread(id) => {
                            if let Some(messages) =
                                store.get::<Vec<Message>>(&format!("thread:{id}"))?
                            {
                                let _ = events.send(Event::Thread {
                                    id: id.clone(),
                                    messages,
                                });
                            }
                            let messages = google.thread(&store, &id)?;
                            let _ = events.send(Event::Thread { id, messages });
                        }
                        Command::Modify { id, add, remove } => {
                            google.modify(&id, &add, &remove)?;
                            store.apply_labels(&id, &add, &remove)?;
                            let _ = events.send(Event::Modified { id, add, remove });
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
                        Command::Images {
                            message,
                            remote,
                            generation,
                        } => {
                            let images = google.images(&message, remote)?;
                            let _ = events.send(Event::Images {
                                generation,
                                remote,
                                id: message.id,
                                images,
                            });
                        }
                        Command::Save(draft) => {
                            let id = google.save_draft(&draft, email)?;
                            let _ = events.send(Event::Saved {
                                original: draft,
                                id,
                            });
                        }
                        Command::Send(draft) => {
                            google.send(&draft, email)?;
                            let _ = events.send(Event::Sent);
                        }
                    }
                    Ok(())
                })();
                if let Some(reply) = push_reply {
                    let _ = reply.send(sync_success && result.is_ok());
                }
                if let Err(error) = result {
                    let _ = events.send(if compose {
                        Event::ComposeFailed(error.to_string())
                    } else {
                        if syncing {
                            Event::SyncError(error.to_string())
                        } else {
                            Event::Error(error.to_string())
                        }
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
                if let Some(ticket) = ticket {
                    let _ = events.send(Event::Loaded { ticket });
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
