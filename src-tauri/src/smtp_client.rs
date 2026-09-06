//! smtp_client.rs — Send emails via SMTP using lettre.
//!
//! Supports TLS, STARTTLS, and plain connections.
//! Includes a reliable outbox queue for offline / retry support.

use anyhow::{Result, Context};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{header::ContentType, MultiPart, SinglePart, Attachment, Mailbox},
    transport::smtp::authentication::Credentials,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::sync_state::MailServerSettings;

/// A prepared outbound email.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundEmail {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_html: String,
    pub attachments: Vec<AttachmentInfo>,
    /// Local id of the message this one replies to. \Answered is set on it only
    /// once SMTP has actually accepted the reply, which may be a retry hours later.
    #[serde(default)]
    pub reply_to_email_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub name: String,
    pub path: String,
}

/// Build the SMTP transport from server settings.
async fn build_transport(settings: &MailServerSettings) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let creds = Credentials::new(
        settings.smtp_username.clone(),
        settings.smtp_password.clone(),
    );

    let transport = match settings.smtp_security.as_str() {
        "ssl" => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.smtp_server)
                .context("Failed to build SMTP relay")?
                .port(settings.smtp_port as u16)
                .credentials(creds)
                .build()
        }
        "tls" => {
            // STARTTLS
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.smtp_server)
                .context("Failed to build SMTP STARTTLS relay")?
                .port(settings.smtp_port as u16)
                .credentials(creds)
                .build()
        }
        _ => {
            // No encryption (not recommended)
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.smtp_server)
                .port(settings.smtp_port as u16)
                .credentials(creds)
                .build()
        }
    };

    Ok(transport)
}

/// Build the full MIME [`Message`] for `email` without sending. `message_id`
/// is written verbatim (lettre adds no angle brackets), so callers must pass
/// the full RFC 5322 form `<id@domain>` — a bare id goes onto the wire as a
/// malformed header. Verbatim writing is what lets the draft-upload path
/// byte-match what IMAP envelopes report.
pub async fn build_message(
    from_email: &str,
    from_name: &str,
    email: &OutboundEmail,
    message_id: Option<&str>,
) -> Result<Message> {
    // Build the from address using Mailbox to properly handle special chars in names
    let from_addr: lettre::Address = from_email.parse().context("Invalid from email address")?;
    let from_mailbox = Mailbox::new(Some(from_name.to_string()), from_addr);

    // Build the message
    let mut builder = Message::builder()
        .from(from_mailbox)
        .message_id(message_id.map(|s| s.to_string()))
        .user_agent(crate::identity::mail_user_agent())
        .subject(&email.subject);

    // Add To recipients
    for addr in &email.to {
        builder = builder.to(addr.parse().context(format!("Invalid To address: {}", addr))?);
    }

    // Add Cc recipients
    for addr in &email.cc {
        if !addr.is_empty() {
            builder = builder.cc(addr.parse().context(format!("Invalid Cc address: {}", addr))?);
        }
    }

    // Add Bcc recipients
    for addr in &email.bcc {
        if !addr.is_empty() {
            builder = builder.bcc(addr.parse().context(format!("Invalid Bcc address: {}", addr))?);
        }
    }

    // Build the message body with optional attachments
    let message = if email.attachments.is_empty() {
        builder
            .header(ContentType::TEXT_HTML)
            .body(email.body_html.clone())
            .context("Failed to build email message")?
    } else {
        // Multipart message with HTML body + attachments
        let html_part = SinglePart::builder()
            .header(ContentType::TEXT_HTML)
            .body(email.body_html.clone());

        let mut multipart = MultiPart::mixed().singlepart(html_part);

        for att in &email.attachments {
            let file_bytes = tokio::fs::read(&att.path)
                .await
                .context(format!("Failed to read attachment: {}", att.path))?;

            let content_type = ContentType::parse(
                &mime_guess::from_path(&att.path)
                    .first_or_octet_stream()
                    .to_string(),
            )
            .unwrap_or(ContentType::parse("application/octet-stream").unwrap());

            let attachment = Attachment::new(att.name.clone()).body(file_bytes, content_type);
            multipart = multipart.singlepart(attachment);
        }

        builder
            .multipart(multipart)
            .context("Failed to build multipart email")?
    };

    Ok(message)
}

