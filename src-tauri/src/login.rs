//! Logins — the credential group that owns synced data.
//!
//! A **login** is one set of server credentials. An **account** is a *view*
//! onto a login: an identity (address, name, colour) plus an optional mailbox
//! filter. When several addresses are delivered to the same server account
//! (e.g. `engineering@` and `takashi.nakamoto@` both landing on one IMAP
//! login), the user adds one account per address and they share a login.
//!
//! Two consequences drive the whole design:
//!
//! 1. **Sync runs once per login**, not once per account.
//! 2. **Data is stored once per login** and separated at *read* time by
//!    [`mailbox_address`]. Toggling `is_mailbox` therefore stores nothing and
//!    needs no re-sync — it only changes what a query returns.
//!
//! Synced tables spell their login column `account_id` (see the `db` module
//! header). [`LoginId`] exists so that value can only come from [`resolve`],
//! and never from an `accounts.id` that happens to be in scope.

use rusqlite::{Connection, Result, params};

use crate::sync_state::MailServerSettings;

/// The id of a login. Distinct from an account id: an account is a *view*, a
/// login *owns the data*. Synced tables key on this, in a column named
/// `account_id`.
///
/// Only [`resolve`] mints one, so passing an account id where a login id
/// belongs is a type error rather than a silent wrong-rows bug.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoginId(String);

impl LoginId {
    /// For binding into SQL. Every use is a spot where a login id enters a
    /// column named `account_id` — that mismatch is intentional.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Only for seeding a brand-new login in `add_account`, where the id is
    /// generated rather than looked up.
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        LoginId(id.into())
    }
}

impl std::fmt::Display for LoginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Bind straight into SQL, so `params![login]` works wherever a login id is
/// the key. Note this does not stop a plain `String` binding to the same slot —
/// the newtype earns its keep at *function signatures*, where it forces a
/// caller to resolve rather than pass whatever id is in scope.
impl rusqlite::ToSql for LoginId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

/// The login an account is a view onto — the only way to obtain a [`LoginId`].
pub fn resolve(conn: &Connection, account_id: &str) -> Result<LoginId> {
    conn.query_row(
        "SELECT login_id FROM accounts WHERE id = ?1",
        params![account_id],
        |row| row.get::<_, String>(0),
    )
    .map(LoginId)
}

/// Identity of a set of credentials: two accounts whose incoming server
/// matches here reach the same server-side account, so they share a login.
///
/// Deliberately **incoming-only**. A login owns SMTP/CalDAV/CardDAV as a set,
/// and attaching adopts the whole set — so those fields must not split logins
/// that are otherwise identical.
pub fn fingerprint(s: &MailServerSettings) -> String {
    format!(
        "{}|{}|{}|{}",
        s.protocol.to_lowercase(),
        s.incoming_server.to_lowercase(),
        s.incoming_port,
        s.incoming_username.to_lowercase(),
    )
}

/// Ids of every account that is a view onto `login`, in display order.
pub fn accounts_on(conn: &Connection, login: &LoginId) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM accounts WHERE login_id = ?1 ORDER BY position, rowid",
    )?;
    let rows = stmt.query_map(params![login.as_str()], |row| row.get::<_, String>(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// The address to filter this account's view to, or `None` when it is not a
/// mailbox and therefore sees everything on its login.
pub fn mailbox_address(conn: &Connection, account_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT is_mailbox, email FROM accounts WHERE id = ?1",
        params![account_id],
        |row| Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?)),
    )
    .map(|(is_mailbox, email)| (is_mailbox == 1).then_some(email))
}

/// What one account can see: which login's data to read, and how much of it.
///
/// Every read path wants this triple, so resolve it once at the command
/// boundary and pass it down rather than re-deriving it per query.
#[derive(Debug, Clone)]
pub struct View {
    /// The account being rendered. Use for *account*-keyed tables only
    /// (`account_collections`, `account_ai_features`, `outbox_queue`).
    pub account_id: String,
    /// The login owning the data. Use for every synced table — this is the
    /// value their `account_id` column holds.
    pub login: LoginId,
    /// `Some(address)` ⇒ show only mail for that address; `None` ⇒ show
    /// everything on the login.
    pub mailbox: Option<String>,
}

impl View {
    /// Bind order for a query that uses [`mailbox_clause`] at index 2:
    /// `?1` = login, `?2` = mailbox address (absent when not a mailbox).
    pub fn params(&self) -> Vec<&dyn rusqlite::ToSql> {
        match &self.mailbox {
            Some(addr) => vec![&self.login.0, addr],
            None => vec![&self.login.0],
        }
    }

