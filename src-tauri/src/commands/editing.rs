//! The in-app editor and the overwrite dialog: reading a text file with its owner, saving it (through `sudo` when the
//! person may not write it), and checksums.

use super::*;

use crate::fileedit::{self, Checksum, EditError, RootError, Saved, TextFile};

/// What the editor opens at most unless asked for less (the compare view reads smaller limits).
const EDITOR_MAX: usize = fileedit::MAX_TEXT_BYTES;

fn edit_error(e: EditError) -> ApiError {
    match e {
        EditError::NotText(why) => ApiError::new("not_text", format!("this is not a text file: {why}")),
        EditError::TooLarge { size, max } => ApiError::new("too_large", format!("this file is {size} bytes; the editor opens files up to {max}")),
        EditError::Changed => ApiError::new("changed_on_server", "the file was changed on the host after it was opened"),
        EditError::File(f) if fileedit::is_permission_denied(&f) => ApiError::new("permission_denied", f.to_string()),
        EditError::File(f) => f.into(),
    }
}

fn root_error(e: RootError) -> ApiError {
    let code = match &e {
        RootError::NeedsPassword => "sudo_password_required",
        RootError::WrongPassword => "sudo_wrong_password",
        RootError::NotAllowed(_) => "sudo_not_allowed",
        RootError::Unsupported => "unsupported",
        RootError::Failed(_) => "sudo_failed",
    };
    ApiError::new(code, e.to_string())
}

/// Read a text file into memory, with its size, time, permissions and owner.
#[tauri::command]
pub async fn files_read_text(state: State<'_, AppState>, session_id: String, path: String, max_bytes: Option<usize>) -> ApiResult<TextFile> {
    let b = state.files.get(&session_id)?;
    let max = max_bytes.unwrap_or(EDITOR_MAX).min(EDITOR_MAX);
    fileedit::read_text(b.as_ref(), &path, max).await.map_err(edit_error)
}

/// Save text over a file in place. With `expect_modified`, a file someone else changed meanwhile is not overwritten.
#[tauri::command]
pub async fn files_write_text(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    text: String,
    crlf: bool,
    expect_modified: Option<u64>,
) -> ApiResult<Saved> {
    let b = state.files.get(&session_id)?;
    fileedit::write_text(b.as_ref(), &path, &text, crlf, expect_modified).await.map_err(edit_error)
}

/// Save text over a file through `sudo`, on a host the person has no right to write to. The password is held only for
/// this call and wiped after it.
#[tauri::command]
pub async fn files_write_as_root(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    text: String,
    crlf: bool,
    password: Option<String>,
) -> ApiResult<()> {
    let b = state.files.get(&session_id)?;
    let password = password.map(Zeroizing::new);
    fileedit::write_as_root(b.as_ref(), &path, &text, crlf, password.as_deref().map(String::as_str)).await.map_err(root_error)
}

/// The SHA-256 of a file, computed by the host where it can run commands.
#[tauri::command]
pub async fn files_checksum(state: State<'_, AppState>, session_id: String, path: String) -> ApiResult<Checksum> {
    let b = state.files.get(&session_id)?;
    fileedit::checksum(b.as_ref(), &path).await.map_err(edit_error)
}
