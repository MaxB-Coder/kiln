//! Kiln's updater core: reads a signed release manifest, downloads an update, checks its
//! minisign signature and puts it in place. The app's update offer (sidebar line, palette,
//! dialogs) lives in `kiln::update`; this crate has no UI code, so its tests build quickly and
//! the release workflow can run them against the packages it has just built.
//!
//! What gets replaced: the app bundle on macOS, the AppImage on Linux, and on Windows the new
//! installer runs (and this process exits). Installs other than those (a `cargo run`, a .deb)
//! never update themselves.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context as _, bail};
use cargo_packager_updater::{
    Config as UpdaterConfig, RemoteRelease, WindowsConfig, WindowsUpdateInstallMode,
};
pub use cargo_packager_updater::{Update, UpdateFormat};
pub use semver::Version;

/// A manifest is a few hundred bytes; anything much larger is not one.
const MANIFEST_LIMIT: usize = 256 * 1024;

/// Where to look and whose signature to trust.
#[derive(Clone, Debug)]
pub struct Source {
    pub endpoint: url::Url,
    pub pubkey: String,
}

impl Source {
    /// The values built into this binary (set by the release workflow), if any.
    pub fn built_in() -> Option<Source> {
        Source::new(
            option_env!("KILN_UPDATE_ENDPOINT")?,
            option_env!("KILN_UPDATE_PUBKEY")?,
        )
    }

    /// An HTTPS manifest address and a minisign public key (base64, as cargo-packager writes
    /// it). Plain HTTP is accepted only for this machine, for tests.
    pub fn new(endpoint: &str, pubkey: &str) -> Option<Source> {
        let endpoint = url::Url::parse(endpoint.trim()).ok()?;
        let local = matches!(
            endpoint.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]")
        );
        if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && local) {
            return None;
        }
        let pubkey = pubkey.trim();
        (!pubkey.is_empty()).then(|| Source {
            endpoint,
            pubkey: pubkey.to_string(),
        })
    }
}

/// The AppImage this process runs from, read once at startup.
static APPIMAGE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// The variables an AppImage's runtime sets for the program inside it.
const APPIMAGE_VARS: &[&str] = &["APPIMAGE", "APPDIR", "ARGV0", "OWD"];

/// Remembers which AppImage this process runs from, then removes the AppImage runtime's
/// variables from the environment, so the shells Kiln starts do not inherit them.
///
/// # Safety
///
/// Changes the process environment: call it first thing in `main`, before any other thread
/// starts.
pub unsafe fn take_appimage_env() {
    let path = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    let _ = APPIMAGE.set(path);
    for var in APPIMAGE_VARS {
        // SAFETY: the caller guarantees no other thread reads or writes the environment.
        unsafe { std::env::remove_var(var) };
    }
}

/// The AppImage this process runs from (Linux).
pub fn appimage() -> Option<PathBuf> {
    APPIMAGE
        .get_or_init(|| std::env::var_os("APPIMAGE").map(PathBuf::from))
        .clone()
}

/// What the updater replaces: the app bundle (macOS), the AppImage (Linux) or the install
/// folder (Windows). `None` when Kiln was not installed in a way it can update itself.
pub fn install_target() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if cfg!(target_os = "macos") {
        // …/Kiln.app/Contents/MacOS/kiln
        let bundle = exe.parent()?.parent()?.parent()?;
        (bundle.extension()? == "app").then(|| bundle.to_path_buf())
    } else if cfg!(windows) {
        // The NSIS installer leaves its uninstaller next to the app.
        let dir = exe.parent()?;
        dir.join("uninstall.exe")
            .is_file()
            .then(|| dir.to_path_buf())
    } else {
        appimage()
    }
}

/// What to start again after an update: the AppImage on Linux (the running binary lives inside
/// its mounted image, which goes away with this process), else the platform's default.
pub fn restart_path() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        appimage()
    } else {
        None
    }
}

/// Reads the manifest and returns the update when it is newer. Blocking.
pub fn check(
    source: &Source,
    current: &Version,
    install_to: PathBuf,
) -> anyhow::Result<Option<Update>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("Kiln/{current}"))
        .build()?;
    let response = client
        .get(source.endpoint.clone())
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .context("reaching the update server")?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    let response = response.error_for_status()?;
    let body = response.bytes()?;
    if body.len() > MANIFEST_LIMIT {
        bail!("the update manifest is too large ({} bytes)", body.len());
    }
    let release: RemoteRelease =
        serde_json::from_slice(&body).context("reading the update manifest")?;
    if release.version <= *current {
        return Ok(None);
    }
    let target = cargo_packager_updater::target().context("no updates for this platform")?;
    Ok(Some(Update {
        config: UpdaterConfig {
            endpoints: vec![source.endpoint.clone()],
            pubkey: source.pubkey.clone(),
            windows: Some(WindowsConfig {
                installer_args: None,
                install_mode: Some(WindowsUpdateInstallMode::Passive),
            }),
        },
        body: release.notes.clone(),
        current_version: current.to_string(),
        version: release.version.to_string(),
        date: release.pub_date,
        target: target.split('-').next().unwrap_or_default().to_string(),
        extract_path: install_to,
        download_url: release.download_url(&target)?.clone(),
        signature: release.signature(&target)?.clone(),
        timeout: Some(Duration::from_secs(600)),
        headers: Default::default(),
        format: release.format(&target)?,
    }))
}

/// Downloads the update, checks its signature, and puts it in place. Blocking. On Windows the
/// installer takes over and this process exits.
pub fn download_and_install(update: &Update) -> anyhow::Result<()> {
    // `download` refuses anything the key did not sign; only those bytes reach `install`.
    let bytes = update.download().context("downloading the update")?;
    update.install(bytes).context("installing the update")?;
    Ok(())
}

/// The manifest platform key for this machine ("linux-x86_64").
pub fn platform() -> Option<String> {
    cargo_packager_updater::target()
}

/// Whether `path` is a file this updater installs from (for scripts and tests).
pub fn format_for(path: &Path) -> Option<UpdateFormat> {
    let name = path.file_name()?.to_str()?;
    if name.ends_with(".app.tar.gz") {
        Some(UpdateFormat::App)
    } else if name.ends_with(".AppImage") {
        Some(UpdateFormat::AppImage)
    } else if name.ends_with("-setup.exe") {
        Some(UpdateFormat::Nsis)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_need_https_except_on_this_machine() {
        assert!(Source::new("https://example.com/latest.json", "key").is_some());
        assert!(Source::new("http://example.com/latest.json", "key").is_none());
        assert!(Source::new("http://127.0.0.1:9/latest.json", "key").is_some());
        assert!(Source::new("https://example.com/latest.json", " ").is_none());
        assert!(Source::new("not a url", "key").is_none());
    }

    #[test]
    fn dev_builds_do_not_update_themselves() {
        // Tests run from target/, not from an installed app (and not from an AppImage).
        if std::env::var_os("APPIMAGE").is_none() {
            assert_eq!(install_target(), None);
        }
    }

    #[test]
    fn knows_the_update_files() {
        assert!(matches!(
            format_for(Path::new("Kiln.app.tar.gz")),
            Some(UpdateFormat::App)
        ));
        assert!(matches!(
            format_for(Path::new("Kiln-2026.10.26-x86_64.AppImage")),
            Some(UpdateFormat::AppImage)
        ));
        assert!(matches!(
            format_for(Path::new("Kiln_2026.10.26_x64-setup.exe")),
            Some(UpdateFormat::Nsis)
        ));
        assert!(format_for(Path::new("kiln_2026.10.26_amd64.deb")).is_none());
    }
}
