//! Databases.

use super::*;

#[tauri::command]
pub fn list_db_connections(state: State<'_, AppState>) -> ApiResult<Vec<Record<crate::models::DbConnection>>> {
    let mut list: Vec<Record<crate::models::DbConnection>> = list_records(&state, Collection::Databases)?;
    for r in &mut list {
        r.data = r.data.as_ref().map(crate::models::DbConnection::redacted);
    }
    Ok(list)
}

/// The stored password of a saved connection, if any.
fn stored_db_password(state: &AppState, id: Uuid) -> ApiResult<Option<String>> {
    Ok(state.session.with_vault(|v| match v.get::<crate::models::DbConnection>(Collection::Databases, id) {
        Ok(r) => Ok(r.data.and_then(|c| c.password)),
        Err(VaultError::NotFound { .. } | VaultError::Deleted { .. }) => Ok(None),
        Err(e) => Err(e),
    })?)
}

fn check_db_connection(c: &crate::models::DbConnection) -> ApiResult<()> {
    if c.name.trim().is_empty() {
        return Err(ApiError::new("validation", "a connection needs a name"));
    }
    if !crate::db::ENGINES.contains(&c.engine.as_str()) {
        return Err(ApiError::new("validation", format!("unsupported database type \"{}\"", c.engine)));
    }
    if c.host.trim().is_empty() {
        return Err(ApiError::new("validation", "enter the server's address"));
    }
    if c.port == 0 {
        return Err(ApiError::new("validation", "enter the server's port"));
    }
    Ok(())
}

/// Save a connection. An empty password keeps the stored one.
#[tauri::command]
pub fn save_db_connection(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    connection: crate::models::DbConnection,
) -> ApiResult<Record<crate::models::DbConnection>> {
    let mut connection = connection;
    check_db_connection(&connection)?;
    connection.host = connection.host.trim().to_string();
    if connection.password.as_deref() == Some("") {
        connection.password = match id {
            Some(id) => stored_db_password(&state, id)?,
            None => None,
        };
    }
    let mut rec = save_record(&state, Collection::Databases, id, base_rev, connection)?;
    rec.data = rec.data.as_ref().map(crate::models::DbConnection::redacted);
    Ok(rec)
}

#[tauri::command]
pub fn delete_db_connection(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    delete_record(&state, Collection::Databases, id, base_rev)
}

/// What the window learns about a connection that just opened.
#[derive(Debug, Clone, Serialize)]
pub struct DbOpened {
    pub session_id: Uuid,
    pub server_version: String,
}

/// Turn a connection from the form (saved or not) into what the engine needs,
/// plus the SSH target to tunnel through, if any.
fn resolve_db(
    state: &AppState,
    id: Option<Uuid>,
    c: &crate::models::DbConnection,
) -> ApiResult<(crate::db::ConnectSpec, Option<Target>)> {
    check_db_connection(c)?;
    let password = match c.password.as_deref() {
        Some("") | None => match id {
            Some(id) => stored_db_password(state, id)?,
            None => None,
        },
        Some(p) => Some(p.to_string()),
    };
    let via = match c.ssh_host_id {
        Some(host_id) => Some(resolve_target(state, host_id, None).map_err(|e| {
            if e.code == "not_found" {
                ApiError::new("validation", "the SSH host this connection goes through no longer exists; pick another")
            } else {
                e
            }
        })?),
        None => None,
    };
    let spec = crate::db::ConnectSpec {
        engine: c.engine.clone(),
        host: c.host.trim().to_string(),
        port: c.port,
        user: c.username.clone(),
        password,
        database: Some(c.database.clone()).filter(|d| !d.is_empty()),
        tls: c.tls,
        tunnel_port: None,
        options: c.options.clone(),
    };
    Ok((spec, via))
}

/// Open a saved connection. The window never sees its password.
#[tauri::command]
pub async fn db_open(state: State<'_, AppState>, id: Uuid) -> ApiResult<DbOpened> {
    let conn = state
        .session
        .with_vault(|v| v.get::<crate::models::DbConnection>(Collection::Databases, id))?
        .data
        .ok_or_else(|| ApiError::new("not_found", "that connection was deleted"))?;
    let (spec, via) = resolve_db(&state, Some(id), &conn)?;
    let (session_id, server_version) = state.dbs.open(spec, via).await?;
    Ok(DbOpened { session_id, server_version })
}

/// Try a connection from the form, saved or not, then close it again.
#[tauri::command]
pub async fn db_test(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    connection: crate::models::DbConnection,
) -> ApiResult<String> {
    let (spec, via) = resolve_db(&state, id, &connection)?;
    let (session_id, version) = state.dbs.open(spec, via).await?;
    state.dbs.close(session_id).await;
    Ok(version)
}

#[tauri::command]
pub async fn db_close(state: State<'_, AppState>, session_id: Uuid) -> ApiResult<()> {
    state.dbs.close(session_id).await;
    Ok(())
}

#[tauri::command]
pub async fn db_query(
    state: State<'_, AppState>,
    session_id: Uuid,
    query_id: Uuid,
    sql: String,
    limit: Option<usize>,
    confirmed: bool,
) -> ApiResult<crate::db::QueryResult> {
    let session = state.dbs.get(session_id)?;
    Ok(crate::db::run_query(&session, query_id, &sql, limit, confirmed).await?)
}

#[tauri::command]
pub async fn db_cancel(state: State<'_, AppState>, session_id: Uuid, query_id: Uuid) -> ApiResult<()> {
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.cancel(query_id).await?)
}