    /// Bind order for a query that uses [`collection_clause`] at index 2:
    /// `?1` = login, `?2` = account id (absent when not a mailbox).
    ///
    /// Note `?2` is the **account** id here, not the address — collection
    /// assignment is per view, not per address.
    pub fn collection_params(&self) -> Vec<&dyn rusqlite::ToSql> {
        match &self.mailbox {
            Some(_) => vec![&self.login.0, &self.account_id],
            None => vec![&self.login.0],
        }
    }
}

/// Resolve an account to the view it presents.
pub fn view_of(conn: &Connection, account_id: &str) -> Result<View> {
    Ok(View {
        account_id: account_id.to_string(),
        login: resolve(conn, account_id)?,
        mailbox: mailbox_address(conn, account_id)?,
    })
}

/// Every login whose stored credentials reach the same server account as
/// `fp`, excluding `exclude`.
///
/// Scans rather than matching in SQL so [`fingerprint`] stays the single
/// definition of "same login". There are only ever a handful of logins.
fn logins_matching(conn: &Connection, fp: &str, exclude: &LoginId) -> Result<Option<LoginId>> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM logins WHERE id != ?1")?;
        let rows = stmt.query_map(params![exclude.as_str()], |row| row.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    for id in ids {
        let candidate = LoginId(id);
        if let Ok(Some(s)) = crate::sync_state::load_mail_settings(conn, &candidate) {
            if fingerprint(&s) == fp {
                return Ok(Some(candidate));
            }
        }
    }
    Ok(None)
}

/// Point `account_id` at whichever login already holds these credentials,
/// merging it into that login's data instead of syncing a second copy.
///
/// This is the "user added `engineering@` with `takashi@`'s credentials" path.
/// Returns the login the account ended up on, and whether it moved.
///
/// When the account's old login is left with no accounts, it and its data are
/// purged: that data belonged to the old login, and the account now reads the
/// target's copy instead.
pub fn attach_to_matching(
    conn: &Connection,
    account_id: &str,
    settings: &MailServerSettings,
) -> Result<(LoginId, bool)> {
    let current = resolve(conn, account_id)?;
    let fp = fingerprint(settings);

    let target = match logins_matching(conn, &fp, &current)? {
        Some(t) => t,
        None => return Ok((current, false)),
    };

    conn.execute(
        "UPDATE accounts SET login_id = ?1 WHERE id = ?2",
        params![target.as_str(), account_id],
    )?;

    if accounts_on(conn, &current)?.is_empty() {
        purge(conn, &current)?;
    }

    Ok((target, true))
}

/// Tables whose `account_id` column holds a real **account** id rather than a
/// login id, and so must not be re-keyed by [`repoint`].
///
/// This deny-list is the only thing to maintain: [`repoint`] discovers every
/// other `account_id` column from the schema, so a login-keyed table added
/// later is handled automatically instead of being silently forgotten.
const ACCOUNT_SCOPED: &[&str] = &[
    "accounts",
    "account_collections",
    "account_ai_features",
    "dav_server_users",
    "outbox_queue",
];

/// Hand a login's data from anchor `from` to sibling `to`.
///
/// Needed because a login's id is one of its accounts' ids — the trick that
/// let this feature ship without moving data. It has a cost: `logins.id`
/// must always name a **live** account (call it the *anchor*), because every
/// synced table still declares `FOREIGN KEY (account_id) REFERENCES
/// accounts(id)` and foreign keys are ON for the whole app (`lib.rs`
/// entry point). Delete the anchor's row while siblings still read its data
/// and two things break: the cascade takes the data with it, and every later
/// `INSERT INTO emails` fails the FK check — so the sibling's sync dies
/// permanently.
///
/// So before an anchor is removed, its data moves to a sibling that is
/// staying. Bounded (one login), indexed, and rare.
pub fn repoint(tx: &rusqlite::Transaction, from: &LoginId, to: &LoginId) -> Result<()> {
    let tables: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT m.name FROM sqlite_master m
             JOIN pragma_table_info(m.name) p
             WHERE m.type = 'table' AND p.name = 'account_id'",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok())
            .filter(|t| !ACCOUNT_SCOPED.contains(&t.as_str()))
            .collect()
    };

    // `to` is already a live account, so each re-keyed row lands on an
    // existing parent and the FK check passes as we go.
    for t in &tables {
        tx.execute(
            &format!("UPDATE {} SET account_id = ?1 WHERE account_id = ?2", t),
            params![to.as_str(), from.as_str()],
        )?;
    }
    tx.execute("UPDATE logins SET id = ?1 WHERE id = ?2", params![to.as_str(), from.as_str()])?;
    tx.execute(
        "UPDATE accounts SET login_id = ?1 WHERE login_id = ?2",
        params![to.as_str(), from.as_str()],
    )?;
    Ok(())
}

