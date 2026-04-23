//! caldav_client.rs — CalDAV sync engine.
//!
//! Design goals:
//!   • Discover calendars via PROPFIND on the principal URL
//!   • Use ctag (collection tag) to detect if anything changed
//!   • Use etag per resource for fine-grained delta sync
//!   • Parse iCalendar (VEVENT) into our CalendarEvent model
//!   • Efficient: skip unchanged collections, only fetch modified resources

use anyhow::{Result, Context, bail};
use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;
use reqwest::Client;
use serde::Serialize;

// ── Public types ────────────────────────────────────────

/// A discovered CalDAV calendar collection.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarInfo {
    pub url: String,
    pub display_name: String,
    pub color: String,
    pub ctag: String,
}

/// A parsed calendar event from an iCalendar resource.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedEvent {
    pub uid: String,
    pub summary: String,
    pub dtstart: String,
    pub dtend: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub is_all_day: bool,
    pub organizer: Option<(String, String)>, // (name, email) of the account owner
    pub attendees: Vec<(String, String, String)>, // (name, email, role: "required"/"optional")
    pub rrule: Option<String>,
    pub exdates: Option<Vec<String>>,  // exception dates, e.g. ["2026-04-15"]
    pub alert_minutes: Option<i32>,
    pub resource_url: String,
    pub etag: String,
}

// ── HTTP helpers ────────────────────────────────────────

fn build_client() -> Result<Client> {
    Client::builder()
        .danger_accept_invalid_certs(false)
        .user_agent(crate::identity::http_user_agent())
        .build()
        .context("Failed to build HTTP client")
}

fn basic_auth(username: &str, password: &str) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", username, password));
    format!("Basic {}", encoded)
}

// ── Calendar discovery ──────────────────────────────────

/// Deduplicate calendars that share the same display name.
///
/// Some providers expose each calendar via multiple URL paths (e.g. a native
/// store and a CalDAV proxy).  When two or more collections share a display
/// name we keep the one that appears most active, determined by:
///   1. Prefer a non-empty ctag that doesn't end with "-0" (indicates changes).
///   2. Fall back to the first one encountered.
fn dedup_calendars_by_name(cals: Vec<CalendarInfo>) -> Vec<CalendarInfo> {
    let mut seen: std::collections::HashMap<String, CalendarInfo> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for cal in cals {
        let key = cal.display_name.to_lowercase();
        if let Some(existing) = seen.get(&key) {
            // Decide which one to keep — prefer the one with real activity
            let new_score = ctag_activity_score(&cal.ctag);
            let old_score = ctag_activity_score(&existing.ctag);
            if new_score > old_score {
                log::info!(
                    "CalDAV: dedup \"{}\" — replacing {} (ctag={}) with {} (ctag={})",
                    cal.display_name, existing.url, existing.ctag, cal.url, cal.ctag
                );
                seen.insert(key, cal);
            } else {
                log::info!(
                    "CalDAV: dedup \"{}\" — keeping {} (ctag={}), skipping {} (ctag={})",
                    cal.display_name, existing.url, existing.ctag, cal.url, cal.ctag
                );
            }
        } else {
            order.push(key.clone());
            seen.insert(key, cal);
        }
    }

    // Preserve original ordering
    order.into_iter().filter_map(|k| seen.remove(&k)).collect()
}

/// Score a ctag to estimate collection activity.  Higher = more likely to be
/// the "real" collection.
fn ctag_activity_score(ctag: &str) -> u8 {
    if ctag.is_empty() { return 0; }
    // A ctag like "53-1745339646826" (timestamp suffix != 0) means there are
    // real changes. A ctag ending in "-0" typically means empty / never written.
    if let Some(suffix) = ctag.rsplit('-').next() {
        if suffix == "0" { return 1; }
    }
    2
}

