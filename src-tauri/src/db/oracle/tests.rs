use super::*;
use crate::db::CellEdit;
use oracle_rs::types::{OracleDate, OracleNumber};
use std::collections::BTreeMap;

fn spec(tls: TlsMode, database: Option<&str>, options: &[(&str, &str)]) -> ConnectSpec {
    ConnectSpec {
        engine: "oracle".into(),
        host: "db.example".into(),
        port: 1521,
        user: "app".into(),
        password: Some("pw".into()),
        database: database.map(str::to_string),
        tls,
        tunnel_port: None,
        options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn names_and_values_are_quoted_for_oracle() {
    assert_eq!(quote_ident("ORDER\"S"), "\"ORDER\"\"S\"");
    let edit = RowEdit {
        database: "HR".into(),
        table: "EMPLOYEES".into(),
        key: vec![CellEdit { column: "ID".into(), value: Some("7".into()) }],
        changes: vec![CellEdit { column: "NAME".into(), value: Some("O'Brien :1".into()) }, CellEdit { column: "NOTE".into(), value: None }],
    };
    assert_eq!(preview_update(&edit), "UPDATE \"HR\".\"EMPLOYEES\" SET \"NAME\" = 'O''Brien :1', \"NOTE\" = NULL WHERE \"ID\" = '7'");
}

#[test]
fn a_connection_needs_a_service_name_or_a_sid_and_a_user() {
    let c = config_for(&spec(TlsMode::Disable, Some("FREEPDB1"), &[])).unwrap();
    assert_eq!((c.host.as_str(), c.port, c.service.service_name()), ("db.example", 1521, Some("FREEPDB1")));
    assert!(!c.is_tls_enabled());
    // The settings' service name wins over the database field; a SID wins over both.
    assert_eq!(config_for(&spec(TlsMode::Disable, Some("A"), &[("service_name", "B")])).unwrap().service.service_name(), Some("B"));
    assert_eq!(config_for(&spec(TlsMode::Disable, Some("A"), &[("sid", "XE")])).unwrap().service.sid(), Some("XE"));
    assert_eq!(config_for(&spec(TlsMode::Disable, None, &[("service_name", "B")])).unwrap().service.service_name(), Some("B"));
    assert!(matches!(config_for(&spec(TlsMode::Disable, None, &[])), Err(DbError::Invalid(m)) if m.contains("service name")));
    assert!(matches!(config_for(&spec(TlsMode::Disable, Some(""), &[])), Err(DbError::Invalid(_))));
    let mut s = spec(TlsMode::Disable, Some("X"), &[]);
    s.user = "  ".into();
    assert!(matches!(config_for(&s), Err(DbError::Invalid(_))));
}

#[test]
fn tls_follows_the_mode_and_dials_the_tunnel_when_there_is_one() {
    for mode in [TlsMode::Require, TlsMode::VerifyFull] {
        let c = config_for(&spec(mode, Some("X"), &[])).unwrap();
        assert!(c.is_tls_enabled(), "{mode:?}");
        assert_eq!(c.tls_config.as_ref().unwrap().server_name.as_deref(), Some("db.example"), "the name stays the server's");
    }
    assert!(!config_for(&spec(TlsMode::Require, Some("X"), &[])).unwrap().tls_config.unwrap().verify_server);
    assert!(config_for(&spec(TlsMode::VerifyFull, Some("X"), &[])).unwrap().tls_config.unwrap().verify_server);
    let c = config_for(&spec(TlsMode::VerifyFull, Some("X"), &[("ca_file", "/etc/ca.pem")])).unwrap();
    assert_eq!(c.tls_config.unwrap().ca_cert_path.as_deref(), Some("/etc/ca.pem"));
    let mut s = spec(TlsMode::Disable, Some("X"), &[]);
    s.tunnel_port = Some(55555);
    let c = config_for(&s).unwrap();
    assert_eq!((c.host.as_str(), c.port), ("127.0.0.1", 55555));
}

#[test]
fn types_become_kinds() {
    assert_eq!(kind_of(OracleType::Number), ColumnKind::Number);
    assert_eq!(kind_of(OracleType::BinaryDouble), ColumnKind::Number);
    assert_eq!(kind_of(OracleType::Date), ColumnKind::DateTime);
    assert_eq!(kind_of(OracleType::TimestampTz), ColumnKind::DateTime);
    assert_eq!(kind_of(OracleType::Raw), ColumnKind::Binary);
    assert_eq!(kind_of(OracleType::Varchar), ColumnKind::Text);
    assert_eq!(kind_of(OracleType::Clob), ColumnKind::Text);
    assert_eq!(kind_of(OracleType::Json), ColumnKind::Json);
    assert_eq!(kind_of(OracleType::Cursor), ColumnKind::Other);
}

#[test]
fn cells_keep_their_meaning() {
    assert_eq!(to_json(&OraValue::Null), Value::Null);
    assert_eq!(to_json(&OraValue::String("héllo".into())), json!("héllo"));
    assert_eq!(to_json(&OraValue::Integer(42)), json!(42));
    assert_eq!(to_json(&OraValue::Integer(i64::MAX)), json!("9223372036854775807"));
    assert_eq!(to_json(&OraValue::Float(1.5)), json!(1.5));
    assert_eq!(to_json(&OraValue::Float(f64::INFINITY)), json!("inf"));
    assert_eq!(to_json(&OraValue::Boolean(true)), json!(true));
    assert_eq!(to_json(&OraValue::Bytes(vec![0xde, 0xad])), json!("0xDEAD"));
    // NUMBER keeps every digit: exact integers as numbers, anything a double would change as text.
    assert_eq!(to_json(&OraValue::Number(OracleNumber::new("12345"))), json!(12345));
    assert_eq!(to_json(&OraValue::Number(OracleNumber::new("12.5"))), json!(12.5));
    assert_eq!(to_json(&OraValue::Number(OracleNumber::new("0.1000000000000000055511151231257827"))), json!("0.1000000000000000055511151231257827"));
    assert_eq!(to_json(&OraValue::Number(OracleNumber::new("123456789012345678901234567890"))), json!("123456789012345678901234567890"));
    assert_eq!(to_json(&OraValue::Number(OracleNumber::new("-3.140"))), json!("-3.140"), "trailing zeros are the server's own digits");
    let d = OracleDate { year: 2024, month: 5, day: 1, hour: 10, minute: 30, second: 5 };
    assert_eq!(to_json(&OraValue::Date(d)), json!("2024-05-01 10:30:05"));
    let cut = to_json(&OraValue::String("é".repeat(MAX_CELL_BYTES)));
    assert_eq!(cut["truncated"], true);
}

#[test]
fn statements_are_told_apart() {
    for block in ["BEGIN NULL; END;", "declare x number; begin x := 1; end;", "CREATE OR REPLACE PROCEDURE p AS BEGIN NULL; END;", "CREATE FUNCTION f RETURN NUMBER IS BEGIN RETURN 1; END;", "-- note\nBEGIN NULL; END;", "CREATE PACKAGE BODY x AS END;", "CREATE TRIGGER t BEFORE INSERT ON a BEGIN NULL; END;"] {
        assert!(is_plsql(block), "{block}");
    }
    for plain in ["SELECT 1 FROM dual", "CREATE TABLE t (a NUMBER)", "INSERT INTO t VALUES (1)", "CREATE INDEX i ON t(a)", "DROP TABLE t", ""] {
        assert!(!is_plsql(plain), "{plain}");
    }
    for dml in ["INSERT INTO t VALUES (1)", "update t set a = 1", "DELETE FROM t", "MERGE INTO t USING s ON (1=1) WHEN MATCHED THEN UPDATE SET a=1", "/* c */ UPDATE t SET a=1"] {
        assert!(is_dml(dml), "{dml}");
    }
    for other in ["SELECT 1 FROM dual", "CREATE TABLE t (a NUMBER)", "COMMIT", "BEGIN NULL; END;", "WITH x AS (SELECT 1 FROM dual) SELECT * FROM x"] {
        assert!(!is_dml(other), "{other}");
    }
}

#[test]
fn oracle_errors_are_worded_with_their_code() {
    use oracle_rs::Error as E;
    assert!(matches!(err(E::OracleError { code: 942, message: "table or view does not exist".into() }), DbError::Server(m) if m == "ORA-00942: table or view does not exist"));
    assert!(matches!(err(E::OracleError { code: 1017, message: "ORA-01017: invalid username/password; logon denied".into() }), DbError::Server(m) if m == "ORA-01017: invalid username/password; logon denied"), "the code is not said twice");
    assert!(matches!(err(E::ConnectionRefused { error_code: Some(12514), message: Some("service not registered".into()) }), DbError::Server(m) if m.contains("ORA-12514") && m.contains("service not registered")));
}

#[test]
fn a_program_block_is_not_a_transaction_to_refuse() {
    // The engine says so; the shared check is what would otherwise refuse `BEGIN … END;`.
    assert!(crate::db::safety::leaves_transaction_open("BEGIN NULL;"));
}

#[tokio::test]
async fn an_unreachable_server_is_explained_not_hung_on() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut s = spec(TlsMode::Disable, Some("FREEPDB1"), &[]);
    s.host = "127.0.0.1".into();
    s.port = closed;
    let started = std::time::Instant::now();
    assert!(matches!(OracleConn::connect(&s).await, Err(DbError::Connect { .. } | DbError::Server(_) | DbError::Timeout(_))));
    assert!(started.elapsed() < std::time::Duration::from_secs(25));
    // Something that answers but isn't an Oracle listener fails with a reason, not a hang.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let _ = tokio::io::AsyncWriteExt::write_all(&mut sock, b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        }
    });
    s.port = port;
    let started = std::time::Instant::now();
    assert!(OracleConn::connect(&s).await.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(25));
}