/// The account a login is anchored on — i.e. whether `login.id` still names a
/// live account, and which sibling could take over if it goes.
pub fn heir_for(conn: &Connection, login: &LoginId, leaving: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM accounts WHERE login_id = ?1 AND id != ?2 ORDER BY position, rowid LIMIT 1",
        params![login.as_str(), leaving],
        |r| r.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    })
}

/// Delete a login and every row it owns.
///
/// Only safe once no account is a view onto it — check [`accounts_on`] first.
///
/// Deletes children explicitly, before their parents, rather than leaning on
/// `ON DELETE CASCADE`. Two reasons: SQLite leaves foreign keys **off** by
/// default, so the declared cascades may simply not fire; and the cascades on
/// login-keyed tables point at `accounts(id)`, which is the wrong parent now
/// that those columns hold a login id. Being explicit makes the result
/// independent of ambient `PRAGMA` state.
pub fn purge(conn: &Connection, login: &LoginId) -> Result<()> {
    debug_assert!(
        accounts_on(conn, login).map(|a| a.is_empty()).unwrap_or(false),
        "purging a login that still has accounts would delete their data",
    );

    for sql in [
        // Children first, keyed through their parent's id.
        "DELETE FROM email_recipients  WHERE email_id IN (SELECT id FROM emails WHERE account_id = ?1)",
        "DELETE FROM email_attachments WHERE email_id IN (SELECT id FROM emails WHERE account_id = ?1)",
        "DELETE FROM email_ai_classification WHERE email_id IN (SELECT id FROM emails WHERE account_id = ?1)",
        "DELETE FROM event_attendees   WHERE event_id IN (SELECT id FROM calendar_events WHERE account_id = ?1)",
        "DELETE FROM contact_emails    WHERE contact_id IN (SELECT id FROM contacts WHERE account_id = ?1)",
        "DELETE FROM contact_phones    WHERE contact_id IN (SELECT id FROM contacts WHERE account_id = ?1)",
        "DELETE FROM contact_addresses WHERE contact_id IN (SELECT id FROM contacts WHERE account_id = ?1)",
        // Then the login-keyed rows themselves.
        "DELETE FROM email_labels    WHERE account_id = ?1",
        "DELETE FROM emails          WHERE account_id = ?1",
        "DELETE FROM calendar_events WHERE account_id = ?1",
        "DELETE FROM contacts        WHERE account_id = ?1",
        "DELETE FROM folders         WHERE account_id = ?1",
        "DELETE FROM imap_folder_state        WHERE account_id = ?1",
        "DELETE FROM imap_uid_map             WHERE account_id = ?1",
        "DELETE FROM caldav_collection_state  WHERE account_id = ?1",
        "DELETE FROM caldav_resource_state    WHERE account_id = ?1",
        "DELETE FROM carddav_collection_state WHERE account_id = ?1",
        "DELETE FROM carddav_resource_state   WHERE account_id = ?1",
        "DELETE FROM mail_server_settings WHERE account_id = ?1",
        "DELETE FROM caldav_settings      WHERE account_id = ?1",
        "DELETE FROM carddav_settings     WHERE account_id = ?1",
        "DELETE FROM sender_blocklist     WHERE account_id = ?1",
    ] {
        conn.execute(sql, params![login.as_str()]).ok();
    }
    conn.execute("DELETE FROM logins WHERE id = ?1", params![login.as_str()])?;
    Ok(())
}

