//! autodiscover.rs — Auto-discover mail server settings.
//!
//! Implements the standard discovery chain:
//!   1. Mozilla ISPDB (Thunderbird autoconfig database)
//!   2. Domain-hosted autoconfig XML
//!   3. Microsoft Autodiscover (Exchange / Outlook)
//!   4. DNS SRV records (RFC 6186)
//!   5. Common port probing as last resort
//!
//! Returns discovered IMAP/SMTP settings so the user doesn't have to
//! type server names and ports manually.

use anyhow::{Result, bail};
use quick_xml::events::Event;
use quick_xml::Reader;

/// Discovered server configuration for a single protocol endpoint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    pub protocol: String,       // "imap", "pop3", "smtp"
    pub hostname: String,
    pub port: u16,
    pub socket_type: String,    // "SSL", "STARTTLS", "plain"
    pub auth: String,           // "password-cleartext", "OAuth2", etc.
    pub username_template: String, // "%EMAILADDRESS%", "%EMAILLOCALPART%", etc.
}

/// Full discovery result with incoming + outgoing servers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredConfig {
    pub display_name: String,
    pub incoming: Vec<ServerConfig>,
    pub outgoing: Vec<ServerConfig>,
    pub source: String,         // "mozilla-ispdb", "autoconfig", "autodiscover", "srv", "probe"
}

/// Run the full discovery chain for the given email address.
/// Tries each method in order and returns the first success.
pub async fn discover(email: &str) -> Result<DiscoveredConfig> {
    let (_local, domain) = split_email(email)?;

    // 1. Mozilla ISPDB
    if let Ok(config) = mozilla_ispdb(domain).await {
        return Ok(config);
    }

    // 2. Domain-hosted autoconfig
    if let Ok(config) = domain_autoconfig(domain).await {
        return Ok(config);
    }

    // 3. Well-known autoconfig (RFC 6186 style path)
    if let Ok(config) = wellknown_autoconfig(domain).await {
        return Ok(config);
    }

    // 4. Microsoft Autodiscover
    if let Ok(config) = ms_autodiscover(domain, email).await {
        return Ok(config);
    }

    // 5. DNS SRV records (RFC 6186)
    if let Ok(config) = dns_srv_lookup(domain).await {
        return Ok(config);
    }

    // 6. Common port probing
    if let Ok(config) = probe_common_ports(domain).await {
        return Ok(config);
    }

    bail!("Could not auto-discover mail settings for {}", email)
}

// ── Helpers ─────────────────────────────────────────────

fn split_email(email: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = email.rsplitn(2, '@').collect();
    if parts.len() != 2 {
        bail!("Invalid email address: {}", email);
    }
    Ok((parts[1], parts[0]))
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(crate::identity::http_user_agent())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

// ── 1. Mozilla ISPDB ────────────────────────────────────

/// Query Mozilla's Thunderbird ISPDB.
/// URL: https://autoconfig.thunderbird.net/v1.1/{domain}
async fn mozilla_ispdb(domain: &str) -> Result<DiscoveredConfig> {
    let url = format!("https://autoconfig.thunderbird.net/v1.1/{}", domain);
    let resp = http_client().get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("ISPDB returned {}", resp.status());
    }
    let xml = resp.text().await?;
    let mut config = parse_autoconfig_xml(&xml)?;
    config.source = "mozilla-ispdb".to_string();
    Ok(config)
}

// ── 2. Domain-hosted autoconfig ─────────────────────────

/// Try https://autoconfig.{domain}/mail/config-v1.1.xml
async fn domain_autoconfig(domain: &str) -> Result<DiscoveredConfig> {
    let url = format!("https://autoconfig.{}/mail/config-v1.1.xml", domain);
    let resp = http_client().get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("Domain autoconfig returned {}", resp.status());
    }
    let xml = resp.text().await?;
    let mut config = parse_autoconfig_xml(&xml)?;
    config.source = "autoconfig".to_string();
    Ok(config)
}

