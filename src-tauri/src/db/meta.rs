//! What a table is made of beyond its columns: indexes, constraints, who points at it, a size and a row estimate, and
//! a definition to read. Read from the server's own catalogs for the engines that have them (PostgreSQL and MySQL /
//! MariaDB); every other engine reports that it has none, and the window hides those tabs instead of showing blanks.
//! Also the object search that goes across a whole connection without loading its tree.

use serde::Serialize;

use super::{ColumnInfo, NodeKind, TableInfo};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IndexMeta {
    pub name: String,
    /// The indexed columns, in index order. An expression index lists only its plain columns; see `definition`.
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
    /// btree, hash, gin, …
    pub method: Option<String>,
    /// The server's own text for it (`CREATE INDEX …`), where it has one.
    pub definition: Option<String>,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    PrimaryKey,
    ForeignKey,
    Unique,
    Check,
    Exclusion,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ForeignRef {
    /// The schema (PostgreSQL) or database (MySQL) of the other table.
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConstraintMeta {
    pub name: String,
    pub kind: ConstraintKind,
    pub columns: Vec<String>,
    /// The server's text for it (`CHECK (price >= 0)`, `FOREIGN KEY (…) REFERENCES …`).
    pub definition: Option<String>,
    pub references: Option<ForeignRef>,
}

/// A foreign key somewhere else that points at this table.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IncomingRef {
    pub name: String,
    pub schema: String,
    pub table: String,
    /// The columns of the table that points here.
    pub columns: Vec<String>,
    /// The columns of this table they point at.
    pub referenced_columns: Vec<String>,
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Ddl {
    pub text: String,
    /// True when SSHVault built the text from catalog metadata; false when the server produced it (`SHOW CREATE`,
    /// `pg_get_viewdef`). A reconstructed definition is close, not guaranteed identical to the original statement.
    pub reconstructed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct TableMeta {
    /// Whether the engine reports these at all; the window hides a tab that has nothing to say.
    pub supported: bool,
    /// "table", "view", "materialized view", "foreign table", "partitioned table".
    pub kind: Option<String>,
    pub comment: Option<String>,
    /// The server's estimate, not a count. `None` when it has none (a PostgreSQL table never analysed).
    pub approx_rows: Option<i64>,
    pub size_bytes: Option<i64>,
    pub indexes: Vec<IndexMeta>,
    pub constraints: Vec<ConstraintMeta>,
    pub referenced_by: Vec<IncomingRef>,
    /// Per column: identity, generated, auto_increment… Only for the columns that have one.
    pub column_notes: Vec<(String, String)>,
    pub ddl: Option<Ddl>,
}

/// One object found by a search of the whole connection.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchHit {
    /// Where it lives in the tree, up to and including its parent: `[schema]` or `[database]`.
    pub parent: Vec<String>,
    pub name: String,
    pub kind: NodeKind,
    pub detail: Option<String>,
}

/// `%`, `_` and the escape itself made literal for a LIKE pattern (escape character `\`).
pub fn like_escape(term: &str) -> String {
    let mut out = String::with_capacity(term.len() + 2);
    for c in term.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A PostgreSQL identifier quoted for use in text.
fn pg_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// What PostgreSQL wants after `ON UPDATE` / `ON DELETE`, from its one-letter code.
pub fn pg_action(code: &str) -> Option<String> {
    match code {
        "a" => None, // NO ACTION is the default; not worth a word
        "r" => Some("RESTRICT".into()),
        "c" => Some("CASCADE".into()),
        "n" => Some("SET NULL".into()),
        "d" => Some("SET DEFAULT".into()),
        _ => None,
    }
}

/// A `CREATE TABLE` built from catalog metadata: columns with their types, NOT NULL and defaults, then the table's
/// constraints as the server prints them, then its other indexes. Reconstructed, and labelled so in the window.
pub fn pg_table_ddl(schema: &str, table: &str, info: &TableInfo, notes: &[(String, String)], constraints: &[ConstraintMeta], indexes: &[IndexMeta]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for ColumnInfo { name, data_type, nullable, default, .. } in &info.columns {
        let mut l = format!("  {} {}", pg_ident(name), data_type);
        let note = notes.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
        match (note, default) {
            (Some("identity (always)"), _) => l.push_str(" GENERATED ALWAYS AS IDENTITY"),
            (Some("identity (by default)"), _) => l.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
            (Some("generated"), Some(d)) => l.push_str(&format!(" GENERATED ALWAYS AS ({d}) STORED")),
            (_, Some(d)) => l.push_str(&format!(" DEFAULT {d}")),
            _ => {}
        }
        if !nullable {
            l.push_str(" NOT NULL");
        }
        lines.push(l);
    }
    for c in constraints {
        if let Some(def) = &c.definition {
            lines.push(format!("  CONSTRAINT {} {}", pg_ident(&c.name), def));
        }
    }
    let mut out = format!("CREATE TABLE {}.{} (\n{}\n);", pg_ident(schema), pg_ident(table), lines.join(",\n"));
    for i in indexes {
        // An index that backs a constraint is already written above.
        if i.primary || constraints.iter().any(|c| c.name == i.name) {
            continue;
        }
        if let Some(def) = &i.definition {
            out.push_str(&format!("\n{def};"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_term_is_made_literal() {
        assert_eq!(like_escape("100%_off\\"), "100\\%\\_off\\\\");
        assert_eq!(like_escape("plain"), "plain");
    }

    #[test]
    fn pg_actions_read_as_words() {
        assert_eq!(pg_action("c").as_deref(), Some("CASCADE"));
        assert_eq!(pg_action("a"), None);
        assert_eq!(pg_action("n").as_deref(), Some("SET NULL"));
    }

    #[test]
    fn a_table_is_written_back_from_its_parts() {
        let info = TableInfo {
            columns: vec![
                ColumnInfo { name: "id".into(), data_type: "integer".into(), nullable: false, primary_key: true, default: None },
                ColumnInfo { name: "Odd \"Name\"".into(), data_type: "text".into(), nullable: true, primary_key: false, default: Some("'x'::text".into()) },
            ],
            primary_key: vec!["id".into()],
        };
        let notes = vec![("id".to_string(), "identity (always)".to_string())];
        let cons = vec![ConstraintMeta { name: "t_pkey".into(), kind: ConstraintKind::PrimaryKey, columns: vec!["id".into()], definition: Some("PRIMARY KEY (id)".into()), references: None }];
        let idx = vec![
            IndexMeta { name: "t_pkey".into(), columns: vec!["id".into()], unique: true, primary: true, method: Some("btree".into()), definition: Some("CREATE UNIQUE INDEX t_pkey ON public.t USING btree (id)".into()), size_bytes: None },
            IndexMeta { name: "t_x".into(), columns: vec![], unique: false, primary: false, method: None, definition: Some("CREATE INDEX t_x ON public.t USING btree (lower(x))".into()), size_bytes: None },
        ];
        let ddl = pg_table_ddl("public", "t", &info, &notes, &cons, &idx);
        assert!(ddl.starts_with("CREATE TABLE \"public\".\"t\" (\n  \"id\" integer GENERATED ALWAYS AS IDENTITY NOT NULL,"));
        assert!(ddl.contains("\"Odd \"\"Name\"\"\" text DEFAULT 'x'::text"));
        assert!(ddl.contains("CONSTRAINT \"t_pkey\" PRIMARY KEY (id)"));
        // The primary key's index isn't repeated; the other one is.
        assert_eq!(ddl.matches("t_pkey").count(), 1);
        assert!(ddl.ends_with("CREATE INDEX t_x ON public.t USING btree (lower(x));"));
    }
}