/// SQL predicate selecting the `emails` rows belonging to a mailbox address,
/// for splicing into a query over `emails` after `account_id = ?<login>`.
/// Binds the address as `?<addr_idx>`.
///
/// A message belongs to the mailbox when **any** of three things holds:
///
/// 1. **`delivered_to` matches** — what the server actually delivered to, and
///    the only signal that catches BCC'd or alias-forwarded mail, which names
///    the address nowhere else. Captured at sync; NULL on rows stored before
///    the column existed.
/// 2. **To/Cc matches** (`email_recipients`) — already populated for *all*
///    existing mail, which is what lets the filter work retroactively with no
///    re-sync.
/// 3. **Delivery is unknown and nobody else claims it** — fail open. Without
///    this, flipping the switch on would make a user's existing BCC'd and
///    mailing-list mail silently disappear: it predates `delivered_to` (1) and
///    names them in neither To nor Cc (2). Such a message shows up in every
///    mailbox on the login rather than vanishing from all of them. Once a
///    re-sync backfills `delivered_to`, rule 1 takes over and it resolves to
///    the one true mailbox.
///
/// Sent and Drafts invert the test: a mailbox's outgoing mail is *from* its
/// address, not *to* it, so matching recipients there would show a sibling's
/// sent mail and hide its own.
///
/// Note a message with a **known** `delivered_to` that matches no mailbox on
/// the login (say, `sales@`) is deliberately hidden from every mailbox — it
/// demonstrably isn't theirs. A non-mailbox account on the same login still
/// sees it, since it applies no filter at all.
///
/// `emails.account_id` is the login id, so `o.login_id = emails.account_id`
/// enumerates the sibling mailboxes competing for the message.
pub const MAILBOX_PREDICATE: &str = "
    AND (CASE WHEN emails.folder IN ('sent', 'drafts') THEN
              lower(emails.from_email) = lower(?ADDR)
              OR NOT EXISTS (SELECT 1 FROM accounts o
                             WHERE o.login_id = emails.account_id AND o.is_mailbox = 1
                               AND lower(o.email) = lower(emails.from_email))
         ELSE
              lower(COALESCE(emails.delivered_to, '')) = lower(?ADDR)
              OR EXISTS (SELECT 1 FROM email_recipients r
                         WHERE r.email_id = emails.id
                           AND lower(r.email) = lower(?ADDR))
              OR (COALESCE(emails.delivered_to, '') = ''
                  AND NOT EXISTS (SELECT 1 FROM accounts o
                                  WHERE o.login_id = emails.account_id AND o.is_mailbox = 1
                                    AND EXISTS (SELECT 1 FROM email_recipients r2
                                                WHERE r2.email_id = emails.id
                                                  AND lower(r2.email) = lower(o.email))))
         END)";

/// [`MAILBOX_PREDICATE`] with `?ADDR` bound to a concrete parameter index, or
/// empty when the account is not a mailbox.
pub fn mailbox_clause(filter: Option<&str>, addr_idx: usize) -> String {
    match filter {
        None => String::new(),
        Some(_) => MAILBOX_PREDICATE.replace("?ADDR", &format!("?{}", addr_idx)),
    }
}

/// Restrict `column` (a `collection_url`) to the collections assigned to this
/// mailbox, binding the account id at `?acct_idx`.
///
/// A CalDAV/CardDAV collection carries no address, so — unlike mail — there is
/// nothing to match on automatically: two mailboxes on one login discover the
/// *same* calendars. The user assigns them instead, in `account_collections`.
/// A non-mailbox account gets no clause and sees all of its login's
/// collections, which is the pre-existing behaviour.
///
/// **Assigning nothing means "show everything", not "show nothing".** A
/// mailbox starts with an empty assignment list, so filtering to it literally
/// would empty the calendar the instant the switch was flipped — the same
/// disappearing-data trap the mail filter's fail-open rule avoids. Assignment
/// is opt-in narrowing: it only takes effect once the user assigns something.
///
/// `kind` is a literal from call sites (`caldav` / `carddav`), never user input.
pub fn collection_clause(view: &View, kind: &str, column: &str, acct_idx: usize) -> String {
    if view.mailbox.is_none() {
        return String::new();
    }
    format!(
        " AND (NOT EXISTS (SELECT 1 FROM account_collections
                           WHERE account_id = ?{acct_idx} AND kind = '{kind}')
               OR {column} IN (SELECT collection_url FROM account_collections
                               WHERE account_id = ?{acct_idx} AND kind = '{kind}'))"
    )
}