// ── 3. Well-known autoconfig path ───────────────────────

/// Try https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml
async fn wellknown_autoconfig(domain: &str) -> Result<DiscoveredConfig> {
    let url = format!(
        "https://{}/.well-known/autoconfig/mail/config-v1.1.xml",
        domain
    );
    let resp = http_client().get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("Well-known autoconfig returned {}", resp.status());
    }
    let xml = resp.text().await?;
    let mut config = parse_autoconfig_xml(&xml)?;
    config.source = "wellknown-autoconfig".to_string();
    Ok(config)
}

// ── 4. Microsoft Autodiscover ───────────────────────────

/// Try Microsoft Autodiscover v2 (JSON) — used by Outlook.com, Office 365, Exchange Online.
/// URL: https://outlook.office365.com/autodiscover/autodiscover.json/v1.0/{email}?Protocol=...
async fn ms_autodiscover(domain: &str, email: &str) -> Result<DiscoveredConfig> {
    // Try the well-known Office 365 endpoint first
    let urls = [
        format!(
            "https://outlook.office365.com/autodiscover/autodiscover.json/v1.0/{}?Protocol=Imap",
            email
        ),
        format!(
            "https://autodiscover.{}/autodiscover/autodiscover.json/v1.0/{}?Protocol=Imap",
            domain, email
        ),
    ];

    let client = http_client();

    for url in &urls {
        if let Ok(resp) = client.get(url).send().await {
            if resp.status().is_success() {
                if let Ok(body) = resp.text().await {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        if let Some(config) = parse_ms_autodiscover_json(&json, domain) {
                            return Ok(config);
                        }
                    }
                }
            }
        }
    }

    // Try classic Autodiscover XML (POX) as fallback
    let pox_urls = [
        format!("https://{}/autodiscover/autodiscover.xml", domain),
        format!("https://autodiscover.{}/autodiscover/autodiscover.xml", domain),
    ];

    let pox_body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/requestschema/2006">
  <Request>
    <EMailAddress>{}</EMailAddress>
    <AcceptableResponseSchema>http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a</AcceptableResponseSchema>
  </Request>
</Autodiscover>"#,
        email
    );

    for url in &pox_urls {
        if let Ok(resp) = client
            .post(url)
            .header("Content-Type", "text/xml")
            .body(pox_body.clone())
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(xml) = resp.text().await {
                    if let Ok(config) = parse_ms_autodiscover_pox(&xml, domain) {
                        return Ok(config);
                    }
                }
            }
        }
    }

    bail!("Microsoft Autodiscover failed for {}", domain)
}

fn parse_ms_autodiscover_json(json: &serde_json::Value, domain: &str) -> Option<DiscoveredConfig> {
    let server = json.get("Server")?.as_str()?;
    let port = json.get("Port")?.as_u64()? as u16;
    let ssl = json.get("SSL")?.as_str().unwrap_or("on");

    let socket_type = if ssl == "on" { "SSL" } else { "STARTTLS" };

    Some(DiscoveredConfig {
        display_name: domain.to_string(),
        incoming: vec![ServerConfig {
            protocol: "imap".to_string(),
            hostname: server.to_string(),
            port,
            socket_type: socket_type.to_string(),
            auth: "password-cleartext".to_string(),
            username_template: "%EMAILADDRESS%".to_string(),
        }],
        outgoing: vec![ServerConfig {
            protocol: "smtp".to_string(),
            hostname: format!("smtp.{}", domain),
            port: 587,
            socket_type: "STARTTLS".to_string(),
            auth: "password-cleartext".to_string(),
            username_template: "%EMAILADDRESS%".to_string(),
        }],
        source: "autodiscover-json".to_string(),
    })
}

