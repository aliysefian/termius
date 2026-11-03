//! Moving whole tables in and out: export rows to CSV, JSON or INSERT statements, and import a CSV file into a table.
//! Both work in chunks, so a table of millions of rows or a file of gigabytes never sits in memory. They run on
//! PostgreSQL and MySQL / MariaDB, the engines that have row-level change sets (see [`super::Engine::supports_changes`]).
//!
//! An export is the rows of one table; it is not a backup (no schema, indexes, other tables, users or grants), and the
//! window says so. An import is saved in batches, each its own transaction: if a batch fails, the batches before it
//! stay saved, and the error says how many rows those were.

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{mysql, pg, CellEdit, Change, DbError, DbResult, Engine, QueryResult};

/// Rows fetched per round trip while exporting, and rows saved per transaction while importing.
pub const EXPORT_CHUNK: usize = 2000;
pub const IMPORT_BATCH: usize = 200;
/// A single field larger than this is refused rather than buffered (a stray quote can swallow a whole file).
const MAX_FIELD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Csv,
    Json,
    Sql,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Json => "json",
            Format::Sql => "sql",
        }
    }
}

struct Dialect {
    quote_ident: fn(&str) -> String,
    quote_literal: fn(&Option<String>) -> String,
}

fn dialect(engine: &Engine) -> DbResult<Dialect> {
    match engine {
        Engine::Postgres(_) => Ok(Dialect { quote_ident: pg::quote_ident, quote_literal: pg::quote_literal }),
        Engine::Mysql(_) => Ok(Dialect { quote_ident: mysql::quote_ident, quote_literal: mysql::quote_literal }),
        Engine::Other(_) => Err(DbError::Invalid("exporting and importing a table is available for PostgreSQL and MySQL / MariaDB".into())),
    }
}

// -- export ------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ExportSpec {
    pub database: String,
    pub table: String,
    pub format: Format,
    /// A `WHERE` expression the person typed, without the word WHERE. It is theirs to get right, like any SQL they run.
    pub filter: Option<String>,
    /// `(column, descending)`. Empty: the table's primary key, when it has one.
    #[serde(default)]
    pub order: Vec<(String, bool)>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct ExportSummary {
    pub rows: u64,
    pub bytes: u64,
    /// Cells longer than the grid's cell limit that were cut (the file holds their first 64 KiB). Zero for most tables.
    pub cut_cells: u64,
    /// False when the table has no key and no order was given, so the row order isn't guaranteed stable.
    pub ordered: bool,
}

/// The text of one cell for a file. NULL is `None`.
fn cell_text(v: &Value, cut: &mut u64) -> Option<String> {
    match v {
        Value::Null => None,
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Object(o) if o.get("truncated") == Some(&Value::Bool(true)) => {
            *cut += 1;
            Some(o.get("preview").and_then(Value::as_str).unwrap_or_default().to_string())
        }
        other => Some(other.to_string()),
    }
}

