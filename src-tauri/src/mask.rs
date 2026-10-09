//! Hiding secrets in text that is about to be kept: command output saved in a run's record, exported logs.
//!
//! This is a safety net, not a guarantee. It recognises the common shapes (private key blocks, `password=...`,
//! `Authorization: Bearer ...`, well-known token prefixes, `user:pass@` in URLs); a secret printed in some other form
//! is not found. It never changes text that doesn't match, and it never makes text longer than a few characters per hit.

pub const HIDDEN: &str = "[hidden]";

/// Words that mark a value as secret when they appear in a name.
const NAMES: [&str; 12] = [
    "password", "passwd", "passphrase", "secret", "token", "api_key", "api-key", "apikey", "access_key", "private_key", "authorization", "credential",
];

/// Token prefixes and the shortest total length that makes a hit (so ordinary words like "sk-learn" stay).
const PREFIXES: [(&str, usize); 7] = [("ghp_", 24), ("gho_", 24), ("github_pat_", 30), ("xoxb-", 20), ("xoxp-", 20), ("xoxa-", 20), ("sk-", 24)];

/// Whether a parameter or field called `name` is likely to hold a secret.
pub fn is_secret_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    NAMES.iter().any(|w| n.contains(w))
}

pub fn secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_key = false;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let eol = &line[bare.len()..];
        if in_key {
            if bare.contains("-----END ") {
                in_key = false;
            }
            continue; // the whole block is replaced by the one line written at its start
        }
        if bare.contains("-----BEGIN ") && bare.contains("PRIVATE KEY") {
            in_key = !bare.contains("-----END ");
            out.push_str("[private key hidden]");
            out.push_str(eol);
            continue;
        }
        out.push_str(&mask_line(bare));
        out.push_str(eol);
    }
    out
}

fn mask_line(line: &str) -> String {
    let line = mask_url_passwords(line);
    let line = mask_named_values(&line);
    mask_prefixed_tokens(&line)
}

/// `scheme://user:pass@host` -> `scheme://user:[hidden]@host`
fn mask_url_passwords(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail.find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'').unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rfind('@') {
            Some(at) => match authority[..at].find(':') {
                Some(colon) => {
                    out.push_str(&authority[..colon + 1]);
                    out.push_str(HIDDEN);
                    out.push_str(&authority[at..]);
                }
                None => out.push_str(authority),
            },
            None => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn is_value_char(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '"' | '\'' | '&' | ',' | ';' | '}' | ')' | '<' | '>'))
}

/// `name=value`, `name: value`, `"name": "value"` where the name mentions a secret word.
fn mask_named_values(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let mut out = String::with_capacity(line.len());
    let mut pos = 0;
    while pos < line.len() {
        // The next secret word at or after `pos`.
        let Some((at, word)) = NAMES.iter().filter_map(|w| lower[pos..].find(w).map(|i| (pos + i, *w))).min_by_key(|(i, _)| *i) else { break };
        let mut i = at + word.len();
        // Let the name run on (`password_hash`, `TOKEN_FILE`): letters, digits, `_`, `-`, `.`.
        while let Some(c) = line[i..].chars().next().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')) {
            i += c.len_utf8();
        }
        // Optional closing quote, spaces, then the separator.
        let mut j = i;
        for skip in [|c: char| c == '"' || c == '\'', |c: char| c == ' ' || c == '\t'] {
            while let Some(c) = line[j..].chars().next().filter(|c| skip(*c)) {
                j += c.len_utf8();
            }
        }
        let Some(sep) = line[j..].chars().next().filter(|c| matches!(c, '=' | ':')) else {
            out.push_str(&line[pos..i]);
            pos = i;
            continue;
        };
        j += sep.len_utf8();
        while let Some(c) = line[j..].chars().next().filter(|c| matches!(c, ' ' | '\t' | '"' | '\'')) {
            j += c.len_utf8();
        }
        // `Authorization: Bearer abc` / `Basic abc`: the scheme isn't the secret.
        for scheme in ["bearer ", "basic ", "token "] {
            if lower[j..].starts_with(scheme) {
                j += scheme.len();
            }
        }
        let value_len: usize = line[j..].chars().take_while(|c| is_value_char(*c)).map(char::len_utf8).sum();
        out.push_str(&line[pos..j]);
        if value_len > 0 {
            out.push_str(HIDDEN);
        }
        pos = j + value_len;
    }
    out.push_str(&line[pos..]);
    out
}

