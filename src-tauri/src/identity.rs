//! Client identity strings sent to mail / DAV servers.
//!
//! One place to bump product name, vendor, or support URL. Version is read
//! from Cargo.toml at compile time so `cargo set-version` flows through.

pub const PRODUCT_NAME: &str = "Cortexist Simple Mail";
pub const PRODUCT_TOKEN: &str = "CortexistSimpleMail";
pub const BUNDLE_ID: &str = "com.cortexist.simple-mail";
pub const VENDOR: &str = "Cortexist, LLC";
pub const SUPPORT_URL: &str = "https://github.com/cortexist/simple-mail";

pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `CortexistSimpleMail/0.8.2 (+https://cortexist.com; com.cortexist.simple-mail)`
pub fn http_user_agent() -> String {
    format!("{}/{} (+{}; {})", PRODUCT_TOKEN, version(), SUPPORT_URL, BUNDLE_ID)
}

/// `Cortexist Simple Mail 0.8.2` — for outgoing mail's User-Agent: header.
pub fn mail_user_agent() -> String {
    format!("{} {}", PRODUCT_NAME, version())
}

/// Key/value pairs for IMAP ID (RFC 2971). Strings only; server echoes back.
pub fn imap_id_fields() -> Vec<(&'static str, String)> {
    vec![
        ("name",        PRODUCT_NAME.to_string()),
        ("version",     version().to_string()),
        ("vendor",      VENDOR.to_string()),
        ("support-url", SUPPORT_URL.to_string()),
        ("os",          std::env::consts::OS.to_string()),
    ]
}