/// Parse a composer recipient string ("Name <a@b>; c@d, e@f") into entries
/// lettre can consume, dropping anything that doesn't parse as an address —
/// drafts legitimately hold half-typed recipients that must not fail an
/// upload. Semicolons always separate; commas separate only outside
/// "Last, First <email>" display names.
pub fn parse_recipient_list(raw: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for seg in raw.split(';') {
        if seg.contains('<') && seg.contains('>') {
            parts.push(seg.trim().to_string());
        } else {
            parts.extend(seg.split(',').map(|s| s.trim().to_string()));
        }
    }
    parts
        .into_iter()
        .filter(|s| !s.is_empty() && s.parse::<Mailbox>().is_ok())
        .collect()
}

/// Send an email directly via SMTP. Returns the raw RFC 822 message bytes on success.
/// If `message_id` is Some, it overrides lettre's auto-generated Message-ID so the same
/// value is stamped on every retry (enables post-crash reconciliation via IMAP SEARCH).
pub async fn send_email(
    settings: &MailServerSettings,
    from_email: &str,
    from_name: &str,
    email: &OutboundEmail,
    message_id: Option<&str>,
) -> Result<Vec<u8>> {
    let transport = build_transport(settings).await?;
    let message = build_message(from_email, from_name, email, message_id).await?;
    let raw = message.formatted();

    // Bounded, and loud on both ends. The transport connects lazily inside
    // send(), so the timeout covers connect + TLS + AUTH + submission. Without
    // it, a server that accepts TCP but withholds its banner (typical when a
    // per-user connection cap is hit) hangs this future forever — the send
    // spinner never resolves and nothing is ever logged.
    log::info!(
        "SMTP: sending via {}:{} ({}) from {}",
        settings.smtp_server, settings.smtp_port, settings.smtp_security, from_email
    );
    tokio::time::timeout(std::time::Duration::from_secs(60), transport.send(message))
        .await
        .map_err(|_| anyhow::anyhow!("SMTP timed out after 60s (server or connection-limit stall)"))?
        .context("SMTP send failed")?;
    log::info!("SMTP: message accepted by {}", settings.smtp_server);

    Ok(raw)
}

/// Send a calendar invitation email with the ICS embedded as a text/calendar part.
pub async fn send_invitation(
    settings: &MailServerSettings,
    from_email: &str,
    from_name: &str,
    to_addresses: &[String],
    subject: &str,
    html_body: &str,
    ics_data: &str,
) -> Result<()> {
    let transport = build_transport(settings).await?;
    let from_addr: lettre::Address = from_email.parse().context("Invalid from email address")?;
    let from_mailbox = Mailbox::new(Some(from_name.to_string()), from_addr);

    let mut builder = Message::builder()
        .from(from_mailbox)
        .user_agent(crate::identity::mail_user_agent())
        .subject(subject);

    for addr in to_addresses {
        builder = builder.to(addr.parse().context(format!("Invalid To address: {}", addr))?);
    }

    let html_part = SinglePart::builder()
        .header(ContentType::TEXT_HTML)
        .body(html_body.to_string());

    let cal_part = SinglePart::builder()
        .header(ContentType::parse("text/calendar; method=REQUEST; charset=UTF-8").unwrap())
        .body(ics_data.to_string());

    let message = builder
        .multipart(MultiPart::mixed().singlepart(html_part).singlepart(cal_part))
        .context("Failed to build invitation email")?;

    transport.send(message).await.context("SMTP invitation send failed")?;
    Ok(())
}

// ── Outbox queue for reliable offline sending ───────────

/// Queue an email for sending. Returns (outbox_id, message_id).
///
/// `from_email` is used to form the domain part of the Message-ID so that
/// upstream servers don't reject the message for an off-domain id. The id is
/// stored in the full RFC 5322 form `<uuid@domain>` — it reaches the wire
/// verbatim via [`build_message`], and a bare `uuid@domain` is a malformed
/// header (SpamAssassin INVALID_MSGID, −1.167).
/// Where a queued message's attachments are spooled: `<storage>/outbox/<queue id>/`.
pub fn spool_dir(storage_dir: &std::path::Path, outbox_id: &str) -> std::path::PathBuf {
    storage_dir.join("outbox").join(outbox_id)
}

