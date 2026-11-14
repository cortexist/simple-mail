//! The text free-text search runs over, derived from a message's HTML body.
//!
//! This is a port of `buildSearchText` in `src/lib/searchQuery.ts`, and it has
//! to produce byte-identical output: the frontend indexes bodies it fetches
//! later with the JS version, and search must behave the same for a message
//! whichever side stripped it. Each step below mirrors one `.replace()` in the
//! JS, applied in the same order — order matters, because the entity steps run
//! sequentially (`&amp;lt;` decodes to `&lt;` and then to `<`).
//!
//! It lives here so `query_emails` can ship this instead of the body. The body
//! was only ever sent so the frontend could strip it into this on load; sending
//! the stripped text directly cuts the startup payload by the markup and lets
//! bodies stay in SQLite until a message is opened.

/// Build the lowercased, HTML-stripped search text for `html`.
pub fn build(html: &str) -> String {
    if html.is_empty() {
        return String::new();
    }
    let s = strip_blocks(html, "style");
    let s = strip_blocks(&s, "script");
    let s = strip_tags(&s);
    let s = replace_ci(&s, "&nbsp;", " ");
    let s = replace_ci(&s, "&amp;", "&");
    let s = replace_ci(&s, "&lt;", "<");
    let s = replace_ci(&s, "&gt;", ">");
    let s = replace_ci(&s, "&quot;", "\"");
    let s = decode_numeric_entities(&s);
    collapse_whitespace(&s).to_lowercase()
}

/// `<name[^>]*>[\s\S]*?</name>` (case-insensitive) → a single space. A block
/// with no closing tag is left alone, exactly as the JS regex leaves it; the
/// open tag then falls to `strip_tags` and the block's text survives.
fn strip_blocks(input: &str, name: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;

    while let Some(rel) = lower[pos..].find(&open) {
        let start = pos + rel;
        // `[^>]*>` — the open tag ends at the next `>`; if there is none, the
        // pattern cannot match anywhere from here on.
        let Some(tag_end_rel) = lower[start..].find('>') else { break };
        let after_tag = start + tag_end_rel + 1;
        let Some(close_rel) = lower[after_tag..].find(&close) else {
            // No closing tag: not a match. Emit this `<` and keep looking.
            out.push_str(&input[pos..start + 1]);
            pos = start + 1;
            continue;
        };
        out.push_str(&input[pos..start]);
        out.push(' ');
        pos = after_tag + close_rel + close.len();
    }
    out.push_str(&input[pos..]);
    out
}

/// `<[^>]+>` → a single space. A `<` with no later `>` — or `<>` with nothing
/// between — is not a tag and is kept.
fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;

    while let Some(rel) = input[pos..].find('<') {
        let start = pos + rel;
        out.push_str(&input[pos..start]);
        match input[start + 1..].find('>') {
            Some(0) => {
                // `<>`: no match. Both characters stay.
                out.push_str("<>");
                pos = start + 2;
            }
            Some(gap) => {
                out.push(' ');
                pos = start + 1 + gap + 1;
            }
            None => {
                out.push_str(&input[start..]);
                return out;
            }
        }
    }
    out.push_str(&input[pos..]);
    out
}

/// Case-insensitive literal replace, for the named entities (`/&nbsp;/gi`).
fn replace_ci(input: &str, needle: &str, with: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    while let Some(rel) = lower[pos..].find(needle) {
        let at = pos + rel;
        out.push_str(&input[pos..at]);
        out.push_str(with);
        pos = at + needle.len();
    }
    out.push_str(&input[pos..]);
    out
}

/// `&#(\d+);` → `String.fromCharCode(parseInt(d, 10))`. `fromCharCode` takes a
/// UTF-16 code unit, so the value is truncated to 16 bits the way JS does it; a
/// lone surrogate has no `char`, so it becomes U+FFFD.
fn decode_numeric_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pos = 0;
    while let Some(rel) = input[pos..].find("&#") {
        let at = pos + rel;
        let digits_start = at + 2;
        let digits_len = input[digits_start..]
            .bytes()
            .take_while(|b| b.is_ascii_digit())
            .count();
        let after_digits = digits_start + digits_len;
        if digits_len == 0 || input.as_bytes().get(after_digits) != Some(&b';') {
            out.push_str(&input[pos..digits_start]);
            pos = digits_start;
            continue;
        }
        out.push_str(&input[pos..at]);
        // parseInt on a long digit run saturates toward Infinity; fromCharCode
        // of that is code unit 0. Use wrapping arithmetic on the parsed value
        // and let overflow map the same way.
        let value = input[digits_start..after_digits]
            .parse::<u128>()
            .map(|v| (v & 0xFFFF) as u32)
            .unwrap_or(0);
        out.push(char::from_u32(value).unwrap_or('\u{FFFD}'));
        pos = after_digits + 1;
    }
    out.push_str(&input[pos..]);
    out
}

