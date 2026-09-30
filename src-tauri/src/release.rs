//! Updates straight from the public GitHub Releases page.
//!
//! Used when a build has no update-signing key (so the signed Tauri updater
//! is off): find the newest release, pick the installer that matches how this
//! copy was installed, download it, check it against the SHA-256 GitHub
//! publishes for the file, and install it.
//!
//! No Tauri types here, so it can be built and tested on its own.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const REPO: &str = "aliysefian/termius";

/// How this copy is installed, which decides what to download and how to
/// install it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    /// A self-contained AppImage: replaced in place.
    AppImage,
    /// Installed from the .deb: updated with dpkg (asks for the password).
    Deb,
    /// Installed from the .rpm: updated with rpm (asks for the password).
    Rpm,
    /// Windows: the NSIS setup program updates in place.
    Nsis,
    /// Anything else: the release page is opened instead.
    Manual,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhAsset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    /// "sha256:<hex>", published by GitHub for every release asset.
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhRelease {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetInfo {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseCheck {
    pub current: String,
    pub latest: String,
    /// The latest release is newer than this copy.
    pub newer: bool,
    pub url: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub install: InstallKind,
    /// The file that would be installed; None means "open the release page".
    pub asset: Option<AssetInfo>,
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// "v1.2.3" or "1.2.3" as a semver version.
pub fn parse_version(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.trim().trim_start_matches(['v', 'V'])).ok()
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Debian/RPM and installer spellings of this machine's CPU architecture.
fn arch_names() -> (&'static str, &'static str, &'static str) {
    match std::env::consts::ARCH {
        "aarch64" => ("arm64", "aarch64", "arm64"),
        "x86" => ("i386", "i686", "x86"),
        _ => ("amd64", "x86_64", "x64"),
    }
}

/// The release file to install for `kind`, if the release has one.
pub fn pick_asset(kind: InstallKind, assets: &[GhAsset]) -> Option<&GhAsset> {
    let (deb, rpm, win) = arch_names();
    let want: Box<dyn Fn(&str) -> bool> = match kind {
        InstallKind::AppImage => Box::new(move |n: &str| n.ends_with(".AppImage") && n.contains(deb)),
        InstallKind::Deb => Box::new(move |n: &str| n.ends_with(&format!("_{deb}.deb"))),
        InstallKind::Rpm => Box::new(move |n: &str| n.ends_with(&format!(".{rpm}.rpm"))),
        InstallKind::Nsis => Box::new(move |n: &str| n.ends_with(&format!("_{win}-setup.exe"))),
        InstallKind::Manual => return None,
    };
    assets.iter().find(|a| want(&a.name))
}

/// The 32 bytes of a "sha256:<hex>" digest.
pub fn parse_sha256(digest: &str) -> Option<[u8; 32]> {
    let hex = digest.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Only files from this project's own releases are ever downloaded.
pub fn trusted_download_url(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{REPO}/releases/download/"))
}

/// `/proc/self/exe` reads "<path> (deleted)" once a package upgrade has
/// replaced the running binary.
pub fn clean_exe_path(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(s) => PathBuf::from(s),
        None => p,
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

fn command_succeeds(program: &str, args: &[&std::ffi::OsStr]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn detect_install_kind() -> InstallKind {
    if cfg!(target_os = "windows") {
        return InstallKind::Nsis;
    }
    if !cfg!(target_os = "linux") {
        return InstallKind::Manual;
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return InstallKind::AppImage;
    }
    let Ok(exe) = std::env::current_exe().map(clean_exe_path) else {
        return InstallKind::Manual;
    };
    let exe = exe.as_os_str();
    if command_succeeds("dpkg-query", &["-S".as_ref(), exe]) {
        InstallKind::Deb
    } else if command_succeeds("rpm", &["-qf".as_ref(), exe]) {
        InstallKind::Rpm
    } else {
        InstallKind::Manual
    }
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

fn client(current: &str) -> Result<reqwest::Client, String> {
    // reqwest is built without a bundled TLS crypto provider (it shares the
    // updater plugin's build); install ring once, as the plugin does.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(format!("SSHVault/{current} (+https://github.com/{REPO})"))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("couldn't start the download client: {e}"))
}

/// The newest published (non-draft, non-prerelease) release.
pub async fn latest_release(current: &str) -> Result<GhRelease, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let res = client(current)?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("couldn't reach GitHub: {e}"))?;
    let status = res.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("GitHub is rate-limiting update checks from this network. Try again in an hour.".into());
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("no release has been published yet".into());
    }
    if !status.is_success() {
        return Err(format!("GitHub answered {status}"));
    }
    let bytes = res.bytes().await.map_err(|e| format!("couldn't read GitHub's answer: {e}"))?;
    let rel: GhRelease = serde_json::from_slice(&bytes).map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    if rel.draft || rel.prerelease {
        return Err("the latest release isn't final yet".into());
    }
    Ok(rel)
}

pub fn summarize(rel: &GhRelease, current: &str, kind: InstallKind) -> ReleaseCheck {
    let latest = rel.tag_name.trim_start_matches(['v', 'V']).to_string();
    ReleaseCheck {
        current: current.to_string(),
        newer: is_newer(&rel.tag_name, current),
        latest,
        url: rel.html_url.clone(),
        notes: rel.body.clone().unwrap_or_default(),
        published_at: rel.published_at.clone(),
        install: kind,
        asset: pick_asset(kind, &rel.assets)
            .filter(|a| a.digest.as_deref().and_then(parse_sha256).is_some() && trusted_download_url(&a.browser_download_url))
            .map(|a| AssetInfo { name: a.name.clone(), size: a.size }),
    }
}

/// Download `asset` to `dest`, reporting (done, total) bytes, and keep it only
/// if its SHA-256 matches the one GitHub published.
pub async fn download_verified(
    current: &str,
    asset: &GhAsset,
    dest: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    if !trusted_download_url(&asset.browser_download_url) {
        return Err("refusing to download from outside this project's releases".into());
    }
    let expected = asset
        .digest
        .as_deref()
        .and_then(parse_sha256)
        .ok_or("GitHub didn't publish a checksum for this file, so it can't be verified")?;
    let part = dest.with_extension("part");
    let result = async {
        let mut res = client(current)?
            .get(&asset.browser_download_url)
            .send()
            .await
            .map_err(|e| format!("download failed: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("download failed: GitHub answered {}", res.status()));
        }
        let total = res.content_length().unwrap_or(asset.size);
        let mut file = tokio::fs::File::create(&part).await.map_err(|e| format!("couldn't save the download: {e}"))?;
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        progress(0, total);
        use tokio::io::AsyncWriteExt;
        loop {
            let chunk = tokio::time::timeout(Duration::from_secs(60), res.chunk())
                .await
                .map_err(|_| "the download stalled".to_string())?
                .map_err(|e| format!("download failed: {e}"))?;
            let Some(chunk) = chunk else { break };
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(|e| format!("couldn't save the download: {e}"))?;
            done += chunk.len() as u64;
            progress(done, total);
        }
        file.flush().await.map_err(|e| format!("couldn't save the download: {e}"))?;
        drop(file);
        if hasher.finalize().as_slice() != expected {
            return Err("the downloaded file doesn't match GitHub's checksum, so it wasn't installed".into());
        }
        tokio::fs::rename(&part, dest).await.map_err(|e| format!("couldn't save the download: {e}"))
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
    }
    result
}

// ---------------------------------------------------------------------------
// Installing
// ---------------------------------------------------------------------------

/// What the caller should do once `install` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// Installed; start `relaunch` and exit.
    Restart,
    /// A separate installer is running; exit so it can replace the files.
    Exit,
}

/// Where to download to before installing. The AppImage goes next to the
/// running one so the swap is an atomic rename on the same disk.
pub fn download_path(kind: InstallKind, asset: &GhAsset, cache_dir: &Path) -> Result<PathBuf, String> {
    if kind == InstallKind::AppImage {
        let current = std::env::var_os("APPIMAGE").map(PathBuf::from).ok_or("not running as an AppImage")?;
        let dir = current.parent().ok_or("can't tell where the AppImage is")?;
        return Ok(dir.join(format!(".{}.download", asset.name)));
    }
    let dir = cache_dir.join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    Ok(dir.join(&asset.name))
}

/// Install a verified download. Returns the program to start afterwards
/// (for `Outcome::Restart`).
pub async fn install(kind: InstallKind, file: &Path) -> Result<(Outcome, Option<PathBuf>), String> {
    match kind {
        InstallKind::AppImage => {
            let target = std::env::var_os("APPIMAGE").map(PathBuf::from).ok_or("not running as an AppImage")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| format!("couldn't make the new AppImage executable: {e}"))?;
            }
            // The running copy stays mounted from the old file until exit.
            std::fs::rename(file, &target).map_err(|e| {
                let _ = std::fs::remove_file(file);
                format!("couldn't replace {}: {e}. Move the AppImage somewhere you can write to, or download the update by hand.", target.display())
            })?;
            Ok((Outcome::Restart, Some(target)))
        }
        InstallKind::Deb | InstallKind::Rpm => {
            let exe = std::env::current_exe().map(clean_exe_path).map_err(|e| e.to_string())?;
            // pkexec shows the desktop's own password prompt.
            let (tool, args): (&str, &[&str]) = if kind == InstallKind::Deb { ("dpkg", &["-i"]) } else { ("rpm", &["-U", "--replacepkgs"]) };
            let status = tokio::process::Command::new("pkexec")
                .arg(tool)
                .args(args)
                .arg(file)
                .status()
                .await
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        "pkexec isn't installed, so the package can't be installed from here. Open the downloaded file to install it.".to_string()
                    } else {
                        format!("couldn't start the installer: {e}")
                    }
                })?;
            match status.code() {
                Some(0) => {
                    let _ = std::fs::remove_file(file);
                    Ok((Outcome::Restart, Some(exe)))
                }
                // pkexec: 126 = dialog dismissed / not authorized, 127 = no auth agent.
                Some(126) | Some(127) => Err("installing needs your password, and the request was cancelled".into()),
                _ => Err(format!("{tool} couldn't install the update ({status})")),
            }
        }
        InstallKind::Nsis => {
            // /P passive (progress only), /UPDATE keep settings, /R reopen SSHVault afterwards.
            let args = ["/P", "/UPDATE", "/R"];
            match std::process::Command::new(file).args(args).spawn() {
                Ok(_) => Ok((Outcome::Exit, None)),
                // ERROR_ELEVATION_REQUIRED: a per-machine install; let Windows ask.
                Err(e) if e.raw_os_error() == Some(740) => {
                    let path = file.to_string_lossy().replace('\'', "''");
                    std::process::Command::new("powershell")
                        .args([
                            "-NoProfile",
                            "-NonInteractive",
                            "-Command",
                            &format!("Start-Process -FilePath '{path}' -ArgumentList '/P','/UPDATE','/R' -Verb RunAs"),
                        ])
                        .spawn()
                        .map_err(|e| format!("couldn't start the installer: {e}"))?;
                    Ok((Outcome::Exit, None))
                }
                Err(e) => Err(format!("couldn't start the installer: {e}")),
            }
        }
        InstallKind::Manual => Err("this copy can't install updates itself".into()),
    }
}