/// Discover CalDAV calendars using the RFC 6764 / RFC 4791 three-step flow:
/// 1. HEAD /.well-known/caldav  → follow redirect to find the context path
/// 2. PROPFIND Depth:0          → current-user-principal
/// 3. PROPFIND Depth:0          → calendar-home-set
/// 4. PROPFIND Depth:1          → list calendar collections
/// Returns (home_set_url, calendars, default_calendar_url).
/// The home-set URL is needed for MKCALENDAR.
/// The default_calendar_url (from RFC 6638 schedule-default-calendar-URL) identifies
/// the provider's default calendar independent of display name.
pub async fn discover_calendars(
    base_url: &str,
    username: &str,
    password: &str,
) -> Result<(String, Vec<CalendarInfo>, Option<String>)> {
    log::info!("CalDAV: discovering calendars at {}", base_url);

    let principal_url = find_caldav_principal(base_url, username, password).await?;
    log::info!("CalDAV: principal URL = {}", principal_url);

    let (home_url, default_cal_url) = find_calendar_home_set(&principal_url, username, password).await?;
    log::info!("CalDAV: calendar home set = {}", home_url);

    let raw_cals = list_calendar_collections(&home_url, username, password).await?;
    log::info!("CalDAV: found {} raw calendar(s)", raw_cals.len());
    for c in &raw_cals {
        log::info!("CalDAV:   \"{}\" url={} ctag={}", c.display_name, c.url, c.ctag);
    }

    // Deduplicate calendars with the same display name.
    // Some providers (e.g. Open-Xchange) expose each calendar through both a
    // native path and a CalDAV proxy, resulting in two collections with
    // identical display names.  We keep the one that looks most active.
    let raw_count = raw_cals.len();
    let cals = dedup_calendars_by_name(raw_cals);
    if cals.len() != raw_count {
        log::info!("CalDAV: after dedup: {} calendar(s)", cals.len());
    }

    Ok((home_url, cals, default_cal_url))
}

/// Create a new calendar collection at `{home_set_url}/{slug}/`.
/// Returns the URL of the created collection, or an error.
pub async fn create_calendar(
    home_set_url: &str,
    display_name: &str,
    color: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let slug = slugify(display_name);
    let new_url = format!("{}/{}/", home_set_url.trim_end_matches('/'), slug);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:mkcalendar xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:ic="http://apple.com/ns/ical/">
  <d:set>
    <d:prop>
      <d:displayname>{}</d:displayname>
      <ic:calendar-color>{}</ic:calendar-color>
    </d:prop>
  </d:set>
</c:mkcalendar>"#,
        xml_escape(display_name),
        xml_escape(color),
    );

    let resp = client
        .request(reqwest::Method::from_bytes(b"MKCALENDAR").unwrap(), &new_url)
        .header("Authorization", &auth)
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .context("MKCALENDAR request failed")?;

    let status = resp.status().as_u16();
    if status == 201 || status == 200 {
        log::info!("CalDAV: created calendar \"{}\" at {}", display_name, new_url);
        Ok(new_url)
    } else {
        bail!("MKCALENDAR returned {}", status);
    }
}

/// Locate the CalDAV principal URL for the authenticated user.
/// Tries /.well-known/caldav (following redirects) then the base URL itself.
async fn find_caldav_principal(base_url: &str, username: &str, password: &str) -> Result<String> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    // HEAD /.well-known/caldav — reqwest follows 301/302 redirects and returns the final URL.
    let well_known = format!("{}/.well-known/caldav", base_url.trim_end_matches('/'));
    let context_url = match client.head(&well_known).header("Authorization", &auth).send().await {
        Ok(r) => r.url().to_string(),
        Err(_) => base_url.to_string(),
    };

    // PROPFIND Depth:0 for current-user-principal
    let body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop><d:current-user-principal/></d:prop>
</d:propfind>"#;

    for candidate in &[context_url.as_str(), base_url] {
        let resp = client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), *candidate)
            .header("Authorization", &auth)
            .header("Depth", "0")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body)
            .send()
            .await;

        if let Ok(r) = resp {
            let final_url = r.url().to_string();
            if r.status().as_u16() == 207 {
                let text = r.text().await.unwrap_or_default();
                if let Some(href) = parse_single_href_prop(&text, "current-user-principal") {
                    return Ok(resolve_url(&final_url, &href));
                }
                // 207 but no principal — the context_url itself is likely the principal
                return Ok(final_url);
            }
        }
    }

    // Fallback: use base URL as principal
    Ok(base_url.to_string())
}

