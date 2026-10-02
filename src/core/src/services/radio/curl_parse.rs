use std::collections::BTreeMap;

use crate::errors::{MhError, MhResult};

/// A stream address and the headers it needs, however the user gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedCurl {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

/// Headers that describe *this* connection rather than the request, and would be
/// wrong to replay: the proxy sets its own, and ffmpeg manages its own encoding.
const DROPPED: &[&str] = &[
    "accept-encoding",
    "connection",
    "content-length",
    "host",
    "keep-alive",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Reads either a bare stream URL or a whole `curl` command copied out of a
/// browser's network panel.
///
/// Accepting the command verbatim is the point: a stream that needs a `Referer`
/// is discovered by copying the request that worked, and retyping thirteen
/// headers into a form is not a thing anyone should have to do.
pub fn parse(input: &str) -> MhResult<ParsedCurl> {
    let tokens = tokenize(input);
    if tokens.is_empty() {
        return Err(MhError::Parse("nothing to read".into()));
    }

    let mut out = ParsedCurl::default();
    let mut index = if tokens[0].eq_ignore_ascii_case("curl") {
        1
    } else {
        0
    };

    // Hoisted out of the loop: it captures nothing per-iteration, and defining it
    // inside read as though it did.
    let take_next = |index: &mut usize| -> Option<String> {
        *index += 1;
        tokens.get(*index).cloned()
    };

    while index < tokens.len() {
        let token = tokens[index].as_str();
        match token {
            "-H" | "--header" => {
                if let Some(raw) = take_next(&mut index) {
                    push_header(&mut out.headers, &raw);
                }
            }
            "-A" | "--user-agent" => {
                if let Some(value) = take_next(&mut index) {
                    push_pair(&mut out.headers, "User-Agent", &value);
                }
            }
            "-e" | "--referer" => {
                if let Some(value) = take_next(&mut index) {
                    // curl's own `;auto` suffix has no meaning off a redirect chain.
                    let value = value.trim_end_matches(";auto");
                    push_pair(&mut out.headers, "Referer", value);
                }
            }
            "--url" => {
                if let Some(value) = take_next(&mut index) {
                    out.url = value;
                }
            }
            // Flags that carry a value we do not want, so the value is skipped
            // rather than mistaken for the URL.
            "-X" | "--request" | "-d" | "--data" | "--data-raw" | "--data-binary" | "-b"
            | "--cookie" | "-o" | "--output" | "-w" | "--write-out" | "--connect-timeout"
            | "-m" | "--max-time" | "-x" | "--proxy" => {
                index += 1;
            }
            other => {
                if !other.starts_with('-') && out.url.is_empty() {
                    out.url = other.to_string();
                }
            }
        }
        index += 1;
    }

    if out.url.is_empty() {
        return Err(MhError::Parse("no URL in that command".into()));
    }
    if !out.url.starts_with("http://") && !out.url.starts_with("https://") {
        return Err(MhError::Parse(format!("{} is not an http(s) URL", out.url)));
    }
    Ok(out)
}

fn push_header(headers: &mut BTreeMap<String, String>, raw: &str) {
    if let Some((name, value)) = raw.split_once(':') {
        push_pair(headers, name, value);
    }
}

fn push_pair(headers: &mut BTreeMap<String, String>, name: &str, value: &str) {
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() || value.is_empty() || name.starts_with(':') {
        return;
    }
    if DROPPED.contains(&name.to_ascii_lowercase().as_str()) {
        return;
    }
    headers.insert(canonical_case(name), value.to_string());
}

/// `sec-ch-ua` and `User-Agent` should not become two different keys for the
/// same header, and a map keyed on raw case would let them.
fn canonical_case(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Splits a shell-ish command into words, honouring both quote styles, escapes
/// and the trailing backslashes that a copied multi-line command is full of.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if quote != Some('\'') => match chars.peek() {
                // A backslash-newline is a line continuation, not a character.
                Some('\n') | Some('\r') => {
                    chars.next();
                }
                Some(next) => {
                    current.push(*next);
                    started = true;
                    chars.next();
                }
                None => {}
            },
            '\'' | '"' if quote.is_none() => {
                quote = Some(c);
                started = true;
            }
            c if Some(c) == quote => quote = None,
            c if quote.is_none() && c.is_whitespace() => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact command from the bug report, backslashes and all.
    const REPORTED: &str = r#"curl --url 'https://live.powerapp.com.tr/powerfmadver/abr/playlist.m3u8' \
  -H 'Accept: */*' \
  -H 'Accept-Language: en-US,en;q=0.9' \
  -H 'Cache-Control: no-cache' \
  -H 'Connection: keep-alive' \
  -H 'DNT: 1' \
  -H 'Origin: https://www.powerapp.com.tr' \
  -H 'Pragma: no-cache' \
  -H 'Referer: https://www.powerapp.com.tr/' \
  -H 'Sec-Fetch-Dest: empty' \
  -H 'Sec-Fetch-Mode: cors' \
  -H 'Sec-Fetch-Site: same-site' \
  -H 'User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36 Edg/151.0.0.0' \
  -H 'sec-ch-ua: "Not=A?Brand";v="99", "Microsoft Edge";v="151", "Chromium";v="151"' \
  -H 'sec-ch-ua-mobile: ?0' \
  -H 'sec-ch-ua-platform: "Linux"'"#;

    #[test]
    fn reads_the_reported_command() {
        let parsed = parse(REPORTED).unwrap();
        assert_eq!(
            parsed.url,
            "https://live.powerapp.com.tr/powerfmadver/abr/playlist.m3u8"
        );
        assert_eq!(
            parsed.headers.get("Referer").map(String::as_str),
            Some("https://www.powerapp.com.tr/")
        );
        assert_eq!(
            parsed.headers.get("Origin").map(String::as_str),
            Some("https://www.powerapp.com.tr")
        );
        assert!(parsed.headers["User-Agent"].contains("Chrome/151.0.0.0"));
        // Quoted commas and semicolons inside a value must survive whole.
        assert_eq!(
            parsed.headers.get("Sec-Ch-Ua").map(String::as_str),
            Some(r#""Not=A?Brand";v="99", "Microsoft Edge";v="151", "Chromium";v="151""#)
        );
        // 15 sent, `Connection` dropped as hop-by-hop.
        assert_eq!(parsed.headers.len(), 14, "{:?}", parsed.headers.keys());
        assert!(!parsed.headers.contains_key("Connection"));
    }

    #[test]
    fn a_bare_url_is_valid_input_too() {
        let parsed = parse("  https://ice6.somafm.com/groovesalad-128-mp3  ").unwrap();
        assert_eq!(parsed.url, "https://ice6.somafm.com/groovesalad-128-mp3");
        assert!(parsed.headers.is_empty());
    }

    #[test]
    fn short_user_agent_and_referer_flags_become_headers() {
        let parsed =
            parse(r#"curl -A "MyPlayer/1.0" -e https://ref.example/ https://a.example/s"#).unwrap();
        assert_eq!(parsed.url, "https://a.example/s");
        assert_eq!(parsed.headers["User-Agent"], "MyPlayer/1.0");
        assert_eq!(parsed.headers["Referer"], "https://ref.example/");
    }

    /// A value-carrying flag must not have its value mistaken for the URL.
    #[test]
    fn flag_values_are_not_mistaken_for_the_url() {
        let parsed = parse("curl -X GET --compressed -o out.bin https://a.example/s").unwrap();
        assert_eq!(parsed.url, "https://a.example/s");
    }

    #[test]
    fn a_command_without_a_url_is_an_error_not_an_empty_station() {
        assert!(parse("curl -H 'Accept: */*'").is_err());
        assert!(parse("curl ftp://a.example/s").is_err());
    }
}
