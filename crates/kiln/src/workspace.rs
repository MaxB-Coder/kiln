//! The window's root view. Phase 1 adds the sidebar, the input bar and splits here; for now it
//! holds one terminal and the space the window controls need.

use std::time::Instant;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement, IntoElement, ParentElement, Pixels,
    Render, Styled, Window, div, px,
};
use kiln_term::{GridSize, SessionOptions};

use crate::CloseWindow;
use crate::terminal_element::CellMetrics;
use crate::terminal_view::TerminalView;
use crate::theme::{self, PALETTE};

/// Height of the top strip that holds the window controls (traffic lights on macOS).
pub const TITLEBAR_HEIGHT: Pixels = px(30.);

pub struct Workspace {
    content: Result<Entity<TerminalView>, String>,
}

impl Workspace {
    pub fn new(
        session: SessionOptions,
        grid: GridSize,
        metrics: CellMetrics,
        bench_startup: bool,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = match TerminalView::spawn_session(session, grid) {
            Ok((session, events)) => Ok(cx.new(|cx| {
                TerminalView::new(session, events, metrics, bench_startup, started, window, cx)
            })),
            Err(err) => Err(format!("{err:#}")),
        };
        Self { content }
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.content {
            Ok(terminal) => terminal.clone().into_any_element(),
            Err(err) => div()
                .p_4()
                .text_color(theme::hsla(PALETTE.failed))
                .child(format!("Kiln could not start the shell: {err}"))
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::hsla(PALETTE.bg))
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .child(div().h(TITLEBAR_HEIGHT).w_full().flex_none())
            .child(div().flex_1().min_h_0().child(body))
    }
}
