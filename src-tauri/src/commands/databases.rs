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
