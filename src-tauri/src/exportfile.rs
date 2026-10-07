//! Writing an export the person chose in the Save dialog (a snippet pack). Narrow on purpose: a `.json`
//! file, at most 2 MB, so the command that uses it can't put anything else anywhere.

use std::path::Path;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;

/// Written beside the target and renamed into place, so a failed write never leaves half a file.
pub fn write_json(p: &Path, contents: &str) -> Result<(), String> {
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
        return Err("Only a .json file can be written here.".into());
    }
    if contents.len() > MAX_BYTES {
        return Err("That is too large to export.".into());
    }
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
    fn refuses_other_kinds_of_file_and_big_ones_and_missing_folders() {
        let dir = tempfile::TempDir::new().unwrap();
        for name in ["pack.txt", "pack", "pack.json.exe", "pack.sh", ".bashrc", "authorized_keys"] {
            assert!(write_json(&dir.path().join(name), "x").is_err(), "{name}");
            assert!(!dir.path().join(name).exists(), "{name}");
        }
        assert!(write_json(&dir.path().join("big.json"), &"x".repeat(MAX_BYTES + 1)).is_err());
        assert!(write_json(&dir.path().join("big.json"), &"x".repeat(MAX_BYTES)).is_ok());
        assert!(write_json(&dir.path().join("no/such/folder/p.json"), "{}").is_err());
    }
}
