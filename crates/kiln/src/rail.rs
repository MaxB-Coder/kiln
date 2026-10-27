//! The ⌘I context rail: the focused pane's changes, running processes and ports, runtimes and
//! session. It reads processes and git status every two seconds in the background while open.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Div, EntityId, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, PromptLevel, SharedString, StatefulInteractiveElement, Styled, Task,
    Window, WindowControlArea, div, px,
};
use kiln_term::process::{ProcessReport, ProcessWatcher};

use crate::git::{self, FileChange, GitStatus};
use crate::grid_paint::FONT_FAMILY;
use crate::terminal_view::SessionInfo;
use crate::theme::{self, palette};
use crate::workspace::{Chrome, TITLEBAR_HEIGHT, Workspace, chrome, window_controls};

pub const RAIL_WIDTH: Pixels = px(280.);
const POLL: Duration = Duration::from_secs(2);
const MAX_FILES: usize = 14;
const MAX_PROCESSES: usize = 12;

/// What the rail last read, and for which pane.
#[derive(Default)]
pub struct RailData {
    pub pane: Option<EntityId>,
    pub git: Option<GitStatus>,
    pub report: ProcessReport,
}

pub struct Rail {
    pub data: RailData,
    watcher: Arc<Mutex<ProcessWatcher>>,
    fetching: bool,
    /// The focus moved while a read was running: read again when it finishes.
    again: bool,
    _poll: Task<()>,
}

