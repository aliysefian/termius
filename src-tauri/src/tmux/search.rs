//! Searching the text of captured panes. The scope is exactly what `capture-pane` returns for the lines asked for: the
//! visible screen and a bounded amount of scrollback. Output tmux has already discarded is not there to find.
//!
//! The pattern is compiled with the `regex` crate, which matches in time linear in the text, so a hostile or careless
//! pattern cannot hang the app; its size is capped as well.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub text: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub regex: bool,
}

/// Longest line kept in a result; a longer one is cut around the first match.
const MAX_LINE_CHARS: usize = 400;
const MAX_QUERY_CHARS: usize = 500;

pub struct Matcher(Regex);

impl Matcher {
    pub fn new(q: &Query) -> Result<Matcher, String> {
        if q.text.is_empty() {
            return Err("Type something to search for.".into());
        }
        if q.text.chars().count() > MAX_QUERY_CHARS {
            return Err("The search text is too long.".into());
        }
        let pattern = if q.regex { q.text.clone() } else { regex::escape(&q.text) };
        RegexBuilder::new(&pattern)
            .case_insensitive(!q.case_sensitive)
            .size_limit(1 << 20)
            .dfa_size_limit(1 << 22)
            .build()
            .map(Matcher)
            .map_err(|e| format!("That is not a valid pattern: {}", e.to_string().lines().last().unwrap_or("").trim()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    /// 1-based, counted from the top of what was captured.
    pub line: u32,
    pub text: String,
    /// Character ranges of the matches inside `text`.
    pub spans: Vec<(u32, u32)>,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// Matches in `text`, at most `max` of them, with `context` lines around each. A match that spans nothing (an empty regex
/// match) is not a hit.
pub fn find(m: &Matcher, text: &str, max: usize, context: usize) -> Vec<Hit> {
    let lines: Vec<&str> = text.lines().collect();
    let mut hits = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let spans: Vec<(usize, usize)> = m.0.find_iter(line).filter(|f| !f.as_str().is_empty()).map(|f| (f.start(), f.end())).take(20).collect();
        if spans.is_empty() {
            continue;
        }
        let (text, spans) = fit(line, &spans);
        let take = |range: std::ops::Range<usize>| range.map(|j| cut(lines[j])).collect::<Vec<_>>();
        hits.push(Hit {
            line: (i + 1) as u32,
            text,
            spans,
            before: take(i.saturating_sub(context)..i),
            after: take((i + 1)..(i + 1 + context).min(lines.len())),
        });
        if hits.len() >= max {
            break;
        }
    }
    hits
}

fn cut(line: &str) -> String {
    line.chars().take(MAX_LINE_CHARS).collect()
}

/// The line, cut to a bounded length around the first match, and the matches as character ranges inside it.
fn fit(line: &str, spans: &[(usize, usize)]) -> (String, Vec<(u32, u32)>) {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let char_of = |byte: usize| chars.iter().position(|(b, _)| *b >= byte).unwrap_or(chars.len());
    let total = chars.len();
    let first = char_of(spans[0].0);
    let from = if total <= MAX_LINE_CHARS { 0 } else { first.saturating_sub(MAX_LINE_CHARS / 4).min(total - MAX_LINE_CHARS) };
    let to = (from + MAX_LINE_CHARS).min(total);
    let text: String = chars[from..to].iter().map(|(_, c)| *c).collect();
    let out = spans
        .iter()
        .filter_map(|&(s, e)| {
            let (cs, ce) = (char_of(s), char_of(e));
            (cs >= from && ce <= to).then_some(((cs - from) as u32, (ce - from) as u32))
        })
        .collect();
    (text, out)
}