#[tauri::command]
pub async fn db_children(
    state: State<'_, AppState>,
    session_id: Uuid,
    path: Vec<String>,
) -> ApiResult<Vec<crate::db::TreeNode>> {
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.children(&path).await?)
}

#[tauri::command]
pub async fn db_table_info(
    state: State<'_, AppState>,
    session_id: Uuid,
    database: String,
    table: String,
) -> ApiResult<crate::db::TableInfo> {
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.table_info(&database, &table).await?)
}

/// The `UPDATE` an inline edit would run, for the confirmation step.
#[tauri::command]
pub fn db_preview_update(
    state: State<'_, AppState>,
    session_id: Uuid,
    edit: crate::db::RowEdit,
) -> ApiResult<String> {
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.preview_update(&edit)?)
}

/// Run an inline edit. Returns how many rows changed.
#[tauri::command]
pub async fn db_apply_update(
    state: State<'_, AppState>,
    session_id: Uuid,
    edit: crate::db::RowEdit,
) -> ApiResult<u64> {
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.apply_update(&edit).await?)
}

/// Write an export to a file the user chose in the save dialog.
#[tauri::command]
pub fn db_save_export(path: String, contents: String) -> ApiResult<()> {
    std::fs::write(&path, contents).map_err(|e| ApiError::new("io", format!("could not write {path}: {e}")))
}

/// Indexes, constraints, relationships and a definition of one table. `supported` is false for an engine that
/// keeps none of them.
#[tauri::command]
pub async fn db_table_meta(
    state: State<'_, AppState>,
    session_id: Uuid,
    database: String,
    table: String,
) -> ApiResult<crate::db::meta::TableMeta> {
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.table_meta(&database, &table).await?)
}

/// Objects whose name contains `term`, across the whole connection (so a table can be found before its schema is
/// opened). Empty for an engine without a catalog to search.
#[tauri::command]
pub async fn db_search(
    state: State<'_, AppState>,
    session_id: Uuid,
    term: String,
    limit: Option<usize>,
) -> ApiResult<Vec<crate::db::meta::SearchHit>> {
    let term = term.trim();
    if term.is_empty() {
        return Ok(Vec::new());
    }
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.search(term, limit.unwrap_or(100)).await?)
}

/// Whether this connection can take a reviewed set of changes in one transaction.
#[tauri::command]
pub fn db_supports_changes(state: State<'_, AppState>, session_id: Uuid) -> ApiResult<bool> {
    Ok(state.dbs.get(session_id)?.engine.supports_changes())
}

/// The statements a set of changes would run, for the review step.
#[tauri::command]
pub fn db_preview_changes(state: State<'_, AppState>, session_id: Uuid, changes: Vec<crate::db::Change>) -> ApiResult<Vec<String>> {
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.preview_changes(&changes)?)
}

/// Apply a reviewed set of edits, inserts and deletes in one transaction. Returns the rows each touched.
#[tauri::command]
pub async fn db_apply_changes(state: State<'_, AppState>, session_id: Uuid, changes: Vec<crate::db::Change>) -> ApiResult<Vec<u64>> {
    let session = state.dbs.get(session_id)?;
    Ok(session.engine.apply_changes(&changes).await?)
}

/// Progress of an export or an import, for the window's progress bar.
#[derive(Debug, Clone, Serialize)]
struct TransferProgress {
    job_id: Uuid,
    rows: u64,
}

const EVENT_DB_TRANSFER: &str = "db:transfer";

/// Write a table's rows to a file, in chunks. `job_id` is also the query id, so Cancel (`db_cancel`) stops it.
#[tauri::command]
pub async fn db_export_table(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: Uuid,
    job_id: Uuid,
    spec: crate::db::transfer::ExportSpec,
    path: String,
) -> ApiResult<crate::db::transfer::ExportSummary> {
    let session = state.dbs.get(session_id)?;
    let out = crate::db::transfer::export_table(&session.engine, job_id, &spec, std::path::Path::new(&path), |rows| {
        let _ = app.emit(EVENT_DB_TRANSFER, TransferProgress { job_id, rows });
    })
    .await?;
    Ok(out)
}

/// The first rows of a CSV file, to map its columns before importing.
#[tauri::command]
pub fn db_import_preview(
    path: String,
    encoding: String,
    delimiter: Option<String>,
    has_header: bool,
) -> ApiResult<crate::db::transfer::CsvPreview> {
    let delim = delimiter.as_deref().filter(|d| !d.is_empty()).map(|d| if d == "\\t" || d == "tab" { '\t' } else { d.chars().next().unwrap_or(',') });
    Ok(crate::db::transfer::csv_preview(std::path::Path::new(&path), &encoding, delim, has_header, 20)?)
}

/// Import a CSV file into a table, in batches of rows that are each saved on their own.
#[tauri::command]
pub async fn db_import_csv(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: Uuid,
    job_id: Uuid,
    spec: crate::db::transfer::ImportSpec,
    path: String,
) -> ApiResult<crate::db::transfer::ImportSummary> {
    let session = state.dbs.get(session_id)?;
    let out = crate::db::transfer::import_csv(&session.engine, job_id, &spec, std::path::Path::new(&path), |rows| {
        let _ = app.emit(EVENT_DB_TRANSFER, TransferProgress { job_id, rows });
    })
    .await?;
    Ok(out)
}

/// Stop an import after the batch it is saving.
#[tauri::command]
pub fn db_stop_import(job_id: Uuid) -> ApiResult<()> {
    crate::db::transfer::request_stop(job_id);
    Ok(())
}