fn parse_ms_autodiscover_pox(xml: &str, domain: &str) -> Result<DiscoveredConfig> {
    // Minimal POX parsing — look for IMAP and SMTP protocol blocks
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_protocol = false;
    let mut current_type = String::new();
    let mut current_server = String::new();
    let mut current_port: u16 = 0;
    let mut current_ssl = String::new();
    let mut current_tag = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "Protocol" || name.ends_with(":Protocol") {
                    in_protocol = true;
                    current_type.clear();
                    current_server.clear();
                    current_port = 0;
                    current_ssl.clear();
                }
                current_tag = name;
            }
            Ok(Event::Text(ref e)) if in_protocol => {
                let text = e.unescape().unwrap_or_default().to_string();
                if current_tag == "Type" || current_tag.ends_with(":Type") {
                    current_type = text;
                } else if current_tag == "Server" || current_tag.ends_with(":Server") {
                    current_server = text;
                } else if current_tag == "Port" || current_tag.ends_with(":Port") {
                    current_port = text.parse().unwrap_or(0);
                } else if current_tag == "SSL" || current_tag.ends_with(":SSL") {
                    current_ssl = text;
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if (name == "Protocol" || name.ends_with(":Protocol")) && in_protocol {
                    in_protocol = false;
                    let socket_type = if current_ssl == "on" { "SSL" } else { "STARTTLS" };
                    let sc = ServerConfig {
                        protocol: current_type.to_lowercase(),
                        hostname: current_server.clone(),
                        port: current_port,
                        socket_type: socket_type.to_string(),
                        auth: "password-cleartext".to_string(),
                        username_template: "%EMAILADDRESS%".to_string(),
                    };
                    match current_type.as_str() {
                        "IMAP" | "POP3" => incoming.push(sc),
                        "SMTP" => outgoing.push(sc),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    if incoming.is_empty() && outgoing.is_empty() {
        bail!("No protocols found in Autodiscover POX response");
    }

    Ok(DiscoveredConfig {
        display_name: domain.to_string(),
        incoming,
        outgoing,
        source: "autodiscover-pox".to_string(),
    })
}

// ── 5. DNS SRV lookup (RFC 6186) ────────────────────────

/// Look up DNS SRV records for standard mail service names.
///
/// RFC 6186 defines:
///   _imaps._tcp.{domain}      → IMAP over TLS (port 993)
///   _imap._tcp.{domain}       → IMAP + STARTTLS (port 143)
///   _pop3s._tcp.{domain}      → POP3 over TLS (port 995)
///   _submission._tcp.{domain}  → SMTP submission (port 587)
///   _submissions._tcp.{domain} → SMTP submission over TLS (port 465)
///
/// We resolve these ourselves using a simple TXT/SRV approach via the OS
/// resolver. Since Rust's std doesn't expose SRV, we shell out to
/// `nslookup` on Windows or parse `/etc/resolv.conf` on Unix.
/// For simplicity, we use a quick tokio DNS lookup to test connectivity.
async fn dns_srv_lookup(domain: &str) -> Result<DiscoveredConfig> {
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();

    // Try IMAPS first (preferred)
    let srv_records = [
        ("_imaps._tcp", "imap", 993u16, "SSL"),
        ("_imap._tcp", "imap", 143, "STARTTLS"),
        ("_pop3s._tcp", "pop3", 995, "SSL"),
        ("_submissions._tcp", "smtp", 465, "SSL"),
        ("_submission._tcp", "smtp", 587, "STARTTLS"),
    ];

    for (prefix, protocol, default_port, socket_type) in &srv_records {
        let srv_name = format!("{}.{}", prefix, domain);
        // Use tokio DNS to resolve — will work if the SRV target has an A record
        // This is a simplification; a full SRV parser would extract priority/weight/port
        if let Ok(addrs) = tokio::net::lookup_host(format!("{}:{}", srv_name, default_port)).await {
            let _: Vec<_> = addrs.collect(); // just check it resolves
            let sc = ServerConfig {
                protocol: protocol.to_string(),
                hostname: srv_name,
                port: *default_port,
                socket_type: socket_type.to_string(),
                auth: "password-cleartext".to_string(),
                username_template: "%EMAILADDRESS%".to_string(),
            };
            match *protocol {
                "imap" | "pop3" => incoming.push(sc),
                "smtp" => outgoing.push(sc),
                _ => {}
            }
        }
    }

    if incoming.is_empty() && outgoing.is_empty() {
        bail!("No SRV records found for {}", domain);
    }

    Ok(DiscoveredConfig {
        display_name: domain.to_string(),
        incoming,
        outgoing,
        source: "srv".to_string(),
    })
}

// ── 6. Common port probing ──────────────────────────────

/// As a last resort, try connecting to common server names and ports.
/// Tests: imap.{domain}:993, mail.{domain}:993, smtp.{domain}:587, etc.
async fn probe_common_ports(domain: &str) -> Result<DiscoveredConfig> {
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();

    let incoming_probes = [
        (format!("imap.{}", domain), 993u16, "imap", "SSL"),
        (format!("mail.{}", domain), 993, "imap", "SSL"),
        (domain.to_string(), 993, "imap", "SSL"),
        (format!("imap.{}", domain), 143, "imap", "STARTTLS"),
        (format!("pop.{}", domain), 995, "pop3", "SSL"),
        (format!("pop3.{}", domain), 995, "pop3", "SSL"),
    ];

    let outgoing_probes = [
        (format!("smtp.{}", domain), 587u16, "smtp", "STARTTLS"),
        (format!("mail.{}", domain), 587, "smtp", "STARTTLS"),
        (format!("smtp.{}", domain), 465, "smtp", "SSL"),
        (domain.to_string(), 587, "smtp", "STARTTLS"),
    ];

    let timeout = std::time::Duration::from_secs(4);

    // Probe incoming servers
    for (host, port, protocol, socket_type) in &incoming_probes {
        if incoming.is_empty() {
            if let Ok(Ok(_)) =
                tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host.as_str(), *port)))
                    .await
            {
                incoming.push(ServerConfig {
                    protocol: protocol.to_string(),
                    hostname: host.clone(),
                    port: *port,
                    socket_type: socket_type.to_string(),
                    auth: "password-cleartext".to_string(),
                    username_template: "%EMAILADDRESS%".to_string(),
                });
            }
        }
    }

    // Probe outgoing servers
    for (host, port, protocol, socket_type) in &outgoing_probes {
        if outgoing.is_empty() {
            if let Ok(Ok(_)) =
                tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host.as_str(), *port)))
                    .await
            {
                outgoing.push(ServerConfig {
                    protocol: protocol.to_string(),
                    hostname: host.clone(),
                    port: *port,
                    socket_type: socket_type.to_string(),
                    auth: "password-cleartext".to_string(),
                    username_template: "%EMAILADDRESS%".to_string(),
                });
            }
        }
    }

    if incoming.is_empty() && outgoing.is_empty() {
        bail!("Port probing found no reachable mail servers for {}", domain);
    }

    Ok(DiscoveredConfig {
        display_name: domain.to_string(),
        incoming,
        outgoing,
        source: "probe".to_string(),
    })
}