/// Drop a queued message's spooled attachments. Called once the row is
/// resolved (sent, or returned to Drafts with the files embedded).
pub fn remove_spool(storage_dir: &std::path::Path, outbox_id: &str) {
    let dir = spool_dir(storage_dir, outbox_id);
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            log::warn!("outbox: could not remove spool {}: {}", dir.display(), e);
        }
    }
}

/// Copy the composer's attachments into the queue's own spool and return the
/// email with its attachment paths pointing there.
///
/// The composer hands us paths wherever the user picked the files -- a
/// Downloads folder, a network mount of another machine. A queued message can
/// wait hours for a retry, and by then that mount may be asleep or the file
/// moved; reading it at retry time would fail the send for a reason that has
/// nothing to do with mail. Copying now makes the queue self-contained. A file
/// that cannot be read NOW is a real error and is reported before queueing.
fn spool_attachments(
    storage_dir: &std::path::Path,
    outbox_id: &str,
    email: &OutboundEmail,
) -> Result<OutboundEmail> {
    if email.attachments.is_empty() {
        return Ok(email.clone());
    }
    let dir = spool_dir(storage_dir, outbox_id);
    std::fs::create_dir_all(&dir).context("Failed to create outbox spool")?;

    let mut spooled = Vec::with_capacity(email.attachments.len());
    for (i, att) in email.attachments.iter().enumerate() {
        // Index prefix: two attachments may share a display name.
        let safe_name: String = att.name.chars()
            .map(|c| if c == '/' || c == '\\' || c == '\0' { '_' } else { c })
            .collect();
        let dest = dir.join(format!("{:03}-{}", i, safe_name));
        std::fs::copy(&att.path, &dest)
            .with_context(|| format!("Failed to read attachment: {}", att.name))?;
        spooled.push(AttachmentInfo {
            name: att.name.clone(),
            path: dest.to_string_lossy().into_owned(),
        });
    }
    Ok(OutboundEmail { attachments: spooled, ..email.clone() })
}

pub fn queue_email(
    conn: &Connection,
    storage_dir: &std::path::Path,
    account_id: &str,
    email: &OutboundEmail,
    from_email: &str,
) -> Result<(String, String, OutboundEmail)> {
    let id = uuid::Uuid::new_v4().to_string();
    let domain = from_email.rsplit('@').next().filter(|d| !d.is_empty()).unwrap_or("localhost");
    let message_id = format!("<{}@{}>", uuid::Uuid::new_v4(), domain);
    let now = chrono::Utc::now().to_rfc3339();

    let email = spool_attachments(storage_dir, &id, email)?;

    conn.execute(
        "INSERT INTO outbox_queue (id, account_id, to_addrs, cc_addrs, bcc_addrs, subject, body_html, attachments, created_at, status, message_id, reply_to_email_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10, ?11)",
        rusqlite::params![
            id,
            account_id,
            serde_json::to_string(&email.to)?,
            serde_json::to_string(&email.cc)?,
            serde_json::to_string(&email.bcc)?,
            email.subject,
            email.body_html,
            serde_json::to_string(&email.attachments)?,
            now,
            message_id,
            email.reply_to_email_id,
        ],
    )?;

    Ok((id, message_id, email))
}

/// The message as RFC 822 bytes, without sending it. Used to hand a message
/// that could not be sent back to the Drafts folder with its attachments intact.
pub async fn render_message(
    from_email: &str,
    from_name: &str,
    email: &OutboundEmail,
    message_id: Option<&str>,
) -> Result<Vec<u8>> {
    Ok(build_message(from_email, from_name, email, message_id).await?.formatted())
}