/// Find the calendar-home-set href from a principal URL.
/// Returns (calendar_home_set_url, Option<default_calendar_url>).
async fn find_calendar_home_set(principal_url: &str, username: &str, password: &str) -> Result<(String, Option<String>)> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    // Fetch both calendar-home-set and schedule-default-calendar-URL in one PROPFIND.
    // schedule-default-calendar-URL (RFC 6638) is the most reliable way to
    // identify the provider's default calendar, independent of display name.
    let body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <c:calendar-home-set/>
    <c:schedule-default-calendar-URL/>
  </d:prop>
</d:propfind>"#;

    let resp = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), principal_url)
        .header("Authorization", &auth)
        .header("Depth", "0")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .context("CalDAV PROPFIND for calendar-home-set failed")?;

    let final_url = resp.url().to_string();
    let text = resp.text().await?;

    let home_url = if let Some(href) = parse_single_href_prop(&text, "calendar-home-set") {
        resolve_url(&final_url, &href)
    } else {
        principal_url.to_string()
    };

    let default_cal_url = parse_single_href_prop(&text, "schedule-default-calendar-URL")
        .map(|href| resolve_url(&final_url, &href));

    if let Some(ref url) = default_cal_url {
        log::info!("CalDAV: schedule-default-calendar-URL = {}", url);
    }

    Ok((home_url, default_cal_url))
}

/// List all calendar collections under a home-set URL (Depth:1 PROPFIND).
pub async fn list_calendar_collections(home_url: &str, username: &str, password: &str) -> Result<Vec<CalendarInfo>> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let propfind_body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:ic="http://apple.com/ns/ical/">
  <d:prop>
    <d:resourcetype/>
    <d:displayname/>
    <cs:getctag/>
    <ic:calendar-color/>
  </d:prop>
</d:propfind>"#;

    let response = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), home_url)
        .header("Authorization", &auth)
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(propfind_body)
        .send()
        .await
        .context("CalDAV PROPFIND calendar list failed")?;

    let status = response.status();
    if !status.is_success() && status.as_u16() != 207 {
        bail!("CalDAV PROPFIND returned status {}", status);
    }

    let body = response.text().await?;
    parse_calendar_propfind(&body, home_url)
}

