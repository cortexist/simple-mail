use rusqlite::{Connection, Result, params};
use chrono::Local;
use crate::crypto;

/// Create all tables with the current schema (idempotent — safe to call on existing DBs).
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS accounts (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            email      TEXT NOT NULL,
            initials   TEXT NOT NULL,
            color      TEXT NOT NULL,
            avatar_url TEXT,
            alias      TEXT NOT NULL DEFAULT '',
            position   INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS folders (
            id          TEXT NOT NULL,
            account_id  TEXT NOT NULL,
            name        TEXT NOT NULL,
            icon        TEXT NOT NULL,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (id, account_id),
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS contacts (
            id          TEXT PRIMARY KEY,
            account_id  TEXT NOT NULL,
            name        TEXT NOT NULL,
            first_name  TEXT NOT NULL DEFAULT '',
            last_name   TEXT NOT NULL DEFAULT '',
            middle_name TEXT NOT NULL DEFAULT '',
            prefix      TEXT NOT NULL DEFAULT '',
            suffix      TEXT NOT NULL DEFAULT '',
            email       TEXT NOT NULL,
            initials    TEXT NOT NULL,
            color       TEXT NOT NULL,
            organization TEXT,
            phone       TEXT,
            mobile      TEXT,
            job_title   TEXT,
            department  TEXT,
            address     TEXT,
            birthday    TEXT,
            notes       TEXT,
            photo_url   TEXT,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS emails (
            id              TEXT PRIMARY KEY,
            account_id      TEXT NOT NULL,
            from_name       TEXT NOT NULL,
            from_email      TEXT NOT NULL,
            from_initials   TEXT NOT NULL,
            from_color      TEXT NOT NULL,
            subject         TEXT NOT NULL,
            preview         TEXT NOT NULL,
            body            TEXT NOT NULL,
            date            TEXT NOT NULL,
            is_read         INTEGER NOT NULL DEFAULT 0,
            is_starred      INTEGER NOT NULL DEFAULT 0,
            is_pinned       INTEGER NOT NULL DEFAULT 0,
            is_focused      INTEGER NOT NULL DEFAULT 1,
            is_focused_manual INTEGER NOT NULL DEFAULT 0,
            has_attachment  INTEGER NOT NULL DEFAULT 0,
            folder          TEXT NOT NULL,
            reply_to        TEXT NOT NULL DEFAULT '',
            message_id      TEXT NOT NULL DEFAULT '',
            auth_results    TEXT NOT NULL DEFAULT '',
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS email_recipients (
            email_id TEXT NOT NULL,
            name     TEXT NOT NULL,
            email    TEXT NOT NULL,
            initials TEXT NOT NULL,
            color    TEXT NOT NULL,
            type     TEXT NOT NULL DEFAULT 'to',
            FOREIGN KEY (email_id) REFERENCES emails(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS calendar_events (
            id                TEXT PRIMARY KEY,
            account_id        TEXT NOT NULL,
            title             TEXT NOT NULL,
            start             TEXT NOT NULL,
            end               TEXT NOT NULL,
            color             TEXT NOT NULL,
            location          TEXT,
            description       TEXT,
            is_all_day        INTEGER NOT NULL DEFAULT 0,
            calendar_id       TEXT NOT NULL,
            is_online_meeting INTEGER NOT NULL DEFAULT 0,
            meeting_url       TEXT,
            rrule             TEXT,
            exdates           TEXT,
            alert_minutes     INTEGER,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS event_attendees (
            event_id TEXT NOT NULL,
            name     TEXT NOT NULL,
            email    TEXT NOT NULL,
            initials TEXT NOT NULL,
            color    TEXT NOT NULL,
            role     TEXT NOT NULL DEFAULT 'required',
            FOREIGN KEY (event_id) REFERENCES calendar_events(id) ON DELETE CASCADE
        );

        -- Ignored email addresses: emails from these senders are skipped during
        -- sync, and contacts matching these addresses are not imported.
        CREATE TABLE IF NOT EXISTS ignored_addresses (
            email TEXT PRIMARY KEY
        );

        -- Contact lists (local-only distribution groups)
        CREATE TABLE IF NOT EXISTS contact_lists (
            id         TEXT PRIMARY KEY,
            account_id TEXT NOT NULL DEFAULT '',
            name       TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS contact_list_members (
            list_id TEXT NOT NULL,
            name    TEXT NOT NULL DEFAULT '',
            email   TEXT NOT NULL,
            FOREIGN KEY (list_id) REFERENCES contact_lists(id) ON DELETE CASCADE,
            PRIMARY KEY (list_id, email)
        );

        CREATE TABLE IF NOT EXISTS email_attachments (
            email_id  TEXT    NOT NULL,
            idx       INTEGER NOT NULL,
            filename  TEXT    NOT NULL,
            mime_type TEXT    NOT NULL,
            size      INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (email_id, idx),
            FOREIGN KEY (email_id) REFERENCES emails(id) ON DELETE CASCADE
        );

        -- Sender blocklist: block/allow entries per account.
        -- list_type is 'block' or 'allow'. Allow overrides block (e.g.
        -- innocent@spammer.com allowed even when *@spammer.com is blocked).
        CREATE TABLE IF NOT EXISTS sender_blocklist (
            email      TEXT NOT NULL,
            account_id TEXT NOT NULL,
            list_type  TEXT NOT NULL DEFAULT 'block',
            created_at TEXT NOT NULL,
            PRIMARY KEY (email, account_id),
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );
    ")?;

    // Incremental migrations — ignore errors on duplicate column (already applied)
    conn.execute("ALTER TABLE emails ADD COLUMN is_replied INTEGER NOT NULL DEFAULT 0", []).ok();
    conn.execute("ALTER TABLE emails ADD COLUMN replied_at TEXT", []).ok();
    conn.execute("ALTER TABLE contacts ADD COLUMN rev TEXT", []).ok();

    // Multi-value contact child tables (vCard 3.0 / CardDAV-aligned)
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS contact_emails (
            id         TEXT PRIMARY KEY,
            contact_id TEXT NOT NULL,
            address    TEXT NOT NULL,
            label      TEXT NOT NULL DEFAULT 'other',
            is_default INTEGER NOT NULL DEFAULT 0,
            position   INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS contact_phones (
            id         TEXT PRIMARY KEY,
            contact_id TEXT NOT NULL,
            number     TEXT NOT NULL,
            label      TEXT NOT NULL DEFAULT 'other',
            subtypes   TEXT NOT NULL DEFAULT '',
            is_default INTEGER NOT NULL DEFAULT 0,
            position   INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS contact_addresses (
            id          TEXT PRIMARY KEY,
            contact_id  TEXT NOT NULL,
            street      TEXT NOT NULL DEFAULT '',
            city        TEXT NOT NULL DEFAULT '',
            region      TEXT NOT NULL DEFAULT '',
            postal_code TEXT NOT NULL DEFAULT '',
            country     TEXT NOT NULL DEFAULT '',
            label       TEXT NOT NULL DEFAULT 'other',
            is_default  INTEGER NOT NULL DEFAULT 0,
            position    INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (contact_id) REFERENCES contacts(id) ON DELETE CASCADE
        );
    ").ok();

    // Indexes — hot paths are "emails for this account+folder" and FK joins on email_id
    conn.execute("CREATE INDEX IF NOT EXISTS idx_emails_account_folder ON emails(account_id, folder)", []).ok();
    conn.execute("CREATE INDEX IF NOT EXISTS idx_emails_date ON emails(date)", []).ok();
    conn.execute("CREATE INDEX IF NOT EXISTS idx_emails_account_message_id ON emails(account_id, message_id)", []).ok();
    conn.execute("CREATE INDEX IF NOT EXISTS idx_email_recipients_email_id ON email_recipients(email_id)", []).ok();
    conn.execute("CREATE INDEX IF NOT EXISTS idx_email_attachments_email_id ON email_attachments(email_id)", []).ok();
    conn.execute("CREATE INDEX IF NOT EXISTS idx_contact_emails_contact_id ON contact_emails(contact_id)", []).ok();
    conn.execute("CREATE INDEX IF NOT EXISTS idx_contact_phones_contact_id ON contact_phones(contact_id)", []).ok();
    conn.execute("CREATE INDEX IF NOT EXISTS idx_contact_addresses_contact_id ON contact_addresses(contact_id)", []).ok();

    Ok(())
}

/// Check whether an email address is on the ignore list.
pub fn is_address_ignored(conn: &Connection, email: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM ignored_addresses WHERE email = ?1",
        params![email.to_lowercase()],
        |_| Ok(()),
    ).is_ok()
}

/// Check whether a sender is blocked for a given account.
/// Returns true if any block entry matches AND no allow entry matches.
/// Supports exact matches and wildcard domain entries (e.g. *@domain.com).
pub fn is_sender_blocked(conn: &Connection, sender: &str, account_id: &str) -> bool {
    let sender_lower = sender.to_lowercase();
    let domain = sender_lower.rsplit('@').next().unwrap_or("");

    // Check for an exact allow entry first — if found, sender is not blocked
    let exact_allow: bool = conn.query_row(
        "SELECT 1 FROM sender_blocklist WHERE email = ?1 AND account_id = ?2 AND list_type = 'allow'",
        params![sender_lower, account_id],
        |_| Ok(()),
    ).is_ok();
    if exact_allow {
        return false;
    }

    // Check for an exact block entry
    let exact_block: bool = conn.query_row(
        "SELECT 1 FROM sender_blocklist WHERE email = ?1 AND account_id = ?2 AND list_type = 'block'",
        params![sender_lower, account_id],
        |_| Ok(()),
    ).is_ok();
    if exact_block {
        return true;
    }

    // Check for a wildcard domain block (*@domain.com)
    let wildcard = format!("*@{}", domain);
    let domain_block: bool = conn.query_row(
        "SELECT 1 FROM sender_blocklist WHERE email = ?1 AND account_id = ?2 AND list_type = 'block'",
        params![wildcard, account_id],
        |_| Ok(()),
    ).is_ok();

    domain_block
}

/// Run data migrations on existing databases.
/// Schema columns are already baked into init_db / init_sync_tables CREATE TABLE statements,
/// so only data transforms belong here.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('schema_version', '1')",
        [],
    )?;

    // Encrypt existing plaintext passwords (one-time, flagged)
    migrate_encrypt_passwords(conn);

    // Clear cached bodies with unresolved cid: references so they get re-fetched
    // with inline image resolution (one-time, flagged)
    migrate_cid_bodies(conn);

    // Add meeting_url column to calendar_events (idempotent)
    conn.execute_batch("ALTER TABLE calendar_events ADD COLUMN meeting_url TEXT").ok();

    // Add auth_results column to emails (idempotent)
    conn.execute_batch("ALTER TABLE emails ADD COLUMN auth_results TEXT NOT NULL DEFAULT ''").ok();

    // Add message_id column to outbox_queue (idempotent) — used to reconcile
    // "sending"-state rows after a crash by searching the Sent folder.
    conn.execute_batch("ALTER TABLE outbox_queue ADD COLUMN message_id TEXT NOT NULL DEFAULT ''").ok();

    // Add position column to accounts (idempotent) and seed it from rowid
    // so existing accounts keep their current insertion order.
    if conn
        .execute_batch("ALTER TABLE accounts ADD COLUMN position INTEGER NOT NULL DEFAULT 0")
        .is_ok()
    {
        conn.execute("UPDATE accounts SET position = rowid", []).ok();
    }

    // Copy scalar contact fields into new multi-value child tables (one-time, flagged)
    migrate_contacts_multivalue(conn);

    Ok(())
}

/// One-time: fan out legacy scalar contacts.email/phone/mobile/address into child tables.
fn migrate_contacts_multivalue(conn: &Connection) {
    let already: bool = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'contacts_multivalue_migrated'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|v| v == "1")
        .unwrap_or(false);

    if already {
        return;
    }

    let rows: Vec<(String, String, String, String, String)> = {
        let mut stmt = match conn.prepare(
            "SELECT id, COALESCE(email,''), COALESCE(phone,''), COALESCE(mobile,''), COALESCE(address,'') FROM contacts"
        ) {
            Ok(s) => s,
            Err(_) => return,
        };
        stmt.query_map([], |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        )))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .collect()
    };

    for (cid, email, phone, mobile, address) in rows {
        // Skip if already populated (re-run safety)
        let has_children: i64 = conn.query_row(
            "SELECT (SELECT COUNT(*) FROM contact_emails WHERE contact_id = ?1) \
                  + (SELECT COUNT(*) FROM contact_phones WHERE contact_id = ?1) \
                  + (SELECT COUNT(*) FROM contact_addresses WHERE contact_id = ?1)",
            params![cid],
            |r| r.get(0),
        ).unwrap_or(0);
        if has_children > 0 {
            continue;
        }

        if !email.trim().is_empty() {
            conn.execute(
                "INSERT INTO contact_emails (id, contact_id, address, label, is_default, position) VALUES (?1, ?2, ?3, 'work', 1, 0)",
                params![format!("{}-e0", cid), cid, email.trim()],
            ).ok();
        }

        let mut phone_pos = 0i64;
        if !phone.trim().is_empty() {
            conn.execute(
                "INSERT INTO contact_phones (id, contact_id, number, label, subtypes, is_default, position) VALUES (?1, ?2, ?3, 'work', 'voice', 1, ?4)",
                params![format!("{}-p{}", cid, phone_pos), cid, phone.trim(), phone_pos],
            ).ok();
            phone_pos += 1;
        }
        if !mobile.trim().is_empty() {
            let is_default = if phone_pos == 0 { 1 } else { 0 };
            conn.execute(
                "INSERT INTO contact_phones (id, contact_id, number, label, subtypes, is_default, position) VALUES (?1, ?2, ?3, 'cell', 'voice', ?4, ?5)",
                params![format!("{}-p{}", cid, phone_pos), cid, mobile.trim(), is_default, phone_pos],
            ).ok();
        }

        if !address.trim().is_empty() {
            conn.execute(
                "INSERT INTO contact_addresses (id, contact_id, street, city, region, postal_code, country, label, is_default, position) VALUES (?1, ?2, ?3, '', '', '', '', 'home', 1, 0)",
                params![format!("{}-a0", cid), cid, address.trim()],
            ).ok();
        }
    }

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('contacts_multivalue_migrated', '1')",
        [],
    ).ok();
}

/// One-time: clear cached email bodies containing unresolved cid: references.
fn migrate_cid_bodies(conn: &Connection) {
    let already: bool = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'cid_bodies_cleared'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|v| v == "1")
        .unwrap_or(false);

    if already {
        return;
    }

    conn.execute(
        "UPDATE emails SET body = '' WHERE body LIKE '%cid:%' AND body != ''",
        [],
    ).ok();

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('cid_bodies_cleared', '1')",
        [],
    ).ok();
}

