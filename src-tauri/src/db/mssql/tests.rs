use super::*;
use crate::db::CellEdit;
use std::collections::BTreeMap;
use std::borrow::Cow;

fn spec(user: &str, tls: TlsMode, options: &[(&str, &str)]) -> ConnectSpec {
    ConnectSpec {
        engine: "mssql".into(),
        host: "db.example".into(),
        port: 1433,
        user: user.into(),
        password: Some("pw".into()),
        database: Some("shop".into()),
        tls,
        tunnel_port: None,
        options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn names_and_values_are_quoted_for_t_sql() {
    assert_eq!(quote_ident("a]b"), "[a]]b]");
    assert_eq!(quote_ident("order items"), "[order items]");
    let edit = RowEdit {
        database: "dbo".into(),
        table: "it]ems".into(),
        key: vec![CellEdit { column: "id".into(), value: Some("1".into()) }],
        changes: vec![CellEdit { column: "name".into(), value: Some("it's @P1".into()) }, CellEdit { column: "note".into(), value: None }],
    };
    assert_eq!(preview_update(&edit), "UPDATE [dbo].[it]]ems] SET [name] = N'it''s @P1', [note] = NULL WHERE [id] = N'1'");
}

#[test]
fn the_login_kind_follows_the_user_name() {
    assert!(matches!(auth_for("sa", "x"), Ok(AuthMethod::SqlServer(_))));
    assert!(matches!(auth_for("ann@corp.example", "x"), Ok(AuthMethod::SqlServer(_))), "an Azure login is a SQL login");
    #[cfg(windows)]
    assert!(matches!(auth_for("CORP\\ann", "x"), Ok(AuthMethod::Windows(_))));
    #[cfg(not(windows))]
    assert!(matches!(auth_for("CORP\\ann", "x"), Err(DbError::Invalid(m)) if m.contains("Windows version")));
}

#[test]
fn a_connection_needs_a_user_and_tls_follows_the_mode() {
    assert!(matches!(config_for(&spec(" ", TlsMode::Disable, &[])), Err(DbError::Invalid(_))));
    for mode in [TlsMode::Disable, TlsMode::Require, TlsMode::VerifyFull] {
        assert!(config_for(&spec("sa", mode, &[])).is_ok());
    }
    assert!(config_for(&spec("sa", TlsMode::VerifyFull, &[("ca_file", "/etc/ca.pem"), ("read_only", "true")])).is_ok());
    assert_eq!(config_for(&spec("sa", TlsMode::Disable, &[])).unwrap().get_addr(), "db.example:1433");
}

#[test]
fn types_become_kinds() {
    assert_eq!(kind_of(ColumnType::Int4), ColumnKind::Number);
    assert_eq!(kind_of(ColumnType::Decimaln), ColumnKind::Number);
    assert_eq!(kind_of(ColumnType::Datetime2), ColumnKind::DateTime);
    assert_eq!(kind_of(ColumnType::BigVarBin), ColumnKind::Binary);
    assert_eq!(kind_of(ColumnType::NVarchar), ColumnKind::Text);
    assert_eq!(kind_of(ColumnType::Xml), ColumnKind::Json);
    assert_eq!(kind_of(ColumnType::SSVariant), ColumnKind::Other);
}

#[test]
fn cells_keep_their_meaning() {
    assert_eq!(to_json(&ColumnData::I32(Some(7))), json!(7));
    assert_eq!(to_json(&ColumnData::I32(None)), Value::Null);
    assert_eq!(to_json(&ColumnData::Bit(Some(true))), json!(true));
    assert_eq!(to_json(&ColumnData::I64(Some(i64::MAX))), json!("9223372036854775807"), "beyond 2^53 stays exact text");
    assert_eq!(to_json(&ColumnData::I64(Some(42))), json!(42));
    assert_eq!(to_json(&ColumnData::F64(Some(1.5))), json!(1.5));
    assert_eq!(to_json(&ColumnData::F64(Some(f64::NAN))), json!("NaN"));
    assert_eq!(to_json(&ColumnData::String(Some(Cow::Borrowed("héllo")))), json!("héllo"));
    assert_eq!(to_json(&ColumnData::Binary(Some(Cow::Borrowed(&[0xde, 0xad, 0x01])))), json!("0xDEAD01"));
    assert_eq!(to_json(&ColumnData::Guid(Some(Uuid::nil()))), json!("00000000-0000-0000-0000-000000000000"));
    let long = "é".repeat(MAX_CELL_BYTES);
    let cut = to_json(&ColumnData::String(Some(Cow::Owned(long))));
    assert_eq!(cut["truncated"], true);
    assert!(cut["preview"].as_str().unwrap().len() <= MAX_CELL_BYTES);
    let big = to_json(&ColumnData::Binary(Some(Cow::Owned(vec![0xAB; MAX_CELL_BYTES]))));
    assert_eq!(big["bytes"], MAX_CELL_BYTES);
}

#[tokio::test]
async fn an_unreachable_server_is_explained_not_hung_on() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut s = spec("sa", TlsMode::Disable, &[]);
    s.host = "127.0.0.1".into();
    s.port = closed;
    assert!(matches!(MssqlConn::connect(&s).await, Err(DbError::Connect { .. })));
    // Something that answers but isn't SQL Server fails at the handshake with a reason, not a hang.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let _ = tokio::io::AsyncWriteExt::write_all(&mut sock, b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        }
    });
    s.port = port;
    let started = std::time::Instant::now();
    assert!(MssqlConn::connect(&s).await.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(25));
}