/// Extract the first `<d:href>` value nested inside a named property.
/// Works for `current-user-principal`, `calendar-home-set`, `addressbook-home-set`, etc.
fn parse_single_href_prop(xml: &str, prop_name: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut in_target = false;
    let mut in_href = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) | Ok(XmlEvent::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == prop_name { in_target = true; }
                else if in_target && local == "href" { in_href = true; }
            }
            Ok(XmlEvent::Text(ref e)) if in_href => {
                let text = e.unescape().unwrap_or_default().to_string();
                if !text.trim().is_empty() { return Some(text.trim().to_string()); }
            }
            Ok(XmlEvent::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == prop_name { in_target = false; }
                else if local == "href" { in_href = false; }
            }
            Ok(XmlEvent::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

/// Parse PROPFIND multistatus response to extract calendar collections.
fn parse_calendar_propfind(xml: &str, base_url: &str) -> Result<Vec<CalendarInfo>> {
    let mut calendars = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut in_response = false;
    let mut in_prop = false;
    let mut current_href = String::new();
    let mut current_name = String::new();
    let mut current_ctag = String::new();
    let mut current_color = String::new();
    let mut is_calendar = false;
    let mut current_tag = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) | Ok(XmlEvent::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                current_tag = local.clone();

                match local.as_str() {
                    "response" => {
                        in_response = true;
                        current_href.clear();
                        current_name.clear();
                        current_ctag.clear();
                        current_color.clear();
                        is_calendar = false;
                    }
                    "prop" => in_prop = true,
                    "calendar" if in_prop => is_calendar = true,
                    _ => {}
                }
            }
            Ok(XmlEvent::Text(ref e)) if in_response => {
                let text = e.unescape().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "href" => current_href = text,
                    "displayname" if in_prop => current_name = text,
                    "getctag" if in_prop => current_ctag = text,
                    "calendar-color" if in_prop => current_color = text,
                    _ => {}
                }
            }
            Ok(XmlEvent::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "response" => {
                        if in_response && is_calendar && !current_href.is_empty() {
                            let url = resolve_url(base_url, &current_href);
                            let color = normalize_color(&current_color);
                            calendars.push(CalendarInfo {
                                url,
                                display_name: if current_name.is_empty() { "Calendar".to_string() } else { current_name.clone() },
                                color,
                                ctag: current_ctag.clone(),
                            });
                        }
                        in_response = false;
                    }
                    "prop" => in_prop = false,
                    _ => {}
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(e) => {
                log::warn!("CalDAV XML parse error: {}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(calendars)
}

// ── Delta sync via ctag/etag ────────────────────────────

/// Sync a single calendar collection using ctag/etag delta strategy.
///
/// 1. Check if ctag changed (if not, skip entirely)
/// 2. REPORT to get all resource URLs + etags
/// 3. Compare with stored etags to find new/changed/deleted resources
/// 4. Fetch only changed/new iCalendar data
pub async fn sync_calendar(
    calendar_url: &str,
    username: &str,
    password: &str,
    known_etags: &[(String, String, String)],
    known_ctag: &str,
    current_ctag: &str,
) -> Result<(Vec<ParsedEvent>, Vec<ParsedEvent>, Vec<String>)> {
    // If ctag hasn't changed, no sync needed
    if known_ctag == current_ctag && !known_ctag.is_empty() {
        log::info!("CalDAV: calendar ctag unchanged ({}), skipping sync", known_ctag);
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }

    log::info!("CalDAV: syncing calendar {} (ctag {} -> {})", calendar_url, known_ctag, current_ctag);
    let client = build_client()?;
    let auth = basic_auth(username, password);

    // Step 1: REPORT to get all etags
    let report_body = r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag/>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT"/>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#;

    let response = client
        .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), calendar_url)
        .header("Authorization", &auth)
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(report_body)
        .send()
        .await
        .context("CalDAV REPORT failed")?;

    let body = response.text().await?;
    let server_etags = parse_etag_report(&body, calendar_url)?;

    // Step 2: Build known etag map from provided data
    let known_map: std::collections::HashMap<String, (String, String)> = known_etags
        .iter()
        .map(|(url, etag, eid)| (url.clone(), (etag.clone(), eid.clone())))
        .collect();

    let server_map: std::collections::HashMap<String, String> = server_etags
        .iter()
        .map(|(url, etag)| (url.clone(), etag.clone()))
        .collect();

    // Step 3: Determine new, changed, deleted
    let mut to_fetch: Vec<String> = Vec::new();
    let mut is_update: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (url, etag) in &server_etags {
        match known_map.get(url) {
            None => to_fetch.push(url.clone()), // New
            Some((old_etag, _)) if old_etag != etag => {
                to_fetch.push(url.clone()); // Changed
                is_update.insert(url.clone());
            }
            _ => {} // Unchanged
        }
    }

    let deleted: Vec<String> = known_map.keys()
        .filter(|url| !server_map.contains_key(*url))
        .cloned()
        .collect();

    // Step 4: Fetch changed/new resources via calendar-multiget
    let mut new_events = Vec::new();
    let mut updated_events = Vec::new();

    if !to_fetch.is_empty() {
        let events = fetch_events_multiget(calendar_url, username, password, &to_fetch).await?;
        for event in events {
            if is_update.contains(&event.resource_url) {
                updated_events.push(event);
            } else {
                new_events.push(event);
            }
        }
    }

    log::info!(
        "CalDAV: sync result — {} new, {} updated, {} deleted (fetched {})",
        new_events.len(), updated_events.len(), deleted.len(), to_fetch.len()
    );

    Ok((new_events, updated_events, deleted))
}

/// Fetch multiple events at once using calendar-multiget REPORT.
async fn fetch_events_multiget(
    calendar_url: &str,
    username: &str,
    password: &str,
    resource_urls: &[String],
) -> Result<Vec<ParsedEvent>> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    // Build href list for multiget
    let hrefs: String = resource_urls
        .iter()
        .map(|u| format!("<d:href>{}</d:href>", u))
        .collect::<Vec<_>>()
        .join("\n    ");

    let multiget_body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag/>
    <c:calendar-data/>
  </d:prop>
  {}