/// One-time migration: encrypt any plaintext passwords still in the database.
/// Uses a settings flag to avoid re-running on every startup.
fn migrate_encrypt_passwords(conn: &Connection) {
    // Check if already migrated
    let already: bool = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'passwords_encrypted'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|v| v == "1")
        .unwrap_or(false);

    if already {
        return;
    }

    // Encrypt mail_server_settings passwords
    if let Ok(mut stmt) = conn.prepare(
        "SELECT account_id, incoming_password, smtp_password FROM mail_server_settings"
    ) {
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        for (id, inc_pw, smtp_pw) in rows {
            if let (Ok(enc_inc), Ok(enc_smtp)) = (
                crypto::encrypt_if_plaintext(&inc_pw),
                crypto::encrypt_if_plaintext(&smtp_pw),
            ) {
                conn.execute(
                    "UPDATE mail_server_settings SET incoming_password = ?1, smtp_password = ?2 WHERE account_id = ?3",
                    params![enc_inc, enc_smtp, id],
                ).ok();
            }
        }
    }

    // Encrypt caldav_settings passwords
    if let Ok(mut stmt) = conn.prepare("SELECT account_id, password FROM caldav_settings") {
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        for (id, pw) in rows {
            if let Ok(enc) = crypto::encrypt_if_plaintext(&pw) {
                conn.execute(
                    "UPDATE caldav_settings SET password = ?1 WHERE account_id = ?2",
                    params![enc, id],
                ).ok();
            }
        }
    }

    // Encrypt carddav_settings passwords
    if let Ok(mut stmt) = conn.prepare("SELECT account_id, password FROM carddav_settings") {
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        for (id, pw) in rows {
            if let Ok(enc) = crypto::encrypt_if_plaintext(&pw) {
                conn.execute(
                    "UPDATE carddav_settings SET password = ?1 WHERE account_id = ?2",
                    params![enc, id],
                ).ok();
            }
        }
    }

    // Encrypt dav_server_users passwords
    if let Ok(mut stmt) = conn.prepare("SELECT email, password FROM dav_server_users") {
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        for (email, pw) in rows {
            if let Ok(enc) = crypto::encrypt_if_plaintext(&pw) {
                conn.execute(
                    "UPDATE dav_server_users SET password = ?1 WHERE email = ?2",
                    params![enc, email],
                ).ok();
            }
        }
    }

    // Mark migration complete
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('passwords_encrypted', '1')",
        [],
    ).ok();
}