fn csv_field(v: &str) -> String {
    // Quote what a spreadsheet could misread, and defuse formulas, as the grid's own CSV copy does.
    let needs_quote = v.contains([',', '"', '\r', '\n']) || v.starts_with(char::is_whitespace) || v.ends_with(char::is_whitespace);
    let numeric = v.parse::<f64>().is_ok();
    let safe = if v.starts_with(['=', '+', '-', '@', '\t', '\r']) && !numeric { format!("'{v}") } else { v.to_string() };
    if needs_quote {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

/// Write every row of a table to `path`. `on_progress` gets the running row count after each chunk. The file is
/// written under a temporary name and moved into place only when it is complete.
pub async fn export_table(
    engine: &Engine,
    qid: Uuid,
    spec: &ExportSpec,
    path: &Path,
    mut on_progress: impl FnMut(u64),
) -> DbResult<ExportSummary> {
    let d = dialect(engine)?;
    if spec.database.is_empty() || spec.table.is_empty() {
        return Err(DbError::Invalid("choose a table to export".into()));
    }
    let info = engine.table_info(&spec.database, &spec.table).await?;
    let mut order = spec.order.clone();
    if order.is_empty() {
        order = info.primary_key.iter().map(|c| (c.clone(), false)).collect();
    }
    let known: Vec<&str> = info.columns.iter().map(|c| c.name.as_str()).collect();
    if let Some((bad, _)) = order.iter().find(|(c, _)| !known.contains(&c.as_str())) {
        return Err(DbError::Invalid(format!("\"{bad}\" is not a column of {}", spec.table)));
    }
    let order_sql = if order.is_empty() {
        String::new()
    } else {
        format!(" ORDER BY {}", order.iter().map(|(c, desc)| format!("{}{}", (d.quote_ident)(c), if *desc { " DESC" } else { "" })).collect::<Vec<_>>().join(", "))
    };
    let filter_sql = match spec.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => {
            if f.contains(';') {
                return Err(DbError::Invalid("the filter is a single condition; remove the semicolon".into()));
            }
            format!(" WHERE {f}")
        }
        None => String::new(),
    };
    let target = format!("{}.{}", (d.quote_ident)(&spec.database), (d.quote_ident)(&spec.table));

    let tmp = path.with_extension(format!("{}.part", spec.format.extension()));
    let file = File::create(&tmp).map_err(|e| DbError::Invalid(format!("could not write {}: {e}", tmp.display())))?;
    let mut out = BufWriter::new(file);
    let outcome = write_rows(engine, qid, spec, &d, &target, &order_sql, &filter_sql, &mut out, &mut on_progress).await;
    let flushed = out.flush().map_err(|e| DbError::Invalid(format!("could not write {}: {e}", tmp.display())));
    drop(out);
    match outcome.and_then(|s| flushed.map(|_| s)) {
        Ok(mut summary) => {
            std::fs::rename(&tmp, path).map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                DbError::Invalid(format!("could not write {}: {e}", path.display()))
            })?;
            summary.ordered = !order.is_empty();
            Ok(summary)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn write_rows(
    engine: &Engine,
    qid: Uuid,
    spec: &ExportSpec,
    d: &Dialect,
    target: &str,
    order_sql: &str,
    filter_sql: &str,
    out: &mut BufWriter<File>,
    on_progress: &mut impl FnMut(u64),
) -> DbResult<ExportSummary> {
    let io = |e: std::io::Error| DbError::Invalid(format!("could not write the file: {e}"));
    let mut summary = ExportSummary::default();
    let mut offset = 0usize;
    let mut names: Vec<String> = Vec::new();
    loop {
        let sql = format!("SELECT * FROM {target}{filter_sql}{order_sql} LIMIT {EXPORT_CHUNK} OFFSET {offset}");
        let chunk: QueryResult = engine.query(qid, &sql, EXPORT_CHUNK).await?;
        if offset == 0 {
            names = chunk.columns.iter().map(|c| c.name.clone()).collect();
            match spec.format {
                Format::Csv => {
                    let line = names.iter().map(|n| csv_field(n)).collect::<Vec<_>>().join(",");
                    write!(out, "{line}\r\n").map_err(io)?;
                }
                Format::Json => out.write_all(b"[").map_err(io)?,
                Format::Sql => {
                    writeln!(out, "-- Rows of {} exported by SSHVault. Not a backup: no schema, indexes or other tables.", spec.table.replace('\n', " ")).map_err(io)?;
                }
            }
        }
        for row in &chunk.rows {
            let cells: Vec<Option<String>> = row.iter().map(|v| cell_text(v, &mut summary.cut_cells)).collect();
            match spec.format {
                Format::Csv => {
                    let line = cells.iter().map(|c| c.as_deref().map(csv_field).unwrap_or_default()).collect::<Vec<_>>().join(",");
                    write!(out, "{line}\r\n").map_err(io)?;
                }
                Format::Json => {
                    let obj: serde_json::Map<String, Value> = names
                        .iter()
                        .zip(row.iter())
                        .map(|(n, v)| {
                            let v = match v {
                                Value::Object(o) if o.get("truncated") == Some(&Value::Bool(true)) => o.get("preview").cloned().unwrap_or(Value::Null),
                                other => other.clone(),
                            };
                            (n.clone(), v)
                        })
                        .collect();
                    write!(out, "{}\n  {}", if summary.rows == 0 { "" } else { "," }, Value::Object(obj)).map_err(io)?;
                }
                Format::Sql => {
                    let cols = names.iter().map(|n| (d.quote_ident)(n)).collect::<Vec<_>>().join(", ");
                    let vals = cells.iter().map(|c| (d.quote_literal)(c)).collect::<Vec<_>>().join(", ");
                    writeln!(out, "INSERT INTO {target} ({cols}) VALUES ({vals});").map_err(io)?;
                }
            }
            summary.rows += 1;
        }
        on_progress(summary.rows);
        if chunk.rows.len() < EXPORT_CHUNK {
            break;
        }
        offset += EXPORT_CHUNK;
    }
    if spec.format == Format::Json {
        out.write_all(if summary.rows == 0 { b"]\n" } else { b"\n]\n" }).map_err(io)?;
    }
    out.flush().map_err(io)?;
    summary.bytes = out.get_ref().metadata().map(|m| m.len()).unwrap_or(0);
    Ok(summary)
}

// -- CSV reading --------------------------------------------------------------------------------------------------

/// The text encodings an import offers. UTF-8 is the default; the others cover files saved by older Windows tools.
pub fn encoding_for(label: &str) -> Option<&'static encoding_rs::Encoding> {
    match label.trim().to_ascii_lowercase().as_str() {
        "" | "utf-8" | "utf8" => Some(encoding_rs::UTF_8),
        "latin1" | "iso-8859-1" | "windows-1252" => Some(encoding_rs::WINDOWS_1252),
        "utf-16le" | "utf-16" => Some(encoding_rs::UTF_16LE),
        "utf-16be" => Some(encoding_rs::UTF_16BE),
        "windows-1256" => Some(encoding_rs::WINDOWS_1256),
        "windows-1251" => Some(encoding_rs::WINDOWS_1251),
        "shift_jis" => Some(encoding_rs::SHIFT_JIS),
        "gbk" => Some(encoding_rs::GBK),
        _ => None,
    }
}

/// Guess the separator from a header line: the one of `, ; tab |` that appears most outside quotes.
pub fn detect_delimiter(first_line: &str) -> char {
    let mut best = (',', 0usize);
    for cand in [',', ';', '\t', '|'] {
        let mut quoted = false;
        let mut n = 0;
        for c in first_line.chars() {
            if c == '"' {
                quoted = !quoted;
            } else if c == cand && !quoted {
                n += 1;
            }
        }
        if n > best.1 {
            best = (cand, n);
        }
    }
    best.0
}

/// Reads records one at a time from any byte source, decoding as it goes. A quoted field may hold the delimiter,
/// doubled quotes and line breaks. Memory use is one chunk plus one record.
pub struct CsvRecords<R: Read> {
    reader: R,
    decoder: encoding_rs::Decoder,
    chunk: usize,
    delimiter: char,
    eof: bool,
    started: bool,
    queue: VecDeque<Vec<String>>,
    row: Vec<String>,
    field: String,
    quoted: bool,
    /// A quote was just read inside a quoted field: the next character says whether it closed it or escaped one.
    quote_pending: bool,
    /// The record has real content (a field was started), so a lone newline after it is a record end.
    dirty: bool,
}

impl<R: Read> CsvRecords<R> {
    pub fn new(reader: R, encoding: &'static encoding_rs::Encoding, delimiter: char) -> Self {
        Self::with_chunk(reader, encoding, delimiter, 64 * 1024)
    }

    pub fn with_chunk(reader: R, encoding: &'static encoding_rs::Encoding, delimiter: char, chunk: usize) -> Self {
        Self {
            reader,
            decoder: encoding.new_decoder_without_bom_handling(),
            chunk: chunk.max(4),
            delimiter,
            eof: false,
            started: false,
            queue: VecDeque::new(),
            row: Vec::new(),
            field: String::new(),
            quoted: false,
            quote_pending: false,
            dirty: false,
        }
    }

    pub fn next_record(&mut self) -> DbResult<Option<Vec<String>>> {
        loop {
            if let Some(r) = self.queue.pop_front() {
                return Ok(Some(r));
            }
            if self.eof {
                return Ok(None);
            }
            self.fill()?;
        }
    }

    fn fill(&mut self) -> DbResult<()> {
        let mut buf = vec![0u8; self.chunk];
        let n = self.reader.read(&mut buf).map_err(|e| DbError::Invalid(format!("could not read the file: {e}")))?;
        let last = n == 0;
        let mut text = String::with_capacity(n * 2 + 4);
        let (_, _, _) = self.decoder.decode_to_string(&buf[..n], &mut text, last);
        let text = if !self.started {
            self.started = true;
            text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text)
        } else {
            text
        };
        for c in text.chars() {
            self.feed(c)?;
        }
        if last {
            self.eof = true;
            if self.quote_pending {
                self.quoted = false;
                self.quote_pending = false;
            }
            self.end_record(false);
        }
        Ok(())
    }

    fn feed(&mut self, c: char) -> DbResult<()> {
        if self.quote_pending {
            self.quote_pending = false;
            if c == '"' {
                self.field.push('"');
                return Ok(());
            }
            self.quoted = false;
        }
        if self.quoted {
            if c == '"' {
                self.quote_pending = true;
            } else {
                self.field.push(c);
                if self.field.len() > MAX_FIELD_BYTES {
                    return Err(DbError::Invalid("a field is larger than 16 MB; check the delimiter and the quotes".into()));
                }
            }
            return Ok(());
        }
        match c {
            '"' if self.field.is_empty() => {
                self.quoted = true;
                self.dirty = true;
            }
            '\r' => {}
            '\n' => self.end_record(true),
            c if c == self.delimiter => {
                self.row.push(std::mem::take(&mut self.field));
                self.dirty = true;
            }
            c => {
                self.field.push(c);
                self.dirty = true;
            }
        }
        Ok(())
    }

    fn end_record(&mut self, _newline: bool) {
        if self.dirty || !self.row.is_empty() || !self.field.is_empty() {
            self.row.push(std::mem::take(&mut self.field));
            self.queue.push_back(std::mem::take(&mut self.row));
        }
        self.dirty = false;
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CsvPreview {
    pub delimiter: String,
    /// The first record when `has_header`, else numbered `column 1`, `column 2`, …
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub file_bytes: u64,
}

/// The first rows of a file, to map its columns and see what will be imported. Reads only what it needs.
pub fn csv_preview(path: &Path, encoding: &str, delimiter: Option<char>, has_header: bool, rows: usize) -> DbResult<CsvPreview> {
    let enc = encoding_for(encoding).ok_or_else(|| DbError::Invalid(format!("unknown encoding \"{encoding}\"")))?;
    let file = File::open(path).map_err(|e| DbError::Invalid(format!("could not open {}: {e}", path.display())))?;
    let file_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
    // Sniff the separator from the first line, in the file's own encoding.
    let delim = match delimiter {
        Some(d) => d,
        None => {
            let mut head = vec![0u8; 8192];
            let n = File::open(path).and_then(|mut f| f.read(&mut head)).map_err(|e| DbError::Invalid(e.to_string()))?;
            let (text, _, _) = enc.decode(&head[..n]);
            detect_delimiter(text.trim_start_matches('\u{feff}').lines().next().unwrap_or_default())
        }
    };
    let mut reader = CsvRecords::new(file, enc, delim);
    let first = reader.next_record()?;
    let (headers, mut out) = match (first, has_header) {
        (Some(h), true) => (h, Vec::new()),
        (Some(r), false) => ((1..=r.len()).map(|i| format!("column {i}")).collect(), vec![r]),
        (None, _) => return Err(DbError::Invalid("the file is empty".into())),
    };
    while out.len() < rows {
        match reader.next_record()? {
            Some(r) => out.push(r),
            None => break,
        }
    }
    Ok(CsvPreview { delimiter: delim.to_string(), headers, rows: out, file_bytes })
}

/// Imports asked to stop. An import looks at this between batches, so it stops cleanly after the batch in flight.
static STOP: Mutex<Option<HashSet<Uuid>>> = Mutex::new(None);

pub fn request_stop(job: Uuid) {
    STOP.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashSet::new).insert(job);
}

fn take_stop(job: Uuid) -> bool {
    STOP.lock().unwrap_or_else(|p| p.into_inner()).as_mut().is_some_and(|s| s.remove(&job))
}

// -- import ------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ImportSpec {
    pub database: String,
    pub table: String,
    pub encoding: String,
    pub delimiter: Option<String>,
    pub has_header: bool,
    /// Per CSV column, the table column it goes into, or `None` to leave it out.
    pub mapping: Vec<Option<String>>,
    /// Save an empty field as NULL instead of an empty string.
    pub empty_as_null: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct ImportSummary {
    pub rows: u64,
    pub batches: u64,
}

/// Import a CSV file into a table, in batches of [`IMPORT_BATCH`] rows, each its own transaction. Stops at the first
/// row or batch the database refuses; rows saved before that stay saved and the error says how many.
pub async fn import_csv(engine: &Engine, job: Uuid, spec: &ImportSpec, path: &Path, mut on_progress: impl FnMut(u64)) -> DbResult<ImportSummary> {
    dialect(engine)?;
    if !engine.supports_changes() {
        return Err(DbError::Invalid("this database type can't import rows".into()));
    }
    let enc = encoding_for(&spec.encoding).ok_or_else(|| DbError::Invalid(format!("unknown encoding \"{}\"", spec.encoding)))?;
    let info = engine.table_info(&spec.database, &spec.table).await?;
    let targets: Vec<&String> = spec.mapping.iter().flatten().collect();
    if targets.is_empty() {
        return Err(DbError::Invalid("map at least one column of the file to a column of the table".into()));
    }
    for (i, t) in targets.iter().enumerate() {
        if !info.columns.iter().any(|c| &c.name == *t) {
            return Err(DbError::Invalid(format!("\"{t}\" is not a column of {}", spec.table)));
        }
        if targets[..i].contains(t) {
            return Err(DbError::Invalid(format!("\"{t}\" is mapped twice")));
        }
    }
    let delim = match spec.delimiter.as_deref().filter(|d| !d.is_empty()) {
        Some("\\t") | Some("tab") => '\t',
        Some(d) => d.chars().next().unwrap_or(','),
        None => csv_preview(path, &spec.encoding, None, spec.has_header, 0)?.delimiter.chars().next().unwrap_or(','),
    };
    let file = File::open(path).map_err(|e| DbError::Invalid(format!("could not open {}: {e}", path.display())))?;
    let mut reader = CsvRecords::new(file, enc, delim);
    let mut record_no = 0u64;
    if spec.has_header {
        reader.next_record()?;
        record_no = 1;
    }
    let mut summary = ImportSummary::default();
    let mut batch: Vec<Change> = Vec::with_capacity(IMPORT_BATCH);
    let flush = |batch: &mut Vec<Change>, summary: &mut ImportSummary, last_record: u64| {
        let rows = std::mem::take(batch);
        let saved = summary.rows;
        async move {
            let n = rows.len() as u64;
            engine.apply_changes(&rows).await.map(|_| n).map_err(|e| {
                DbError::Invalid(format!(
                    "{e}\n\nThe import stopped in the batch ending at line {last_record}. {saved} row{} before it {} saved.",
                    if saved == 1 { "" } else { "s" },
                    if saved == 1 { "was" } else { "were" }
                ))
            })
        }
    };
    while let Some(rec) = reader.next_record()? {
        record_no += 1;
        if rec.len() != spec.mapping.len() {
            return Err(DbError::Invalid(format!(
                "line {record_no} has {} fields but the first line has {}. {} row{} before it {} saved.",
                rec.len(),
                spec.mapping.len(),
                summary.rows,
                if summary.rows == 1 { "" } else { "s" },
                if summary.rows == 1 { "was" } else { "were" }
            )));
        }
        let values: Vec<CellEdit> = spec
            .mapping
            .iter()
            .zip(rec)
            .filter_map(|(m, v)| m.as_ref().map(|col| CellEdit { column: col.clone(), value: if v.is_empty() && spec.empty_as_null { None } else { Some(v) } }))
            .collect();
        batch.push(Change::Insert { database: spec.database.clone(), table: spec.table.clone(), values });
        if batch.len() >= IMPORT_BATCH {
            summary.rows += flush(&mut batch, &mut summary, record_no).await?;
            summary.batches += 1;
            on_progress(summary.rows);
            if take_stop(job) {
                return Err(DbError::Cancelled);
            }
        }
    }
    if !batch.is_empty() {
        summary.rows += flush(&mut batch, &mut summary, record_no).await?;
        summary.batches += 1;
        on_progress(summary.rows);
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(bytes: &[u8], enc: &'static encoding_rs::Encoding, delim: char, chunk: usize) -> Vec<Vec<String>> {
        let mut r = CsvRecords::with_chunk(bytes, enc, delim, chunk);
        let mut out = Vec::new();
        while let Some(rec) = r.next_record().unwrap() {
            out.push(rec);
        }
        out
    }

    #[test]
    fn records_hold_quotes_delimiters_and_line_breaks() {
        let csv = "a,b,c\r\n1,\"x, y\",\"he said \"\"hi\"\"\"\r\n2,\"line\nbreak\",\r\n\r\n3,,\n";
        let r = read_all(csv.as_bytes(), encoding_rs::UTF_8, ',', 64);
        assert_eq!(r[0], ["a", "b", "c"]);
        assert_eq!(r[1], ["1", "x, y", "he said \"hi\""]);
        assert_eq!(r[2], ["2", "line\nbreak", ""]);
        assert_eq!(r[3], ["3", "", ""]);
        assert_eq!(r.len(), 4);
    }

    #[test]
    fn the_same_records_come_out_whatever_the_chunk_size() {
        let csv = "id,name\n1,\"héllo, wörld\"\n2,日本語\n3,\"a\"\"b\"\n";
        let whole = read_all(csv.as_bytes(), encoding_rs::UTF_8, ',', 64 * 1024);
        for chunk in [4, 5, 7, 13] {
            assert_eq!(read_all(csv.as_bytes(), encoding_rs::UTF_8, ',', chunk), whole, "chunk {chunk}");
        }
        assert_eq!(whole[2][1], "日本語");
    }

    #[test]
    fn a_byte_order_mark_and_a_missing_final_newline_are_fine() {
        let r = read_all(b"\xef\xbb\xbfa;b\n1;2", encoding_rs::UTF_8, ';', 3);
        assert_eq!(r, [vec!["a", "b"], vec!["1", "2"]]);
    }

    #[test]
    fn other_encodings_are_decoded() {
        // "café;ü" in Latin-1 / Windows-1252.
        let r = read_all(b"caf\xe9;\xfc\n", encoding_rs::WINDOWS_1252, ';', 4);
        assert_eq!(r, [vec!["café", "ü"]]);
        let utf16: Vec<u8> = "a,b\n1,é\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(read_all(&utf16, encoding_rs::UTF_16LE, ',', 5), [vec!["a", "b"], vec!["1", "é"]]);
        assert!(encoding_for("nope").is_none());
    }

    #[test]
    fn an_unterminated_quote_is_bounded() {
        let mut big = String::from("a,\"");
        big.push_str(&"x".repeat(MAX_FIELD_BYTES + 10));
        let mut r = CsvRecords::new(big.as_bytes(), encoding_rs::UTF_8, ',');
        assert!(r.next_record().is_err());
    }

    #[test]
    fn the_separator_is_guessed_outside_quotes() {
        assert_eq!(detect_delimiter("a;b;c"), ';');
        assert_eq!(detect_delimiter("a\tb\tc"), '\t');
        assert_eq!(detect_delimiter("\"a,b,c\";d;e"), ';');
        assert_eq!(detect_delimiter("single"), ',');
    }

    #[test]
    fn csv_fields_are_quoted_and_formulas_defused() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("-12.5"), "-12.5");
    }

    #[test]
    fn a_preview_reads_only_the_head() {
        let dir = std::env::temp_dir().join(format!("sshvault-csv-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.csv");
        std::fs::write(&p, "id;name\n1;a\n2;b\n3;c\n").unwrap();
        let pv = csv_preview(&p, "utf-8", None, true, 2).unwrap();
        assert_eq!((pv.delimiter.as_str(), pv.headers.clone(), pv.rows.len()), (";", vec!["id".to_string(), "name".to_string()], 2));
        let nh = csv_preview(&p, "utf-8", Some(';'), false, 10).unwrap();
        assert_eq!(nh.headers, ["column 1", "column 2"]);
        assert_eq!(nh.rows.len(), 4);
        assert!(csv_preview(&p, "bogus", None, true, 1).is_err());
        std::fs::write(&p, "").unwrap();
        assert!(csv_preview(&p, "utf-8", None, true, 1).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