</c:calendar-multiget>"#,
        hrefs
    );

    let response = client
        .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), calendar_url)
        .header("Authorization", &auth)
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(multiget_body)
        .send()
        .await
        .context("CalDAV multiget failed")?;

    let body = response.text().await?;
    parse_multiget_response(&body, calendar_url)
}

// ── iCalendar parsing ───────────────────────────────────

/// Parse iCalendar data (VCALENDAR/VEVENT) into our event model.
fn parse_ical_event(ical_data: &str, resource_url: &str, etag: &str) -> Option<ParsedEvent> {
    use ical::parser::ical::IcalParser;

    let parser = IcalParser::new(ical_data.as_bytes());
    for calendar in parser {
        let calendar = match calendar {
            Ok(c) => c,
            Err(_) => continue,
        };
        for event in calendar.events {
            let mut uid = String::new();
            let mut summary = String::new();
            let mut dtstart = String::new();
            let mut dtend = String::new();
            let mut location = None;
            let mut description = None;
            let mut is_all_day = false;
            let mut organizer: Option<(String, String)> = None;
            let mut attendees = Vec::new();
            let mut rrule: Option<String> = None;
            let mut exdates: Vec<String> = Vec::new();
            let mut alert_minutes: Option<i32> = None;

            // Parse VALARM from alarms sub-components
            for alarm in &event.alarms {
                for prop in &alarm.properties {
                    if prop.name == "TRIGGER" {
                        if let Some(ref val) = prop.value {
                            alert_minutes = parse_trigger_to_minutes(val);
                        }
                    }
                }
                if alert_minutes.is_some() { break; } // use first alarm only
            }

            for prop in &event.properties {
                match prop.name.as_str() {
                    "UID" => uid = prop.value.clone().unwrap_or_default(),
                    "SUMMARY" => summary = prop.value.clone().unwrap_or_default(),
                    "DTSTART" => {
                        let val = prop.value.clone().unwrap_or_default();
                        // Check if it's a date-only value (all-day event) — by value format or VALUE=DATE param
                        let has_value_date = prop.params.as_ref()
                            .and_then(|p| p.iter().find(|(k, _)| k == "VALUE"))
                            .map(|(_, v)| v.iter().any(|s| s == "DATE"))
                            .unwrap_or(false);
                        if has_value_date || (val.len() == 8 && val.chars().all(|c| c.is_ascii_digit())) {
                            is_all_day = true;
                        }
                        dtstart = normalize_ical_date(&val);
                    }
                    "DTEND" => {
                        let val = prop.value.clone().unwrap_or_default();
                        dtend = normalize_ical_date(&val);
                    }
                    "LOCATION" => location = prop.value.clone().filter(|s| !s.is_empty()),
                    "DESCRIPTION" => description = prop.value.clone().filter(|s| !s.is_empty()),
                    "RRULE" => {
                        rrule = prop.value.clone().filter(|s| !s.is_empty());
                    }
                    "EXDATE" => {
                        if let Some(ref val) = prop.value {
                            for part in val.split(',') {
                                let d = normalize_ical_date(part.trim());
                                if d.len() >= 10 {
                                    exdates.push(d[..10].to_string());
                                }
                            }
                        }
                    }
                    "ORGANIZER" => {
                        let org_email = prop.value.clone().unwrap_or_default()
                            .replace("mailto:", "");
                        let org_name = prop.params.as_ref()
                            .and_then(|p| p.iter().find(|(k, _)| k == "CN"))
                            .map(|(_, v)| v.join(","))
                            .unwrap_or_else(|| org_email.clone());
                        organizer = Some((org_name, org_email));
                    }
                    "ATTENDEE" => {
                        let email = prop.value.clone().unwrap_or_default()
                            .replace("mailto:", "");
                        let name = prop.params.as_ref()
                            .and_then(|p| p.iter().find(|(k, _)| k == "CN"))
                            .map(|(_, v)| v.join(","))
                            .unwrap_or_else(|| email.clone());
                        // ROLE=OPT-PARTICIPANT → optional, everything else → required
                        let role_raw = prop.params.as_ref()
                            .and_then(|p| p.iter().find(|(k, _)| k == "ROLE"))
                            .map(|(_, v)| v.join(","))
                            .unwrap_or_default();
                        let role = if role_raw.to_uppercase() == "OPT-PARTICIPANT" {
                            "optional".to_string()
                        } else {
                            "required".to_string()
                        };
                        attendees.push((name, email, role));
                    }
                    _ => {}
                }
            }

            if !uid.is_empty() {
                return Some(ParsedEvent {
                    uid,
                    summary,
                    dtstart,
                    dtend,
                    location,
                    description,
                    is_all_day,
                    organizer,
                    attendees,
                    rrule,
                    exdates: if exdates.is_empty() { None } else { Some(exdates) },
                    alert_minutes,
                    resource_url: resource_url.to_string(),
                    etag: etag.to_string(),
                });
            }
        }
    }
    None
}