/// Reconcile outbox rows stuck in 'sending' status — typically because the app
/// crashed / was suspended between SMTP submission and the status update.
///
/// For each such row we search the IMAP Sent folder for the stored Message-ID:
/// - found  → the message did go out, mark 'sent'
/// - missed → flip back to 'pending' so the next flush_outbox retries it
///
/// A row without a persisted message_id (legacy, pre-migration) is also flipped
/// back to 'pending' — safe default since Message-ID is what makes upstream
/// servers dedupe in the event of a double-submit.
pub async fn reconcile_sending(
    db: Arc<Mutex<Connection>>,
    account_id: &str,
    settings: &MailServerSettings,
) -> Result<()> {
    let rows: Vec<(String, String)> = {
        let conn = db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, message_id FROM outbox_queue WHERE account_id = ?1 AND status = 'sending'"
        )?;
        let out: Vec<(String, String)> = stmt.query_map(rusqlite::params![account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
        out
    };

    if rows.is_empty() {
        return Ok(());
    }

    let set_status = |id: &str, status: &str| {
        if let Ok(conn) = db.lock() {
            let _ = conn.execute(
                "UPDATE outbox_queue SET status = ?2 WHERE id = ?1",
                rusqlite::params![id, status],
            );
        }
    };

    let mut session = match crate::imap_client::connect(settings).await {
        Ok(s) => s,
        Err(e) => {
            log::warn!("reconcile_sending: IMAP connect failed ({}), leaving rows as-is", e);
            return Ok(());
        }
    };

    let remote_folders = crate::imap_client::list_folders(&mut session).await.unwrap_or_default();
    let sent_folder = remote_folders
        .iter()
        .find(|(_, mapped)| mapped == "sent")
        .map(|(name, _)| name.clone());

    if let Some(folder) = sent_folder {
        if let Err(e) = session.select(&folder).await {
            log::warn!("reconcile_sending: SELECT '{}' failed: {}", folder, e);
            let _ = session.logout().await;
            return Ok(());
        }
        for (id, message_id) in &rows {
            if message_id.is_empty() {
                set_status(id, "pending");
                continue;
            }
            let query = format!("HEADER Message-ID \"{}\"", message_id);
            let found = match session.search(&query).await {
                Ok(uids) => !uids.is_empty(),
                Err(e) => {
                    log::warn!("reconcile_sending: SEARCH failed for {}: {}", message_id, e);
                    continue;
                }
            };
            let new_status = if found { "sent" } else { "pending" };
            set_status(id, new_status);
            log::info!("reconcile_sending: row {} message_id={} → {}", id, message_id, new_status);
        }
    } else {
        for (id, _) in &rows {
            set_status(id, "pending");
        }
    }
    let _ = session.logout().await;
    Ok(())
}

/// Test SMTP connection with the given settings. Returns Ok(()) if successful.
pub async fn test_connection(settings: &MailServerSettings) -> Result<()> {
    let transport = build_transport(settings).await?;
    transport.test_connection().await.context("SMTP connection test failed")?;
    Ok(())
}

#[cfg(test)]
mod send_tests {
    use super::*;

    fn local_settings() -> MailServerSettings {
        MailServerSettings {
            protocol: "imap".into(),
            incoming_server: "localhost".into(),
            incoming_port: 143,
            incoming_username: "cortexist@example.test".into(),
            incoming_password: "pw".into(),
            incoming_security: "none".into(),
            smtp_server: "127.0.0.1".into(),
            smtp_port: 2525,
            smtp_username: "cortexist@example.test".into(),
            smtp_password: "pw".into(),
            smtp_security: "none".into(),
            sync_interval_minutes: 5,
        }
    }

    /// Mailbox send-as over a shared login: SMTP AUTH user is the login
    /// (cortexist@) while From is the mailbox address (alias.person@).
    /// Requires a local debug SMTP server on 127.0.0.1:2525 — run explicitly:
    /// `cargo test --lib send_tests -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn send_as_alias_over_local_smtp() {
        let email = OutboundEmail {
            to: vec!["recipient@example.test".into()],
            cc: vec![],
            bcc: vec![],
            subject: "send-as test".into(),
            body_html: "<p>hello</p>".into(),
            attachments: vec![],
            reply_to_email_id: None,
        };
        let raw = send_email(
            &local_settings(),
            "alias.person@example.test",
            "Alias Person",
            &email,
            Some("<test-msgid-123@example.test>"),
        )
        .await
        .expect("send through local SMTP should succeed");
        assert!(!raw.is_empty());
    }
}
