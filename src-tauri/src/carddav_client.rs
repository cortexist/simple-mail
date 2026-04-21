//! carddav_client.rs — CardDAV sync engine for contacts.
//!
//! Design goals:
//!   • Discover address books via PROPFIND on the principal URL
//!   • Use ctag (collection tag) to detect if anything changed
//!   • Use etag per resource for fine-grained delta sync
//!   • Parse vCard (.vcf) into our Contact model
//!   • Create/update/delete contacts on the server

use anyhow::{Result, Context, bail};
use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;
use reqwest::Client;
use serde::Serialize;

// ── Public types ────────────────────────────────────────

/// A discovered CardDAV address book.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressBookInfo {
    pub url: String,
    pub display_name: String,
    pub ctag: String,
    pub read_only: bool,
}

/// A parsed contact from a vCard resource.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedContact {
    pub uid: String,
    pub full_name: String,
    pub first_name: String,
    pub last_name: String,
    pub middle_name: String,
    pub prefix: String,
    pub suffix: String,
    pub emails: Vec<ContactEmail>,
    pub phones: Vec<ContactPhone>,
    pub addresses: Vec<ContactAddress>,
    pub organization: String,
    pub department: Option<String>,
    pub title: String,
    pub birthday: Option<String>,
    pub photo_url: Option<String>,
    pub rev: Option<String>,
    pub resource_url: String,
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactEmail {
    pub email: String,
    #[serde(default)]
    pub label: String, // "home", "work", "other"
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactPhone {
    pub number: String,
    #[serde(default)]
    pub label: String, // "home", "work", "cell", "other"
    #[serde(default)]
    pub subtypes: Vec<String>, // "voice", "text", "fax", "video", "pager"
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContactAddress {
    #[serde(default)]
    pub street: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub postal_code: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub label: String, // "home", "work", "other"
    #[serde(default)]
    pub is_default: bool,
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
/// Download a remote photo URL and return it as a `data:image/...;base64,...` URI.
/// Returns `None` if the download fails or the response isn't an image.
pub async fn download_photo_as_data_uri(url: &str) -> Option<String> {
    use base64::Engine;
    let client = build_client().ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();
    // Only allow image types
    if !content_type.starts_with("image/") {
        return None;
    }
    let mime = content_type.split(';').next().unwrap_or("image/jpeg").trim();
    let bytes = resp.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{b64}"))
}
// ── Address book discovery ──────────────────────────────

/// Discover CardDAV address books using the RFC 6764 / RFC 6352 three-step flow:
/// 1. HEAD /.well-known/carddav → follow redirect to context path
/// 2. PROPFIND Depth:0          → current-user-principal
/// 3. PROPFIND Depth:0          → addressbook-home-set
/// 4. PROPFIND Depth:1          → list address book collections
/// Returns (home_set_url, address_books).  The home-set URL is needed for MKCOL.
pub async fn discover_address_books(
    base_url: &str,
    username: &str,
    password: &str,
) -> Result<(String, Vec<AddressBookInfo>)> {
    log::info!("CardDAV: discovering address books at {}", base_url);

    let principal_url = find_carddav_principal(base_url, username, password).await?;
    log::info!("CardDAV: principal URL = {}", principal_url);

    let home_url = find_addressbook_home_set(&principal_url, username, password).await?;
    log::info!("CardDAV: addressbook home set = {}", home_url);

    let books = list_addressbook_collections(&home_url, username, password).await?;
    log::info!("CardDAV: found {} address book(s)", books.len());
    for b in &books {
        log::info!("CardDAV:   \"{}\" url={} ctag={} read_only={}", b.display_name, b.url, b.ctag, b.read_only);
    }
    Ok((home_url, books))
}

/// Create a new CardDAV address book collection at `{home_set_url}/{slug}/`.
pub async fn create_address_book(
    home_set_url: &str,
    display_name: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let slug = slugify(display_name);
    let new_url = format!("{}/{}/", home_set_url.trim_end_matches('/'), slug);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:mkcol xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:set>
    <d:prop>
      <d:resourcetype>
        <d:collection/>
        <card:addressbook/>
      </d:resourcetype>
      <d:displayname>{}</d:displayname>
    </d:prop>
  </d:set>
</d:mkcol>"#,
        xml_escape(display_name),
    );

    let resp = client
        .request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), &new_url)
        .header("Authorization", &auth)
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .context("MKCOL request failed")?;

    let status = resp.status().as_u16();
    if status == 201 || status == 200 {
        log::info!("CardDAV: created address book \"{}\" at {}", display_name, new_url);
        Ok(new_url)
    } else {
        bail!("MKCOL returned {}", status);
    }
}