impl Workspace {
    pub(crate) fn toggle_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rail.take().is_some() {
            cx.notify();
            return;
        }
        let poll = cx.spawn_in(window, async move |this, cx| {
            loop {
                if this.update(cx, |this, cx| this.refresh_rail(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(POLL).await;
            }
        });
        self.rail = Some(Rail {
            data: RailData::default(),
            watcher: Arc::new(Mutex::new(ProcessWatcher::new())),
            fetching: false,
            again: false,
            _poll: poll,
        });
        cx.notify();
    }

    /// Reads the focused pane's processes and git status in the background.
    pub(crate) fn refresh_rail(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.focused_view() else {
            return;
        };
        let (pid, cwd) = {
            let view = view.read(cx);
            (view.session_info().pid, view.cwd().map(str::to_string))
        };
        let pane = view.entity_id();
        let Some(rail) = self.rail.as_mut() else {
            return;
        };
        if rail.fetching {
            rail.again = true;
            return;
        }
        rail.fetching = true;
        let watcher = rail.watcher.clone();
        cx.spawn(async move |this, cx| {
            let (git, report) = cx
                .background_executor()
                .spawn(async move {
                    let report = {
                        let mut watcher = watcher.lock().unwrap_or_else(|e| e.into_inner());
                        watcher.refresh();
                        pid.map(|pid| watcher.report(pid)).unwrap_or_default()
                    };
                    (cwd.as_deref().and_then(git::status), report)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(rail) = this.rail.as_mut() else {
                    return;
                };
                rail.fetching = false;
                rail.data = RailData {
                    pane: Some(pane),
                    git,
                    report,
                };
                if std::mem::take(&mut rail.again) {
                    this.refresh_rail(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_diff(
        &mut self,
        root: String,
        file: FileChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = git::diff_command(&root, &file);
        self.split_right(Some(PathBuf::from(root)), Some(command), window, cx);
    }

    /// Opens a local port in the browser, after asking (links always ask first).
    fn open_port(&mut self, port: u16, window: &mut Window, cx: &mut Context<Self>) {
        let url = format!("http://localhost:{port}");
        let answer = window.prompt(
            PromptLevel::Info,
            &format!("Open {url}?"),
            Some("It opens in your default browser."),
            &["Open", "Cancel"],
            cx,
        );
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                cx.update(|cx| cx.open_url(&url));
            }
        })
        .detach();
    }

    pub(crate) fn render_rail(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let rail = self.rail.as_ref()?;
        let p = palette();
        let view = self.focused_view();
        let (info, runtimes) = match &view {
            Some(view) => {
                let view = view.read(cx);
                (Some(view.session_info()), view.runtimes().to_vec())
            }
            None => (None, Vec::new()),
        };
        // Data read for another pane is stale; show nothing until the new read lands.
        let fresh = view.as_ref().map(|v| v.entity_id()) == rail.data.pane;

        // The first section's title sits in the header row (8 px down centres its 16 px line
        // in the 32 px row); the rest are 16 px apart.
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .pt(px(8.))
            .px(px(14.))
            .pb(px(14.));
        if fresh && let Some(status) = &rail.data.git {
            body = body.child(self.render_changes(status, cx));
        }
        body = body.child(render_processes(fresh.then_some(&rail.data.report)));
        if fresh && !rail.data.report.ports.is_empty() {
            body = body.child(self.render_ports(&rail.data.report, cx));
        }
        if !runtimes.is_empty() {
            let mut section = section("Runtimes", None);
            for (name, version) in runtimes {
                section = section.child(key_value(name, version));
            }
            body = body.child(section);
        }
        if let Some(info) = info {
            body = body.child(render_session(&info));
        }

        let mac = chrome() == Chrome::Mac;
        // A strip to move the window by, along the top. Where the window controls are (Windows
        // and Linux) it holds them and the content starts below; on macOS they are on the left,
        // so the content starts at the top and a shorter strip lies over the first title.
        let strip = div()
            .flex_none()
            .flex()
            .flex_row()
            .justify_end()
            .map(|d| {
                if mac {
                    d.absolute().top_0().left_0().w_full().h(px(24.))
                } else {
                    d.h(TITLEBAR_HEIGHT).child(window_controls(window))
                }
            })
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else if !cfg!(windows) {
                    window.start_window_move();
                }
            });
        let body = div()
            .id("rail-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(body);
        Some(
            div()
                .id("rail")
                .relative()
                .flex_none()
                .w(RAIL_WIDTH)
                .h_full()
                .flex()
                .flex_col()
                .bg(theme::hsla(p.bar))
                .font_family(FONT_FAMILY)
                .text_size(px(12.))
                .map(|d| {
                    if mac {
                        d.child(body).child(strip)
                    } else {
                        d.child(strip).child(body)
                    }
                })
                .into_any_element(),
        )
    }

    fn render_changes(&self, status: &GitStatus, cx: &mut Context<Self>) -> Div {
        let p = palette();
        let mut aside = status.branch.clone().unwrap_or_else(|| "detached".into());
        if status.ahead > 0 {
            aside.push_str(&format!(" ↑{}", status.ahead));
        }
        if status.behind > 0 {
            aside.push_str(&format!(" ↓{}", status.behind));
        }
        let mut section = section("Changes", Some(aside));
        if status.files.is_empty() {
            return section.child(dim("Working tree clean"));
        }
        for (i, file) in status.files.iter().take(MAX_FILES).enumerate() {
            let color = match file.kind {
                'A' => p.ok,
                'D' | 'U' => p.failed,
                'M' => p.modified,
                '?' => p.muted,
                _ => p.fg_dim,
            };
            let root = status.root.clone();
            let file = file.clone();
            section = section.child(
                row(("rail-file", i))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hsla(p.raised)))
                    .child(
                        div()
                            .flex_none()
                            .w(px(12.))
                            .text_color(theme::hsla(color))
                            .child(file.kind.to_string()),
                    )
                    .child(ellipsis(file.path.clone()).text_color(theme::hsla(p.fg_dim)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_diff(root.clone(), file.clone(), window, cx)
                    })),
            );
        }
        if status.files.len() > MAX_FILES {
            section = section.child(dim(format!("{} more", status.files.len() - MAX_FILES)));
        }
        section
    }

    fn render_ports(&self, report: &ProcessReport, cx: &mut Context<Self>) -> Div {
        let p = palette();
        let mut section = section("Ports", None);
        for port in &report.ports {
            let number = port.port;
            let group: SharedString = format!("rail-port-{number}").into();
            section = section.child(
                row(("rail-port", number as usize))
                    .group(group.clone())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hsla(p.raised)))
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme::hsla(p.fg))
                            .child(format!(":{number}")),
                    )
                    .child(ellipsis(port.process.clone()).text_color(theme::hsla(p.muted)))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::hsla(p.accent))
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .child("open"),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_port(number, window, cx)),
                    ),
            );
        }
        section
    }
}