/// Normalize iCalendar date formats to ISO 8601.
fn normalize_ical_date(val: &str) -> String {
    // "20260310T140000Z" → "2026-03-10T14:00:00Z"
    // "20260310" → "2026-03-10T00:00:00"
    let clean = val.trim();
    if clean.len() >= 15 {
        format!(
            "{}-{}-{}T{}:{}:{}{}",
            &clean[0..4], &clean[4..6], &clean[6..8],
            &clean[9..11], &clean[11..13], &clean[13..15],
            if clean.ends_with('Z') { "Z" } else { "" }
        )
    } else if clean.len() >= 8 {
        format!("{}-{}-{}T00:00:00", &clean[0..4], &clean[4..6], &clean[6..8])
    } else {
        clean.to_string()
    }
}

// ── XML response parsers ────────────────────────────────

fn parse_etag_report(xml: &str, base_url: &str) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut in_response = false;
    let mut current_href = String::new();
    let mut current_etag = String::new();
    let mut current_tag = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                current_tag = local.clone();
                if local == "response" {
                    in_response = true;
                    current_href.clear();
                    current_etag.clear();
                }
            }
            Ok(XmlEvent::Text(ref e)) if in_response => {
                let text = e.unescape().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "href" => current_href = text,
                    "getetag" => current_etag = text.trim_matches('"').to_string(),
                    _ => {}
                }
            }
            Ok(XmlEvent::End(ref e)) => {
                if local_name(e.name().as_ref()) == "response" && in_response {
                    if !current_href.is_empty() && !current_etag.is_empty() {
                        let url = resolve_url(base_url, &current_href);
                        results.push((url, current_etag.clone()));
                    }
                    in_response = false;
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(results)
}

fn parse_multiget_response(xml: &str, base_url: &str) -> Result<Vec<ParsedEvent>> {
    let mut events = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut in_response = false;
    let mut current_href = String::new();
    let mut current_etag = String::new();
    let mut current_cal_data = String::new();
    let mut current_tag = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                current_tag = local.clone();
                if local == "response" {
                    in_response = true;
                    current_href.clear();
                    current_etag.clear();
                    current_cal_data.clear();
                }
            }
            Ok(XmlEvent::Text(ref e)) if in_response => {
                let text = e.unescape().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "href" => current_href = text,
                    "getetag" => current_etag = text.trim_matches('"').to_string(),
                    "calendar-data" => current_cal_data = text,
                    _ => {}
                }
            }
            Ok(XmlEvent::End(ref e)) => {
                if local_name(e.name().as_ref()) == "response" && in_response {
                    if !current_cal_data.is_empty() {
                        let url = resolve_url(base_url, &current_href);
                        if let Some(evt) = parse_ical_event(&current_cal_data, &url, &current_etag) {
                            events.push(evt);
                        }
                    }
                    in_response = false;
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(events)
}

// ── Event creation/update via PUT ───────────────────────

/// Create or update a calendar event on the server via PUT.
pub async fn put_event(
    calendar_url: &str,
    username: &str,
    password: &str,
    event: &ParsedEvent,
) -> Result<String> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let resource_url = if event.resource_url.is_empty() {
        format!("{}{}.ics", calendar_url.trim_end_matches('/'), format!("/{}", event.uid))
    } else {
        event.resource_url.clone()
    };

    let ical = build_ical_event(event);

    let req = client
        .put(&resource_url)
        .header("Authorization", &auth)
        .header("Content-Type", "text/calendar; charset=utf-8")
        .body(ical);

    // No If-Match — unconditional overwrite. ETags are only useful for
    // multi-user conflict detection; for a single-user app they cause stale-ETag
    // 409/412 errors when the server's stored ETag diverges from our cached copy.

    let response = req.send().await.context("CalDAV PUT failed")?;

    if !response.status().is_success() && response.status().as_u16() != 201 && response.status().as_u16() != 204 {
        bail!("CalDAV PUT returned {}", response.status());
    }

    // Return the new etag from the response
    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim_matches('"')
        .to_string();

    Ok(new_etag)
}

