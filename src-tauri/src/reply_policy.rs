//! reply_policy.rs — Can a reply to this message reach anyone?
//!
//! Replyability is decided per message in a fixed order: a model verdict
//! (decision model, else language model — see `ai_service::judge_replyability`)
//! when the model was confident either way, and otherwise the address pattern
//! here. A model only disables the reply buttons on a very confident "no": a
//! false positive hides Reply from a real person, a false negative merely lets
//! a reply bounce.
//!
//! The pattern judges the **effective reply address** — Reply-To when present,
//! else From — because that is where a reply goes. Real mail depends on this:
//! `noreply@github.com` routinely carries `Reply-To: support@github.com`, and
//! A bank's `no_reply@alerts.bank.example` sets a per-message `reply-…@` address.
//!
//! The matcher works on the local part split at `.`, `_`, `-` and `+`, and
//! looks for a no-reply phrase as whole tokens. Token matching (rather than a
//! substring test for "reply") is what keeps `reply-…@alerts.bank.example`,
//! `aws-marketing-email-replies@…` and `ebay@reply.ebay.com` replyable while
//! still catching `portal.no-reply`, `googleplay-noreply`, `no.reply.alerts`,
//! `do-not-reply-message-center` and `no-reply-<random>`.

/// Token runs that mark a mailbox as not accepting replies. Each entry is a
/// sequence of consecutive local-part tokens.
const NO_REPLY_PHRASES: &[&[&str]] = &[
    &["noreply"],
    &["no", "reply"],
    &["donotreply"],
    &["do", "not", "reply"],
    &["donot", "reply"],
    &["dont", "reply"],
    &["notreply"],
    &["noresponse"],
    &["no", "response"],
    &["mailer", "daemon"],
];

/// Where a reply to this message is addressed: Reply-To when set, else From.
/// Reply-To may be a bare address or `Name <addr>`.
pub fn effective_reply_address<'a>(from_email: &'a str, reply_to: &'a str) -> &'a str {
    let rt = reply_to.trim();
    if rt.is_empty() {
        return from_email.trim();
    }
    match (rt.rfind('<'), rt.rfind('>')) {
        (Some(open), Some(close)) if open < close => rt[open + 1..close].trim(),
        _ => rt,
    }
}

/// True when `address`'s local part names an unmonitored no-reply mailbox.
pub fn is_no_reply_address(address: &str) -> bool {
    let Some((local, _domain)) = address.rsplit_once('@') else { return false };
    let local = local.to_ascii_lowercase();
    let tokens: Vec<&str> = local
        .split(|c: char| matches!(c, '.' | '_' | '-' | '+'))
        .filter(|t| !t.is_empty())
        .collect();
    NO_REPLY_PHRASES
        .iter()
        .any(|phrase| tokens.windows(phrase.len()).any(|w| w == *phrase))
}

/// Pattern-only verdict for a message, used when no model verdict is stored.
pub fn pattern_says_no_reply(from_email: &str, reply_to: &str) -> bool {
    is_no_reply_address(effective_reply_address(from_email, reply_to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_no_reply_senders_seen_in_real_mail() {
        for addr in [
            "noreply@medium.com",
            "no-reply@email.claude.com",
            "no_reply@alerts.bank.example",
            "no.reply.alerts@bank.example",
            "do-not-reply@certs.example",
            "donotreply@example.com",
            "community-no-reply@vendor.example",
            "googleplay-noreply@google.com",
            "noreply-dmarc-support@google.com",
            "Portal.No-Reply@agency.example",
            "sa.noreply@example.com",
            "account-security-noreply@accountprotection.microsoft.com",
            "do-not-reply-message-center@example.com",
            "no-reply-abcdefghijklmnopqrstu@docs.google.com",
            "MAILER-DAEMON@mx.example.com",
        ] {
            assert!(is_no_reply_address(addr), "{addr} should be no-reply");
        }
    }

    #[test]
    fn leaves_replyable_lookalikes_alone() {
        for addr in [
            "reply-a1b2c3d4e5f60718-16_HTML-100000001-200000002-3000@alerts.bank.example",
            "aws-marketing-email-replies@amazon.com",
            "replies@example.com",
            "ebay@reply.ebay.com",
            "noreplyguy@example.com",
            "support@github.com",
            "postmaster@example.com",
            "shaw.tan@cortexist.com",
            "not-an-address",
        ] {
            assert!(!is_no_reply_address(addr), "{addr} should be replyable");
        }
    }

    #[test]
    fn a_real_reply_to_rescues_a_no_reply_sender() {
        assert!(!pattern_says_no_reply("noreply@github.com", "support@github.com"));
        assert!(!pattern_says_no_reply("no-reply@stamps.com", "person@vendor.example"));
        assert!(!pattern_says_no_reply(
            "no_reply@alerts.bank.example",
            "reply-QWERTYUIOPASDFGHJKLZXCVBNM.100001@alerts.bank.example",
        ));
    }

    #[test]
    fn a_no_reply_reply_to_is_still_no_reply() {
        assert!(pattern_says_no_reply("noreply@medium.com", "noreply@medium.com"));
        assert!(pattern_says_no_reply("noreply@account.pinterest.com", "noreply@reply.pinterest.com"));
        assert!(pattern_says_no_reply("alerts@example.com", "Example <no-reply@example.com>"));
        assert!(pattern_says_no_reply("noreply@example.com", ""));
    }

    #[test]
    fn effective_address_parses_display_form() {
        assert_eq!(effective_reply_address("a@x.com", ""), "a@x.com");
        assert_eq!(effective_reply_address("a@x.com", "b@y.com"), "b@y.com");
        assert_eq!(effective_reply_address("a@x.com", "Bee <b@y.com>"), "b@y.com");
    }
}
