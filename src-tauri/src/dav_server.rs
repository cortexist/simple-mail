//! dav_server.rs — Embedded CardDAV / CalDAV server for local sync (e.g. DAVx5 over Bluetooth PAN).
//!
//! Implements a minimal WebDAV/CardDAV/CalDAV server using Axum that serves contacts and
//! calendar events from the app's SQLite database. The DAVx5 client authenticates with
//! Basic Auth — the email address determines which account to sync against.
//!
//! URL layout:
//!   /.well-known/carddav              → 301 /carddav/
//!   /.well-known/caldav               → 301 /caldav/
//!   /                                 → PROPFIND principal (current-user-principal)
//!   /principals/{account_id}/         → PROPFIND user principal
//!   /carddav/{account_id}/            → PROPFIND address book collection
//!   /carddav/{account_id}/{contact}.vcf → GET/PUT/DELETE individual vCard
//!   /caldav/{account_id}/             → PROPFIND calendar collection
//!   /caldav/{account_id}/{event}.ics  → GET/PUT/DELETE individual iCal event

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use rusqlite::Connection;
use std::sync::Mutex;
use tokio::sync::watch;

use crate::carddav_client;
use crate::caldav_client;

// ── Shared state ────────────────────────────────────────

/// Shared state for the DAV server — wraps the same DB connection as the Tauri app.
#[derive(Clone)]
pub struct DavState {
    pub db: Arc<Mutex<Connection>>,
    /// Map of email → (account_id, password_hash) for Basic Auth.
    /// Populated from the `dav_server_users` settings table.
    pub users: Arc<Mutex<HashMap<String, (String, String)>>>,
}

/// Handle for the running server — used to stop it.
pub struct DavServerHandle {
    shutdown_tx: watch::Sender<bool>,
    local_addr: SocketAddr,
}

impl DavServerHandle {
    pub fn addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

// ── Server start / stop ────────────────────────────────

/// Start the embedded DAV server on the given address.
/// Returns a handle that can be used to get the bound address and stop the server.
pub async fn start_server(
    db: Arc<Mutex<Connection>>,
    bind_addr: SocketAddr,
) -> Result<DavServerHandle, String> {
    // Load configured users from DB
    let users = load_dav_users(&db);

    let state = DavState {
        db,
        users: Arc::new(Mutex::new(users)),
    };

    let app = Router::new()
        // Well-known redirects
        .route("/.well-known/carddav", any(well_known_carddav))
        .route("/.well-known/caldav", any(well_known_caldav))
        // Root principal discovery
        .route("/", any(root_propfind))
        // User principal
        .route("/principals/{account_id}/", any(principal_propfind))
        // CardDAV address book collection
        .route("/carddav/{account_id}/", any(carddav_collection))
        // CardDAV individual resource
        .route("/carddav/{account_id}/{resource}", any(carddav_resource))
        // CalDAV calendar collection
        .route("/caldav/{account_id}/", any(caldav_collection))
        // CalDAV individual resource
        .route("/caldav/{account_id}/{resource}", any(caldav_resource))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("Failed to bind DAV server to {}: {}", bind_addr, e))?;

    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {}", e))?;

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                while !*shutdown_rx.borrow_and_update() {
                    if shutdown_rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
            .ok();
    });

    log::info!("DAV server started on {}", local_addr);

    Ok(DavServerHandle {
        shutdown_tx,
        local_addr,
    })
}

// ── User management ─────────────────────────────────────

/// Users are stored in a `dav_server_users` table: (email, password, account_id).
/// Passwords are AES-256-GCM encrypted; decrypted at load time into the in-memory map.
fn load_dav_users(db: &Arc<Mutex<Connection>>) -> HashMap<String, (String, String)> {
    let conn = match db.lock() {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };

    // Ensure the table exists
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS dav_server_users (
            email      TEXT PRIMARY KEY,
            password   TEXT NOT NULL,
            account_id TEXT NOT NULL,
            FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
        );"
    ).ok();

    let mut map = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT email, password, account_id FROM dav_server_users") {
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                // Decrypt the stored password; fall back to raw value if decryption fails
                let pw = crate::crypto::decrypt_password(&row.1).unwrap_or(row.1);
                map.insert(row.0.to_lowercase(), (pw, row.2));
            }
        }
    }
    map
}