/// Returns true if there's data already seeded.
pub fn is_seeded(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
    Ok(count > 0)
}

/// Seed the database with all mock data.
/// All dates are relative to "today" so the demo always looks fresh.
pub fn seed(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    // ── Date helper ──
    let today = Local::now().date_naive();
    let dt = |day_offset: i64, hour: u32, min: u32| -> String {
        let d = today + chrono::Duration::days(day_offset);
        format!("{}T{:02}:{:02}:00", d, hour, min)
    };
    let dt_allday = |day_offset: i64| -> String {
        let d = today + chrono::Duration::days(day_offset);
        format!("{}T00:00:00", d)
    };
    let dt_allday_end = |day_offset: i64| -> String {
        let d = today + chrono::Duration::days(day_offset);
        format!("{}T23:59:59", d)
    };

    // ── Helper macros ──
    macro_rules! insert_account {
        ($id:expr, $name:expr, $email:expr, $initials:expr, $color:expr, $avatar:expr) => {
            conn.execute(
                "INSERT INTO accounts (id, name, email, initials, color, avatar_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![$id, $name, $email, $initials, $color, $avatar as Option<&str>],
            )?;
        };
    }

    macro_rules! insert_folder {
        ($id:expr, $acct:expr, $name:expr, $icon:expr, $fav:expr) => {
            conn.execute(
                "INSERT INTO folders (id, account_id, name, icon, is_favorite) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![$id, $acct, $name, $icon, $fav as i32],
            )?;
        };
    }

    macro_rules! insert_contact {
        ($id:expr, $acct:expr, $name:expr, $email:expr, $initials:expr, $color:expr,
         $phone:expr, $mobile:expr, $job:expr, $dept:expr, $company:expr,
         $addr:expr, $bday:expr, $notes:expr, $fav:expr, $photo:expr) => {
            conn.execute(
                "INSERT INTO contacts (id, account_id, name, email, initials, color, phone, mobile, job_title, department, organization, address, birthday, notes, is_favorite, photo_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![$id, $acct, $name, $email, $initials, $color,
                    $phone as Option<&str>, $mobile as Option<&str>, $job as Option<&str>,
                    $dept as Option<&str>, $company as Option<&str>, $addr as Option<&str>,
                    $bday as Option<&str>, $notes as Option<&str>, $fav as i32,
                    $photo as Option<&str>],
            )?;
        };
    }

    macro_rules! insert_email {
        ($id:expr, $acct:expr, $fn:expr, $fe:expr, $fi:expr, $fc:expr,
         $subj:expr, $prev:expr, $body:expr, $date:expr,
         $read:expr, $star:expr, $attach:expr, $folder:expr,
         $recipients:expr) => {
            conn.execute(
                "INSERT INTO emails (id, account_id, from_name, from_email, from_initials, from_color, subject, preview, body, date, is_read, is_starred, has_attachment, folder)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![$id, $acct, $fn, $fe, $fi, $fc, $subj, $prev, $body, &$date,
                    $read as i32, $star as i32, $attach as i32, $folder],
            )?;
            for (rn, re, ri, rc) in $recipients.iter() {
                conn.execute(
                    "INSERT INTO email_recipients (email_id, name, email, initials, color) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![$id, rn, re, ri, rc],
                )?;
            }
        };
    }

    macro_rules! insert_cal_cat {
        ($id:expr, $acct:expr, $name:expr, $color:expr, $vis:expr) => {
            conn.execute(
                "INSERT INTO caldav_collection_state (account_id, collection_url, ctag, display_name, color, visible) VALUES (?1, ?2, '', ?3, ?4, ?5)",
                params![$acct, $id, $name, $color, $vis as i32],
            )?;
        };
    }

    macro_rules! insert_event {
        ($id:expr, $acct:expr, $title:expr, $start:expr, $end:expr, $color:expr,
         $loc:expr, $desc:expr, $allday:expr, $calid:expr, $online:expr, $meeting_url:expr,
         $attendees:expr) => {
            conn.execute(
                "INSERT INTO calendar_events (id, account_id, title, start, end, color, location, description, is_all_day, calendar_id, is_online_meeting, meeting_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![$id, $acct, $title, &$start, &$end, $color,
                    $loc as Option<&str>, $desc as Option<&str>,
                    $allday as i32, $calid, $online as i32, $meeting_url as Option<&str>],
            )?;
            for (an, ae, ai, ac) in $attendees.iter() {
                conn.execute(
                    "INSERT INTO event_attendees (event_id, name, email, initials, color) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![$id, an, ae, ai, ac],
                )?;
            }
        };
    }

    // Default theme setting
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('theme', 'system')",
        [],
    )?;

    // ═══════════════════════════════════════════════════════
    //  ACCOUNT 1 — Work (Cortexist Inc.)
    // ═══════════════════════════════════════════════════════
    insert_account!("work", "Santoshi Nakamoto", "santoshi.nakamoto@cortexist.com", "AR", "#0078d4", Some("/avatars/santoshi-nakamoto.jpg"));

    // Folders
    insert_folder!("inbox",   "work", "Inbox",         "inbox",   true);
    insert_folder!("sent",    "work", "Sent Items",    "sent",    true);
    insert_folder!("drafts",  "work", "Drafts",        "drafts",  true);
    insert_folder!("deleted", "work", "Deleted Items", "deleted", false);
    insert_folder!("junk",    "work", "Junk Email",    "junk",    false);
    insert_folder!("archive", "work", "Archive",       "archive", false);

    // Contacts
    let no_str: Option<&str> = None;
    insert_contact!("c-1","work","Alex Johnson","alex.j@company.com","AJ","#00b7c3",
        Some("+1 (555) 234-5678"),no_str,Some("Engineering Lead"),Some("Engineering"),Some("Cortexist Inc."),no_str,no_str,no_str,true,Some("/avatars/alex-johnson.jpg"));
    insert_contact!("c-2","work","Roku Kaneshiro","roku.kaneshiro@company.com","DK","#da3b01",
        Some("+1 (555) 345-6789"),Some("+1 (555) 345-0000"),Some("VP of Finance"),Some("Finance"),Some("Cortexist Inc."),no_str,no_str,no_str,true,Some("/avatars/roku-kaneshiro.jpg"));
    insert_contact!("c-3","work","Emily Wang","emily.wang@company.com","EW","#8764b8",
        Some("+1 (555) 456-7890"),no_str,Some("Senior Designer"),Some("Design"),Some("Cortexist Inc."),no_str,no_str,no_str,true,no_str);
    insert_contact!("c-4","work","James Wilson","james.w@partner.com","JW","#986f0b",
        Some("+1 (555) 567-8901"),no_str,Some("Partner Relations"),no_str,Some("SyncSpace"),no_str,no_str,no_str,false,no_str);
    insert_contact!("c-5","work","Lisa Bergström","lisa.bergström@company.com","LP","#e3008c",
        Some("+1 (555) 678-9012"),Some("+1 (555) 678-0000"),Some("Product Manager"),Some("Product"),Some("Cortexist Inc."),no_str,Some("March 4"),no_str,true,Some("/avatars/lisa-bergstrom.jpg"));
    insert_contact!("c-6","work","Michael Torres","michael.t@company.com","MT","#498205",
        Some("+1 (555) 789-0123"),no_str,Some("Software Engineer"),Some("Engineering"),Some("Cortexist Inc."),no_str,no_str,no_str,false,no_str);
    insert_contact!("c-7","work","Rachel Adams","rachel.a@company.com","RA","#c239b3",
        Some("+1 (555) 890-1234"),no_str,Some("HR Business Partner"),Some("Human Resources"),Some("Cortexist Inc."),no_str,no_str,no_str,false,no_str);
    insert_contact!("c-8","work","Sara Chen","sara.chen@company.com","SC","#0078d4",
        Some("+1 (555) 123-4567"),Some("+1 (555) 123-0000"),Some("Product Director"),Some("Product"),Some("Cortexist Inc."),no_str,no_str,no_str,true,Some("/avatars/sara-chen.jpg"));
    insert_contact!("c-9","work","Tom Martinez","tom.m@company.com","TM","#107c10",
        Some("+1 (555) 901-2345"),no_str,Some("DevOps Engineer"),Some("Engineering"),Some("Cortexist Inc."),no_str,no_str,no_str,false,no_str);
    insert_contact!("c-10","work","Victoria Lee","victoria.l@company.com","VL","#7719aa",
        Some("+1 (555) 012-3456"),no_str,Some("QA Lead"),Some("Engineering"),Some("Cortexist Inc."),no_str,no_str,no_str,false,no_str);

    // Emails — dates relative to today
    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    let all_recip: Vec<(&str,&str,&str,&str)> = vec![("All Staff","all@company.com","AS","#c239b3")];

    insert_email!("1","work","Sara Chen","sara.chen@company.com","SC","#0078d4",
        "Product Roadmap Review",
        "Hi team, I wanted to share the updated roadmap for your review. Please take a look at the attached document and let me know if you have any questions…",
        "<p>Hi team,</p><p>I wanted to share the updated roadmap for your review. Please take a look at the attached document and let me know if you have any questions or concerns.</p><p><strong>Key highlights:</strong></p><ul><li>New mail client launch — next month</li><li>Calendar integration — following month</li><li>Mobile app beta — end of quarter</li></ul><p>Let's discuss in our Thursday meeting.</p><p>Best,<br/>Sara</p>",
        dt(0, 9, 30),false,true,true,"inbox",you_recip);

    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    insert_email!("2","work","Michael Torres","michael.t@company.com","MT","#498205",
        "Team Lunch Friday 🍕",
        "Hey everyone! I'm organizing a team lunch this Friday at noon. We're thinking Italian — does that work for everyone?",
        "<p>Hey everyone!</p><p>I'm organizing a team lunch this Friday at noon. We're thinking Italian — does that work for everyone?</p><p><strong>Options:</strong></p><ol><li>Olive Garden (casual)</li><li>Carrabba's (slightly nicer)</li><li>Local pizza place (quick and easy)</li></ol><p>Reply with your preference by Wednesday!</p><p>Cheers,<br/>Michael</p>",
        dt(0, 8, 15),false,false,false,"inbox",you_recip);

    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    insert_email!("3","work","Emily Wang","emily.wang@company.com","EW","#8764b8",
        "Design System v3.0 Update",
        "The new design system is ready for review. I've updated all components to use the new Fluent tokens and added dark mode support…",
        "<p>Hi all,</p><p>The new design system is ready for review. I've updated all components to use the new Fluent Design tokens and added comprehensive dark mode support.</p><p><strong>Changes include:</strong></p><ul><li>Updated color tokens to match Windows 11 Fluent Design</li><li>New spacing and typography scales</li><li>Dark mode for all 47 components</li><li>Improved accessibility (WCAG 2.1 AA compliance)</li></ul><p>Please review the Figma file linked in the project board and leave your feedback by EOW.</p><p>Thanks,<br/>Emily</p>",
        dt(-1, 16, 45),true,false,true,"inbox",you_recip);

    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    insert_email!("4","work","Roku Kaneshiro","roku.kaneshiro@company.com","DK","#da3b01",
        "Re: Budget Approval for Q2",
        "Good news — the budget has been approved! We can proceed with the hiring plan and infrastructure upgrades as discussed…",
        "<p>Good news!</p><p>The budget has been approved! We can proceed with the hiring plan and infrastructure upgrades as discussed in last week's meeting.</p><p><strong>Approved items:</strong></p><ul><li>3 new engineering hires — $450K</li><li>Cloud infrastructure upgrade — $120K</li><li>Design tooling licenses — $30K</li><li>Team offsite Q2 — $25K</li></ul><p>I'll set up individual meetings to discuss timelines for each item.</p><p>Best,<br/>Takeshi</p>",
        dt(-1, 14, 20),true,true,true,"inbox",you_recip);

    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    insert_email!("5","work","Alex Johnson","alex.j@company.com","AJ","#00b7c3",
        "Weekly Standup Notes",
        "Here are the notes from this week's standup. Action items are highlighted in bold. Please check your assignments…",
        "<p>Team,</p><p>Here are the notes from this week's standup:</p><p><strong>Completed:</strong></p><ul><li>Auth service migration to v2 — Alex</li><li>UI component library audit — Emily</li><li>Performance benchmarks — Sara</li></ul><p><strong>In Progress:</strong></p><ul><li>Mail client frontend — You</li><li>Calendar API integration — Michael</li><li>Database optimization — Takeshi</li></ul><p><strong>Blockers:</strong></p><ul><li>Need DevOps review for deployment pipeline changes</li></ul><p>Next standup: Monday at 10 AM.</p><p>— Alex</p>",
        dt(-2, 10, 0),false,false,false,"inbox",you_recip);

    insert_email!("6","work","HR Department","hr@company.com","HR","#c239b3",
        "Updated Remote Work Policy",
        "Please review the updated remote work policy that takes effect next month. Key changes include flexible core hours…",
        "<p>Dear Team,</p><p>Please review the updated remote work policy that takes effect next month.</p><p><strong>Key changes:</strong></p><ul><li><strong>Flexible core hours:</strong> 10 AM – 3 PM (previously 9 AM – 4 PM)</li><li><strong>Remote days:</strong> Up to 3 days per week (previously 2)</li><li><strong>Home office stipend:</strong> Increased to $1,000/year</li><li><strong>Quarterly in-person meetings:</strong> Required for all teams</li></ul><p>Please acknowledge receipt of this policy through the HR portal.</p><p>Questions? Reach out to your HR business partner.</p><p>Best regards,<br/>Human Resources</p>",
        dt(-3, 9, 0),true,false,true,"inbox",all_recip);

    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    insert_email!("7","work","Lisa Bergström","lisa.bergström@company.com","LP","#e3008c",
        "Project Phoenix Kickoff",
        "Excited to announce the kickoff for Project Phoenix! This will be our flagship product. Meeting details inside…",
        "<p>Hi everyone,</p><p>Excited to announce the kickoff for <strong>Project Phoenix</strong>! This will be our flagship product initiative.</p><p><strong>Kickoff Meeting:</strong></p><ul><li>Date: Next week</li><li>Time: 2:00 PM – 4:00 PM</li><li>Location: Conference Room A / Teams Link</li></ul><p><strong>Agenda:</strong></p><ol><li>Project vision and goals (30 min)</li><li>Technical architecture overview (30 min)</li><li>Team structure and roles (20 min)</li><li>Timeline and milestones (20 min)</li><li>Q&A (20 min)</li></ol><p>Please come prepared with questions. Pre-reading materials are attached.</p><p>— Lisa</p>",
        dt(-4, 11, 30),true,true,false,"inbox",you_recip);

    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    insert_email!("8","work","James Wilson","james.w@partner.com","JW","#986f0b",
        "Invitation: Product Demo",
        "You're invited to an exclusive demo of our new collaboration platform. We think it would be a great fit for your team…",
        "<p>Hello,</p><p>You're invited to an exclusive demo of our new collaboration platform, <strong>SyncSpace</strong>.</p><p>Based on your team's needs, we think SyncSpace would be a great fit for:</p><ul><li>Real-time document collaboration</li><li>Integrated project management</li><li>AI-powered meeting summaries</li><li>Cross-platform support (Windows, macOS, Linux, mobile)</li></ul><p><strong>Demo Details:</strong></p><ul><li>Date: Next week</li><li>Time: 11:00 AM EST</li><li>Duration: 45 minutes</li></ul><p>Looking forward to showing you what we've built!</p><p>Best,<br/>James Wilson<br/>Partner Relations, SyncSpace</p>",
        dt(-7, 15, 0),true,false,false,"inbox",you_recip);

    // Drafts
    let team_recip: Vec<(&str,&str,&str,&str)> = vec![("Team","team@company.com","TM","#498205")];
    insert_email!("9","work","You","you@company.com","YO","#0078d4",
        "Quarterly Report Draft",
        "Here's the draft of our quarterly report. Please review sections 2 and 3 which cover our product milestones…",
        "<p>Hi Team,</p><p>Here's the draft of our quarterly report. Please review sections 2 and 3 which cover our product milestones and financial projections.</p><p><em>[Draft in progress…]</em></p>",
        dt(-1, 17, 0),true,false,false,"drafts",team_recip);

    // Sent
    let sara_recip: Vec<(&str,&str,&str,&str)> = vec![("Sara Chen","sara.chen@company.com","SC","#0078d4")];
    insert_email!("10","work","You","you@company.com","YO","#0078d4",
        "Re: Product Roadmap Review",
        "Thanks Sara! I've reviewed the roadmap and everything looks great. Just a few suggestions on the timeline for the mobile…",
        "<p>Thanks Sara!</p><p>I've reviewed the roadmap and everything looks great. Just a few suggestions on the timeline for the mobile app beta — I think we might need an extra two weeks given the complexity of offline sync.</p><p>Let's discuss Thursday.</p><p>Best</p>",
        dt(0, 10, 0),true,false,false,"sent",sara_recip);

    let michael_recip: Vec<(&str,&str,&str,&str)> = vec![("Michael Torres","michael.t@company.com","MT","#498205")];
    insert_email!("11","work","You","you@company.com","YO","#0078d4",
        "Re: Team Lunch Friday",
        "Count me in! I'd vote for the local pizza place — quick and easy works best for me this week.",
        "<p>Count me in! I'd vote for the local pizza place — quick and easy works best for me this week.</p><p>See you Friday!</p>",
        dt(0, 8, 45),true,false,false,"sent",michael_recip);

    // Junk
    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    insert_email!("12","work","Prize Center","noreply@prizes-winner.biz","PC","#8a8a8a",
        "Congratulations! You have won!",
        "You have been selected as the winner of our exclusive giveaway. Click here to claim your $10,000 prize now before it expires…",
        "<p>🎉 CONGRATULATIONS! 🎉</p><p>You have been selected as the winner of our EXCLUSIVE giveaway!</p><p>Click below to claim your <strong>$10,000 prize</strong> now!</p><p><em>This is obviously spam.</em></p>",
        dt(-1, 6, 0),false,false,false,"junk",you_recip);

    let you_recip: Vec<(&str,&str,&str,&str)> = vec![("You","you@company.com","YO","#0078d4")];
    insert_email!("13","work","Deals Today","offers@deals-today.spam","DT","#8a8a8a",
        "URGENT: Limited time offer just for you!!!",
        "Don't miss this incredible deal! 90% OFF on everything. This offer expires in 24 hours. Act now or regret forever…",
        "<p>🔥 URGENT: LIMITED TIME OFFER 🔥</p><p>90% OFF EVERYTHING!!!</p><p>This offer expires in 24 hours.</p><p><em>Definitely spam.</em></p>",
        dt(-2, 4, 30),false,false,false,"junk",you_recip);

    // Calendar categories
    insert_cal_cat!("personal","work","Calendar","#0078d4",true);
    insert_cal_cat!("work","work","Work","#498205",true);
    insert_cal_cat!("birthdays","work","Birthdays","#e3008c",true);
    insert_cal_cat!("holidays","work","US Holidays","#da3b01",true);

    // Calendar events — dates relative to today
    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];

    insert_event!("evt-1","work","Weekly Standup",
        dt(-2, 10, 0), dt(-2, 10, 30),"#498205",
        Some("Conference Room B"),Some("Weekly team standup to discuss progress and blockers."),
        false,"work",true,Some("https://teams.microsoft.com/l/meetup-join/standup"),empty_att);

    let emily_att: Vec<(&str,&str,&str,&str)> = vec![("Emily Wang","emily.wang@company.com","EW","#8764b8")];
    insert_event!("evt-2","work","Design Review",
        dt(-2, 14, 0), dt(-2, 15, 30),"#0078d4",
        Some("Teams Meeting"),Some("Review the new design system components with Emily."),
        false,"personal",true,Some("https://teams.microsoft.com/l/meetup-join/design-review"),emily_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("evt-3","work","Lunch with Michael",
        dt(-1, 12, 0), dt(-1, 13, 0),"#498205",
        Some("Café Uno"),None::<&str>,false,"work",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("evt-4","work","1:1 with Roku Kaneshiro",
        dt(-1, 15, 0), dt(-1, 15, 30),"#498205",
        Some("Office 301"),Some("Discuss Q2 budget and hiring plan."),false,"work",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("evt-5","work","Dentist Appointment",
        dt(0, 9, 0), dt(0, 10, 0),"#0078d4",
        Some("Downtown Dental"),None::<&str>,false,"personal",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("evt-6","work","Sprint Planning",
        dt(0, 13, 0), dt(0, 14, 30),"#498205",
        Some("Conference Room A"),Some("Plan next sprint — prioritize backlog items."),false,"work",true,Some("https://teams.microsoft.com/l/meetup-join/sprint"),empty_att);

    let sara_att: Vec<(&str,&str,&str,&str)> = vec![("Sara Chen","sara.chen@company.com","SC","#0078d4")];
    insert_event!("evt-7","work","Product Roadmap Review",
        dt(1, 9, 30), dt(1, 11, 0),"#498205",
        Some("Conference Room A / Teams"),Some("Review roadmap with Sara Chen."),false,"work",true,Some("https://teams.microsoft.com/l/meetup-join/roadmap"),sara_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("evt-8","work","Gym",
        dt(1, 17, 30), dt(1, 18, 30),"#0078d4",
        None::<&str>,None::<&str>,false,"personal",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("evt-9","work","Team Lunch 🍕",
        dt(2, 12, 0), dt(2, 13, 0),"#498205",
        Some("Local Pizza Place"),Some("Friday team lunch organized by Michael."),false,"work",false,no_str,empty_att);

    let phoenix_att: Vec<(&str,&str,&str,&str)> = vec![
        ("Lisa Bergström","lisa.bergström@company.com","LP","#e3008c"),
        ("Sara Chen","sara.chen@company.com","SC","#0078d4"),
        ("Roku Kaneshiro","roku.kaneshiro@company.com","DK","#da3b01"),
    ];
    insert_event!("evt-10","work","Project Phoenix Kickoff",
        dt(5, 14, 0), dt(5, 16, 0),"#498205",
        Some("Conference Room A / Teams Link"),Some("Flagship product initiative kickoff meeting."),false,"work",true,Some("https://teams.microsoft.com/l/meetup-join/phoenix"),phoenix_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("evt-11","work","Lisa Bergström's Birthday",
        dt_allday(0), dt_allday_end(0),"#e3008c",
        None::<&str>,None::<&str>,true,"birthdays",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("evt-12","work","Product Demo — SyncSpace",
        dt(7, 11, 0), dt(7, 11, 45),"#0078d4",
        Some("Online"),Some("Demo of SyncSpace collaboration platform by James Wilson."),false,"personal",true,Some("https://meet.syncspace.io/demo-session"),empty_att);

    // ═══════════════════════════════════════════════════════
    //  ACCOUNT 2 — Personal (Gmail)
    // ═══════════════════════════════════════════════════════
    insert_account!("personal", "Santoshi Nakamoto", "santoshi.nakamoto@gmail.com", "AR", "#498205", Some("/avatars/gmail.jpg"));

    insert_folder!("inbox",   "personal", "Inbox",         "inbox",   true);
    insert_folder!("sent",    "personal", "Sent Items",    "sent",    true);
    insert_folder!("drafts",  "personal", "Drafts",        "drafts",  true);
    insert_folder!("deleted", "personal", "Deleted Items", "deleted", false);
    insert_folder!("junk",    "personal", "Junk Email",    "junk",    false);
    insert_folder!("archive", "personal", "Archive",       "archive", false);

    // Personal emails
    let you_p: Vec<(&str,&str,&str,&str)> = vec![("You","santoshi.nakamoto@gmail.com","AR","#498205")];
    insert_email!("p1","personal","Mom","mom@gmail.com","MR","#e3008c",
        "Dinner this Sunday? 🍝",
        "Hi sweetheart! Your dad and I were thinking of having a family dinner this Sunday. Can you make it?",
        "<p>Hi sweetheart!</p><p>Your dad and I were thinking of having a family dinner this Sunday around 6 PM. I'm making your favorite lasagna! 🍝</p><p>Can you make it? Let me know if you're bringing anyone.</p><p>Love,<br/>Mom</p>",
        dt(0, 12, 0),false,true,false,"inbox",you_p);

    let you_p: Vec<(&str,&str,&str,&str)> = vec![("You","santoshi.nakamoto@gmail.com","AR","#498205")];
    insert_email!("p2","personal","Netflix","info@netflix.com","NF","#e50914",
        "New arrivals this week on Netflix",
        "Check out what's new this week — including the highly anticipated Season 3 of your favorite show!",
        "<p>Hi Alex,</p><p>Here's what's new this week on Netflix:</p><ul><li><strong>Cyber Drift: Season 3</strong> — The crew returns for their biggest heist yet</li><li><strong>Mountain Echo</strong> — Award-winning documentary</li><li><strong>Last Laugh</strong> — New stand-up comedy special</li></ul><p>Start watching today!</p>",
        dt(0, 6, 0),true,false,false,"inbox",you_p);

    let you_p: Vec<(&str,&str,&str,&str)> = vec![("You","santoshi.nakamoto@gmail.com","AR","#498205")];
    insert_email!("p3","personal","Jake Rivera","jake.r@gmail.com","JR","#00b7c3",
        "Road trip next month?",
        "Hey! I was thinking we could do a road trip to the mountains next month. What do you think?",
        "<p>Hey!</p><p>I was thinking we could do a road trip to the mountains next month. Maybe a long weekend?</p><p><strong>Ideas:</strong></p><ul><li>Blue Ridge Parkway</li><li>Shenandoah National Park</li><li>Great Smoky Mountains</li></ul><p>Let me know if you're in! 🏔️</p><p>— Jake</p>",
        dt(-1, 20, 30),false,false,false,"inbox",you_p);

    let you_p: Vec<(&str,&str,&str,&str)> = vec![("You","santoshi.nakamoto@gmail.com","AR","#498205")];
    insert_email!("p4","personal","Amazon","ship-confirm@amazon.com","AZ","#ff9900",
        "Your order has shipped! 📦",
        "Great news — your order #112-7429816 has shipped and is on its way. Expected delivery: in 2 days.",
        "<p>Hello Alex,</p><p>Great news! Your order has shipped.</p><p><strong>Order #112-7429816</strong></p><ul><li>Sony WH-1000XM5 Headphones</li><li>Expected delivery: <strong>in 2 days</strong></li></ul><p>Track your package in the Amazon app.</p>",
        dt(-1, 14, 0),true,false,false,"inbox",you_p);

    let you_p: Vec<(&str,&str,&str,&str)> = vec![("You","santoshi.nakamoto@gmail.com","AR","#498205")];
    insert_email!("p5","personal","Mia Chen","mia.c@gmail.com","MC","#8764b8",
        "Photos from last weekend 📸",
        "Here are the photos from the hiking trip! Some really great shots. Check out the album link…",
        "<p>Hey Alex!</p><p>Here are the photos from Saturday's hike. Some really great shots came out!</p><p>I uploaded them to Google Photos — here's the album link.</p><p>My favorites are the sunset ones from the summit. 🌅</p><p>— Mia</p>",
        dt(-2, 18, 0),true,true,true,"inbox",you_p);

    let jake_recip: Vec<(&str,&str,&str,&str)> = vec![("Jake Rivera","jake.r@gmail.com","JR","#00b7c3")];
    insert_email!("p6","personal","You","santoshi.nakamoto@gmail.com","AR","#498205",
        "Re: Road trip next month?",
        "I'm totally in! Shenandoah gets my vote. Let's start planning!",
        "<p>I'm totally in! Shenandoah gets my vote — the trails there are amazing in spring.</p><p>Let's start planning this weekend!</p>",
        dt(-1, 21, 0),true,false,false,"sent",jake_recip);

    // Personal calendar
    insert_cal_cat!("personal","personal","Personal","#498205",true);
    insert_cal_cat!("family","personal","Family","#e3008c",true);
    insert_cal_cat!("social","personal","Social","#00b7c3",true);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("pevt-1","personal","Family Dinner",
        dt(3, 18, 0), dt(3, 20, 0),"#e3008c",
        Some("Mom & Dad's house"),Some("Sunday family dinner — lasagna!"),false,"family",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("pevt-2","personal","Gym — Leg Day",
        dt(-1, 7, 0), dt(-1, 8, 0),"#498205",
        Some("FitZone Gym"),None::<&str>,false,"personal",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("pevt-3","personal","Movie Night with Mia",
        dt(2, 19, 0), dt(2, 22, 0),"#00b7c3",
        Some("AMC Theater"),None::<&str>,false,"social",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("pevt-4","personal","Car Service Appointment",
        dt(1, 8, 0), dt(1, 9, 30),"#498205",
        Some("AutoCare Express"),None::<&str>,false,"personal",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("pevt-5","personal","Jake's Birthday",
        dt_allday(7), dt_allday_end(7),"#e3008c",
        None::<&str>,None::<&str>,true,"family",false,no_str,empty_att);

    // Personal contacts
    insert_contact!("pc-1","personal","Mom (Maria Nakamoto)","mom@gmail.com","MR","#e3008c",
        Some("+1 (555) 100-1000"),Some("+1 (555) 100-1001"),no_str,no_str,no_str,no_str,no_str,Some("❤️"),true,Some("/avatars/mom.jpg"));
    insert_contact!("pc-2","personal","Jung-Jae Lee","jung-jae.lee@gmail.com","CR","#da3b01",
        Some("+1 (555) 100-2000"),no_str,no_str,no_str,no_str,no_str,no_str,no_str,true,no_str);
    insert_contact!("pc-3","personal","Jake Rivera","jake.r@gmail.com","JR","#00b7c3",
        Some("+1 (555) 200-3000"),Some("+1 (555) 200-3001"),no_str,no_str,no_str,no_str,Some("March 12"),Some("Brother"),true,no_str);
    insert_contact!("pc-4","personal","Mia Chen","mia.c@gmail.com","MC","#8764b8",
        Some("+1 (555) 300-4000"),no_str,no_str,no_str,no_str,no_str,no_str,no_str,true,no_str);
    insert_contact!("pc-5","personal","Dr. Patel","office@downtowndental.com","DP","#986f0b",
        Some("+1 (555) 400-5000"),no_str,Some("Dentist"),no_str,Some("Downtown Dental"),no_str,no_str,no_str,false,no_str);

    // ═══════════════════════════════════════════════════════
    //  ACCOUNT 3 — Side Project (Indie Dev)
    // ═══════════════════════════════════════════════════════
    insert_account!("side", "PixelForge", "alex@pixelforge.dev", "PF", "#7719aa", Some("/avatars/github.jpg"));

    insert_folder!("inbox",   "side", "Inbox",         "inbox",   true);
    insert_folder!("sent",    "side", "Sent Items",    "sent",    true);
    insert_folder!("drafts",  "side", "Drafts",        "drafts",  true);
    insert_folder!("deleted", "side", "Deleted Items", "deleted", false);
    insert_folder!("junk",    "side", "Junk Email",    "junk",    false);
    insert_folder!("archive", "side", "Archive",       "archive", false);

    // Side emails
    let you_s: Vec<(&str,&str,&str,&str)> = vec![("You","alex@pixelforge.dev","PF","#7719aa")];
    insert_email!("s1","side","GitHub","noreply@github.com","GH","#24292e",
        "[pixelforge/aurora] Issue #47: Dark mode flickers on startup",
        "New issue opened by @codemaster99: \"When launching the app in dark mode, there is a brief white flash before the theme applies…\"",
        "<p><strong>@codemaster99</strong> opened issue <strong>#47</strong> in <strong>pixelforge/aurora</strong>:</p><p>When launching the app in dark mode, there's a brief white flash before the theme applies. This happens on both Windows and macOS.</p><p><strong>Steps to reproduce:</strong></p><ol><li>Enable system dark mode</li><li>Launch Aurora</li><li>Observe brief white flash</li></ol><p><strong>Expected:</strong> App should launch directly in dark mode without flash.</p>",
        dt(0, 14, 0),false,true,false,"inbox",you_s);

    let you_s: Vec<(&str,&str,&str,&str)> = vec![("You","alex@pixelforge.dev","PF","#7719aa")];
    insert_email!("s2","side","Stripe","notifications@stripe.com","ST","#635bff",
        "Your monthly payout has been sent",
        "A payout of $1,284.50 has been initiated to your bank account ending in 4821. Expected arrival: in 2 days.",
        "<p>Hello PixelForge,</p><p>Your monthly payout has been processed:</p><ul><li><strong>Amount:</strong> $1,284.50</li><li><strong>Bank account:</strong> ****4821</li><li><strong>Expected arrival:</strong> in 2 days</li></ul><p>View your full payout details on the Stripe Dashboard.</p>",
        dt(-1, 9, 0),true,false,false,"inbox",you_s);

    let you_s: Vec<(&str,&str,&str,&str)> = vec![("You","alex@pixelforge.dev","PF","#7719aa")];
    insert_email!("s3","side","Nina Patel","nina@designcraft.io","NP","#e3008c",
        "Redesigned icons for Aurora v2 🎨",
        "Hey Alex! I finished the new icon set for Aurora v2. Attached is the full SVG pack — let me know what you think!",
        "<p>Hey Alex!</p><p>I finished the new icon set for Aurora v2. The pack includes:</p><ul><li>48 app icons (light + dark variants)</li><li>24 toolbar icons</li><li>12 status icons</li></ul><p>Attached as SVG. I also included a Figma link in the project channel.</p><p>Let me know what you think! 🎨</p><p>— Nina</p>",
        dt(-2, 15, 30),false,true,true,"inbox",you_s);

    let you_s: Vec<(&str,&str,&str,&str)> = vec![("You","alex@pixelforge.dev","PF","#7719aa")];
    insert_email!("s4","side","Product Hunt","noreply@producthunt.com","PH","#da552f",
        "🎉 Aurora is trending on Product Hunt!",
        "Congrats! Aurora has been featured and is currently #3 on Product Hunt with 342 upvotes!",
        "<p>🎉 Congratulations!</p><p><strong>Aurora</strong> by PixelForge is trending on Product Hunt!</p><ul><li><strong>Current rank:</strong> #3</li><li><strong>Upvotes:</strong> 342</li><li><strong>Comments:</strong> 28</li></ul><p>Keep engaging with the community — your launch is going great!</p>",
        dt(-3, 10, 0),true,true,false,"inbox",you_s);

    let nina_recip: Vec<(&str,&str,&str,&str)> = vec![("Nina Patel","nina@designcraft.io","NP","#e3008c")];
    insert_email!("s5","side","You","alex@pixelforge.dev","PF","#7719aa",
        "Re: Redesigned icons for Aurora v2 🎨",
        "These look incredible! I love the new toolbar set. A few tweaks on the status icons…",
        "<p>Nina, these look incredible!</p><p>I especially love the new toolbar set — the weight feels just right. A few notes on the status icons:</p><ul><li>The \"offline\" icon could use a bit more contrast</li><li>Love the \"syncing\" animation concept</li></ul><p>I'll push these into the dev branch this weekend. Thanks!</p>",
        dt(-2, 17, 0),true,false,false,"sent",nina_recip);

    // Side calendar
    insert_cal_cat!("dev","side","Development","#7719aa",true);
    insert_cal_cat!("marketing","side","Marketing","#da552f",true);
    insert_cal_cat!("meetings","side","Meetings","#0078d4",true);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("sevt-1","side","Aurora v2 Sprint",
        dt(-2, 9, 0), dt(-2, 12, 0),"#7719aa",
        None::<&str>,Some("Focus block: dark mode fix + new icon integration."),false,"dev",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("sevt-2","side","Call with Nina — Icon Review",
        dt(0, 14, 0), dt(0, 14, 30),"#0078d4",
        Some("Google Meet"),None::<&str>,false,"meetings",true,Some("https://meet.google.com/abc-defg-hij"),empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("sevt-3","side","Product Hunt Launch Day",
        dt_allday(-3), dt_allday_end(-3),"#da552f",
        None::<&str>,Some("Aurora launches on Product Hunt!"),true,"marketing",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("sevt-4","side","Write blog post: Aurora v2 roadmap",
        dt(2, 10, 0), dt(2, 12, 0),"#da552f",
        None::<&str>,None::<&str>,false,"marketing",false,no_str,empty_att);

    let empty_att: Vec<(&str,&str,&str,&str)> = vec![];
    insert_event!("sevt-5","side","Community AMA on Discord",
        dt(3, 16, 0), dt(3, 17, 0),"#0078d4",
        Some("Discord #general"),Some("Answer community questions about Aurora v2."),false,"meetings",false,no_str,empty_att);

    // Side contacts
    insert_contact!("sc-1","side","Nina Patel","nina@designcraft.io","NP","#e3008c",
        Some("+1 (555) 500-6000"),no_str,Some("Freelance Designer"),no_str,Some("DesignCraft"),no_str,no_str,no_str,true,Some("/avatars/nina-patel.jpg"));
    insert_contact!("sc-2","side","Ryan Kim","ryan@betatesters.co","RK","#00b7c3",
        no_str,no_str,Some("Beta Testing Lead"),no_str,Some("BetaTesters Co."),no_str,no_str,no_str,false,no_str);
    insert_contact!("sc-3","side","Stripe Support","support@stripe.com","SS","#635bff",
        no_str,no_str,no_str,no_str,Some("Stripe"),no_str,no_str,no_str,false,no_str);

    Ok(())
}
