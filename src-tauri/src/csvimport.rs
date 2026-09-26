//! A small RFC 4180 CSV reader for host lists exported by other tools
//! (Termius, spreadsheets). Quoted fields may hold commas, quotes (doubled)
//! and line breaks. The frontend maps columns to host fields itself.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct CsvTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Parse CSV text. The first line is the header. A `;` or tab separator is
/// detected from the header when it has no commas.
pub fn parse(text: &str) -> CsvTable {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let sep = detect_separator(text.lines().next().unwrap_or_default());
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
        } else {
            match c {
                '"' if field.is_empty() => quoted = true,
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    if row.iter().any(|f| !f.trim().is_empty()) {
                        records.push(std::mem::take(&mut row));
                    } else {
                        row.clear();
                    }
                }
                c if c == sep => row.push(std::mem::take(&mut field)),
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|f| !f.trim().is_empty()) {
            records.push(row);
        }
    }
    let mut it = records.into_iter();
    let headers: Vec<String> = it.next().unwrap_or_default().into_iter().map(|h| h.trim().to_string()).collect();
    let width = headers.len();
    let rows = it
        .map(|mut r| {
            r.resize(width, String::new());
            r.into_iter().map(|f| f.trim().to_string()).collect()
        })
        .collect();
    CsvTable { headers, rows }
}

fn detect_separator(header: &str) -> char {
    if header.contains(',') {
        ','
    } else if header.contains(';') {
        ';'
    } else if header.contains('\t') {
        '\t'
    } else {
        ','
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_commas_newlines_and_bom() {
        let t = parse("\u{feff}Label,Address,Port,Notes\r\n\"web, prod\",10.0.0.1,22,\"multi\nline \"\"quoted\"\"\"\r\ndb,10.0.0.2,,\n\n");
        assert_eq!(t.headers, ["Label", "Address", "Port", "Notes"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0], ["web, prod", "10.0.0.1", "22", "multi\nline \"quoted\""]);
        assert_eq!(t.rows[1], ["db", "10.0.0.2", "", ""]);
    }

    #[test]
    fn semicolon_files_from_spreadsheets() {
        let t = parse("name;host\na;b\n");
        assert_eq!(t.headers, ["name", "host"]);
        assert_eq!(t.rows, vec![vec!["a".to_string(), "b".to_string()]]);
    }
}
