//! The update offer in the app: a quiet line at the bottom of the sidebar and the first palette
//! item when a newer version exists, the dialogs, and the daily check. Kiln's only network use.
//!
//! A release build carries the address of its release manifest and the public key its updates
//! are signed with (`KILN_UPDATE_ENDPOINT`, `KILN_UPDATE_PUBKEY`, set by the release workflow).
//! Builds without them never check. Kiln checks a minute after launch and then once a day, and
//! installs only when asked. The checking, the signature check and the install itself are in
//! the `kiln_update` crate.

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Global, PromptLevel, Window};
use kiln_update::{Source, Update, Version, check, download_and_install, install_target};

/// How long after launch the first check runs, and how often after that.
const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

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
                    cx.update(|cx| {
                        // An AppImage restarts as the AppImage, not as the binary in its image.
                        if let Some(path) = kiln_update::restart_path() {
                            cx.set_restart_path(path);
                        }
                        cx.restart()
                    });
                }
            })
            .detach();
        }
        UpdateState::None | UpdateState::Installing(_) => {}
    }
}