// ── Autoconfig XML parser (Mozilla & domain-hosted) ─────

/// Parse the standard autoconfig XML format used by Mozilla ISPDB
/// and domain-hosted autoconfig files.
///
/// Example structure:
/// ```xml
/// <clientConfig version="1.1">
///   <emailProvider id="gmail.com">
///     <displayName>Google Mail</displayName>
///     <incomingServer type="imap">
///       <hostname>imap.gmail.com</hostname>
///       <port>993</port>
///       <socketType>SSL</socketType>
///       <authentication>OAuth2</authentication>
///       <username>%EMAILADDRESS%</username>
///     </incomingServer>
///     <outgoingServer type="smtp">
///       <hostname>smtp.gmail.com</hostname>
///       <port>465</port>
///       <socketType>SSL</socketType>
///       <authentication>OAuth2</authentication>
///       <username>%EMAILADDRESS%</username>
///     </outgoingServer>
///   </emailProvider>
/// </clientConfig>
/// ```
fn parse_autoconfig_xml(xml: &str) -> Result<DiscoveredConfig> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut display_name = String::new();
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();

    // Parser state
    let mut in_incoming = false;
    let mut in_outgoing = false;
    let mut server_type = String::new(); // "imap", "pop3", "smtp"
    let mut current_tag = String::new();

    let mut srv_hostname = String::new();
    let mut srv_port: u16 = 0;
    let mut srv_socket_type = String::new();
    let mut srv_auth = String::new();
    let mut srv_username = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                match local.as_str() {
                    "incomingServer" => {
                        in_incoming = true;
                        server_type = extract_attr(e, "type").unwrap_or_default();
                        srv_hostname.clear();
                        srv_port = 0;
                        srv_socket_type.clear();
                        srv_auth.clear();
                        srv_username.clear();
                    }
                    "outgoingServer" => {
                        in_outgoing = true;
                        server_type = "smtp".to_string();
                        srv_hostname.clear();
                        srv_port = 0;
                        srv_socket_type.clear();
                        srv_auth.clear();
                        srv_username.clear();
                    }
                    _ => {}
                }
                current_tag = local;
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_incoming || in_outgoing {
                    match current_tag.as_str() {
                        "hostname" => srv_hostname = text,
                        "port" => srv_port = text.parse().unwrap_or(0),
                        "socketType" => srv_socket_type = text,
                        "authentication" => srv_auth = text,
                        "username" => srv_username = text,
                        _ => {}
                    }
                } else if current_tag == "displayName" && display_name.is_empty() {
                    display_name = text;
                }
            }
            Ok(Event::End(ref e)) => {
                let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if local == "incomingServer" && in_incoming {
                    in_incoming = false;
                    incoming.push(ServerConfig {
                        protocol: server_type.clone(),
                        hostname: srv_hostname.clone(),
                        port: srv_port,
                        socket_type: srv_socket_type.clone(),
                        auth: srv_auth.clone(),
                        username_template: srv_username.clone(),
                    });
                } else if local == "outgoingServer" && in_outgoing {
                    in_outgoing = false;
                    outgoing.push(ServerConfig {
                        protocol: "smtp".to_string(),
                        hostname: srv_hostname.clone(),
                        port: srv_port,
                        socket_type: srv_socket_type.clone(),
                        auth: srv_auth.clone(),
                        username_template: srv_username.clone(),
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    if incoming.is_empty() && outgoing.is_empty() {
        bail!("No server entries found in autoconfig XML");
    }

    Ok(DiscoveredConfig {
        display_name,
        incoming,
        outgoing,
        source: String::new(), // caller sets this
    })
}

/// Extract an attribute value from an XML start element by name.
fn extract_attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == name.as_bytes() {
            return Some(String::from_utf8_lossy(&attr.value).to_string());
        }
    }
    None
}

/// Expand a username template: replace %EMAILADDRESS%, %EMAILLOCALPART%, %EMAILDOMAIN%.
///
/// Currently unused — `ServerConfig.username_template` is parsed from autoconfig
/// XML but not yet consumed when converting to `MailServerSettings`. Providers
/// that default to `%EMAILADDRESS%` (most) work by accident; providers using
/// `%EMAILLOCALPART%` (e.g. Fastmail variants) will fail login until this is
/// wired into the settings conversion.
#[allow(dead_code)]
pub fn expand_username(template: &str, email: &str) -> String {
    let (local, domain) = if let Some(at) = email.rfind('@') {
        (&email[..at], &email[at + 1..])
    } else {
        (email, "")
    };

    template
        .replace("%EMAILADDRESS%", email)
        .replace("%EMAILLOCALPART%", local)
        .replace("%EMAILDOMAIN%", domain)
}