/// Authenticate a request via Basic Auth. Returns the account_id on success.
fn authenticate(headers: &HeaderMap, state: &DavState) -> Result<String, Response> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("WWW-Authenticate", "Basic realm=\"Mail DAV\"")
                .body(Body::empty())
                .unwrap()
        })?;

    if !auth_header.starts_with("Basic ") {
        return Err(Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("WWW-Authenticate", "Basic realm=\"Mail DAV\"")
            .body(Body::empty())
            .unwrap());
    }

    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(auth_header.trim_start_matches("Basic "))
        .map_err(|_| {
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("WWW-Authenticate", "Basic realm=\"Mail DAV\"")
                .body(Body::empty())
                .unwrap()
        })?;

    let cred = String::from_utf8(decoded).map_err(|_| {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("WWW-Authenticate", "Basic realm=\"Mail DAV\"")
            .body(Body::empty())
            .unwrap()
    })?;

    let (email, password) = cred.split_once(':').ok_or_else(|| {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("WWW-Authenticate", "Basic realm=\"Mail DAV\"")
            .body(Body::empty())
            .unwrap()
    })?;

    let users = state.users.lock().unwrap();
    let (stored_pw, account_id) = users.get(&email.to_lowercase()).ok_or_else(|| {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("WWW-Authenticate", "Basic realm=\"Mail DAV\"")
            .body(Body::empty())
            .unwrap()
    })?;

    if stored_pw != password {
        return Err(Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("WWW-Authenticate", "Basic realm=\"Mail DAV\"")
            .body(Body::empty())
            .unwrap());
    }

    Ok(account_id.clone())
}

// ── Well-known redirects ────────────────────────────────

async fn well_known_carddav() -> Response {
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header("Location", "/carddav/")
        .body(Body::empty())
        .unwrap()
}

async fn well_known_caldav() -> Response {
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header("Location", "/caldav/")
        .body(Body::empty())
        .unwrap()
}

// ── Root PROPFIND (principal discovery) ─────────────────

async fn root_propfind(
    method: Method,
    headers: HeaderMap,
    State(state): State<DavState>,
) -> Response {
    if method != Method::from_bytes(b"PROPFIND").unwrap() && method != Method::OPTIONS {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    if method == Method::OPTIONS {
        return dav_options_response();
    }

    let account_id = match authenticate(&headers, &state) {
        Ok(id) => id,
        Err(r) => return r,
    };

    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/</d:href>
    <d:propstat>
      <d:prop>
        <d:current-user-principal>
          <d:href>/principals/{account_id}/</d:href>
        </d:current-user-principal>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#
    );

    multistatus_response(&xml)
}

// ── Principal PROPFIND ──────────────────────────────────

async fn principal_propfind(
    method: Method,
    headers: HeaderMap,
    Path(account_id): Path<String>,
    State(state): State<DavState>,
) -> Response {
    if method == Method::OPTIONS {
        return dav_options_response();
    }

    if method != Method::from_bytes(b"PROPFIND").unwrap() {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let authed_account = match authenticate(&headers, &state) {
        Ok(id) => id,
        Err(r) => return r,
    };

    if authed_account != account_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/principals/{account_id}/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/><d:principal/></d:resourcetype>
        <card:addressbook-home-set>
          <d:href>/carddav/{account_id}/</d:href>
        </card:addressbook-home-set>
        <cal:calendar-home-set>
          <d:href>/caldav/{account_id}/</d:href>
        </cal:calendar-home-set>
        <d:displayname>Mail Contacts &amp; Calendar</d:displayname>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#
    );

    multistatus_response(&xml)
}

// ── CardDAV collection (address book) ──────────────────