/// [`collection_clause`], but also admitting rows whose collection is NULL.
///
/// For tables where belonging to no server collection is legitimate: a
/// locally-created contact has no `carddav_resource_state` row, so a plain
/// `IN (…)` would make every local contact silently vanish the moment an
/// account became a mailbox. Local rows aren't tied to a server collection, so
/// every view onto the login sees them.
pub fn collection_clause_keep_local(view: &View, kind: &str, column: &str, acct_idx: usize) -> String {
    if view.mailbox.is_none() {
        return String::new();
    }
    format!(
        " AND ({column} IS NULL
               OR NOT EXISTS (SELECT 1 FROM account_collections
                              WHERE account_id = ?{acct_idx} AND kind = '{kind}')
               OR {column} IN (SELECT collection_url FROM account_collections
                               WHERE account_id = ?{acct_idx} AND kind = '{kind}'))"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Two accounts, each on its own login, one email owned by `work`'s login.
    fn db_with_two_accounts() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&conn).unwrap();
        crate::sync_state::init_sync_tables(&conn).unwrap();
        for (id, email) in [
            ("work", "takashi.nakamoto@cortexist.com"),
            ("eng", "engineering@cortexist.com"),
        ] {
            conn.execute(
                "INSERT INTO logins (id, created_at) VALUES (?1, '')",
                params![id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO accounts (id, name, email, initials, color, login_id)
                 VALUES (?1, ?1, ?2, 'XX', '#000', ?1)",
                params![id, email],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO emails (id, account_id, from_name, from_email, from_initials,
                                 from_color, subject, preview, body, date, folder)
             VALUES ('e1', 'work', 'A', 'a@x.com', 'A', '#000', 's', 'p', 'b', '2026-01-01', 'inbox')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn matching_credentials_attach_to_the_existing_login() {
        let conn = db_with_two_accounts();
        let creds = settings("imap.cortexist.com", 993, "takashi");
        crate::sync_state::save_mail_settings(&conn, &LoginId("work".into()), &creds).unwrap();
        crate::sync_state::save_mail_settings(&conn, &LoginId("eng".into()), &creds).unwrap();

        let (target, moved) = attach_to_matching(&conn, "eng", &creds).unwrap();

        assert!(moved, "identical credentials should have merged the logins");
        assert_eq!(target.as_str(), "work");
        assert_eq!(resolve(&conn, "eng").unwrap().as_str(), "work");
        // Both accounts now read one dataset — the point of the whole design.
        assert_eq!(accounts_on(&conn, &LoginId("work".into())).unwrap().len(), 2);
        // The vacated login is gone, not left dangling.
        assert_eq!(
            0i64,
            conn.query_row("SELECT COUNT(*) FROM logins WHERE id = 'eng'", [], |r| r.get::<_, i64>(0)).unwrap()
        );
    }

    #[test]
    fn different_credentials_leave_logins_separate() {
        let conn = db_with_two_accounts();
        crate::sync_state::save_mail_settings(&conn, &LoginId("work".into()), &settings("imap.cortexist.com", 993, "takashi")).unwrap();
        let other = settings("imap.example.net", 993, "someone");
        crate::sync_state::save_mail_settings(&conn, &LoginId("eng".into()), &other).unwrap();

        let (target, moved) = attach_to_matching(&conn, "eng", &other).unwrap();

        assert!(!moved);
        assert_eq!(target.as_str(), "eng");
        assert_eq!(accounts_on(&conn, &LoginId("work".into())).unwrap(), vec!["work"]);
    }

    #[test]
    fn purge_removes_the_login_and_its_data() {
        let conn = db_with_two_accounts();
        conn.execute("DELETE FROM accounts WHERE id = 'work'", []).unwrap();

        purge(&conn, &LoginId("work".into())).unwrap();

        assert_eq!(
            0i64,
            conn.query_row("SELECT COUNT(*) FROM emails WHERE account_id = 'work'", [], |r| r.get::<_, i64>(0)).unwrap()
        );
        assert_eq!(
            0i64,
            conn.query_row("SELECT COUNT(*) FROM logins WHERE id = 'work'", [], |r| r.get::<_, i64>(0)).unwrap()
        );
    }

    /// The sharp edge. Login `work` is anchored on account `work`, and
    /// `emails.account_id` declares a cascading FK to `accounts(id)` with
    /// foreign keys ON app-wide. Deleting the anchor while `eng` shares the
    /// login must neither take the mail with it nor strand the rows against a
    /// deleted parent.
    #[test]
    fn deleting_the_anchor_of_a_shared_login_keeps_the_data_and_leaves_it_writable() {
        let mut conn = db_with_two_accounts();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute("DELETE FROM logins WHERE id = 'eng'", []).unwrap();
        conn.execute("UPDATE accounts SET login_id = 'work' WHERE id = 'eng'", []).unwrap();

        // Mirrors delete_account's shared-login branch.
        let tx = conn.transaction().unwrap();
        let lg = LoginId("work".into());
        let heir = heir_for(&tx, &lg, "work").unwrap().expect("eng should be the heir");
        assert_eq!(heir, "eng");
        repoint(&tx, &lg, &LoginId(heir)).unwrap();
        tx.execute("DELETE FROM accounts WHERE id = 'work'", []).unwrap();
        tx.commit().unwrap();

        // The mail survived, re-keyed onto the heir.
        assert_eq!(
            1i64,
            conn.query_row("SELECT COUNT(*) FROM emails WHERE account_id = 'eng'", [], |r| r.get::<_, i64>(0)).unwrap(),
            "deleting the anchor destroyed the mail its sibling still reads"
        );
        assert_eq!(resolve(&conn, "eng").unwrap().as_str(), "eng");
        assert_eq!(
            1i64,
            conn.query_row("SELECT COUNT(*) FROM logins WHERE id = 'eng'", [], |r| r.get::<_, i64>(0)).unwrap(),
            "the login should have been re-anchored, not dropped"
        );

        // The assertion that actually catches the dangling-FK variant: a later
        // sync must still be able to insert. Before anchor promotion this threw
        // FOREIGN KEY constraint failed, and the sibling's sync died for good.
        conn.execute(
            "INSERT INTO emails (id, account_id, from_name, from_email, from_initials,
                                 from_color, subject, preview, body, date, folder)
             VALUES ('e2', 'eng', 'B', 'b@x.com', 'B', '#000', 's', 'p', 'b', '2026-01-02', 'inbox')",
            [],
        )
        .expect("sync insert after anchor removal must not fail the FK check");
    }

    /// The other half: removing the last account really does take the data.
    #[test]
    fn deleting_the_last_account_takes_the_login_and_its_data() {
        let mut conn = db_with_two_accounts();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        let tx = conn.transaction().unwrap();
        assert!(heir_for(&tx, &LoginId("work".into()), "work").unwrap().is_none());
        tx.execute("DELETE FROM logins WHERE id = 'work'", []).unwrap();
        tx.execute("DELETE FROM accounts WHERE id = 'work'", []).unwrap();
        tx.commit().unwrap();

        // Cascade from accounts reaches the login-scoped rows, because
        // login.id == the deleted account's id.
        assert_eq!(
            0i64,
            conn.query_row("SELECT COUNT(*) FROM emails WHERE account_id = 'work'", [], |r| r.get::<_, i64>(0)).unwrap(),
            "the last account's mail should have gone with it"
        );
    }

    fn settings(server: &str, port: i32, user: &str) -> MailServerSettings {
        MailServerSettings {
            protocol: "imap".into(),
            incoming_server: server.into(),
            incoming_port: port,
            incoming_username: user.into(),
            incoming_password: "secret".into(),
            incoming_security: "ssl".into(),
            smtp_server: "smtp.example.com".into(),
            smtp_port: 587,
            smtp_username: user.into(),
            smtp_password: "secret".into(),
            smtp_security: "tls".into(),
            sync_interval_minutes: 5,
        }
    }

    #[test]
    fn same_credentials_share_a_fingerprint() {
        let a = settings("imap.cortexist.com", 993, "takashi");
        let b = settings("imap.cortexist.com", 993, "takashi");
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn fingerprint_ignores_case() {
        let a = settings("imap.cortexist.com", 993, "takashi");
        let b = settings("IMAP.Cortexist.COM", 993, "Takashi");
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn different_user_or_host_or_port_splits_the_login() {
        let base = settings("imap.cortexist.com", 993, "takashi");
        assert_ne!(fingerprint(&base), fingerprint(&settings("imap.cortexist.com", 993, "engineering")));
        assert_ne!(fingerprint(&base), fingerprint(&settings("imap.other.com", 993, "takashi")));
        assert_ne!(fingerprint(&base), fingerprint(&settings("imap.cortexist.com", 143, "takashi")));
    }

    #[test]
    fn outgoing_settings_do_not_split_a_login() {
        // A login owns SMTP as part of its set; differing SMTP must not strand
        // two accounts that reach the same incoming server on separate logins.
        let a = settings("imap.cortexist.com", 993, "takashi");
        let mut b = settings("imap.cortexist.com", 993, "takashi");
        b.smtp_server = "smtp.elsewhere.com".into();
        b.smtp_port = 2525;
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn non_mailbox_account_gets_no_clause() {
        assert_eq!(mailbox_clause(None, 2), "");
    }

    // ── The mailbox predicate, run against real SQL ──────────────────────
    //
    // String assertions can't tell us whether the filter actually selects the
    // right rows, so these drive it through SQLite. Both accounts are
    // mailboxes on one shared login, mirroring the motivating setup.

    const TAKASHI: &str = "takashi.nakamoto@cortexist.com";
    const ENG: &str = "engineering@cortexist.com";

    fn shared_login_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&conn).unwrap();
        crate::sync_state::init_sync_tables(&conn).unwrap();
        conn.execute("INSERT INTO logins (id, created_at) VALUES ('work', '')", []).unwrap();
        for (id, email) in [("work", TAKASHI), ("eng", ENG)] {
            conn.execute(
                "INSERT INTO accounts (id, name, email, initials, color, login_id, is_mailbox)
                 VALUES (?1, ?1, ?2, 'XX', '#000', 'work', 1)",
                params![id, email],
            )
            .unwrap();
        }
        conn
    }

    /// `delivered_to = None` models mail synced before the column existed.
    fn add_mail(conn: &Connection, id: &str, folder: &str, from: &str, delivered_to: Option<&str>, to: &[&str]) {
        conn.execute(
            "INSERT INTO emails (id, account_id, from_name, from_email, from_initials, from_color,
                                 subject, preview, body, date, folder, delivered_to)
             VALUES (?1, 'work', 'S', ?2, 'S', '#000', 's', 'p', 'b', '2026-01-01', ?3, ?4)",
            params![id, from, folder, delivered_to],
        )
        .unwrap();
        for addr in to {
            conn.execute(
                "INSERT INTO email_recipients (email_id, name, email, initials, color, type)
                 VALUES (?1, 'R', ?2, 'R', '#000', 'to')",
                params![id, addr],
            )
            .unwrap();
        }
    }

    /// Ids visible to `account_id`, through the real predicate.
    fn visible(conn: &Connection, account_id: &str) -> Vec<String> {
        let view = view_of(conn, account_id).unwrap();
        let sql = format!(
            "SELECT id FROM emails WHERE account_id = ?1{} ORDER BY id",
            mailbox_clause(view.mailbox.as_deref(), 2)
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt.query_map(view.params().as_slice(), |r| r.get::<_, String>(0)).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    #[test]
    fn each_mailbox_sees_only_mail_addressed_to_it() {
        let conn = shared_login_db();
        add_mail(&conn, "to_takashi", "inbox", "a@x.com", Some(TAKASHI), &[TAKASHI]);
        add_mail(&conn, "to_eng", "inbox", "a@x.com", Some(ENG), &[ENG]);

        assert_eq!(visible(&conn, "work"), vec!["to_takashi"]);
        assert_eq!(visible(&conn, "eng"), vec!["to_eng"]);
    }

    #[test]
    fn bcc_is_matched_by_delivered_to_alone() {
        let conn = shared_login_db();
        // BCC: the address appears in no recipient header at all.
        add_mail(&conn, "bcc_eng", "inbox", "a@x.com", Some(ENG), &[]);

        assert_eq!(visible(&conn, "eng"), vec!["bcc_eng"], "delivered_to should have claimed it");
        assert!(visible(&conn, "work").is_empty(), "leaked a sibling's BCC'd mail");
    }

    #[test]
    fn to_cc_still_matches_when_delivery_is_unknown() {
        let conn = shared_login_db();
        add_mail(&conn, "old_to_eng", "inbox", "a@x.com", None, &[ENG]);

        // Retroactive match with no re-sync — the reason To/Cc stays in the rule.
        assert_eq!(visible(&conn, "eng"), vec!["old_to_eng"]);
        assert!(visible(&conn, "work").is_empty());
    }

    /// Fail open: pre-existing BCC'd or mailing-list mail must not vanish when
    /// the switch is flipped on.
    #[test]
    fn unknown_delivery_claimed_by_nobody_shows_in_every_mailbox() {
        let conn = shared_login_db();
        add_mail(&conn, "orphan", "inbox", "list@x.com", None, &["announce@lists.example"]);

        assert_eq!(visible(&conn, "work"), vec!["orphan"]);
        assert_eq!(visible(&conn, "eng"), vec!["orphan"], "flipping the toggle hid a message");
    }

    /// …but only when nobody claims it. An unknown-delivery message that names
    /// a sibling in To/Cc belongs to that sibling, not to everyone.
    #[test]
    fn unknown_delivery_claimed_by_a_sibling_does_not_fail_open() {
        let conn = shared_login_db();
        add_mail(&conn, "old_to_eng", "inbox", "a@x.com", None, &[ENG]);

        assert!(
            visible(&conn, "work").is_empty(),
            "fail-open leaked mail that engineering@ had already claimed"
        );
    }

    #[test]
    fn sent_mail_is_matched_on_the_sender_not_the_recipient() {
        let conn = shared_login_db();
        // Sent by engineering@ to an outsider. Matching recipients here would
        // put it in neither mailbox; matching From puts it in the right one.
        add_mail(&conn, "sent_by_eng", "sent", ENG, None, &["outsider@x.com"]);
        // Sent by takashi@ *to* engineering@ — must not surface as eng's sent mail.
        add_mail(&conn, "sent_by_takashi", "sent", TAKASHI, None, &[ENG]);

        assert_eq!(visible(&conn, "eng"), vec!["sent_by_eng"]);
        assert_eq!(visible(&conn, "work"), vec!["sent_by_takashi"]);
    }

    #[test]
    fn a_non_mailbox_account_sees_everything_on_its_login() {
        let conn = shared_login_db();
        conn.execute("UPDATE accounts SET is_mailbox = 0 WHERE id = 'work'", []).unwrap();
        add_mail(&conn, "to_takashi", "inbox", "a@x.com", Some(TAKASHI), &[TAKASHI]);
        add_mail(&conn, "to_eng", "inbox", "a@x.com", Some(ENG), &[ENG]);
        add_mail(&conn, "to_sales", "inbox", "a@x.com", Some("sales@cortexist.com"), &[]);

        // The catch-all keeps mail that no mailbox claims reachable.
        assert_eq!(visible(&conn, "work"), vec!["to_eng", "to_sales", "to_takashi"]);
        assert_eq!(visible(&conn, "eng"), vec!["to_eng"]);
    }

    /// The filter is a property of the flag, not of whether a login is shared.
    #[test]
    fn a_solo_mailbox_still_filters() {
        let conn = shared_login_db();
        conn.execute("DELETE FROM accounts WHERE id = 'eng'", []).unwrap();
        add_mail(&conn, "to_takashi", "inbox", "a@x.com", Some(TAKASHI), &[TAKASHI]);
        add_mail(&conn, "to_eng", "inbox", "a@x.com", Some(ENG), &[ENG]);

        assert_eq!(
            visible(&conn, "work"),
            vec!["to_takashi"],
            "a solo mailbox picked up mail for another address"
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        let conn = shared_login_db();
        add_mail(&conn, "shouty", "inbox", "a@x.com", Some("Engineering@Cortexist.COM"), &[]);

        assert_eq!(visible(&conn, "eng"), vec!["shouty"]);
    }

    // ── Collection assignment ────────────────────────────────────────────

    fn add_calendar(conn: &Connection, url: &str) {
        conn.execute(
            "INSERT INTO caldav_collection_state (account_id, collection_url, display_name)
             VALUES ('work', ?1, ?1)",
            params![url],
        )
        .unwrap();
    }

    fn visible_calendars(conn: &Connection, account_id: &str) -> Vec<String> {
        let view = view_of(conn, account_id).unwrap();
        let sql = format!(
            "SELECT cs.collection_url FROM caldav_collection_state cs
             WHERE cs.account_id = ?1{} ORDER BY cs.collection_url",
            collection_clause(&view, "caldav", "cs.collection_url", 2)
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt.query_map(view.collection_params().as_slice(), |r| r.get::<_, String>(0)).unwrap();
        rows.filter_map(|r| r.ok()).collect()
    }

    /// The trap: a mailbox has assigned nothing yet, so filtering to its
    /// assignments literally would empty its calendar the moment the switch
    /// went on. Assigning nothing must mean "everything", not "nothing".
    #[test]
    fn a_mailbox_with_no_assignments_still_sees_every_calendar() {
        let conn = shared_login_db();
        add_calendar(&conn, "/cal/personal/");
        add_calendar(&conn, "/cal/engineering/");

        assert_eq!(
            visible_calendars(&conn, "eng"),
            vec!["/cal/engineering/", "/cal/personal/"],
            "flipping the mailbox switch emptied the calendar"
        );
    }

    #[test]
    fn assigning_a_calendar_narrows_the_mailbox_to_it() {
        let conn = shared_login_db();
        add_calendar(&conn, "/cal/personal/");
        add_calendar(&conn, "/cal/engineering/");
        conn.execute(
            "INSERT INTO account_collections (account_id, kind, collection_url)
             VALUES ('eng', 'caldav', '/cal/engineering/')",
            [],
        )
        .unwrap();

        assert_eq!(visible_calendars(&conn, "eng"), vec!["/cal/engineering/"]);
        // The sibling assigned nothing, so it is unaffected and still sees all.
        assert_eq!(
            visible_calendars(&conn, "work"),
            vec!["/cal/engineering/", "/cal/personal/"]
        );
    }

    #[test]
    fn mailbox_clause_binds_every_addr_slot() {
        let sql = mailbox_clause(Some("engineering@cortexist.com"), 2);
        assert!(!sql.contains("?ADDR"), "unsubstituted placeholder left in: {sql}");
        // from_email, delivered_to, r.email — an unsubstituted slot would make
        // rusqlite reject the statement at runtime, not compile time.
        assert_eq!(sql.matches("?2").count(), 3);
    }
}
