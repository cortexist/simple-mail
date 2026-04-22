//! imap_client.rs — Efficient IMAP4 sync engine.
//!
//! Design goals:
//!   • Envelope-only initial fetch (subject, from, date, flags) — body fetched on demand
//!   • UID-based delta sync: only fetch UIDs > UIDNEXT from last sync
//!   • CONDSTORE (HIGHESTMODSEQ) for flag changes without full re-scan
//!   • IDLE support for real-time push notifications
//!   • Paged fetches in chunks of 100 to avoid memory spikes on huge mailboxes
//!   • Multi-device safe: detects UID validity changes and does full re-sync

use anyhow::{Result, Context, bail};
use async_imap::types::{Fetch, Flag};
use async_native_tls::TlsConnector;
use base64::Engine;
use chrono::DateTime;
use futures::{AsyncRead, AsyncWrite, TryStreamExt};
use std::collections::HashSet;
use std::fmt::Debug;
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::sync_state::{ImapFolderState, MailServerSettings};

/// Envelope data extracted from IMAP FETCH — lightweight, no body.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvelopeInfo {
    pub uid: u32,
    pub from_name: String,
    pub from_email: String,
    pub to_list: Vec<(String, String)>, // (name, email)
    pub cc_list: Vec<(String, String)>, // (name, email)
    pub reply_to: String,
    pub message_id: String,
    pub subject: String,
    pub date: String,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_replied: bool,
    pub has_attachment: bool,
    pub preview: String,
    #[serde(skip)]
    pub body: String,
    /// Parsed Authentication-Results: "spf=pass dkim=pass dmarc=pass" etc.
    pub auth_results: String,
    /// Gmail labels (X-GM-LABELS) including internal markers like \Inbox or
    /// CATEGORY_PERSONAL. Empty for non-Gmail servers. Display-side strips the
    /// internal markers; storage keeps them so the Priority mapper can read
    /// them without a second round trip.
    pub labels: Vec<String>,
    /// Per-attachment metadata extracted during body fetch.
    #[serde(skip)]
    pub attachments: Vec<AttachmentMeta>,
}

/// Result of a sync operation.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub new_emails: Vec<EnvelopeInfo>,
    pub deleted_uids: Vec<u32>,
    pub flag_changes: Vec<(u32, bool, bool, bool)>, // (uid, is_read, is_starred, is_replied)
    pub full_resync: bool,
    #[serde(skip)]
    pub new_state: ImapFolderState,
}

// ── Connection ──────────────────────────────────────────

pub(crate) trait ImapTransport: AsyncRead + AsyncWrite + Unpin + Send + Debug {}

impl<T> ImapTransport for T where T: AsyncRead + AsyncWrite + Unpin + Send + Debug {}

pub(crate) type ImapSession = async_imap::Session<Box<dyn ImapTransport>>;

/// Connect and authenticate to the IMAP server, returning a live session.
pub async fn connect(settings: &MailServerSettings) -> Result<ImapSession> {
    let host = &settings.incoming_server;
    let port = settings.incoming_port as u16;
    let security = settings.incoming_security.to_ascii_lowercase();

    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .with_context(|| format!("Failed to connect to IMAP server {}:{}", host, port))?;

    // Wrap tokio TcpStream with compat to provide futures_io traits
    let tcp_compat = tcp.compat();

    let tls = TlsConnector::new();
    let client = match security.as_str() {
        "ssl" => {
            let tls_stream = tls
                .connect(host, tcp_compat)
                .await
                .with_context(|| format!("TLS handshake failed for IMAP {}:{} using implicit TLS", host, port))?;
            let mut client = async_imap::Client::new(Box::new(tls_stream) as Box<dyn ImapTransport>);
            client
                .read_response()
                .await
                .with_context(|| format!("Failed to read IMAP greeting from {}:{}", host, port))?;
            client
        }
        "tls" => {
            let mut client = async_imap::Client::new(Box::new(tcp_compat) as Box<dyn ImapTransport>);
            client
                .read_response()
                .await
                .with_context(|| format!("Failed to read IMAP greeting from {}:{} before STARTTLS", host, port))?;
            client
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .with_context(|| format!("IMAP STARTTLS command failed for {}:{}", host, port))?;

            let stream = client.into_inner();
            let tls_stream = tls
                .connect(host, stream)
                .await
                .with_context(|| format!("TLS handshake failed for IMAP {}:{} after STARTTLS", host, port))?;
            async_imap::Client::new(Box::new(tls_stream) as Box<dyn ImapTransport>)
        }
        "none" => {
            let mut client = async_imap::Client::new(Box::new(tcp_compat) as Box<dyn ImapTransport>);
            client
                .read_response()
                .await
                .with_context(|| format!("Failed to read IMAP greeting from {}:{}", host, port))?;
            client
        }
        _ => {
            bail!("Unsupported IMAP security mode '{}' for {}:{}", settings.incoming_security, host, port);
        }
    };

    log::info!("IMAP connected to {}:{} using '{}', attempting login as '{}'", host, port, security, settings.incoming_username);

    let mut session = client
        .login(&settings.incoming_username, &settings.incoming_password)
        .await
        .map_err(|(e, _)| e)
        .with_context(|| format!("IMAP login failed for {}:{} using {}", host, port, security))?;

    log::info!("IMAP login successful for {}:{}", host, port);

    // RFC 2971 ID — best-effort; servers without the extension return BAD,
    // which must not sink the session. Some providers (e.g. Yahoo) require
    // ID to be sent before further commands will succeed.
    let fields = crate::identity::imap_id_fields();
    let id_pairs: Vec<(&str, Option<&str>)> = fields.iter()
        .map(|(k, v)| (*k, Some(v.as_str())))
        .collect();
    if let Err(e) = session.id(id_pairs).await {
        log::debug!("IMAP ID not accepted by {}:{} ({}); continuing without it", host, port, e);
    }

    Ok(session)
}

/// Probe the post-login server for the X-GM-EXT-1 capability (Gmail-only).
/// Returning false on error is intentional — non-Gmail servers shouldn't be
/// burdened with a Gmail-specific FETCH atom.
pub async fn detect_gmail(session: &mut ImapSession) -> bool {
    match session.capabilities().await {
        Ok(caps) => caps.has_str("X-GM-EXT-1"),
        Err(_) => false,
    }
}

/// Map IMAP folder name to our local folder id.
/// Handles common IMAP folder names, special-use attributes, and
/// hierarchical names like "INBOX.Sent" or "INBOX/Drafts".
pub fn map_folder_name(imap_name: &str) -> Option<&'static str> {
    let lower = imap_name.to_lowercase();
    // Strip common hierarchy prefixes ("INBOX.", "INBOX/", "[Gmail]/")
    let leaf = lower
        .strip_prefix("inbox.")
        .or_else(|| lower.strip_prefix("inbox/"))
        .unwrap_or(&lower);
    match leaf {
        "inbox" => Some("inbox"),
        "sent" | "sent items" | "sent messages" | "[gmail]/sent mail" => Some("sent"),
        "drafts" | "draft" | "[gmail]/drafts" => Some("drafts"),
        "trash" | "deleted items" | "deleted" | "[gmail]/trash" | "[gmail]/bin" => Some("deleted"),
        "junk" | "junk email" | "spam" | "bulk mail" | "[gmail]/spam" => Some("junk"),
        "archive" | "archives" => Some("archive"),
        _ => None,
    }
}