/// Delete a calendar event on the server via DELETE.
pub async fn delete_event(
    resource_url: &str,
    username: &str,
    password: &str,
    _etag: &str,
) -> Result<()> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let req = client
        .delete(resource_url)
        .header("Authorization", &auth);

    // No If-Match — unconditional delete, same reasoning as PUT.

    let response = req.send().await.context("CalDAV DELETE failed")?;

    if !response.status().is_success() && response.status().as_u16() != 204 {
        bail!("CalDAV DELETE returned {}", response.status());
    }

    Ok(())
}

/// Build an iCalendar VCALENDAR/VEVENT string from our event model.
fn build_ical_event(event: &ParsedEvent) -> String {
    let mut ical = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Mail//CalDAV//EN\r\nBEGIN:VEVENT\r\n");
    ical.push_str(&format!("UID:{}\r\n", event.uid));
    ical.push_str(&format!("SUMMARY:{}\r\n", event.summary));

    if event.is_all_day {
        let ds = event.dtstart.replace("-", "").replace("T00:00:00", "");
        let de = event.dtend.replace("-", "").replace("T00:00:00", "").replace("T23:59:59", "");
        ical.push_str(&format!("DTSTART;VALUE=DATE:{}\r\n", &ds[..8.min(ds.len())]));
        if !de.is_empty() {
            ical.push_str(&format!("DTEND;VALUE=DATE:{}\r\n", &de[..8.min(de.len())]));
        }
    } else {
        let ds = compact_ical_datetime(&event.dtstart);
        let de = compact_ical_datetime(&event.dtend);
        ical.push_str(&format!("DTSTART:{}\r\n", ds));
        ical.push_str(&format!("DTEND:{}\r\n", de));
    }

    if let Some(ref loc) = event.location {
        ical.push_str(&format!("LOCATION:{}\r\n", loc));
    }
    if let Some(ref desc) = event.description {
        ical.push_str(&format!("DESCRIPTION:{}\r\n", desc));
    }
    if let Some((ref org_name, ref org_email)) = event.organizer {
        ical.push_str(&format!("ORGANIZER;CN={}:mailto:{}\r\n", org_name, org_email));
    }
    for (name, email, role) in &event.attendees {
        let role_param = if role == "optional" { "OPT-PARTICIPANT" } else { "REQ-PARTICIPANT" };
        ical.push_str(&format!("ATTENDEE;ROLE={};PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN={}:mailto:{}\r\n", role_param, name, email));
    }
    if let Some(ref rule) = event.rrule {
        if !rule.is_empty() {
            ical.push_str(&format!("RRULE:{}\r\n", rule));
        }
    }
    if let Some(ref exdates) = event.exdates {
        for exd in exdates {
            let compact = exd.replace('-', "");
            if event.is_all_day {
                ical.push_str(&format!("EXDATE;VALUE=DATE:{}\r\n", compact));
            } else {
                ical.push_str(&format!("EXDATE:{}T000000Z\r\n", compact));
            }
        }
    }
    if let Some(mins) = event.alert_minutes {
        ical.push_str("BEGIN:VALARM\r\n");
        if mins == 0 {
            ical.push_str("TRIGGER:PT0S\r\n");
        } else {
            ical.push_str(&format!("TRIGGER:-PT{}M\r\n", mins));
        }
        ical.push_str("ACTION:DISPLAY\r\nDESCRIPTION:Reminder\r\nEND:VALARM\r\n");
    }
    ical.push_str(&format!("DTSTAMP:{}Z\r\n", chrono::Utc::now().format("%Y%m%dT%H%M%S")));
    ical.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    ical
}