fn mask_prefixed_tokens(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    'scan: while !rest.is_empty() {
        for (prefix, min_len) in PREFIXES {
            if rest.starts_with(prefix) {
                let len: usize = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).map(char::len_utf8).sum();
                if len >= min_len {
                    out.push_str(HIDDEN);
                    rest = &rest[len..];
                    continue 'scan;
                }
            }
        }
        // AWS access key ids: AKIA / ASIA followed by 16 upper-case letters or digits.
        if (rest.starts_with("AKIA") || rest.starts_with("ASIA")) && rest.len() >= 20 && rest[4..20].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            out.push_str(HIDDEN);
            rest = &rest[20..];
            continue;
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_values_are_hidden_but_the_names_stay() {
        assert_eq!(secrets("DB_PASSWORD=hunter2"), "DB_PASSWORD=[hidden]");
        assert_eq!(secrets("password: hunter2 and more"), "password: [hidden] and more");
        assert_eq!(secrets(r#"{"api_key": "abc123", "user": "bob"}"#), r#"{"api_key": "[hidden]", "user": "bob"}"#);
        assert_eq!(secrets("export SECRET_TOKEN='abc' && run"), "export SECRET_TOKEN='[hidden]' && run");
        assert_eq!(secrets("curl -H 'Authorization: Bearer abc.def.ghi' x"), "curl -H 'Authorization: Bearer [hidden]' x");
        assert_eq!(secrets("mysql --password=s3cret -u root"), "mysql --password=[hidden] -u root");
        assert_eq!(secrets("two: password=a token=b"), "two: password=[hidden] token=[hidden]");
    }

    #[test]
    fn a_name_without_a_value_is_left_alone() {
        for t in ["the password was changed", "enter your password:", "token", "no secrets here", "passwords are hard", "password:"] {
            assert_eq!(secrets(t), t);
        }
    }

    #[test]
    fn private_key_blocks_are_replaced_whole() {
        let t = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\nBBBB\n-----END OPENSSH PRIVATE KEY-----\nafter\n";
        assert_eq!(secrets(t), "before\n[private key hidden]\nafter\n");
        let cut = "x\n-----BEGIN RSA PRIVATE KEY-----\nAAAA\n";
        assert_eq!(secrets(cut), "x\n[private key hidden]\n", "an unfinished block is hidden to the end");
        assert_eq!(secrets("-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----"), "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----", "public material stays");
    }

    #[test]
    fn credentials_in_urls_and_known_token_shapes() {
        assert_eq!(secrets("git clone https://bob:pa55@example.com/r.git"), "git clone https://bob:[hidden]@example.com/r.git");
        assert_eq!(secrets("https://example.com/a:b"), "https://example.com/a:b");
        assert_eq!(secrets("id AKIAIOSFODNN7EXAMPLE end"), "id [hidden] end");
        assert_eq!(secrets("t=ghp_abcdefghijklmnopqrstuvwxyz0123456789"), "t=[hidden]");
        assert_eq!(secrets("use sk-learn for models"), "use sk-learn for models");
        assert_eq!(secrets("key sk-abcdefghijklmnopqrstuvwxyz012345 ok"), "key [hidden] ok");
    }

    #[test]
    fn secret_names_are_recognised() {
        for n in ["password", "DB_PASSWORD", "api_key", "authToken", "client_secret"] {
            assert!(is_secret_name(n), "{n}");
        }
        for n in ["service", "host", "limit"] {
            assert!(!is_secret_name(n), "{n}");
        }
    }

    #[test]
    fn line_endings_and_plain_text_are_untouched() {
        let t = "line one\r\nline two\n\nüñí ✓ text\n";
        assert_eq!(secrets(t), t);
        assert_eq!(secrets("a=1\r\npassword=x\r\n"), "a=1\r\npassword=[hidden]\r\n");
        assert_eq!(secrets(""), "");
    }
}