async fn find_carddav_principal(base_url: &str, username: &str, password: &str) -> Result<String> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let well_known = format!("{}/.well-known/carddav", base_url.trim_end_matches('/'));
    let context_url = match client.head(&well_known).header("Authorization", &auth).send().await {
        Ok(r) => r.url().to_string(),
        Err(_) => base_url.to_string(),
    };

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
                return Ok(final_url);
            }
        }
    }

    Ok(base_url.to_string())
}

async fn find_addressbook_home_set(principal_url: &str, username: &str, password: &str) -> Result<String> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:prop><card:addressbook-home-set/></d:prop>
</d:propfind>"#;

    let resp = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), principal_url)
        .header("Authorization", &auth)
        .header("Depth", "0")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .context("CardDAV PROPFIND for addressbook-home-set failed")?;

    let final_url = resp.url().to_string();
    let text = resp.text().await?;
    if let Some(href) = parse_single_href_prop(&text, "addressbook-home-set") {
        return Ok(resolve_url(&final_url, &href));
    }

    Ok(principal_url.to_string())
}

async fn list_addressbook_collections(home_url: &str, username: &str, password: &str) -> Result<Vec<AddressBookInfo>> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let propfind_body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cs="http://calendarserver.org/ns/">
  <d:prop>
    <d:resourcetype/>
    <d:displayname/>
    <cs:getctag/>
    <d:current-user-privilege-set/>
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
        .context("CardDAV PROPFIND address book list failed")?;

    let status = response.status();
    if !status.is_success() && status.as_u16() != 207 {
        bail!("CardDAV PROPFIND returned status {}", status);
    }

    let body = response.text().await?;
    log::debug!("CardDAV: address book PROPFIND response:\n{}", body);
    parse_addressbook_propfind(&body, home_url)
}

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