/// Compact an ISO datetime string to iCalendar format, stripping sub-second precision.
/// "2026-03-30T14:00:00.000Z" → "20260330T140000Z"
fn compact_ical_datetime(s: &str) -> String {
    let s = s.replace('-', "").replace(':', "");
    if let Some(dot) = s.find('.') {
        let tz = s[dot + 1..].find(|c: char| c == 'Z' || c == '+' || c == '-')
            .map(|p| dot + 1 + p)
            .unwrap_or(s.len());
        format!("{}{}", &s[..dot], &s[tz..])
    } else {
        s
    }
}

// ── Utilities ───────────────────────────────────────────

/// Convert a display name to a URL-safe slug.
fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Escape special XML characters.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Get the local name from a potentially namespaced XML tag.
fn local_name(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name).to_string();
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

/// Resolve a potentially relative href against a base URL.
fn resolve_url(base_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if let Ok(base) = url::Url::parse(base_url) {
        if let Ok(resolved) = base.join(href) {
            return resolved.to_string();
        }
    }
    // Fallback: simple concatenation
    format!("{}{}", base_url.trim_end_matches('/'), href)
}

/// Parse an iCalendar TRIGGER duration (e.g. `-PT15M`, `PT1H`, `PT0S`, `-P1D`)
/// into the app's `alert_minutes` convention: **positive = minutes before the
/// event** (see `eventAlerts.ts`, which subtracts `alertMinutes * 60000` from
/// the event start). A leading `-` on the RFC 5545 duration means "before",
/// so it maps to a positive value; unsigned / `+` means "after" and maps to
/// a negative value.
fn parse_trigger_to_minutes(val: &str) -> Option<i32> {
    let s = val.trim();
    let negative = s.starts_with('-');
    let s = s.trim_start_matches('-').trim_start_matches('+');
    if !s.starts_with('P') { return None; }
    let s = &s[1..]; // strip 'P'
    let mut total = 0i32;
    let mut num_buf = String::new();
    let mut in_time = false;
    for c in s.chars() {
        if c == 'T' { in_time = true; continue; }
        if c.is_ascii_digit() { num_buf.push(c); continue; }
        let n: i32 = num_buf.parse().unwrap_or(0);
        num_buf.clear();
        match c {
            'D' => total += n * 1440,
            'H' if in_time => total += n * 60,
            'M' if in_time => total += n,
            'S' if in_time => {} // ignore seconds
            'W' => total += n * 10080,
            _ => {}
        }
    }
    Some(if negative { total } else { -total })
}

/// Normalize CalDAV color values (may be #RRGGBBAA or other formats).
fn normalize_color(color: &str) -> String {
    let c = color.trim();
    if c.is_empty() {
        return "#0078d4".to_string();
    }
    // Strip alpha channel if present (#RRGGBBAA → #RRGGBB)
    if c.len() == 9 && c.starts_with('#') {
        return c[..7].to_string();
    }
    c.to_string()
}

/// Test CalDAV connection. Returns Ok(()) if authentication succeeds.
pub async fn test_connection(url: &str, username: &str, password: &str) -> Result<()> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let response = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url)
        .header("Authorization", &auth)
        .header("Depth", "0")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/></d:prop></d:propfind>"#)
        .send()
        .await
        .context("CalDAV connection failed")?;

    if response.status().is_success() || response.status().as_u16() == 207 {
        Ok(())
    } else {
        bail!("CalDAV returned status {}", response.status())
    }
}

// ── Public wrappers for DAV server ──────────────────────

pub fn parse_ical_event_public(ical_data: &str, resource_url: &str, etag: &str) -> Option<ParsedEvent> {
    parse_ical_event(ical_data, resource_url, etag)
}

pub fn build_ical_event_public(event: &ParsedEvent) -> String {
    build_ical_event(event)
}

/// Build an iMIP invitation ICS with METHOD:REQUEST for sending via email.
pub fn build_imip_invitation(event: &ParsedEvent) -> String {
    let base = build_ical_event(event);
    // Insert METHOD:REQUEST after PRODID line
    base.replace(
        "BEGIN:VEVENT\r\n",
        "METHOD:REQUEST\r\nBEGIN:VEVENT\r\n",
    )
}
