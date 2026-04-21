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

    let raw = message.formatted();

    transport
        .send(message)
        .await
        .context("SMTP send failed")?;

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
/// upstream servers don't reject the message for an off-domain id.
pub fn queue_email(
    conn: &Connection,
    account_id: &str,
    email: &OutboundEmail,
    from_email: &str,
) -> Result<(String, String)> {
    let id = uuid::Uuid::new_v4().to_string();
    let domain = from_email.rsplit('@').next().filter(|d| !d.is_empty()).unwrap_or("localhost");
    let message_id = format!("{}@{}", uuid::Uuid::new_v4(), domain);
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO outbox_queue (id, account_id, to_addrs, cc_addrs, bcc_addrs, subject, body_html, attachments, created_at, status, message_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10)",
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
        ],
    )?;

    Ok((id, message_id))
}

/// Process all pending emails in the outbox queue for an account.
/// Returns the number of successfully sent emails.
pub async fn flush_outbox(
    conn: &Connection,
    account_id: &str,
    settings: &MailServerSettings,
    from_email: &str,
    from_name: &str,
) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, to_addrs, cc_addrs, bcc_addrs, subject, body_html, attachments, message_id
         FROM outbox_queue
         WHERE account_id = ?1 AND status = 'pending'
         ORDER BY created_at"
    )?;

    let entries: Vec<(String, String, String, String, String, String, String, String)> = stmt
        .query_map(rusqlite::params![account_id], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?,
                row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut sent_count = 0;

    for (id, to_json, cc_json, bcc_json, subject, body_html, att_json, message_id) in entries {
        let email = OutboundEmail {
            to: serde_json::from_str(&to_json).unwrap_or_default(),
            cc: serde_json::from_str(&cc_json).unwrap_or_default(),
            bcc: serde_json::from_str(&bcc_json).unwrap_or_default(),
            subject,
            body_html,
            attachments: serde_json::from_str(&att_json).unwrap_or_default(),
        };

        // Mark as sending
        conn.execute(
            "UPDATE outbox_queue SET status = 'sending' WHERE id = ?1",
            rusqlite::params![id],
        )?;

        let mid = if message_id.is_empty() { None } else { Some(message_id.as_str()) };
        match send_email(settings, from_email, from_name, &email, mid).await {
            Ok(_raw) => {
                conn.execute(
                    "UPDATE outbox_queue SET status = 'sent' WHERE id = ?1",
                    rusqlite::params![id],
                )?;
                sent_count += 1;
            }
            Err(e) => {
                log::error!("Failed to send queued email {}: {}", id, e);
                // Revert to 'pending' (not 'failed') so it's retried on next flush,
                // and reconcile_sending can also verify if it actually delivered.
                conn.execute(
                    "UPDATE outbox_queue SET status = 'pending', error = ?2, retry_count = retry_count + 1 WHERE id = ?1",
                    rusqlite::params![id, e.to_string()],
                )?;
            }
        }
    }

    // Clean up old sent entries (keep for 7 days for audit)
    conn.execute(
        "DELETE FROM outbox_queue WHERE status = 'sent' AND created_at < datetime('now', '-7 days')",
        [],
    )?;

    Ok(sent_count)
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