/// Start `program` once this process has exited, so the new copy never
/// overlaps the old one (both would claim the same control socket).
pub fn relaunch_after_exit(program: &Path, args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(r#"pid=$1; shift; i=0; while kill -0 "$pid" 2>/dev/null && [ $i -lt 150 ]; do sleep 0.2; i=$((i+1)); done; exec "$@""#)
            .arg("sshvault-relaunch")
            .arg(std::process::id().to_string())
            .arg(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("couldn't restart SSHVault: {e}"))
    }
    #[cfg(not(unix))]
    {
        std::process::Command::new(program)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("couldn't restart SSHVault: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GhAsset {
        GhAsset {
            name: name.into(),
            size: 1,
            browser_download_url: format!("https://github.com/{REPO}/releases/download/v1.0.0/{name}"),
            digest: Some(format!("sha256:{}", "ab".repeat(32))),
        }
    }

    #[test]
    fn versions() {
        assert!(is_newer("v0.9.6", "0.9.5"));
        assert!(is_newer("v0.10.0", "0.9.5"));
        assert!(is_newer("1.0.0", "1.0.0-beta.1"));
        assert!(!is_newer("v0.9.5", "0.9.5"));
        assert!(!is_newer("v0.9.4", "0.9.5"));
        assert!(!is_newer("nightly", "0.9.5"));
    }

    #[test]
    fn picks_the_matching_installer() {
        let assets: Vec<_> = [
            "SSHVault-0.9.5-1.x86_64.rpm",
            "SSHVault_0.9.5_amd64.AppImage",
            "SSHVault_0.9.5_amd64.AppImage.sig",
            "SSHVault_0.9.5_amd64.deb",
            "SSHVault_0.9.5_x64-setup.exe",
            "SSHVault_0.9.5_x64-setup.exe.sig",
            "SSHVault_0.9.5_x64_en-US.msi",
        ]
        .into_iter()
        .map(asset)
        .collect();
        if std::env::consts::ARCH != "x86_64" {
            return;
        }
        let name = |k| pick_asset(k, &assets).map(|a| a.name.as_str());
        assert_eq!(name(InstallKind::AppImage), Some("SSHVault_0.9.5_amd64.AppImage"));
        assert_eq!(name(InstallKind::Deb), Some("SSHVault_0.9.5_amd64.deb"));
        assert_eq!(name(InstallKind::Rpm), Some("SSHVault-0.9.5-1.x86_64.rpm"));
        assert_eq!(name(InstallKind::Nsis), Some("SSHVault_0.9.5_x64-setup.exe"));
        assert_eq!(name(InstallKind::Manual), None);
    }

    #[test]
    fn digests_and_urls() {
        assert_eq!(parse_sha256(&format!("sha256:{}", "0f".repeat(32))), Some([0x0f; 32]));
        assert_eq!(parse_sha256("sha256:abc"), None);
        assert_eq!(parse_sha256(&format!("sha512:{}", "0f".repeat(32))), None);
        assert_eq!(parse_sha256(&format!("sha256:{}", "zz".repeat(32))), None);
        assert!(trusted_download_url(&format!("https://github.com/{REPO}/releases/download/v1/x.deb")));
        assert!(!trusted_download_url("https://github.com/evil/termius/releases/download/v1/x.deb"));
        assert!(!trusted_download_url(&format!("http://github.com/{REPO}/releases/download/v1/x.deb")));
    }

    #[test]
    fn deleted_exe_suffix() {
        assert_eq!(clean_exe_path("/usr/bin/sshvault (deleted)".into()), PathBuf::from("/usr/bin/sshvault"));
        assert_eq!(clean_exe_path("/usr/bin/sshvault".into()), PathBuf::from("/usr/bin/sshvault"));
    }

    #[test]
    fn no_checksum_means_no_asset() {
        let mut a = asset("SSHVault_1.0.0_amd64.deb");
        a.digest = None;
        let rel = GhRelease {
            tag_name: "v1.0.0".into(),
            html_url: String::new(),
            body: None,
            published_at: None,
            draft: false,
            prerelease: false,
            assets: vec![a],
        };
        let s = summarize(&rel, "0.9.5", InstallKind::Deb);
        assert!(s.newer);
        assert_eq!(s.latest, "1.0.0");
        assert!(s.asset.is_none());
    }
}