/// Parse PROPFIND multistatus to extract address book collections.
fn parse_addressbook_propfind(xml: &str, base_url: &str) -> Result<Vec<AddressBookInfo>> {
    let mut books = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut in_response = false;
    let mut in_prop = false;
    let mut current_href = String::new();
    let mut current_name = String::new();
    let mut current_ctag = String::new();
    let mut is_addressbook = false;
    let mut has_write_privilege = false;
    let mut has_bind_privilege = false;
    let mut in_privilege_set = false;
    let mut saw_privilege_set = false;
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
                        is_addressbook = false;
                        has_write_privilege = false;
                        has_bind_privilege = false;
                        in_privilege_set = false;
                        saw_privilege_set = false;
                    }
                    "prop" => in_prop = true,
                    "addressbook" if in_prop => is_addressbook = true,
                    "current-user-privilege-set" if in_prop => {
                        in_privilege_set = true;
                        saw_privilege_set = true;
                    }
                    // RFC 3744 privileges
                    "write" | "write-content" if in_privilege_set => has_write_privilege = true,
                    "bind" if in_privilege_set => has_bind_privilege = true,
                    _ => {}
                }
            }
            Ok(XmlEvent::Text(ref e)) if in_response => {
                let text = e.unescape().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "href" => current_href = text,
                    "displayname" if in_prop => current_name = text,
                    "getctag" if in_prop => current_ctag = text,
                    _ => {}
                }
            }
            Ok(XmlEvent::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "current-user-privilege-set" => in_privilege_set = false,
                    "response" => {
                        if in_response && is_addressbook && !current_href.is_empty() {
                            let url = resolve_url(base_url, &current_href);
                            // If the server returned current-user-privilege-set, check for
                            // write + bind (add new resources).  Missing bind means the
                            // collection is effectively read-only (e.g. Global address book).
                            // If the server didn't return the property at all, assume writable.
                            let read_only = saw_privilege_set && (!has_write_privilege || !has_bind_privilege);
                            books.push(AddressBookInfo {
                                url,
                                display_name: if current_name.is_empty() { "Contacts".to_string() } else { current_name.clone() },
                                ctag: current_ctag.clone(),
                                read_only,
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
                log::warn!("CardDAV XML parse error: {}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(books)
}

// ── Delta sync via ctag/etag ────────────────────────────

/// Sync a single address book using ctag/etag delta strategy.
pub async fn sync_address_book(
    book_url: &str,
    username: &str,
    password: &str,
    known_etags: &[(String, String, String)],
    known_ctag: &str,
    current_ctag: &str,
) -> Result<(Vec<ParsedContact>, Vec<ParsedContact>, Vec<String>)> {
    // If ctag hasn't changed, skip
    if known_ctag == current_ctag && !known_ctag.is_empty() {
        log::info!("CardDAV: address book ctag unchanged ({}), skipping sync", known_ctag);
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }

    log::info!("CardDAV: syncing address book {} (ctag {} -> {})", book_url, known_ctag, current_ctag);
    let client = build_client()?;
    let auth = basic_auth(username, password);

    // REPORT to get all etags
    let report_body = r#"<?xml version="1.0" encoding="utf-8"?>
<card:addressbook-query xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:prop>
    <d:getetag/>
  </d:prop>
</card:addressbook-query>"#;

    let response = client
        .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), book_url)
        .header("Authorization", &auth)
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(report_body)
        .send()
        .await
        .context("CardDAV REPORT failed")?;

    let body = response.text().await?;
    let server_etags = parse_etag_report(&body, book_url)?;

    // Build known map from provided etags
    let known_map: std::collections::HashMap<String, (String, String)> = known_etags
        .iter()
        .map(|(url, etag, cid)| (url.clone(), (etag.clone(), cid.clone())))
        .collect();

    let server_map: std::collections::HashMap<String, String> = server_etags
        .iter()
        .map(|(url, etag)| (url.clone(), etag.clone()))
        .collect();

    // Determine changes
    let mut to_fetch: Vec<String> = Vec::new();
    let mut is_update: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (url, etag) in &server_etags {
        match known_map.get(url) {
            None => to_fetch.push(url.clone()),
            Some((old_etag, _)) if old_etag != etag => {
                to_fetch.push(url.clone());
                is_update.insert(url.clone());
            }
            _ => {}
        }
    }

    let deleted: Vec<String> = known_map.keys()
        .filter(|url| !server_map.contains_key(*url))
        .cloned()
        .collect();

    // Fetch changed/new
    let mut new_contacts = Vec::new();
    let mut updated_contacts = Vec::new();

    if !to_fetch.is_empty() {
        let contacts = fetch_contacts_multiget(book_url, username, password, &to_fetch).await?;
        for contact in contacts {
            if is_update.contains(&contact.resource_url) {
                updated_contacts.push(contact);
            } else {
                new_contacts.push(contact);
            }
        }
    }

    log::info!(
        "CardDAV: sync result — {} new, {} updated, {} deleted (fetched {})",
        new_contacts.len(), updated_contacts.len(), deleted.len(), to_fetch.len()
    );

    Ok((new_contacts, updated_contacts, deleted))
}

/// Fetch multiple contacts via addressbook-multiget REPORT.
async fn fetch_contacts_multiget(
    book_url: &str,
    username: &str,
    password: &str,
    resource_urls: &[String],
) -> Result<Vec<ParsedContact>> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let hrefs: String = resource_urls
        .iter()
        .map(|u| format!("<d:href>{}</d:href>", u))
        .collect::<Vec<_>>()
        .join("\n    ");

    let multiget_body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<card:addressbook-multiget xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav">
  <d:prop>
    <d:getetag/>
    <card:address-data/>
  </d:prop>
  {}
</card:addressbook-multiget>"#,
        hrefs
    );

    let response = client
        .request(reqwest::Method::from_bytes(b"REPORT").unwrap(), book_url)
        .header("Authorization", &auth)
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(multiget_body)
        .send()
        .await
        .context("CardDAV multiget failed")?;

    let body = response.text().await?;
    parse_contact_multiget_response(&body, book_url)
}

// ── vCard parsing ───────────────────────────────────────

/// Parse a vCard string into our contact model.
fn parse_vcard(vcard_data: &str, resource_url: &str, etag: &str) -> Option<ParsedContact> {
    let mut uid = String::new();
    let mut full_name = String::new();
    let mut first_name = String::new();
    let mut last_name = String::new();
    let mut middle_name = String::new();
    let mut prefix = String::new();
    let mut suffix = String::new();
    let mut organization = String::new();
    let mut department: Option<String> = None;
    let mut title = String::new();
    let mut addresses: Vec<ContactAddress> = Vec::new();
    let mut birthday: Option<String> = None;
    let mut photo_url: Option<String> = None;
    let mut rev: Option<String> = None;
    let mut emails: Vec<ContactEmail> = Vec::new();
    let mut phones: Vec<ContactPhone> = Vec::new();

    // Simple line-by-line vCard parser since vcard_parser may fail on
    // non-standard cards. We handle the most common properties.
    for line in unfold_vcard_lines(vcard_data) {
        let line = line.trim();
        if line.is_empty() { continue; }

        if let Some((prop_with_params, value)) = line.split_once(':') {
            let (prop_name, params) = if let Some((name, p)) = prop_with_params.split_once(';') {
                (name.to_uppercase(), p.to_string())
            } else {
                (prop_with_params.to_uppercase(), String::new())
            };

            match prop_name.as_str() {
                "UID" => uid = value.to_string(),
                "FN" => full_name = value.to_string(),
                "N" => {
                    // N:Last;First;Middle;Prefix;Suffix
                    let parts: Vec<&str> = value.split(';').collect();
                    if parts.len() >= 1 { last_name = parts[0].to_string(); }
                    if parts.len() >= 2 { first_name = parts[1].to_string(); }
                    if parts.len() >= 3 { middle_name = parts[2].to_string(); }
                    if parts.len() >= 4 { prefix = parts[3].to_string(); }
                    if parts.len() >= 5 { suffix = parts[4].to_string(); }
                }
                "ORG" => {
                    let parts: Vec<&str> = value.splitn(2, ';').collect();
                    organization = parts[0].to_string();
                    if parts.len() > 1 && !parts[1].is_empty() {
                        department = Some(parts[1].to_string());
                    }
                }
                "TITLE" => title = value.to_string(),
                "ADR" => {
                    // ADR:PO Box;Extended;Street;City;Region;Postal;Country
                    let parts: Vec<&str> = value.split(';').collect();
                    let get = |i: usize| parts.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
                    let street = {
                        let po = get(0);
                        let ext = get(1);
                        let st = get(2);
                        [po, ext, st].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
                    };
                    let types = extract_all_types(&params);
                    let label = pick_location_label(&types);
                    let is_default = types.iter().any(|t| t == "pref");
                    let adr = ContactAddress {
                        street,
                        city: get(3),
                        region: get(4),
                        postal_code: get(5),
                        country: get(6),
                        label,
                        is_default,
                    };
                    let is_empty = adr.street.is_empty() && adr.city.is_empty() && adr.region.is_empty()
                        && adr.postal_code.is_empty() && adr.country.is_empty();
                    if !is_empty {
                        addresses.push(adr);
                    }
                }
                "BDAY" => {
                    if !value.is_empty() {
                        birthday = Some(value.to_string());
                    }
                }
                "REV" => {
                    if !value.is_empty() {
                        rev = Some(value.to_string());
                    }
                }
                "EMAIL" => {
                    let types = extract_all_types(&params);
                    let label = pick_location_label(&types);
                    let is_default = types.iter().any(|t| t == "pref");
                    emails.push(ContactEmail {
                        email: value.to_string(),
                        label,
                        is_default,
                    });
                }
                "TEL" => {
                    let types = extract_all_types(&params);
                    let label = pick_phone_label(&types);
                    let subtypes: Vec<String> = types.iter()
                        .filter(|t| matches!(t.as_str(), "voice" | "text" | "fax" | "video" | "pager"))
                        .cloned()
                        .collect();
                    let is_default = types.iter().any(|t| t == "pref");
                    phones.push(ContactPhone {
                        number: value.to_string(),
                        label,
                        subtypes,
                        is_default,
                    });
                }
                "PHOTO" => {
                    if value.starts_with("http://") || value.starts_with("https://") {
                        photo_url = Some(value.to_string());
                    } else if !value.is_empty() {
                        // Inline base64 photo — detect encoding/type from params
                        let params_upper = params.to_uppercase();
                        let mime = if params_upper.contains("JPEG") || params_upper.contains("JPG") {
                            "image/jpeg"
                        } else if params_upper.contains("PNG") {
                            "image/png"
                        } else if params_upper.contains("GIF") {
                            "image/gif"
                        } else {
                            "image/jpeg" // default assumption
                        };
                        if params_upper.contains("BASE64") || params_upper.contains("ENCODING=B") {
                            photo_url = Some(format!("data:{};base64,{}", mime, value));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Unescape vCard values (RFC 6350 §3.4: \, \; \n \\ are escape sequences)
    fn vcard_unescape(s: &str) -> String {
        s.replace("\\,", ",")
         .replace("\\;", ";")
         .replace("\\n", "\n")
         .replace("\\N", "\n")
         .replace("\\\\", "\\")
    }

    full_name = vcard_unescape(&full_name);
    first_name = vcard_unescape(&first_name);
    last_name = vcard_unescape(&last_name);
    middle_name = vcard_unescape(&middle_name);
    prefix = vcard_unescape(&prefix);
    suffix = vcard_unescape(&suffix);
    organization = vcard_unescape(&organization);
    title = vcard_unescape(&title);
    if let Some(ref d) = department { department = Some(vcard_unescape(d)); }
    for e in &mut emails { e.email = vcard_unescape(&e.email); }
    for p in &mut phones { p.number = vcard_unescape(&p.number); }
    for a in &mut addresses {
        a.street = vcard_unescape(&a.street);
        a.city = vcard_unescape(&a.city);
        a.region = vcard_unescape(&a.region);
        a.postal_code = vcard_unescape(&a.postal_code);
        a.country = vcard_unescape(&a.country);
    }

    // Ensure exactly one default per collection (first entry wins if none marked)
    fn ensure_default<T, F: Fn(&T) -> bool, S: Fn(&mut T, bool)>(items: &mut [T], is_def: F, set: S) {
        if items.is_empty() { return; }
        if !items.iter().any(|i| is_def(i)) {
            set(&mut items[0], true);
        }
    }
    ensure_default(&mut emails, |e| e.is_default, |e, v| e.is_default = v);
    ensure_default(&mut phones, |p| p.is_default, |p, v| p.is_default = v);
    ensure_default(&mut addresses, |a| a.is_default, |a, v| a.is_default = v);

    // Generate UID if not present
    if uid.is_empty() {
        uid = uuid::Uuid::new_v4().to_string();
    }

    if full_name.is_empty() && first_name.is_empty() && last_name.is_empty() {
        // Skip contacts with no name at all
        if emails.is_empty() {
            return None;
        }
        full_name = emails[0].email.clone();
    }

    if full_name.is_empty() {
        full_name = format!("{} {}", first_name, last_name).trim().to_string();
    }

    Some(ParsedContact {
        uid,
        full_name,
        first_name,
        last_name,
        middle_name,
        prefix,
        suffix,
        emails,
        phones,
        addresses,
        organization,
        department,
        title,
        birthday,
        photo_url,
        rev,
        resource_url: resource_url.to_string(),
        etag: etag.to_string(),
    })
}

/// Unfold vCard lines (RFC 6350: lines starting with space/tab are continuations).
fn unfold_vcard_lines(data: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for line in data.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation of previous line
            current.push_str(line[1..].into());
        } else {
            if !current.is_empty() {
                lines.push(current);
            }
            current = line.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Extract all TYPE parameter values from vCard property params as a flat lowercased list.
/// Handles both `TYPE=HOME,VOICE` (comma-joined) and `TYPE=HOME;TYPE=VOICE` (repeated).
fn extract_all_types(params: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for param in params.split(';') {
        let p = param.trim();
        if p.is_empty() { continue; }
        let val = if let Some(v) = p.strip_prefix("TYPE=").or_else(|| p.strip_prefix("type=")) {
            v
        } else {
            let lower = p.to_lowercase();
            if matches!(lower.as_str(), "home" | "work" | "cell" | "fax" | "pref"
                | "voice" | "text" | "video" | "pager" | "other") {
                out.push(lower);
            }
            continue;
        };
        for part in val.split(',') {
            let t = part.trim().to_lowercase();
            if !t.is_empty() {
                out.push(t);
            }
        }
    }
    out
}

/// Pick the location label (home/work/other) from a type list.
fn pick_location_label(types: &[String]) -> String {
    for t in types {
        match t.as_str() {
            "home" | "work" => return t.clone(),
            _ => {}
        }
    }
    "other".to_string()
}

/// Pick the phone label (home/work/cell/other) from a type list.
fn pick_phone_label(types: &[String]) -> String {
    for t in types {
        match t.as_str() {
            "home" | "work" | "cell" => return t.clone(),
            _ => {}
        }
    }
    "other".to_string()
}

// ── Contact creation/update/delete ──────────────────────

/// Create or update a contact on the server via PUT.
pub async fn put_contact(
    book_url: &str,
    username: &str,
    password: &str,
    contact: &ParsedContact,
) -> Result<String> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let resource_url = if contact.resource_url.is_empty() {
        format!("{}/{}.vcf", book_url.trim_end_matches('/'), contact.uid)
    } else {
        contact.resource_url.clone()
    };

    let vcard = build_vcard(contact);

    let mut req = client
        .put(&resource_url)
        .header("Authorization", &auth)
        .header("Content-Type", "text/vcard; charset=utf-8")
        .body(vcard);

    if !contact.etag.is_empty() {
        req = req.header("If-Match", format!("\"{}\"", contact.etag));
    }

    let response = req.send().await.context("CardDAV PUT failed")?;

    if !response.status().is_success() && response.status().as_u16() != 201 && response.status().as_u16() != 204 {
        bail!("CardDAV PUT returned {}", response.status());
    }

    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim_matches('"')
        .to_string();

    Ok(new_etag)
}

/// Delete a contact from the server.
pub async fn delete_contact(
    resource_url: &str,
    username: &str,
    password: &str,
    etag: &str,
) -> Result<()> {
    let client = build_client()?;
    let auth = basic_auth(username, password);

    let mut req = client
        .delete(resource_url)
        .header("Authorization", &auth);

    if !etag.is_empty() {
        req = req.header("If-Match", format!("\"{}\"", etag));
    }

    let response = req.send().await.context("CardDAV DELETE failed")?;

    if !response.status().is_success() && response.status().as_u16() != 204 {
        bail!("CardDAV DELETE returned {}", response.status());
    }

    Ok(())
}

/// Build a vCard 3.0 string from our contact model.
fn build_vcard(contact: &ParsedContact) -> String {
    let mut vcard = String::from("BEGIN:VCARD\r\nVERSION:3.0\r\n");
    vcard.push_str(&format!("UID:{}\r\n", contact.uid));
    vcard.push_str(&format!("FN:{}\r\n", contact.full_name));
    vcard.push_str(&format!("N:{};{};{};{};{}\r\n",
        contact.last_name, contact.first_name, contact.middle_name,
        contact.prefix, contact.suffix));

    if !contact.organization.is_empty() {
        let dept = contact.department.as_deref().unwrap_or("");
        if dept.is_empty() {
            vcard.push_str(&format!("ORG:{}\r\n", contact.organization));
        } else {
            vcard.push_str(&format!("ORG:{};{}\r\n", contact.organization, dept));
        }
    }
    if !contact.title.is_empty() {
        vcard.push_str(&format!("TITLE:{}\r\n", contact.title));
    }
    for adr in &contact.addresses {
        let mut types: Vec<&str> = Vec::new();
        let label = if adr.label.is_empty() { "other" } else { adr.label.as_str() };
        types.push(label);
        if adr.is_default { types.push("pref"); }
        let esc = |s: &str| s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,");
        vcard.push_str(&format!(
            "ADR;TYPE={}:;;{};{};{};{};{}\r\n",
            types.join(","),
            esc(&adr.street),
            esc(&adr.city),
            esc(&adr.region),
            esc(&adr.postal_code),
            esc(&adr.country),
        ));
    }
    if let Some(ref bday) = contact.birthday {
        if !bday.is_empty() {
            vcard.push_str(&format!("BDAY:{}\r\n", bday));
        }
    }

    for email in &contact.emails {
        let mut types: Vec<&str> = Vec::new();
        let label = if email.label.is_empty() { "other" } else { email.label.as_str() };
        types.push(label);
        if email.is_default { types.push("pref"); }
        vcard.push_str(&format!("EMAIL;TYPE={}:{}\r\n", types.join(","), email.email));
    }
    for phone in &contact.phones {
        let mut types: Vec<String> = Vec::new();
        let label = if phone.label.is_empty() { "other".to_string() } else { phone.label.clone() };
        types.push(label);
        for st in &phone.subtypes {
            if !st.is_empty() { types.push(st.clone()); }
        }
        if phone.is_default { types.push("pref".to_string()); }
        vcard.push_str(&format!("TEL;TYPE={}:{}\r\n", types.join(","), phone.number));
    }

    if let Some(ref photo) = contact.photo_url {
        if photo.starts_with("http") {
            vcard.push_str(&format!("PHOTO;VALUE=uri:{}\r\n", photo));
        } else if let Some(b64) = photo.strip_prefix("data:image/jpeg;base64,") {
            vcard.push_str(&format!("PHOTO;ENCODING=b;TYPE=JPEG:{}\r\n", b64));
        } else if let Some(b64) = photo.strip_prefix("data:image/png;base64,") {
            vcard.push_str(&format!("PHOTO;ENCODING=b;TYPE=PNG:{}\r\n", b64));
        }
    }

    vcard.push_str(&format!("REV:{}Z\r\n", chrono::Utc::now().format("%Y%m%dT%H%M%S")));
    vcard.push_str("END:VCARD\r\n");
    vcard
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

fn parse_contact_multiget_response(xml: &str, base_url: &str) -> Result<Vec<ParsedContact>> {
    let mut contacts = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);  // Don't trim — vCard data has meaningful whitespace

    let mut in_response = false;
    let mut current_href = String::new();
    let mut current_etag = String::new();
    let mut current_vcard_data = String::new();
    let mut current_tag = String::new();
    let mut in_address_data = false;
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
                    current_vcard_data.clear();
                } else if local == "address-data" && in_response {
                    in_address_data = true;
                    current_vcard_data.clear();
                }
            }
            Ok(XmlEvent::Text(ref e)) if in_response => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_address_data {
                    current_vcard_data.push_str(&text);
                } else {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        match current_tag.as_str() {
                            "href" => current_href = trimmed.to_string(),
                            "getetag" => current_etag = trimmed.trim_matches('"').to_string(),
                            _ => {}
                        }
                    }
                }
            }
            Ok(XmlEvent::CData(ref e)) if in_response && in_address_data => {
                let text = String::from_utf8_lossy(e.as_ref()).to_string();
                current_vcard_data.push_str(&text);
            }
            Ok(XmlEvent::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == "address-data" {
                    in_address_data = false;
                } else if local == "response" && in_response {
                    if !current_vcard_data.is_empty() {
                        let url = resolve_url(base_url, &current_href);
                        if let Some(contact) = parse_vcard(&current_vcard_data, &url, &current_etag) {
                            contacts.push(contact);
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

    Ok(contacts)
}

// ── Utilities ───────────────────────────────────────────

fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn local_name(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name).to_string();
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

fn resolve_url(base_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if let Ok(base) = url::Url::parse(base_url) {
        if let Ok(resolved) = base.join(href) {
            return resolved.to_string();
        }
    }
    format!("{}{}", base_url.trim_end_matches('/'), href)
}

/// Test CardDAV connection.
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
        .context("CardDAV connection failed")?;

    if response.status().is_success() || response.status().as_u16() == 207 {
        Ok(())
    } else {
        bail!("CardDAV returned status {}", response.status())
    }
}

// ── Public wrappers for DAV server ──────────────────────

pub fn parse_vcard_public(vcard_data: &str, resource_url: &str, etag: &str) -> Option<ParsedContact> {
    parse_vcard(vcard_data, resource_url, etag)
}

pub fn build_vcard_public(contact: &ParsedContact) -> String {
    build_vcard(contact)
}