async fn carddav_collection(
    method: Method,
    headers: HeaderMap,
    Path(account_id): Path<String>,
    State(state): State<DavState>,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return dav_options_response();
    }

    let authed_account = match authenticate(&headers, &state) {
        Ok(id) => id,
        Err(r) => return r,
    };

    if authed_account != account_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let m = method.as_str();

    match m {
        "PROPFIND" => carddav_collection_propfind(&account_id, &state),
        "REPORT" => carddav_collection_report(&account_id, &state, &body),
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

fn carddav_collection_propfind(account_id: &str, state: &DavState) -> Response {
    let conn = state.db.lock().unwrap();
    let ctag = compute_contacts_ctag(&conn, account_id);

    // Check Depth header — if Depth: 0, return only the collection itself
    // For Depth:1, also list all contact resources
    let contact_entries = list_contact_resources(&conn, account_id);
    drop(conn);

    let mut entries_xml = String::new();
    for (contact_id, etag) in &contact_entries {
        entries_xml.push_str(&format!(
            r#"  <d:response>
    <d:href>/carddav/{account_id}/{contact_id}.vcf</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"{etag}"</d:getetag>
        <d:getcontenttype>text/vcard; charset=utf-8</d:getcontenttype>
        <d:resourcetype/>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
"#
        ));
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cs="http://calendarserver.org/ns/">
  <d:response>
    <d:href>/carddav/{account_id}/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/><card:addressbook/></d:resourcetype>
        <d:displayname>Contacts</d:displayname>
        <cs:getctag>{ctag}</cs:getctag>
        <d:supported-report-set>
          <d:supported-report><d:report><card:addressbook-multiget/></d:report></d:supported-report>
          <d:supported-report><d:report><card:addressbook-query/></d:report></d:supported-report>
        </d:supported-report-set>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
{entries_xml}</d:multistatus>"#
    );

    multistatus_response(&xml)
}

fn carddav_collection_report(account_id: &str, state: &DavState, body: &str) -> Response {
    // Parse the requested hrefs from an addressbook-multiget REPORT
    let requested_hrefs = parse_multiget_hrefs(body);
    let conn = state.db.lock().unwrap();

    let mut responses = String::new();

    if requested_hrefs.is_empty() {
        // addressbook-query: return all contacts
        let contacts = load_contacts_as_vcards(&conn, account_id, None);
        for (contact_id, etag, vcard) in &contacts {
            let escaped = xml_escape(vcard);
            responses.push_str(&format!(
                r#"  <d:response>
    <d:href>/carddav/{account_id}/{contact_id}.vcf</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"{etag}"</d:getetag>
        <card:address-data>{escaped}</card:address-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
"#
            ));
        }
    } else {
        // multiget: return only requested contacts
        let prefix = format!("/carddav/{}/", account_id);
        let ids: Vec<String> = requested_hrefs
            .iter()
            .filter_map(|h| h.strip_prefix(&prefix))
            .filter_map(|s| s.strip_suffix(".vcf"))
            .map(|s| s.to_string())
            .collect();

        let contacts = load_contacts_as_vcards(&conn, account_id, Some(&ids));
        for (contact_id, etag, vcard) in &contacts {
            let escaped = xml_escape(vcard);
            responses.push_str(&format!(
                r#"  <d:response>
    <d:href>/carddav/{account_id}/{contact_id}.vcf</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"{etag}"</d:getetag>
        <card:address-data>{escaped}</card:address-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
"#
            ));
        }
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
{responses}</d:multistatus>"#
    );

    multistatus_response(&xml)
}

// ── CardDAV individual resource ──────────────────────────

async fn carddav_resource(
    method: Method,
    headers: HeaderMap,
    Path((account_id, resource)): Path<(String, String)>,
    State(state): State<DavState>,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return dav_options_response();
    }

    let authed_account = match authenticate(&headers, &state) {
        Ok(id) => id,
        Err(r) => return r,
    };

    if authed_account != account_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let contact_id = resource.strip_suffix(".vcf").unwrap_or(&resource);

    match method {
        ref m if m == Method::GET => carddav_get(contact_id, &account_id, &state),
        ref m if m == Method::PUT => carddav_put(contact_id, &account_id, &state, &body),
        ref m if m == Method::DELETE => carddav_delete(contact_id, &account_id, &state),
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

fn carddav_get(contact_id: &str, account_id: &str, state: &DavState) -> Response {
    let conn = state.db.lock().unwrap();
    let contacts = load_contacts_as_vcards(&conn, account_id, Some(&[contact_id.to_string()]));

    match contacts.first() {
        Some((_, etag, vcard)) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/vcard; charset=utf-8")
            .header("ETag", format!("\"{}\"", etag))
            .body(Body::from(vcard.clone()))
            .unwrap(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn carddav_put(contact_id: &str, account_id: &str, state: &DavState, vcard_body: &str) -> Response {
    // Parse the incoming vCard
    let parsed = match carddav_client::parse_vcard_public(vcard_body, "", "") {
        Some(c) => c,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let conn = state.db.lock().unwrap();

    // Check if contact already exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM contacts WHERE id = ?1 AND account_id = ?2",
            rusqlite::params![contact_id, account_id],
            |row| row.get::<_, i32>(0),
        )
        .unwrap_or(0)
        > 0;

    let email = parsed.emails.first().map(|e| e.email.as_str()).unwrap_or("");
    let phone = parsed.phones.iter().find(|p| p.label == "work" || p.label == "other")
        .map(|p| p.number.clone());
    let mobile = parsed.phones.iter().find(|p| p.label == "cell" || p.label == "mobile")
        .map(|p| p.number.clone());

    let new_etag = generate_etag();

    if exists {
        conn.execute(
            "UPDATE contacts SET name=?2, email=?3, phone=?4, mobile=?5, job_title=?6, organization=?7, photo_url=?8, rev=?9 WHERE id=?1 AND account_id=?10",
            rusqlite::params![
                contact_id, parsed.full_name, email, phone, mobile,
                if parsed.title.is_empty() { None } else { Some(&parsed.title) },
                if parsed.organization.is_empty() { None } else { Some(&parsed.organization) },
                parsed.photo_url,
                parsed.rev,
                account_id,
            ],
        ).ok();
    } else {
        let initials = make_initials(&parsed.full_name, email);
        let color = make_avatar_color(email);
        conn.execute(
            "INSERT OR IGNORE INTO contacts (id, account_id, name, email, initials, color, phone, mobile, job_title, organization, photo_url, is_favorite, rev)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,0,?12)",
            rusqlite::params![
                contact_id, account_id, parsed.full_name, email,
                initials, color, phone, mobile,
                if parsed.title.is_empty() { None } else { Some(&parsed.title) },
                if parsed.organization.is_empty() { None } else { Some(&parsed.organization) },
                parsed.photo_url,
                parsed.rev,
            ],
        ).ok();
    }

    crate::write_contact_children(&conn, contact_id, &parsed.emails, &parsed.phones, &parsed.addresses);

    let status = if exists { StatusCode::NO_CONTENT } else { StatusCode::CREATED };

    Response::builder()
        .status(status)
        .header("ETag", format!("\"{}\"", new_etag))
        .body(Body::empty())
        .unwrap()
}

fn carddav_delete(contact_id: &str, account_id: &str, state: &DavState) -> Response {
    let conn = state.db.lock().unwrap();
    let deleted = conn
        .execute(
            "DELETE FROM contacts WHERE id = ?1 AND account_id = ?2",
            rusqlite::params![contact_id, account_id],
        )
        .unwrap_or(0);

    if deleted > 0 {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

// ── CalDAV collection (calendar) ───────────────────────

async fn caldav_collection(
    method: Method,
    headers: HeaderMap,
    Path(account_id): Path<String>,
    State(state): State<DavState>,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return dav_options_response();
    }

    let authed_account = match authenticate(&headers, &state) {
        Ok(id) => id,
        Err(r) => return r,
    };

    if authed_account != account_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let m = method.as_str();

    match m {
        "PROPFIND" => caldav_collection_propfind(&account_id, &state),
        "REPORT" => caldav_collection_report(&account_id, &state, &body),
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

fn caldav_collection_propfind(account_id: &str, state: &DavState) -> Response {
    let conn = state.db.lock().unwrap();
    let ctag = compute_events_ctag(&conn, account_id);
    let event_entries = list_event_resources(&conn, account_id);
    drop(conn);

    let mut entries_xml = String::new();
    for (event_id, etag) in &event_entries {
        entries_xml.push_str(&format!(
            r#"  <d:response>
    <d:href>/caldav/{account_id}/{event_id}.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"{etag}"</d:getetag>
        <d:getcontenttype>text/calendar; charset=utf-8</d:getcontenttype>
        <d:resourcetype/>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
"#
        ));
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/">
  <d:response>
    <d:href>/caldav/{account_id}/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
        <d:displayname>Calendar</d:displayname>
        <cs:getctag>{ctag}</cs:getctag>
        <cal:supported-calendar-component-set>
          <cal:comp name="VEVENT"/>
        </cal:supported-calendar-component-set>
        <d:supported-report-set>
          <d:supported-report><d:report><cal:calendar-multiget/></d:report></d:supported-report>
          <d:supported-report><d:report><cal:calendar-query/></d:report></d:supported-report>
        </d:supported-report-set>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
{entries_xml}</d:multistatus>"#
    );

    multistatus_response(&xml)
}

fn caldav_collection_report(account_id: &str, state: &DavState, body: &str) -> Response {
    let requested_hrefs = parse_multiget_hrefs(body);
    let conn = state.db.lock().unwrap();

    let mut responses = String::new();

    if requested_hrefs.is_empty() {
        // calendar-query: return all events
        let events = load_events_as_ical(&conn, account_id, None);
        for (event_id, etag, ical) in &events {
            let escaped = xml_escape(ical);
            responses.push_str(&format!(
                r#"  <d:response>
    <d:href>/caldav/{account_id}/{event_id}.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"{etag}"</d:getetag>
        <cal:calendar-data>{escaped}</cal:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
"#
            ));
        }
    } else {
        let prefix = format!("/caldav/{}/", account_id);
        let ids: Vec<String> = requested_hrefs
            .iter()
            .filter_map(|h| h.strip_prefix(&prefix))
            .filter_map(|s| s.strip_suffix(".ics"))
            .map(|s| s.to_string())
            .collect();

        let events = load_events_as_ical(&conn, account_id, Some(&ids));
        for (event_id, etag, ical) in &events {
            let escaped = xml_escape(ical);
            responses.push_str(&format!(
                r#"  <d:response>
    <d:href>/caldav/{account_id}/{event_id}.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"{etag}"</d:getetag>
        <cal:calendar-data>{escaped}</cal:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
"#
            ));
        }
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
{responses}</d:multistatus>"#
    );

    multistatus_response(&xml)
}

// ── CalDAV individual resource ──────────────────────────

async fn caldav_resource(
    method: Method,
    headers: HeaderMap,
    Path((account_id, resource)): Path<(String, String)>,
    State(state): State<DavState>,
    body: String,
) -> Response {
    if method == Method::OPTIONS {
        return dav_options_response();
    }

    let authed_account = match authenticate(&headers, &state) {
        Ok(id) => id,
        Err(r) => return r,
    };

    if authed_account != account_id {
        return StatusCode::FORBIDDEN.into_response();
    }

    let event_id = resource.strip_suffix(".ics").unwrap_or(&resource);

    match method {
        ref m if m == Method::GET => caldav_get(event_id, &account_id, &state),
        ref m if m == Method::PUT => caldav_put(event_id, &account_id, &state, &body),
        ref m if m == Method::DELETE => caldav_delete(event_id, &account_id, &state),
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

fn caldav_get(event_id: &str, account_id: &str, state: &DavState) -> Response {
    let conn = state.db.lock().unwrap();
    let events = load_events_as_ical(&conn, account_id, Some(&[event_id.to_string()]));

    match events.first() {
        Some((_, etag, ical)) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/calendar; charset=utf-8")
            .header("ETag", format!("\"{}\"", etag))
            .body(Body::from(ical.clone()))
            .unwrap(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn caldav_put(event_id: &str, account_id: &str, state: &DavState, ical_body: &str) -> Response {
    let parsed = match caldav_client::parse_ical_event_public(ical_body, "", "") {
        Some(e) => e,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let conn = state.db.lock().unwrap();

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM calendar_events WHERE id = ?1 AND account_id = ?2",
            rusqlite::params![event_id, account_id],
            |row| row.get::<_, i32>(0),
        )
        .unwrap_or(0)
        > 0;

    // Get first calendar for color and id, or default
    let color: String = conn
        .query_row(
            "SELECT color FROM caldav_collection_state WHERE account_id = ?1 LIMIT 1",
            rusqlite::params![account_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "#0078d4".to_string());

    let calendar_id: String = conn
        .query_row(
            "SELECT collection_url FROM caldav_collection_state WHERE account_id = ?1 LIMIT 1",
            rusqlite::params![account_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "personal".to_string());

    let new_etag = generate_etag();

    if exists {
        conn.execute(
            "UPDATE calendar_events SET title=?2, start=?3, end=?4, location=?5, description=?6, is_all_day=?7 WHERE id=?1 AND account_id=?8",
            rusqlite::params![
                event_id, parsed.summary, parsed.dtstart, parsed.dtend,
                parsed.location, parsed.description, parsed.is_all_day as i32,
                account_id,
            ],
        ).ok();

        // Update attendees
        conn.execute("DELETE FROM event_attendees WHERE event_id = ?1", rusqlite::params![event_id]).ok();
    } else {
        conn.execute(
            "INSERT INTO calendar_events (id, account_id, title, start, end, color, location, description, is_all_day, calendar_id, is_online_meeting)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0)",
            rusqlite::params![
                event_id, account_id, parsed.summary, parsed.dtstart, parsed.dtend,
                color, parsed.location, parsed.description, parsed.is_all_day as i32,
                calendar_id,
            ],
        ).ok();
    }

    // Insert attendees
    for (name, email, role) in &parsed.attendees {
        let initials = make_initials(name, email);
        let acolor = make_avatar_color(email);
        conn.execute(
            "INSERT INTO event_attendees (event_id, name, email, initials, color, role) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![event_id, name, email, initials, acolor, role],
        ).ok();
    }

    let status = if exists { StatusCode::NO_CONTENT } else { StatusCode::CREATED };

    Response::builder()
        .status(status)
        .header("ETag", format!("\"{}\"", new_etag))
        .body(Body::empty())
        .unwrap()
}

fn caldav_delete(event_id: &str, account_id: &str, state: &DavState) -> Response {
    let conn = state.db.lock().unwrap();

    // Delete attendees first
    conn.execute("DELETE FROM event_attendees WHERE event_id = ?1", rusqlite::params![event_id]).ok();

    let deleted = conn
        .execute(
            "DELETE FROM calendar_events WHERE id = ?1 AND account_id = ?2",
            rusqlite::params![event_id, account_id],
        )
        .unwrap_or(0);

    if deleted > 0 {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

// ── DB helpers ──────────────────────────────────────────

/// Compute a ctag for the contacts collection (hash of all etags / updated-at timestamps).
fn compute_contacts_ctag(conn: &Connection, account_id: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    // Hash total count + name+email of all contacts for change detection
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM contacts WHERE account_id = ?1",
            rusqlite::params![account_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    count.hash(&mut hasher);

    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, name, email, phone, mobile, job_title, company FROM contacts WHERE account_id = ?1 ORDER BY id"
    ) {
        let rows = stmt.query_map(rusqlite::params![account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                row.0.hash(&mut hasher);
                row.1.hash(&mut hasher);
                row.2.hash(&mut hasher);
                row.3.hash(&mut hasher);
                row.4.hash(&mut hasher);
                row.5.hash(&mut hasher);
                row.6.hash(&mut hasher);
            }
        }
    }

    format!("{:x}", hasher.finish())
}

/// Compute a ctag for the calendar collection.
fn compute_events_ctag(conn: &Connection, account_id: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM calendar_events WHERE account_id = ?1",
            rusqlite::params![account_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    count.hash(&mut hasher);

    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, title, start, end, location, description, is_all_day FROM calendar_events WHERE account_id = ?1 ORDER BY id"
    ) {
        let rows = stmt.query_map(rusqlite::params![account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i32>(6)?,
            ))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                row.0.hash(&mut hasher);
                row.1.hash(&mut hasher);
                row.2.hash(&mut hasher);
                row.3.hash(&mut hasher);
                row.4.hash(&mut hasher);
                row.5.hash(&mut hasher);
                row.6.hash(&mut hasher);
            }
        }
    }

    format!("{:x}", hasher.finish())
}

/// List all contact IDs + etag for a collection PROPFIND.
fn list_contact_resources(conn: &Connection, account_id: &str) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT id, name, email FROM contacts WHERE account_id = ?1 ORDER BY id")
        .unwrap();
    stmt.query_map(rusqlite::params![account_id], |row| {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let email: String = row.get(2)?;
        // Generate a stable etag from content
        let etag = generate_etag_from(&format!("{}{}{}", id, name, email));
        Ok((id, etag))
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// List all event IDs + etag for a collection PROPFIND.
fn list_event_resources(conn: &Connection, account_id: &str) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT id, title, start, end FROM calendar_events WHERE account_id = ?1 ORDER BY id")
        .unwrap();
    stmt.query_map(rusqlite::params![account_id], |row| {
        let id: String = row.get(0)?;
        let title: String = row.get(1)?;
        let start: String = row.get(2)?;
        let end: String = row.get(3)?;
        let etag = generate_etag_from(&format!("{}{}{}{}", id, title, start, end));
        Ok((id, etag))
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// Load contacts as vCard strings.
fn load_contacts_as_vcards(
    conn: &Connection,
    account_id: &str,
    filter_ids: Option<&[String]>,
) -> Vec<(String, String, String)> {
    let contacts = crate::query_contacts(conn, account_id);
    let mut results = Vec::new();

    for c in contacts {
        if let Some(ids) = filter_ids {
            if !ids.iter().any(|id| id == &c.id) {
                continue;
            }
        }

        let primary_email = c.emails.iter().find(|e| e.is_default).or_else(|| c.emails.first())
            .map(|e| e.email.clone()).unwrap_or_default();
        let etag = generate_etag_from(&format!("{}{}{}{}", c.id, c.name, primary_email, c.rev.clone().unwrap_or_default()));

        let first = c.first_name.clone().unwrap_or_default();
        let last = c.last_name.clone().unwrap_or_default();

        let emails: Vec<carddav_client::ContactEmail> = c.emails.iter().map(|e| carddav_client::ContactEmail {
            email: e.email.clone(), label: e.label.clone(), is_default: e.is_default,
        }).collect();
        let phones: Vec<carddav_client::ContactPhone> = c.phones.iter().map(|p| carddav_client::ContactPhone {
            number: p.number.clone(), label: p.label.clone(), subtypes: p.subtypes.clone(), is_default: p.is_default,
        }).collect();
        let addresses: Vec<carddav_client::ContactAddress> = c.addresses.iter().map(|a| carddav_client::ContactAddress {
            street: a.street.clone(), city: a.city.clone(), region: a.region.clone(),
            postal_code: a.postal_code.clone(), country: a.country.clone(),
            label: a.label.clone(), is_default: a.is_default,
        }).collect();

        let parsed = carddav_client::ParsedContact {
            uid: c.id.clone(),
            full_name: c.name,
            first_name: first,
            last_name: last,
            middle_name: c.middle_name.unwrap_or_default(),
            prefix: c.prefix.unwrap_or_default(),
            suffix: c.suffix.unwrap_or_default(),
            emails,
            phones,
            addresses,
            organization: c.organization.unwrap_or_default(),
            department: c.department,
            title: c.job_title.unwrap_or_default(),
            birthday: c.birthday,
            photo_url: c.photo_url,
            rev: c.rev,
            resource_url: String::new(),
            etag: String::new(),
        };

        let vcard = carddav_client::build_vcard_public(&parsed);
        results.push((c.id, etag, vcard));
    }

    results
}

/// Load events as iCal strings.
fn load_events_as_ical(
    conn: &Connection,
    account_id: &str,
    filter_ids: Option<&[String]>,
) -> Vec<(String, String, String)> {
    let events = crate::query_events(conn, account_id);
    let mut results = Vec::new();

    for e in events {
        if let Some(ids) = filter_ids {
            if !ids.iter().any(|id| id == &e.id) {
                continue;
            }
        }

        let etag = generate_etag_from(&format!("{}{}{}{}", e.id, e.title, e.start, e.end));

        let attendees: Vec<(String, String, String)> = e
            .attendees
            .unwrap_or_default()
            .iter()
            .map(|a| (a.name.clone(), a.email.clone(), a.role.clone()))
            .collect();
        let rrule = e.recurrence.as_ref().map(|r| crate::recurrence_to_rrule(r));

        let exdates = e.recurrence.as_ref().and_then(|r| r.exdates.clone());
        let parsed = caldav_client::ParsedEvent {
            uid: e.id.clone(),
            summary: e.title,
            dtstart: e.start,
            dtend: e.end,
            location: e.location,
            description: e.description,
            is_all_day: e.is_all_day,
            organizer: None,
            attendees,
            rrule,
            exdates,
            alert_minutes: e.alert_minutes,
            resource_url: String::new(),
            etag: String::new(),
        };

        let ical = caldav_client::build_ical_event_public(&parsed);
        results.push((e.id, etag, ical));
    }

    results
}

// ── Utility functions ───────────────────────────────────

fn multistatus_response(xml: &str) -> Response {
    Response::builder()
        .status(StatusCode::from_u16(207).unwrap()) // 207 Multi-Status
        .header("Content-Type", "application/xml; charset=utf-8")
        .header("DAV", "1, 2, 3, addressbook, calendar-access")
        .body(Body::from(xml.to_string()))
        .unwrap()
}

fn dav_options_response() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("Allow", "OPTIONS, GET, PUT, DELETE, PROPFIND, REPORT")
        .header("DAV", "1, 2, 3, addressbook, calendar-access")
        .body(Body::empty())
        .unwrap()
}

fn generate_etag() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn generate_etag_from(data: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Parse <d:href> elements from a multiget REPORT request body.
fn parse_multiget_hrefs(body: &str) -> Vec<String> {
    let mut hrefs = Vec::new();
    // Simple regex-free parser: look for <d:href>...</d:href> or <D:href>...</D:href>
    let lower = body.to_string();
    for variant in &["<d:href>", "<D:href>", "<href>"] {
        let end_tag = variant.replace('<', "</");
        let mut search = lower.as_str();
        while let Some(start) = search.find(variant) {
            let after = &search[start + variant.len()..];
            if let Some(end) = after.find(&end_tag) {
                let href = after[..end].trim().to_string();
                if !href.is_empty() {
                    hrefs.push(href);
                }
                search = &after[end + end_tag.len()..];
            } else {
                break;
            }
        }
    }
    hrefs
}

fn make_initials(name: &str, email: &str) -> String {
    let src = if name.is_empty() { email } else { name };
    src.split(|c: char| c.is_whitespace() || c == '.' || c == '@')
        .filter(|w| !w.is_empty())
        .take(2)
        .map(|w| w.chars().next().unwrap_or(' '))
        .collect::<String>()
        .to_uppercase()
}

fn make_avatar_color(email: &str) -> String {
    let colors = [
        "#0078d4", "#498205", "#8764b8", "#ca5010",
        "#c50f1f", "#038387", "#6b69d6", "#bf0077",
    ];
    let hash = email.bytes().fold(0usize, |acc, b| acc.wrapping_add(b as usize));
    colors[hash % colors.len()].to_string()
}
