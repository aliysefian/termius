//! A check that no plain function calls `tokio::spawn`.
//!
//! Tauri runs a non-`async` command on the main thread, where there is no tokio runtime, so `tokio::spawn` in code a
//! sync command can reach panics, and a release build aborts on a panic: the app closes with no message. That closed
//! the app three times (container logs, Kubernetes logs, the monitor's log stream). Code that may be called from a
//! sync command must use `tauri::async_runtime::spawn`, which works from any thread.
//!
//! The test below reads the sources and flags a `tokio::spawn(` whose nearest enclosing function is not `async` (a
//! spawn inside an `async fn` or an `async` block is already running on the runtime, so it is fine). A spawn that is
//! known to be safe for another reason says so with `// spawn-ok: <why>` on the same or the previous line.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    const NEEDLES: [&str; 2] = ["tokio::spawn(", "tokio::task::spawn("];

    /// Line numbers (1-based) of spawns outside any async context in `source`.
    fn violations(source: &str) -> Vec<usize> {
        // Only the code before the test module counts.
        let code = source.split("\n#[cfg(test)]").next().unwrap_or(source);
        let bytes = code.as_bytes();
        let mut out = Vec::new();
        for needle in NEEDLES {
            let mut from = 0;
            while let Some(i) = code[from..].find(needle) {
                let at = from + i;
                from = at + needle.len();
                let line_start = code[..at].rfind('\n').map_or(0, |n| n + 1);
                let line = code[line_start..].lines().next().unwrap_or("");
                // A mention in a comment is not a call.
                if line.trim_start().starts_with("//") || line[..at - line_start].contains("//") {
                    continue;
                }
                let prev_line = code[..line_start].lines().last().unwrap_or("");
                if line.contains("spawn-ok:") || prev_line.contains("spawn-ok:") {
                    continue;
                }
                if !in_async_context(bytes, code, at) {
                    out.push(code[..at].matches('\n').count() + 1);
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Walks outward from `at` through the enclosing blocks: an `async` block or `async fn` means yes, a plain `fn` no.
    fn in_async_context(bytes: &[u8], code: &str, at: usize) -> bool {
        let mut depth = 0usize;
        let mut i = at;
        while i > 0 {
            i -= 1;
            match bytes[i] {
                b'}' => depth += 1,
                b'{' if depth > 0 => depth -= 1,
                b'{' => {
                    let start = code[..i].rfind([';', '{', '}']).map_or(0, |n| n + 1);
                    let header = &code[start..i];
                    if header.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "async") {
                        return true;
                    }
                    if header.contains("fn ") {
                        return false;
                    }
                    // `impl`, `if`, `match`, a closure...: keep going outward.
                }
                _ => {}
            }
        }
        // Not inside any function (a static, a macro body): nothing to say.
        true
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    #[test]
    fn the_scanner_catches_the_pattern_that_closed_the_app() {
        // The three shapes that shipped: a plain method that spawns.
        let bad = "impl M {\n    pub fn start_stream(self: &Arc<Self>, id: Uuid) -> Result<Uuid> {\n        let me = Arc::clone(self);\n        tokio::spawn(async move {\n            work().await;\n        });\n        Ok(id)\n    }\n}\n";
        assert_eq!(violations(bad), vec![4]);
        let in_closure = "fn f() {\n    let go = || {\n        tokio::spawn(async {});\n    };\n}\n";
        assert_eq!(violations(in_closure), vec![3]);
        let task_path = "fn f() {\n    tokio::task::spawn(async {});\n}\n";
        assert_eq!(violations(task_path), vec![2]);
    }

    #[test]
    fn the_scanner_leaves_alone_what_is_already_on_the_runtime() {
        let ok_async_fn = "impl M {\n    pub async fn start(&self) {\n        tokio::spawn(async {});\n    }\n}\n";
        assert!(violations(ok_async_fn).is_empty());
        let ok_async_block = "fn f() {\n    tauri::async_runtime::spawn(async move {\n        tokio::spawn(async {});\n    });\n}\n";
        assert!(violations(ok_async_block).is_empty());
        let ok_nested = "async fn f() {\n    if x {\n        for i in 0..3 {\n            tokio::spawn(async move { i });\n        }\n    }\n}\n";
        assert!(violations(ok_nested).is_empty());
        let fixed = "fn start() {\n    tauri::async_runtime::spawn(async {});\n}\n";
        assert!(violations(fixed).is_empty());
        let comment = "fn f() {\n    // tokio::spawn(x) would panic here\n    let s = 1; // tokio::spawn(y)\n}\n";
        assert!(violations(comment).is_empty());
        let marked = "fn f() {\n    // spawn-ok: only called from async code\n    tokio::spawn(async {});\n    tokio::spawn(async {}); // spawn-ok: same\n}\n";
        assert!(violations(marked).is_empty());
        let tests_after = "fn f() {}\n#[cfg(test)]\nmod t {\n    fn g() {\n        tokio::spawn(async {});\n    }\n}\n";
        assert!(violations(tests_after).is_empty());
    }

    #[test]
    fn no_plain_function_in_the_app_spawns_on_tokio_directly() {
        // `SPAWNLINT_DIR` lets the check run against the sources from a stripped-down crate.
        let dir = std::env::var_os("SPAWNLINT_DIR").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
        let mut files = Vec::new();
        rust_files(&dir, &mut files);
        assert!(!files.is_empty(), "no sources found under {}", dir.display());
        let mut found = Vec::new();
        for f in files {
            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // The tests may build their own runtime, and this file only mentions the call.
            if name == "spawnlint.rs" || name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            for line in violations(&text) {
                found.push(format!("{}:{line}", f.strip_prefix(&dir).unwrap_or(&f).display()));
            }
        }
        assert!(
            found.is_empty(),
            "tokio::spawn outside an async context (use tauri::async_runtime::spawn, or mark it `// spawn-ok: why`):\n  {}",
            found.join("\n  ")
        );
    }
}