fn render_processes(report: Option<&ProcessReport>) -> Div {
    let p = palette();
    let mut section = section("Processes", None);
    let Some(report) = report else {
        return section.child(dim("…"));
    };
    if report.processes.is_empty() {
        return section.child(dim("Nothing running"));
    }
    for process in report.processes.iter().take(MAX_PROCESSES) {
        section = section.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .h(px(22.))
                .pl(px(6. + 12. * process.depth.min(6) as f32))
                .pr(px(6.))
                .child(ellipsis(process.name.clone()).text_color(theme::hsla(p.fg_dim)))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(theme::hsla(p.muted))
                        .child(format!(
                            "{:.0}%  {}",
                            process.cpu,
                            bytes_label(process.memory_bytes)
                        )),
                ),
        );
    }
    if report.processes.len() > MAX_PROCESSES {
        section = section.child(dim(format!(
            "{} more",
            report.processes.len() - MAX_PROCESSES
        )));
    }
    section
}

fn render_session(info: &SessionInfo) -> Div {
    let p = palette();
    let mut section = section("Session", None)
        .child(key_value("shell", info.shell.clone()))
        .when_some(info.pid, |d, pid| {
            d.child(key_value("pid", pid.to_string()))
        })
        .child(key_value(
            "size",
            format!("{} × {}", info.columns, info.lines),
        ))
        .child(key_value("started", ago_label(info.started.elapsed())))
        .child(key_value("commands", info.commands.to_string()));
    if let Some(code) = info.last_exit {
        let failed = code != 0 && code != 130;
        section = section.child(
            key_value("last exit", code.to_string())
                .when(failed, |d| d.text_color(theme::hsla(p.failed))),
        );
    }
    if let Some(host) = &info.remote_host {
        section = section.child(key_value("remote", host.clone()));
    }
    section
}

fn section(title: &str, aside: Option<String>) -> Div {
    let p = palette();
    div().flex().flex_col().child(
        div()
            .flex()
            .flex_row()
            .justify_between()
            .gap(px(8.))
            .px(px(6.))
            .pb(px(4.))
            .text_size(px(11.))
            .text_color(theme::hsla(p.muted))
            .child(div().flex_none().child(title.to_string()))
            .when_some(aside, |d, aside| {
                d.child(
                    div()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(aside),
                )
            }),
    )
}

fn row(id: impl Into<gpui::ElementId>) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .h(px(22.))
        .px(px(6.))
        .rounded(px(5.))
}

fn key_value(key: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    let p = palette();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .h(px(22.))
        .px(px(6.))
        .text_color(theme::hsla(p.fg_dim))
        .child(
            div()
                .flex_none()
                .w(px(72.))
                .text_color(theme::hsla(p.muted))
                .child(key.into()),
        )
        .child(ellipsis(value.into()))
}

fn ellipsis(text: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(text.into())
}

fn dim(text: impl Into<SharedString>) -> Div {
    div()
        .px(px(6.))
        .h(px(22.))
        .flex()
        .items_center()
        .text_color(theme::hsla(palette().muted))
        .child(text.into())
}

/// "640 KB", "48 MB", "1.2 GB".
pub fn bytes_label(bytes: u64) -> String {
    const KB: f64 = 1024.;
    let b = bytes as f64;
    if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.0} MB", b / (KB * KB))
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

/// "12 s ago", "4 min ago", "2 h 5 min ago", "3 d ago".
pub fn ago_label(d: Duration) -> String {
    let s = d.as_secs();
    let text = if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else if s < 86_400 {
        format!("{} h {} min", s / 3600, s % 3600 / 60)
    } else {
        format!("{} d", s / 86_400)
    };
    format!("{text} ago")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(bytes_label(512 * 1024), "512 KB");
        assert_eq!(bytes_label(48 * 1024 * 1024), "48 MB");
        assert_eq!(bytes_label(1288 * 1024 * 1024), "1.3 GB");
        assert_eq!(ago_label(Duration::from_secs(12)), "12 s ago");
        assert_eq!(ago_label(Duration::from_secs(7500)), "2 h 5 min ago");
        assert_eq!(ago_label(Duration::from_secs(3 * 86_400 + 5)), "3 d ago");
    }
}
