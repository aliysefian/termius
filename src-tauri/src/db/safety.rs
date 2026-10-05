//! Spotting statements that destroy data, so the UI can ask first.
//!
//! This reads SQL just far enough to ignore comments and string literals,
//! split on `;`, and look at each statement's leading keywords. It is a
//! safety net, not a parser: it can miss exotic spellings, and it flags
//! some harmless statements, which only costs one confirmation.

/// Why a statement needs confirming, or `None` when it looks routine.
/// With several statements, the first that qualifies is reported.
pub fn destructive_reason(sql: &str) -> Option<String> {
    for stmt in statements(sql) {
        let words: Vec<String> = stmt.iter().map(|w| w.to_ascii_lowercase()).collect();
        let first = words.first().map(String::as_str).unwrap_or("");
        let has_where = words.iter().any(|w| w == "where");
        let reason = match first {
            "drop" => Some(format!("DROP {}", words.get(1).map(|w| w.to_ascii_uppercase()).unwrap_or_default())),
            "truncate" => Some("TRUNCATE".to_string()),
            "delete" if !has_where => Some("DELETE without WHERE".to_string()),
            "update" if !has_where => Some("UPDATE without WHERE".to_string()),
            "alter" if words.iter().any(|w| w == "drop") => Some("ALTER ... DROP".to_string()),
            _ => None,
        };
        if let Some(r) = reason {
            return Some(r.trim().to_string());
        }
    }
    None
}

/// Statements as lists of bare words, with comments removed and every
/// string, quoted name and number collapsed so their contents can't be
/// mistaken for keywords (`WHERE` inside a string, `;` inside a name).
fn statements(sql: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut i = 0;

    fn flush(word: &mut String, cur: &mut Vec<String>) {
        if !word.is_empty() {
            cur.push(std::mem::take(word));
        }
    }

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '-' if next == Some('-') && chars.get(i + 2).is_none_or(|c| c.is_whitespace()) => {
                flush(&mut word, &mut cur);
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '#' => {
                flush(&mut word, &mut cur);
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if next == Some('*') => {
                flush(&mut word, &mut cur);
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
            }
            '\'' | '"' | '`' => {
                flush(&mut word, &mut cur);
                let quote = c;
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' && quote != '`' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == quote {
                        if chars.get(i + 1) == Some(&quote) {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                // A quoted thing stands in as one opaque word.
                cur.push("?".to_string());
                i += 1;
            }
            ';' => {
                flush(&mut word, &mut cur);
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                i += 1;
            }
            c if c.is_alphanumeric() || c == '_' || c == '$' => {
                word.push(c);
                i += 1;
            }
            _ => {
                flush(&mut word, &mut cur);
                i += 1;
            }
        }
    }
    flush(&mut word, &mut cur);
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flagged(sql: &str) -> bool {
        destructive_reason(sql).is_some()
    }

    #[test]
    fn flags_the_obvious() {
        assert!(flagged("DROP TABLE users"));
        assert!(flagged("drop database shop;"));
        assert!(flagged("TRUNCATE TABLE logs"));
        assert!(flagged("DELETE FROM users"));
        assert!(flagged("UPDATE users SET admin = 1"));
        assert!(flagged("ALTER TABLE t DROP COLUMN c"));
    }

    #[test]
    fn where_clauses_make_it_routine() {
        assert!(!flagged("DELETE FROM users WHERE id = 3"));
        assert!(!flagged("update users set a = 1 where id = 2"));
        assert!(!flagged("SELECT * FROM users"));
        assert!(!flagged("INSERT INTO t VALUES (1)"));
        assert!(!flagged("ALTER TABLE t ADD COLUMN c INT"));
        assert!(!flagged(""));
    }

    #[test]
    fn looks_through_comments_and_strings() {
        assert!(flagged("/* hi */ DROP TABLE t"));
        assert!(flagged("-- note\nDELETE FROM t"));
        assert!(flagged("# note\nDELETE FROM t"));
        assert!(flagged("DELETE FROM t -- WHERE id = 1"));
        assert!(flagged("DELETE FROM t /* WHERE id = 1 */"));
        assert!(flagged("UPDATE t SET note = 'WHERE x'"));
        assert!(!flagged("SELECT 'DROP TABLE x'"));
        assert!(!flagged("SELECT 1 -- DROP TABLE x"));
        assert!(!flagged("SELECT `drop`, \"truncate\" FROM t"));
        assert!(!flagged("SELECT 'it''s; DROP TABLE x'"));
        assert!(!flagged("SELECT 'a\\'; DROP TABLE x'"));
    }

    #[test]
    fn checks_every_statement() {
        assert!(flagged("SELECT 1; DROP TABLE t"));
        assert!(flagged("DELETE FROM a WHERE id = 1; DELETE FROM b"));
        assert!(!flagged("SELECT 1; SELECT 2;"));
    }

    #[test]
    fn names_the_reason() {
        assert_eq!(destructive_reason("drop table t").as_deref(), Some("DROP TABLE"));
        assert_eq!(destructive_reason("DELETE FROM t").as_deref(), Some("DELETE without WHERE"));
        assert_eq!(destructive_reason("SELECT 1"), None);
    }

    #[test]
    fn handles_odd_input() {
        // Unterminated strings and comments must not panic or loop.
        assert!(!flagged("SELECT 'unterminated"));
        assert!(!flagged("SELECT /* unterminated"));
        assert!(flagged("DROP TABLE t /* unterminated"));
        assert!(!flagged("SELECT 'é'; -- ünï"));
    }
}
