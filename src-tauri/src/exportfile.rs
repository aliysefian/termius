//! Writing an export the person chose in the Save dialog (a snippet pack, a log). Narrow on purpose: a `.json` or
//! `.log` file, at most 2 MB, so the command that uses it can't put anything else anywhere.

use std::path::Path;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// What may be written: data and plain-text logs, never anything a shell or the system would run.
const ALLOWED: [&str; 2] = ["json", "log"];

/// Written beside the target and renamed into place, so a failed write never leaves half a file.
pub fn write_json(p: &Path, contents: &str) -> Result<(), String> {
    if !p.extension().is_some_and(|e| ALLOWED.iter().any(|a| e.eq_ignore_ascii_case(a))) {
        return Err("Only a .json or .log file can be written here.".into());
    }
    if contents.len() > MAX_BYTES {
        return Err("That is too large to export.".into());
    }
    write_atomic(p, contents)
}

/// The encrypted host export: only the `.svhosts` file the person named, up to the size an import will read.
pub fn write_host_export(p: &Path, contents: &str) -> Result<(), String> {
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case(crate::hostexport::EXTENSION)) {
        return Err(format!("An export is saved as a .{} file.", crate::hostexport::EXTENSION));
    }
    if contents.len() as u64 > crate::hostexport::MAX_FILE_BYTES {
        return Err("That is too large to export.".into());
    }
    write_atomic(p, contents)
}

/// A host list for another SSH client (MobaXterm sessions, an Ásbrú config): only those file types, same size cap.
pub fn write_interchange(p: &Path, contents: &str) -> Result<(), String> {
    const TYPES: [&str; 3] = ["mxtsessions", "yml", "yaml"];
    if !p.extension().is_some_and(|e| TYPES.iter().any(|a| e.eq_ignore_ascii_case(a))) {
        return Err("A host list for another client is saved as a .mxtsessions, .yml or .yaml file.".into());
    }
    if contents.len() > MAX_BYTES {
        return Err("That is too large to export.".into());
    }
    write_atomic(p, contents)
}

fn write_atomic(p: &Path, contents: &str) -> Result<(), String> {
    let name = p.file_name().ok_or("No file name.")?.to_string_lossy().into_owned();
    let tmp = p.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, contents).map_err(|e| format!("{}: {e}", p.display()))?;
    std::fs::rename(&tmp, p).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", p.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_json_file_and_leaves_no_temporary_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("pack.json");
        write_json(&p, "{\"a\":1}").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\"a\":1}");
        write_json(&p, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{}");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(leftovers, ["pack.json"]);
        assert!(write_json(&dir.path().join("UPPER.JSON"), "{}").is_ok());
    }

    #[test]
    fn a_host_export_is_written_only_as_svhosts() {
        let dir = tempfile::TempDir::new().unwrap();
        write_host_export(&dir.path().join("hosts.svhosts"), "{}").unwrap();
        write_host_export(&dir.path().join("HOSTS.SVHOSTS"), "{}").unwrap();
        for name in ["hosts.json", "hosts", "hosts.svhosts.sh", ".bashrc", "hosts.log"] {
            assert!(write_host_export(&dir.path().join(name), "x").is_err(), "{name}");
            assert!(!dir.path().join(name).exists(), "{name}");
        }
        // And the other writer doesn't take that extension.
        assert!(write_json(&dir.path().join("x.svhosts"), "{}").is_err());
    }

    #[test]
    fn an_interchange_file_is_written_only_as_a_host_list_type() {
        let dir = tempfile::TempDir::new().unwrap();
        for name in ["a.mxtsessions", "b.yml", "C.YAML"] {
            write_interchange(&dir.path().join(name), "x").unwrap();
        }
        for name in ["a.sh", "a.json", ".bashrc", "a.yml.sh", "yml"] {
            assert!(write_interchange(&dir.path().join(name), "x").is_err(), "{name}");
            assert!(!dir.path().join(name).exists(), "{name}");
        }
    }

    #[test]
    fn refuses_other_kinds_of_file_and_big_ones_and_missing_folders() {
        let dir = tempfile::TempDir::new().unwrap();
        write_json(&dir.path().join("session.log"), "a line\n").unwrap();
        for name in ["pack.txt", "pack", "pack.json.exe", "pack.sh", ".bashrc", "authorized_keys", "run.log.sh", "x.csv"] {
            assert!(write_json(&dir.path().join(name), "x").is_err(), "{name}");
            assert!(!dir.path().join(name).exists(), "{name}");
        }
        assert!(write_json(&dir.path().join("big.json"), &"x".repeat(MAX_BYTES + 1)).is_err());
        assert!(write_json(&dir.path().join("big.json"), &"x".repeat(MAX_BYTES)).is_ok());
        assert!(write_json(&dir.path().join("no/such/folder/p.json"), "{}").is_err());
    }
}
