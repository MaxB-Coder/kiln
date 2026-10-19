//! Updates: Kiln's only network use.
//!
//! A release build carries the address of its release manifest and the public key its updates
//! are signed with (`KILN_UPDATE_ENDPOINT`, `KILN_UPDATE_PUBKEY`, set by the release workflow).
//! Builds without them never check. Kiln checks a minute after launch and then once a day,
//! offers the update in the sidebar and the palette, and installs only when asked. The download
//! is checked against the key before anything is replaced.
//!
//! Installing uses cargo-packager's updater: on macOS it swaps the app bundle, on Linux the
//! AppImage, and on Windows it runs the new installer (Kiln closes for that). Builds that are
//! not installed that way (a `cargo run`, a .deb) do not check.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, bail};
use cargo_packager_updater::{
    Config as UpdaterConfig, RemoteRelease, Update, WindowsConfig, WindowsUpdateInstallMode,
};
use gpui::{App, Global, PromptLevel, Window};
use semver::Version;

/// How long after launch the first check runs, and how often after that.
const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// A manifest is a few hundred bytes; anything much larger is not one.
const MANIFEST_LIMIT: usize = 256 * 1024;

/// Where to look and whose signature to trust, when this build has them.
#[derive(Clone, Debug)]
pub struct Source {
    pub endpoint: url::Url,
    pub pubkey: String,
}

impl Source {
    /// The values built into this binary.
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
        std::env::var_os("APPIMAGE").map(PathBuf::from)
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

/// The update shown in the app.
#[derive(Clone, Default)]
pub enum UpdateState {
    #[default]
    None,
    Available(Arc<Update>),
    Installing(String),
    /// Installed; takes effect when Kiln restarts.
    Ready(String),
}

impl UpdateState {
    /// "Update to 2026.10.26" and friends, for the sidebar.
    pub fn label(&self) -> Option<String> {
        match self {
            UpdateState::None => None,
            UpdateState::Available(update) => Some(format!("Update to {}", update.version)),
            UpdateState::Installing(version) => Some(format!("Installing {version}…")),
            UpdateState::Ready(version) => Some(format!("Restart for {version}")),
        }
    }
}

struct Updates(UpdateState);
impl Global for Updates {}

pub fn state(cx: &App) -> UpdateState {
    cx.try_global::<Updates>()
        .map(|u| u.0.clone())
        .unwrap_or_default()
}

pub fn set_state(state: UpdateState, cx: &mut App) {
    cx.set_global(Updates(state));
    cx.refresh_windows();
}

/// Starts the daily check, when this build can update itself and the config allows it.
pub fn start(cx: &mut App) {
    let Some(source) = Source::built_in() else {
        log::info!("updates: this build has no update address");
        return;
    };
    let Some(install_to) = install_target() else {
        log::info!("updates: not installed from a Kiln installer; not checking");
        return;
    };
    let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return;
    };
    cx.spawn(async move |cx| {
        let mut wait = FIRST_CHECK;
        loop {
            cx.background_executor().timer(wait).await;
            wait = CHECK_EVERY;
            if !crate::config::current().updates.check {
                continue;
            }
            // Something is already on offer or installed: nothing to check.
            if cx.update(|cx| !matches!(state(cx), UpdateState::None)) {
                continue;
            }
            let (source, current, install_to) =
                (source.clone(), current.clone(), install_to.clone());
            let result = cx
                .background_executor()
                .spawn(async move { check(&source, &current, install_to) })
                .await;
            match result {
                Ok(Some(update)) => {
                    log::info!("update available: {}", update.version);
                    cx.update(|cx| set_state(UpdateState::Available(Arc::new(update)), cx));
                }
                Ok(None) => log::info!("updates: up to date"),
                Err(err) => log::warn!("updates: check failed: {err:#}"),
            }
        }
    })
    .detach();
}

/// Asks, then installs the update on offer; or, once installed, offers to restart.
pub fn install(window: &mut Window, cx: &mut App) {
    match state(cx) {
        UpdateState::Available(update) => {
            let detail = if cfg!(windows) {
                "Kiln closes to run the installer. Running commands stop."
            } else {
                "It takes effect the next time Kiln starts."
            };
            let answer = window.prompt(
                PromptLevel::Info,
                &format!("Install Kiln {}?", update.version),
                Some(detail),
                &["Install", "Not now"],
                cx,
            );
            cx.spawn(async move |cx| {
                if answer.await != Ok(0) {
                    return;
                }
                let version = update.version.clone();
                cx.update(|cx| set_state(UpdateState::Installing(version.clone()), cx));
                let result = cx
                    .background_executor()
                    .spawn(async move { download_and_install(&update) })
                    .await;
                cx.update(|cx| match result {
                    Ok(()) => set_state(UpdateState::Ready(version), cx),
                    Err(err) => {
                        log::error!("update failed: {err:#}");
                        // Offer it again at the next daily check.
                        set_state(UpdateState::None, cx);
                    }
                });
            })
            .detach();
        }
        UpdateState::Ready(version) => {
            let answer = window.prompt(
                PromptLevel::Info,
                &format!("Restart Kiln to use {version}?"),
                Some("Your sessions reopen in their folders; running commands stop."),
                &["Restart", "Later"],
                cx,
            );
            cx.spawn(async move |cx| {
                if answer.await == Ok(0) {
                    cx.update(|cx| cx.restart());
                }
            })
            .detach();
        }
        UpdateState::None | UpdateState::Installing(_) => {}
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
        // Tests run from target/, not from an installed app.
        assert_eq!(install_target(), None);
    }
}