// ── Sync: envelope-only delta fetch ─────────────────────

/// Perform a delta sync on a single IMAP folder.
///
/// Strategy:
///   1. SELECT folder, get UIDVALIDITY + UIDNEXT + HIGHESTMODSEQ
///   2. If UIDVALIDITY changed → full re-sync (another client deleted/recreated mailbox)
///   3. Fetch UIDs from UIDNEXT..* to find new messages (envelope only)
///   4. If CONDSTORE supported and HIGHESTMODSEQ changed → fetch flag updates
///   5. Compare local UID set with server UID set to find deletions
///   6. Pages fetches in chunks of FETCH_PAGE_SIZE
pub async fn sync_folder(
    session: &mut ImapSession,
    prev_state: Option<ImapFolderState>,
    local_uids: Vec<(u32, String)>,
    account_id: &str,
    imap_folder: &str,
    local_folder: &str,
    is_gmail: bool,
) -> Result<SyncResult> {
    const FETCH_PAGE_SIZE: usize = 100;

    // SELECT the folder
    let mailbox = session
        .select(imap_folder)
        .await
        .context(format!("Failed to SELECT {}", imap_folder))?;

    let uid_validity = mailbox.uid_validity.unwrap_or(0);
    let uid_next = mailbox.uid_next.unwrap_or(1);
    let highest_modseq = mailbox
        .highest_modseq
        .unwrap_or(0);

    log::info!(
        "sync_folder {}/{}: exists={}, uid_validity={}, uid_next={}, modseq={}, local_uids={}",
        account_id, local_folder, mailbox.exists, uid_validity, uid_next, highest_modseq, local_uids.len()
    );

    let mut result = SyncResult {
        new_emails: Vec::new(),
        deleted_uids: Vec::new(),
        flag_changes: Vec::new(),
        full_resync: false,
        new_state: ImapFolderState {
            uid_validity,
            uid_next,
            highest_modseq,
        },
    };

    // Check UID validity — if changed, everything must be re-synced
    if let Some(ref prev) = prev_state {
        if prev.uid_validity != uid_validity && prev.uid_validity != 0 {
            log::warn!(
                "UIDVALIDITY changed for {}/{}: {} → {}. Full re-sync required.",
                account_id, local_folder, prev.uid_validity, uid_validity
            );
            result.full_resync = true;
        }
    }

    // Determine the UID range to fetch
    let fetch_from = if result.full_resync {
        1u32
    } else if let Some(ref prev) = prev_state {
        prev.uid_next
    } else {
        // First sync — fetch envelopes only for the most recent batch
        // For huge mailboxes, only sync the last ~500 messages initially
        let exists = mailbox.exists;
        if exists > 500 {
            // Fetch sequence numbers to get the UIDs of the last 500
            let range = format!("{}:*", exists.saturating_sub(499));
            let fetches: Vec<Fetch> = session.fetch(&range, "(UID)").await?.try_collect().await?;
            let mut min_uid = u32::MAX;
            for f in &fetches {
                if let Some(uid) = f.uid {
                    min_uid = min_uid.min(uid);
                }
            }
            if min_uid == u32::MAX { 1 } else { min_uid }
        } else {
            1
        }
    };

    log::info!(
        "sync_folder {}/{}: fetching UIDs from {} (uid_next={}), full_resync={}",
        account_id, local_folder, fetch_from, uid_next, result.full_resync
    );

    // Fetch new message envelopes (body + preview fetched on demand)
    if fetch_from < uid_next || result.full_resync {
        let range = format!("{}:*", fetch_from);
        let query = if is_gmail {
            "(UID FLAGS ENVELOPE X-GM-LABELS)"
        } else {
            "(UID FLAGS ENVELOPE)"
        };

        log::info!("sync_folder {}/{}: uid_fetch range='{}' query='{}'", account_id, local_folder, range, query);
        let fetches: Vec<Fetch> = session.uid_fetch(&range, query).await?.try_collect().await?;
        log::info!("sync_folder {}/{}: got {} fetch responses", account_id, local_folder, fetches.len());
        let mut page_count = 0;

        for fetch in &fetches {
            if let Some(uid) = fetch.uid {
                let envelope = parse_envelope(fetch);
                let env = EnvelopeInfo {
                    uid,
                    from_name: envelope.0,
                    from_email: envelope.1,
                    to_list: envelope.2,
                    cc_list: envelope.3,
                    reply_to: envelope.4,
                    message_id: envelope.5,
                    subject: envelope.6,
                    date: envelope.7,
                    is_read: has_flag(fetch, Flag::Seen),
                    is_starred: has_flag(fetch, Flag::Flagged),
                    is_replied: has_flag(fetch, Flag::Answered),
                    has_attachment: envelope.8,
                    preview: String::new(),
                    body: String::new(),
                    auth_results: String::new(),
                    labels: extract_gmail_labels(fetch),
                    attachments: Vec::new(),
                };
                result.new_emails.push(env);
                page_count += 1;

                // Log progress for large syncs
                if page_count % FETCH_PAGE_SIZE == 0 {
                    log::info!("Synced {} envelopes for {}/{}", page_count, account_id, local_folder);
                }
            }
        }
    } else {
        log::info!(
            "sync_folder {}/{}: skipping fetch — fetch_from ({}) >= uid_next ({}) and not full_resync",
            account_id, local_folder, fetch_from, uid_next
        );
    }

    // ── Second pass: fetch full bodies for new messages to extract previews ──
    //
    // On initial sync or full resync, only fetch bodies for the most recent
    // messages — older ones load lazily via `fetch_email_body` when the user
    // opens them. This keeps the initial sync of a large mailbox from turning
    // into hundreds of serial IMAP round trips.
    if !result.new_emails.is_empty() {
        const INITIAL_BODY_FETCH_CAP: usize = 30;
        let is_initial = prev_state.is_none() || result.full_resync;

        let new_uids: Vec<u32> = if is_initial && result.new_emails.len() > INITIAL_BODY_FETCH_CAP {
            let mut sorted: Vec<u32> = result.new_emails.iter().map(|e| e.uid).collect();
            sorted.sort_unstable_by(|a, b| b.cmp(a));
            sorted.truncate(INITIAL_BODY_FETCH_CAP);
            sorted
        } else {
            result.new_emails.iter().map(|e| e.uid).collect()
        };

        log::info!(
            "sync_folder {}/{}: fetching bodies for {}/{} new emails (initial={})",
            account_id, local_folder, new_uids.len(), result.new_emails.len(), is_initial
        );

        // Build a UID→index map for quick lookup
        let uid_to_idx: std::collections::HashMap<u32, usize> = result.new_emails.iter()
            .enumerate()
            .map(|(i, e)| (e.uid, i))
            .collect();

        for chunk in new_uids.chunks(25) {
            let range: Vec<String> = chunk.iter().map(|u| u.to_string()).collect();
            let range_str = range.join(",");

            match session.uid_fetch(&range_str, "(UID BODY.PEEK[])").await {
                Ok(stream) => {
                    let fetches: Vec<Fetch> = stream.try_collect().await.unwrap_or_default();
                    for fetch in &fetches {
                        if let (Some(uid), Some(raw_bytes)) = (fetch.uid, fetch.body()) {
                            let raw = String::from_utf8_lossy(raw_bytes);
                            let preview = extract_preview(&raw);
                            let html = extract_html_body(&raw);
                            let auth = extract_auth_results(&raw);
                            let has_attach = detect_has_attachment(&raw);
                            let attach_meta = extract_attachment_meta(&raw);
                            if let Some(&idx) = uid_to_idx.get(&uid) {
                                result.new_emails[idx].preview = preview;
                                result.new_emails[idx].body = html;
                                result.new_emails[idx].auth_results = auth;
                                result.new_emails[idx].has_attachment = has_attach;
                                result.new_emails[idx].attachments = attach_meta;
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("sync_folder {}/{}: body batch fetch failed: {}", account_id, local_folder, e);
                }
            }
        }
        log::info!("sync_folder {}/{}: body fetch complete", account_id, local_folder);
    }

    // Reconcile server UIDs with our local UID set: detect deletions AND
    // backfill any UIDs the server still has that we don't. The backfill
    // recovers from local data loss (crash mid-sync, or historical bugs that
    // nuked rows) — without it, a gap-fill only happens when the user
    // deletes sync state or bumps UIDVALIDITY.
    if !result.full_resync {
        // Always query the full server UID list when we have prior state,
        // even if local_uids is empty — that case is exactly when the local
        // store was wiped and we need to recover.
        let need_reconcile = prev_state.is_some() || !local_uids.is_empty();
        if need_reconcile {
            const GAP_FILL_CAP: usize = 200;

            let local_uid_set: HashSet<u32> = local_uids.iter().map(|(u, _)| *u).collect();
            let fetches: Vec<Fetch> = session.uid_fetch("1:*", "(UID)").await?.try_collect().await?;
            let server_uids: HashSet<u32> = fetches.iter().filter_map(|f| f.uid).collect();

            for uid in &local_uid_set {
                if !server_uids.contains(uid) {
                    result.deleted_uids.push(*uid);
                }
            }

            // Backfill: UIDs server has that we don't, and that we haven't
            // already fetched in this pass (the UID-next range above covers
            // fetch_from..*; exclude those to avoid double-work).
            let already_fetched: HashSet<u32> = result.new_emails.iter().map(|e| e.uid).collect();
            let mut missing: Vec<u32> = server_uids.iter()
                .filter(|u| !local_uid_set.contains(u) && !already_fetched.contains(u))
                .copied()
                .collect();
            if !missing.is_empty() {
                missing.sort_unstable_by(|a, b| b.cmp(a)); // newest-first
                if missing.len() > GAP_FILL_CAP {
                    missing.truncate(GAP_FILL_CAP);
                    log::warn!(
                        "sync_folder {}/{}: {} missing UIDs exceeds cap; backfilling top {} by UID and deferring the rest",
                        account_id, local_folder, missing.len() + GAP_FILL_CAP, GAP_FILL_CAP
                    );
                } else {
                    log::info!(
                        "sync_folder {}/{}: backfilling {} missing server UID(s)",
                        account_id, local_folder, missing.len()
                    );
                }

                // Batch envelope fetch for the missing UIDs.
                let query = if is_gmail {
                    "(UID FLAGS ENVELOPE X-GM-LABELS)"
                } else {
                    "(UID FLAGS ENVELOPE)"
                };
                for chunk in missing.chunks(100) {
                    let range: Vec<String> = chunk.iter().map(|u| u.to_string()).collect();
                    let range_str = range.join(",");
                    match session.uid_fetch(&range_str, query).await {
                        Ok(stream) => {
                            let chunk_fetches: Vec<Fetch> = stream.try_collect().await.unwrap_or_default();
                            for fetch in &chunk_fetches {
                                if let Some(uid) = fetch.uid {
                                    let envelope = parse_envelope(fetch);
                                    result.new_emails.push(EnvelopeInfo {
                                        uid,
                                        from_name: envelope.0,
                                        from_email: envelope.1,
                                        to_list: envelope.2,
                                        cc_list: envelope.3,
                                        reply_to: envelope.4,
                                        message_id: envelope.5,
                                        subject: envelope.6,
                                        date: envelope.7,
                                        is_read: has_flag(fetch, Flag::Seen),
                                        is_starred: has_flag(fetch, Flag::Flagged),
                                        is_replied: has_flag(fetch, Flag::Answered),
                                        has_attachment: envelope.8,
                                        preview: String::new(),
                                        body: String::new(),
                                        auth_results: String::new(),
                                        labels: extract_gmail_labels(fetch),
                                        attachments: Vec::new(),
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "sync_folder {}/{}: gap-fill fetch failed for range {}: {}",
                                account_id, local_folder, range_str, e
                            );
                        }
                    }
                }
            }
        }
    }

    // Detect flag changes via CONDSTORE if available
    if !result.full_resync {
        if let Some(ref prev) = prev_state {
            if prev.highest_modseq > 0 && highest_modseq > prev.highest_modseq {
                let range = format!("1:* (CHANGEDSINCE {})", prev.highest_modseq);
                if let Ok(stream) = session.uid_fetch(&range, "(UID FLAGS)").await {
                    let fetches: Vec<Fetch> = stream.try_collect().await.unwrap_or_default();
                    for fetch in &fetches {
                        if let Some(uid) = fetch.uid {
                            let is_read = has_flag(fetch, Flag::Seen);
                            let is_starred = has_flag(fetch, Flag::Flagged);
                            let is_replied = has_flag(fetch, Flag::Answered);
                            result.flag_changes.push((uid, is_read, is_starred, is_replied));
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

// ── Fetch email body on demand ──────────────────────────

/// Fetch the full RFC 822 body for a single message by UID.
/// Returns `(html_body, plain_text_preview, auth_results, has_attachment, attachments)`.
pub async fn fetch_body(session: &mut ImapSession, imap_folder: &str, uid: u32) -> Result<(String, String, String, bool, Vec<AttachmentMeta>)> {
    session.select(imap_folder).await?;

    let uid_str = uid.to_string();
    let fetches: Vec<Fetch> = session.uid_fetch(&uid_str, "BODY.PEEK[]").await?.try_collect().await?;

    for fetch in &fetches {
        if let Some(body) = fetch.body() {
            let raw = String::from_utf8_lossy(body).to_string();
            let html = extract_html_body(&raw);
            let preview = extract_preview(&raw);
            let auth = extract_auth_results(&raw);
            let has_attach = detect_has_attachment(&raw);
            let attachments = extract_attachment_meta(&raw);
            return Ok((html, preview, auth, has_attach, attachments));
        }
    }

    bail!("No body found for UID {}", uid)
}

/// Fetch the raw RFC 822 bytes for a single message by UID.
/// Used by `open_attachment` to extract individual attachment bytes without
/// re-parsing the full body pipeline.
pub async fn fetch_raw(session: &mut ImapSession, imap_folder: &str, uid: u32) -> Result<String> {
    session.select(imap_folder).await?;

    let uid_str = uid.to_string();
    let fetches: Vec<Fetch> = session.uid_fetch(&uid_str, "BODY.PEEK[]").await?.try_collect().await?;

    for fetch in &fetches {
        if let Some(body) = fetch.body() {
            return Ok(String::from_utf8_lossy(body).to_string());
        }
    }

    bail!("No raw body found for UID {}", uid)
}

/// Fetch full body + derived metadata for multiple UIDs at once (paged).
/// Mirrors the second pass in `sync_folder` but callable for backfill.
/// Returns `(uid, html, preview, auth_results, has_attachment, attachments)`.
pub async fn fetch_bodies_full_batch(
    session: &mut ImapSession,
    imap_folder: &str,
    uids: &[u32],
) -> Result<Vec<(u32, String, String, String, bool, Vec<AttachmentMeta>)>> {
    if uids.is_empty() { return Ok(Vec::new()); }

    session.select(imap_folder).await?;

    let mut results = Vec::with_capacity(uids.len());

    for chunk in uids.chunks(25) {
        let range: Vec<String> = chunk.iter().map(|u| u.to_string()).collect();
        let range_str = range.join(",");

        match session.uid_fetch(&range_str, "(UID BODY.PEEK[])").await {
            Ok(stream) => {
                let fetches: Vec<Fetch> = stream.try_collect().await.unwrap_or_default();
                for fetch in &fetches {
                    if let (Some(uid), Some(raw_bytes)) = (fetch.uid, fetch.body()) {
                        let raw = String::from_utf8_lossy(raw_bytes);
                        let html = extract_html_body(&raw);
                        let preview = extract_preview(&raw);
                        let auth = extract_auth_results(&raw);
                        let has_attach = detect_has_attachment(&raw);
                        let attach_meta = extract_attachment_meta(&raw);
                        results.push((uid, html, preview, auth, has_attach, attach_meta));
                    }
                }
            }
            Err(e) => {
                log::warn!("fetch_bodies_full_batch: chunk failed on {}: {}", imap_folder, e);
            }
        }
    }

    Ok(results)
}

/// Fetch bodies for multiple UIDs at once (paged).
pub async fn fetch_bodies_batch(
    session: &mut ImapSession,
    imap_folder: &str,
    uids: &[u32],
) -> Result<Vec<(u32, String)>> {
    if uids.is_empty() { return Ok(Vec::new()); }

    session.select(imap_folder).await?;

    let mut results = Vec::with_capacity(uids.len());

    // Fetch in chunks to avoid overwhelming the server
    for chunk in uids.chunks(50) {
        let uid_range: Vec<String> = chunk.iter().map(|u| u.to_string()).collect();
        let range = uid_range.join(",");

        let fetches: Vec<Fetch> = session.uid_fetch(&range, "UID BODY[]").await?.try_collect().await?;

        for fetch in &fetches {
            if let (Some(uid), Some(body)) = (fetch.uid, fetch.body()) {
                let body_str = String::from_utf8_lossy(body).to_string();
                let html = extract_html_body(&body_str);
                results.push((uid, html));
            }
        }
    }

    Ok(results)
}

// ── IDLE for real-time push ─────────────────────────────

/// Start IMAP IDLE on the given folder. Returns when the server sends
/// an update (new mail, flag change, expunge). Caller should re-sync after.
///
/// `timeout` is how long to IDLE before restarting (RFC recommends ≤29 min).
pub async fn idle_wait(
    mut session: ImapSession,
    imap_folder: &str,
    timeout: std::time::Duration,
) -> Result<ImapSession> {
    session.select(imap_folder).await?;

    let mut idle = session.idle();
    idle.init().await?;

    // idle.wait() returns (future, stop_source) — we await the future
    let (idle_future, _stop) = idle.wait();
    let _result = tokio::time::timeout(timeout, idle_future).await;

    // wait_keepalive() / manual done returns the session
    let session = idle.done().await?;

    Ok(session)
}

// ── Folder listing ──────────────────────────────────────

/// List all IMAP folders / mailboxes.
///
/// Uses RFC 6154 special-use attributes (`\Sent`, `\Trash`, …) when the server
/// provides them, which works regardless of the folder's display language.
/// Falls back to English name matching via `map_folder_name` for servers
/// that don't advertise attributes.
pub async fn list_folders(session: &mut ImapSession) -> Result<Vec<(String, String)>> {
    use async_imap::types::NameAttribute;

    let list: Vec<_> = session.list(None, Some("*")).await?.try_collect().await?;
    let mut folders = Vec::new();
    for mailbox in &list {
        let name = mailbox.name().to_string();

        // Try RFC 6154 special-use attributes first (language-independent)
        let from_attr = mailbox.attributes().iter().find_map(|attr| match attr {
            NameAttribute::Sent     => Some("sent"),
            NameAttribute::Drafts   => Some("drafts"),
            NameAttribute::Trash    => Some("deleted"),
            NameAttribute::Junk     => Some("junk"),
            NameAttribute::Archive => Some("archive"),
            // Skip \All (Gmail's "All Mail"): it contains every message in
            // the account, so syncing it retriggers the Message-ID dedup path
            // in sync_mail and moves Inbox rows to Archive. Gmail users keep
            // their Inbox/Sent/Trash folders; archived-only mail remains
            // reachable via Gmail web. Matches Thunderbird's default.
            NameAttribute::All => Some(""),
            _ => None,
        });

        // INBOX is always "INBOX" per RFC 3501, but handle case-insensitive
        let local = if name.eq_ignore_ascii_case("INBOX") {
            "inbox".to_string()
        } else if let Some(mapped) = from_attr {
            mapped.to_string()
        } else {
            // Fallback: English name matching for servers without attributes
            map_folder_name(&name).unwrap_or("").to_string()
        };

        folders.push((name, local));
    }
    Ok(folders)
}

// ── Flag mutations (multi-device safe) ──────────────────

/// Set flags on messages by UID. Uses +FLAGS so it's additive and safe
/// even if another client has modified flags concurrently.
pub async fn set_flags(
    session: &mut ImapSession,
    imap_folder: &str,
    uids: &[u32],
    flags: &[Flag<'_>],
) -> Result<()> {
    if uids.is_empty() { return Ok(()); }
    session.select(imap_folder).await?;

    let uid_str: Vec<String> = uids.iter().map(|u| u.to_string()).collect();
    let range = uid_str.join(",");

    let flag_strs: Vec<String> = flags.iter().map(|f| flag_to_str(f)).collect();
    let flag_str = flag_strs.join(" ");

    session.uid_store(&range, format!("+FLAGS ({})", flag_str)).await?.try_collect::<Vec<_>>().await?;
    Ok(())
}

/// Remove flags from messages by UID.
pub async fn remove_flags(
    session: &mut ImapSession,
    imap_folder: &str,
    uids: &[u32],
    flags: &[Flag<'_>],
) -> Result<()> {
    if uids.is_empty() { return Ok(()); }
    session.select(imap_folder).await?;

    let uid_str: Vec<String> = uids.iter().map(|u| u.to_string()).collect();
    let range = uid_str.join(",");

    let flag_strs: Vec<String> = flags.iter().map(|f| flag_to_str(f)).collect();
    let flag_str = flag_strs.join(" ");

    session.uid_store(&range, format!("-FLAGS ({})", flag_str)).await?.try_collect::<Vec<_>>().await?;
    Ok(())
}

/// Move a message to another folder (IMAP MOVE or COPY+DELETE).
/// Returns the new UID in the destination folder if it can be resolved via a
/// Message-ID SEARCH — otherwise Ok(None) (caller should drop the stale map
/// entry and let the next sync discover the message).
pub async fn move_message(
    session: &mut ImapSession,
    from_folder: &str,
    to_folder: &str,
    uid: u32,
    message_id: &str,
) -> Result<Option<u32>> {
    session.select(from_folder).await?;
    let uid_str = uid.to_string();

    // Try MOVE extension first (RFC 6851), fall back to COPY + STORE \Deleted + EXPUNGE
    if session.uid_mv(&uid_str, to_folder).await.is_err() {
        session.uid_copy(&uid_str, to_folder).await?;
        session.uid_store(&uid_str, "+FLAGS (\\Deleted)").await?.try_collect::<Vec<_>>().await?;
        session.expunge().await?.try_collect::<Vec<_>>().await?;
    }

    // Resolve the new UID in the destination by Message-ID header SEARCH.
    // Works on every server; independent of UIDPLUS / COPYUID support.
    let new_uid = find_uid_by_message_id(session, to_folder, message_id).await.ok().flatten();
    Ok(new_uid)
}

/// SELECT a folder and return the UID of a message matching the given Message-ID.
/// Strips surrounding angle brackets for broader server compatibility.
async fn find_uid_by_message_id(
    session: &mut ImapSession,
    folder: &str,
    message_id: &str,
) -> Result<Option<u32>> {
    let trimmed = message_id.trim().trim_start_matches('<').trim_end_matches('>');
    if trimmed.is_empty() { return Ok(None); }
    session.select(folder).await?;
    let query = format!("HEADER Message-ID \"{}\"", trimmed.replace('"', "\\\""));
    let uids = session.uid_search(&query).await?;
    Ok(uids.into_iter().max())
}

/// Append a raw RFC 822 message to a mailbox (used to save sent messages).
pub async fn append_to_mailbox(
    session: &mut ImapSession,
    imap_folder: &str,
    raw_message: &[u8],
) -> Result<()> {
    session
        .append(imap_folder, Some("(\\Seen)"), None, raw_message)
        .await
        .with_context(|| format!("IMAP APPEND to '{}' failed", imap_folder))?;
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────

fn has_flag(fetch: &Fetch, flag: Flag<'_>) -> bool {
    fetch.flags().any(|f| f == flag)
}

/// Extract Gmail X-GM-LABELS as owned strings; empty Vec on non-Gmail or
/// when the FETCH didn't request labels.
///
/// Labels returned as IMAP quoted-strings need unescaping: imap-proto's
/// `quoted` parser uses nom's `escaped` combinator, which preserves
/// `\\` and `\"` escape sequences in its output rather than decoding them.
/// Without this pass, `\Important` comes through as `\\Important` (two
/// backslashes) because Gmail quotes it on the wire as `"\\Important"`.
fn extract_gmail_labels(fetch: &Fetch) -> Vec<String> {
    fetch.gmail_labels()
        .map(|labels| labels.iter().map(|c| unescape_imap_quoted(c)).collect())
        .unwrap_or_default()
}

/// Reverse IMAP quoted-string escaping: `\\` → `\`, `\"` → `"`. Leaves
/// unquoted/atom-parsed labels unchanged.
fn unescape_imap_quoted(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next == '\\' || next == '"' {
                    out.push(next);
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

fn flag_to_str(flag: &Flag<'_>) -> String {
    match flag {
        Flag::Seen => "\\Seen".to_string(),
        Flag::Answered => "\\Answered".to_string(),
        Flag::Flagged => "\\Flagged".to_string(),
        Flag::Deleted => "\\Deleted".to_string(),
        Flag::Draft => "\\Draft".to_string(),
        Flag::Recent => "\\Recent".to_string(),
        Flag::MayCreate => "\\*".to_string(),
        Flag::Custom(s) => s.to_string(),
    }
}

/// Extract sender name and email from IMAP envelope.
fn parse_envelope(fetch: &Fetch) -> (String, String, Vec<(String, String)>, Vec<(String, String)>, String, String, String, String, bool) {
    let envelope = fetch.envelope();
    let (from_name, from_email) = if let Some(env) = &envelope {
        let from = env.from.as_ref().and_then(|v| v.first());
        let name = from
            .and_then(|a| a.name.as_ref())
            .map(|n| decode_mime_header(&String::from_utf8_lossy(n)))
            .unwrap_or_default();
        let mailbox = from
            .and_then(|a| a.mailbox.as_ref())
            .map(|m| String::from_utf8_lossy(m).to_string())
            .unwrap_or_default();
        let host = from
            .and_then(|a| a.host.as_ref())
            .map(|h| String::from_utf8_lossy(h).to_string())
            .unwrap_or_default();
        let email = if !mailbox.is_empty() && !host.is_empty() {
            format!("{}@{}", mailbox, host)
        } else {
            mailbox
        };
        (name, email)
    } else {
        (String::new(), String::new())
    };

    let to_list = if let Some(env) = &envelope {
        env.to.as_ref().map(|addrs| {
            addrs.iter().map(|a| {
                let name = a.name.as_ref()
                    .map(|n| decode_mime_header(&String::from_utf8_lossy(n)))
                    .unwrap_or_default();
                let mailbox = a.mailbox.as_ref()
                    .map(|m| String::from_utf8_lossy(m).to_string())
                    .unwrap_or_default();
                let host = a.host.as_ref()
                    .map(|h| String::from_utf8_lossy(h).to_string())
                    .unwrap_or_default();
                let email = if !mailbox.is_empty() && !host.is_empty() {
                    format!("{}@{}", mailbox, host)
                } else {
                    mailbox
                };
                (name, email)
            }).collect()
        }).unwrap_or_default()
    } else {
        Vec::new()
    };

    let cc_list = if let Some(env) = &envelope {
        env.cc.as_ref().map(|addrs| {
            addrs.iter().map(|a| {
                let name = a.name.as_ref()
                    .map(|n| decode_mime_header(&String::from_utf8_lossy(n)))
                    .unwrap_or_default();
                let mailbox = a.mailbox.as_ref()
                    .map(|m| String::from_utf8_lossy(m).to_string())
                    .unwrap_or_default();
                let host = a.host.as_ref()
                    .map(|h| String::from_utf8_lossy(h).to_string())
                    .unwrap_or_default();
                let email = if !mailbox.is_empty() && !host.is_empty() {
                    format!("{}@{}", mailbox, host)
                } else {
                    mailbox
                };
                (name, email)
            }).collect()
        }).unwrap_or_default()
    } else {
        Vec::new()
    };

    let reply_to = if let Some(env) = &envelope {
        env.reply_to.as_ref()
            .and_then(|v| v.first())
            .map(|a| {
                let mailbox = a.mailbox.as_ref()
                    .map(|m| String::from_utf8_lossy(m).to_string())
                    .unwrap_or_default();
                let host = a.host.as_ref()
                    .map(|h| String::from_utf8_lossy(h).to_string())
                    .unwrap_or_default();
                if !mailbox.is_empty() && !host.is_empty() {
                    format!("{}@{}", mailbox, host)
                } else {
                    mailbox
                }
            })
            .unwrap_or_default()
    } else {
        String::new()
    };

    let message_id = if let Some(env) = &envelope {
        env.message_id.as_ref()
            .map(|m| String::from_utf8_lossy(m).trim().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };

    let subject = envelope
        .as_ref()
        .and_then(|e| e.subject.as_ref())
        .map(|s| decode_mime_header(&String::from_utf8_lossy(s)))
        .unwrap_or_default();

    let date = envelope
        .as_ref()
        .and_then(|e| e.date.as_ref())
        .map(|d| {
            let raw = String::from_utf8_lossy(d);
            // Parse RFC 2822 date to ISO 8601 for correct sorting
            DateTime::parse_from_rfc2822(raw.trim())
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|_| {
                    // Try common IMAP date variants
                    parse_loose_date(&raw)
                        .unwrap_or_else(|| raw.to_string())
                })
        })
        .unwrap_or_default();

    // has_attachment is detected during the body-fetch pass (see second pass below)
    // where we have the full RFC 822 bytes available for MIME inspection.
    let has_attachment = false;

    (from_name, from_email, to_list, cc_list, reply_to, message_id, subject, date, has_attachment)
}

/// Parse loose date formats commonly seen in IMAP envelopes.
fn parse_loose_date(s: &str) -> Option<String> {
    // Try without day name: "10 Mar 2025 14:30:00 +0000"
    let formats = [
        "%d %b %Y %H:%M:%S %z",
        "%d %b %Y %H:%M:%S",
        "%a, %d %b %Y %H:%M:%S %z",
    ];
    for fmt in &formats {
        if let Ok(dt) = DateTime::parse_from_str(s.trim(), fmt) {
            return Some(dt.to_rfc3339());
        }
    }
    // Last resort: try chrono's parser for NaiveDateTime without timezone
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s.trim(), "%d %b %Y %H:%M:%S") {
        let dt = ndt.and_utc();
        return Some(dt.to_rfc3339());
    }
    None
}

/// Extract a short plain-text preview (~200 chars) from a raw MIME message snippet.
///
/// Guarantee: the returned string is never empty if the message was parsed at
/// all. A genuinely textless message (e.g. attachment-only) yields a single
/// space so callers can distinguish "fetched, nothing to preview" from
/// "envelope-only, preview not yet fetched" (which stays as `""`).
fn extract_preview(raw: &str) -> String {
    let result = extract_preview_inner(raw);
    if result.is_empty() { " ".to_string() } else { result }
}

fn extract_preview_inner(raw: &str) -> String {
    if let Some(msg) = mail_parser::MessageParser::default().parse(raw.as_bytes()) {
        // Prefer plain text part, but skip it if it looks like CSS/code
        if let Some(text) = msg.body_text(0) {
            let trimmed = text.trim();
            if !trimmed.is_empty() && !looks_like_css(trimmed) {
                let cleaned = clean_preview(&text);
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
        }
        // Fall back to HTML stripped of tags and style/script/hidden content.
        // Email marketers routinely open with a hidden preheader stuffed with
        // invisible spacer characters, so an initial preview that reduces to
        // empty or a handful of punctuation marks isn't meaningful — retry
        // the next HTML part.
        for idx in 0..msg.parts.len().max(1) {
            if let Some(html) = msg.body_html(idx) {
                let cleaned = clean_preview(&strip_html_tags(&html));
                if has_visible_text(&cleaned) {
                    return cleaned;
                }
            }
        }
        // Last resort: plain text part even if it's mostly whitespace.
        if let Some(text) = msg.body_text(0) {
            return clean_preview(&text);
        }
    }
    // Fallback: skip headers, strip tags from body
    if let Some(body) = raw.split("\r\n\r\n").nth(1) {
        return clean_preview(&strip_html_tags(body));
    }
    String::new()
}

/// Cheap combining-mark check covering the three core Unicode ranges. We
/// don't need perfect coverage — the goal is to drop stray combining diacritics
/// that marketing preheaders leave over base characters to pad the preview.
fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | // Combining Diacritical Marks
        0x1AB0..=0x1AFF | // Combining Diacritical Marks Extended
        0x1DC0..=0x1DFF | // Combining Diacritical Marks Supplement
        0x20D0..=0x20FF | // Combining Diacritical Marks for Symbols
        0xFE20..=0xFE2F)  // Combining Half Marks
}

/// Return true if `s` has at least a few printable, non-punctuation characters.
/// Used to reject preview strings that reduce to only spacer punctuation after
/// invisible characters are stripped.
fn has_visible_text(s: &str) -> bool {
    s.chars().filter(|c| c.is_alphanumeric()).count() >= 3
}

/// Metadata for a single MIME attachment part.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AttachmentMeta {
    pub filename: String,
    pub mime_type: String,
    pub size: usize,
}

/// Return true if the raw RFC 822 message contains at least one attachment part.
fn detect_has_attachment(raw: &str) -> bool {
    if let Some(msg) = mail_parser::MessageParser::default().parse(raw.as_bytes()) {
        return msg.attachment(0).is_some();
    }
    false
}

/// Extract metadata (filename, MIME type, size) for every attachment in the message.
pub fn extract_attachment_meta(raw: &str) -> Vec<AttachmentMeta> {
    use mail_parser::MimeHeaders;
    use mail_parser::PartType;

    let mut result = Vec::new();
    if let Some(msg) = mail_parser::MessageParser::default().parse(raw.as_bytes()) {
        let mut idx = 0;
        while let Some(part) = msg.attachment(idx) {
            let filename = part.attachment_name()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or("attachment")
                .to_string();
            let mime_type = part.content_type().map(|ct| {
                let sub = ct.c_subtype.as_deref().unwrap_or("octet-stream");
                format!("{}/{}", ct.c_type.as_ref(), sub)
            }).unwrap_or_else(|| "application/octet-stream".to_string());
            let size = match &part.body {
                PartType::Binary(data) | PartType::InlineBinary(data) => data.len(),
                PartType::Text(text) => text.len(),
                PartType::Html(html) => html.len(),
                _ => 0,
            };
            result.push(AttachmentMeta { filename, mime_type, size });
            idx += 1;
        }
    }
    result
}

/// Extract the raw bytes for attachment at `idx` from a raw RFC 822 message.
/// Returns `(filename, bytes)` or `None` if the index is out of range.
pub fn extract_attachment_bytes(raw: &str, idx: usize) -> Option<(String, Vec<u8>)> {
    use mail_parser::MimeHeaders;
    use mail_parser::PartType;

    let msg = mail_parser::MessageParser::default().parse(raw.as_bytes())?;
    let part = msg.attachment(idx)?;

    let filename = part.attachment_name()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or("attachment")
        .to_string();

    let bytes: Vec<u8> = match &part.body {
        PartType::Binary(data) | PartType::InlineBinary(data) => data.to_vec(),
        PartType::Text(text) => text.as_bytes().to_vec(),
        PartType::Html(html) => html.as_bytes().to_vec(),
        _ => return None,
    };

    Some((filename, bytes))
}

/// Detect if text content looks like CSS rather than readable email body.
fn looks_like_css(text: &str) -> bool {
    let end = text.char_indices().nth(300).map(|(i, _)| i).unwrap_or(text.len());
    let sample = &text[..end];
    let brace_count = sample.chars().filter(|&c| c == '{' || c == '}').count();
    brace_count >= 3 || sample.contains("!important") || sample.contains("font-weight:") || sample.contains("line-height:")
}

/// Strip HTML tags and content inside style/script and visually-hidden
/// elements (preheader spacers). Handles the common inline-style patterns
/// used by marketing mail: `display:none`, `visibility:hidden`, `opacity:0`,
/// and Outlook's `mso-hide:all`.
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut skip_verbatim = false;                 // inside <style> / <script>
    let mut hidden_stack: Vec<String> = Vec::new(); // nested hidden elements
    let mut tag_buf = String::new();
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let tag_lower = tag_buf.to_lowercase();

                // style/script — skip their text content wholesale
                if tag_lower.starts_with("style") || tag_lower.starts_with("script") {
                    skip_verbatim = true;
                } else if tag_lower.starts_with("/style") || tag_lower.starts_with("/script") {
                    skip_verbatim = false;
                }

                // Extract the element name (first word after "/" if closing).
                let is_closing = tag_lower.starts_with('/');
                let name: String = tag_lower
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                let is_self_closing = tag_buf.trim_end().ends_with('/');

                if !name.is_empty() {
                    if is_closing {
                        // Pop the innermost matching hidden element.
                        if let Some(pos) = hidden_stack.iter().rposition(|n| n == &name) {
                            hidden_stack.truncate(pos);
                        }
                    } else if !is_self_closing && is_hidden_tag(&tag_lower) {
                        hidden_stack.push(name);
                    }
                }

                tag_buf.clear();
            }
            _ if in_tag => { tag_buf.push(ch); }
            _ if !skip_verbatim && hidden_stack.is_empty() => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Heuristic: does this opening tag's attribute blob indicate the element is
/// visually hidden? Operates on the already-lowercased tag contents, e.g.
/// `div style="display:none; ...; mso-hide:all"`.
fn is_hidden_tag(tag_lower: &str) -> bool {
    // Collapse whitespace so "display : none" matches too.
    let normalized: String = tag_lower.chars().filter(|c| !c.is_whitespace()).collect();
    normalized.contains("display:none")
        || normalized.contains("visibility:hidden")
        || normalized.contains("opacity:0")
        || normalized.contains("max-height:0")
        || normalized.contains("mso-hide:all")
        || normalized.contains("hidden=\"true\"")
        || normalized.contains("hidden=true")
}

/// Collapse whitespace and truncate to ~200 chars for the message list.
fn clean_preview(text: &str) -> String {
    let trimmed = text.trim_start_matches('\u{FEFF}');
    // Strip zero-width and other invisible Unicode characters used as email preview spacers.
    // U+034F (COMBINING GRAPHEME JOINER) shows up rendered as `͏` and is a
    // favorite preheader filler; the combining-mark ranges below catch stray
    // diacritics left behind by Gmail-style spacer trickery.
    let stripped: String = trimmed.chars().filter(|&c| !matches!(c,
        '\u{034F}' |
        '\u{00AD}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}' |
        '\u{180E}' |
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{200E}' | '\u{200F}' |
        '\u{202A}' | '\u{202B}' | '\u{202C}' | '\u{202D}' | '\u{202E}' |
        '\u{2028}' | '\u{2029}' | '\u{2060}' | '\u{2061}' | '\u{2062}' | '\u{2063}' | '\u{2064}' |
        '\u{FEFF}' |
        '\u{3164}' | '\u{FFA0}'
    ) && !is_combining_mark(c)).collect();
    let collapsed: String = stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.len() > 200 {
        let mut end = 200;
        while !collapsed.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}…", &collapsed[..end])
    } else {
        collapsed
    }
}

/// Decode RFC 2047 encoded-words in a MIME header value.
/// Handles `=?charset?Q?...?=` (quoted-printable) and `=?charset?B?...?=` (base64).
pub fn decode_mime_header(input: &str) -> String {
    // Fast path: no encoded-words
    if !input.contains("=?") {
        return input.to_string();
    }

    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(start) = remaining.find("=?") {
        // Text before the encoded-word
        let prefix = &remaining[..start];
        // Only add whitespace-only prefix between adjacent encoded-words if it's not just spacing
        if !prefix.is_empty() && (!result.ends_with('?') || prefix.trim().len() > 0) {
            result.push_str(prefix);
        }
        remaining = &remaining[start + 2..];

        // Parse charset
        let Some(q1) = remaining.find('?') else {
            result.push_str("=?");
            break;
        };
        let charset = &remaining[..q1];
        remaining = &remaining[q1 + 1..];

        // Parse encoding (Q or B)
        let Some(q2) = remaining.find('?') else {
            result.push_str(&format!("=?{}?", charset));
            break;
        };
        let encoding = &remaining[..q2];
        remaining = &remaining[q2 + 1..];

        // Parse encoded text until closing ?=
        let Some(end) = remaining.find("?=") else {
            result.push_str(&format!("=?{}?{}?", charset, encoding));
            break;
        };
        let encoded_text = &remaining[..end];
        remaining = &remaining[end + 2..];

        // Decode the payload bytes
        let decoded_bytes = match encoding.to_ascii_uppercase().as_str() {
            "Q" => decode_quoted_printable_header(encoded_text),
            "B" => base64::engine::general_purpose::STANDARD
                .decode(encoded_text)
                .unwrap_or_else(|_| encoded_text.as_bytes().to_vec()),
            _ => encoded_text.as_bytes().to_vec(),
        };

        // Convert decoded bytes using the specified charset
        let decoded = decode_charset(charset, &decoded_bytes);
        result.push_str(&decoded);
    }

    // Append any remaining text
    result.push_str(remaining);
    result
}

/// Decode quoted-printable bytes in an RFC 2047 Q-encoded word.
/// In Q-encoding, underscores represent spaces and =XX is a hex byte.
fn decode_quoted_printable_header(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'_' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'=' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                &String::from_utf8_lossy(&bytes[i + 1..i + 3]),
                16,
            ) {
                out.push(byte);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Decode bytes from a given charset to a UTF-8 String.
fn decode_charset(charset: &str, bytes: &[u8]) -> String {
    match charset.to_ascii_uppercase().as_str() {
        "UTF-8" | "UTF8" => String::from_utf8_lossy(bytes).to_string(),
        "ISO-8859-1" | "LATIN1" | "LATIN-1" => {
            bytes.iter().map(|&b| b as char).collect()
        }
        "ISO-8859-15" | "LATIN9" => {
            // ISO-8859-15 is mostly ISO-8859-1 with a few overrides
            bytes.iter().map(|&b| match b {
                0xA4 => '\u{20AC}', // Euro sign
                0xA6 => '\u{0160}', // Š
                0xA8 => '\u{0161}', // š
                0xB4 => '\u{017D}', // Ž
                0xB8 => '\u{017E}', // ž
                0xBC => '\u{0152}', // Œ
                0xBD => '\u{0153}', // œ
                0xBE => '\u{0178}', // Ÿ
                _ => b as char,
            }).collect()
        }
        "US-ASCII" | "ASCII" => String::from_utf8_lossy(bytes).to_string(),
        "WINDOWS-1252" | "CP1252" => {
            bytes.iter().map(|&b| match b {
                0x80 => '\u{20AC}',
                0x85 => '\u{2026}',
                0x91 => '\u{2018}',
                0x92 => '\u{2019}',
                0x93 => '\u{201C}',
                0x94 => '\u{201D}',
                0x96 => '\u{2013}',
                0x97 => '\u{2014}',
                _ => b as char,
            }).collect()
        }
        _ => {
            // Fallback: try UTF-8, then latin1
            String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|_| bytes.iter().map(|&b| b as char).collect())
        }
    }
}

/// Extract SPF, DKIM, and DMARC results from the Authentication-Results header.
/// Returns a compact summary like "spf=pass dkim=pass dmarc=pass" or empty string if absent.
fn extract_auth_results(raw: &str) -> String {
    if let Some(msg) = mail_parser::MessageParser::default().parse(raw.as_bytes()) {
        // mail_parser gives us header values; Authentication-Results may appear multiple times
        let mut parts: Vec<String> = Vec::new();
        for hdr in msg.headers() {
            if hdr.name().to_string().eq_ignore_ascii_case("authentication-results") {
                if let mail_parser::HeaderValue::Text(val) = hdr.value() {
                    // Parse out spf=, dkim=, dmarc= results from the header value
                    let text = val.to_lowercase();
                    for protocol in &["spf", "dkim", "dmarc"] {
                        if let Some(pos) = text.find(&format!("{}=", protocol)) {
                            let after = &text[pos..];
                            // Extract "protocol=result" — result is the next token
                            let result_str: String = after.chars()
                                .take_while(|c| !c.is_whitespace() && *c != ';')
                                .collect();
                            if !parts.iter().any(|p| p.starts_with(protocol)) {
                                parts.push(result_str);
                            }
                        }
                    }
                }
            }
        }
        return parts.join(" ");
    }
    String::new()
}

/// Extract HTML body from raw RFC 822 message.
/// Falls back to wrapping plaintext in <pre> if no HTML part found.
/// Inline images referenced via `cid:` URIs are resolved to `data:` URIs.
fn extract_html_body(raw: &str) -> String {
    let message = match mail_parser::MessageParser::default().parse(raw.as_bytes()) {
        Some(msg) => msg,
        None => {
            log::warn!("mail-parser failed to parse message, using fallback");
            return fallback_extract(raw);
        }
    };

    // Prefer HTML body, then plain text
    if let Some(html) = message.body_html(0) {
        return resolve_cid_references(&message, &html);
    }

    if let Some(text) = message.body_text(0) {
        return format!(
            "<pre style=\"white-space: pre-wrap; font-family: inherit;\">{}</pre>",
            html_escape(&text)
        );
    }

    // Walk all parts looking for any text content
    for part_id in 0..message.parts.len() {
        if let Some(html) = message.body_html(part_id) {
            return resolve_cid_references(&message, &html);
        }
    }
    for part_id in 0..message.parts.len() {
        if let Some(text) = message.body_text(part_id) {
            return format!(
                "<pre style=\"white-space: pre-wrap; font-family: inherit;\">{}</pre>",
                html_escape(&text)
            );
        }
    }

    fallback_extract(raw)
}

/// Replace `cid:xxx` references in HTML with inline `data:` URIs built from
/// the message's MIME parts that carry a matching Content-ID header.
fn resolve_cid_references(message: &mail_parser::Message<'_>, html: &str) -> String {
    use mail_parser::MimeHeaders;
    use mail_parser::PartType;

    // Quick check — skip the work if there are no cid: references
    if !html.contains("cid:") {
        return html.to_string();
    }

    // Build a map of content-id → data URI from all MIME parts
    let mut cid_map: std::collections::HashMap<&str, String> = std::collections::HashMap::new();

    for part in &message.parts {
        let cid = match part.content_id() {
            Some(id) => id.trim_matches(|c| c == '<' || c == '>'),
            None => continue,
        };

        let ct = part.content_type().map(|ct| {
            let main = ct.c_type.as_ref();
            let sub = ct.c_subtype.as_deref().unwrap_or("octet-stream");
            format!("{}/{}", main, sub)
        }).unwrap_or_else(|| "application/octet-stream".to_string());

        let bytes: Option<&[u8]> = match &part.body {
            PartType::Binary(data) | PartType::InlineBinary(data) => Some(data.as_ref()),
            _ => None,
        };

        if let Some(data) = bytes {
            let b64 = base64::engine::general_purpose::STANDARD.encode(data);
            cid_map.insert(cid, format!("data:{};base64,{}", ct, b64));
        }
    }

    if cid_map.is_empty() {
        return html.to_string();
    }

    // Replace all cid: references in the HTML
    let mut result = html.to_string();
    for (cid, data_uri) in &cid_map {
        // Match both quoted and unquoted forms: src="cid:xxx" or src=cid:xxx
        result = result.replace(&format!("cid:{}", cid), data_uri);
    }
    result
}

/// Simple HTML entity escaping for plaintext bodies.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Last-resort extraction when mail-parser fails.
fn fallback_extract(raw: &str) -> String {
    let text = raw
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or(raw);
    format!("<pre style=\"white-space: pre-wrap;\">{}</pre>", html_escape(text))
}