/// JS `\s`: Unicode White_Space plus U+FEFF, minus U+0085.
fn is_js_whitespace(c: char) -> bool {
    c == '\u{FEFF}' || (c.is_whitespace() && c != '\u{85}')
}

/// `\s+` → one space, then `trim()`.
fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_space = false;
    for c in input.chars() {
        if is_js_whitespace(c) {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out.trim_matches(is_js_whitespace).to_string()
}

#[cfg(test)]
mod tests {
    use super::build;

    #[test]
    fn empty_in_empty_out() {
        assert_eq!(build(""), "");
    }

    #[test]
    fn strips_tags_and_lowercases() {
        assert_eq!(build("<p>Hello <b>World</b></p>"), "hello world");
    }

    #[test]
    fn drops_style_and_script_blocks_with_their_contents() {
        assert_eq!(
            build("<STYLE type=\"x\">p{color:red}</Style>Visible<script>alert(1)</script>"),
            "visible"
        );
    }

    #[test]
    fn an_unclosed_style_block_keeps_its_text() {
        // The JS regex needs `</style>` to match; without it only the open tag
        // is removed and the CSS survives as text.
        assert_eq!(build("<style>p{a:b}"), "p{a:b}");
    }

    #[test]
    fn decodes_entities_in_order() {
        assert_eq!(build("a&nbsp;b &amp;amp; &lt;x&gt; &quot;q&quot;"), "a b &amp; <x> \"q\"");
        // `&amp;lt;` becomes `&lt;` and then `<`, because the steps run in sequence.
        assert_eq!(build("&amp;lt;"), "<");
    }

    #[test]
    fn decodes_numeric_entities_like_from_char_code() {
        assert_eq!(build("&#65;&#66;"), "ab");
        assert_eq!(build("&#8217;s"), "\u{2019}s");
        // Truncated to 16 bits, as fromCharCode does.
        assert_eq!(build("&#65601;"), "a");
        // Not an entity: no digits, or no terminating semicolon.
        assert_eq!(build("&#;&#12"), "&#;&#12");
    }

    /// `<[^>]+>` is greedy up to the next `>` wherever it is, so a stray `<`
    /// followed later by any `>` swallows everything between. Only a `<` with
    /// no `>` after it, or an empty `<>`, survives. These five are the JS
    /// implementation's own answers.
    #[test]
    fn angle_brackets_match_the_js_regex_exactly() {
        assert_eq!(build("1 < 2 and <> stays"), "1 stays");
        assert_eq!(build("<>"), "<>");
        assert_eq!(build("1 < 2"), "1 < 2");
        assert_eq!(build("a <b"), "a <b");
        assert_eq!(build("<> x <> y"), "<> x <> y");
    }

    #[test]
    fn collapses_all_whitespace_and_trims() {
        assert_eq!(build("  a\n\n\tb\u{A0}\u{FEFF}c  "), "a b c");
    }

    /// Byte-for-byte agreement with the JS implementation over a real corpus.
    /// Run `node scripts/dump-search-text.mjs > corpus.json` first and point
    /// `SIMPLE_MAIL_SEARCH_TEXT_CORPUS` at the file.
    #[test]
    #[ignore]
    fn matches_the_js_implementation_over_the_corpus() {
        let path = std::env::var("SIMPLE_MAIL_SEARCH_TEXT_CORPUS")
            .expect("set SIMPLE_MAIL_SEARCH_TEXT_CORPUS to the dumped corpus JSON");
        let data = std::fs::read_to_string(&path).unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&data).unwrap();
        let mut mismatches = 0;
        for row in &rows {
            let id = row["id"].as_str().unwrap();
            let body = row["body"].as_str().unwrap();
            let expected = row["expected"].as_str().unwrap();
            let actual = build(body);
            if actual != expected {
                mismatches += 1;
                let at = actual.bytes().zip(expected.bytes()).position(|(a, b)| a != b)
                    .unwrap_or(actual.len().min(expected.len()));
                eprintln!(
                    "MISMATCH {id} at byte {at}\n   rust: {:?}\n     js: {:?}",
                    &actual[at.saturating_sub(40)..(at + 40).min(actual.len())],
                    &expected[at.saturating_sub(40)..(at + 40).min(expected.len())],
                );
            }
        }
        assert_eq!(mismatches, 0, "{mismatches} of {} messages differ from JS", rows.len());
    }
}
