//! Draws rows of the live grid: the whole screen (classic mode and full-screen programs), the
//! running command's output, or the prompt in the input bar.

use std::panic::Location;

use gpui::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, FocusHandle, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, ShapedLine, Style, TextAlign,
    TextRun, UnderlineStyle, Window, fill, point, px, relative, size,
};
use kiln_term::engine::grid::Dimensions;
use kiln_term::engine::index::{Column, Line, Point as GridPoint};
use kiln_term::engine::vte::ansi::CursorShape;

use crate::grid_paint::{CellState, GridPainter, PaintedGrid, Tone, base_font};
use crate::terminal_view::{HitRegion, LiveRole, TerminalView};
use crate::theme::{self, palette};

/// Underline and beam cursor thickness in points.
const CURSOR_THICKNESS: f32 = 2.;

pub struct LiveElement {
    view: Entity<TerminalView>,
    focus: FocusHandle,
    role: LiveRole,
    /// First row to draw, absolute (`history_size + screen line`).
    top: usize,
    rows: usize,
    /// Fill the parent instead of sizing to `rows`.
    fill: bool,
    show_cursor: bool,
    /// This element receives typed text from the input method.
    input: bool,
}

impl LiveElement {
    pub fn new(
        view: Entity<TerminalView>,
        focus: FocusHandle,
        role: LiveRole,
        top: usize,
        rows: usize,
    ) -> Self {
        Self {
            view,
            focus,
            role,
            top,
            rows,
            fill: false,
            show_cursor: true,
            input: true,
        }
    }

    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }
}

pub struct LiveFrame {
    grid: PaintedGrid,
    cursor: Vec<Bounds<Pixels>>,
    cursor_under_text: bool,
    marked: Option<(Point<Pixels>, ShapedLine, Bounds<Pixels>)>,
    line_height: Pixels,
}

impl IntoElement for LiveElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for LiveElement {
    type RequestLayoutState = ();
    type PrepaintState = LiveFrame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let line_height = self.view.read(cx).metrics.line_height;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = if self.fill {
            relative(1.).into()
        } else {
            (line_height * self.rows as f32).into()
        };
        style.flex_shrink = 0.;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> LiveFrame {
        let focused = self.focus.is_focused(window);
        let view = self.view.read(cx);
        let metrics = view.metrics;
        let blink_on = view.cursor_blink_on;
        let marked_text = view.marked_text.clone().filter(|_| self.input);
        let first_frame_logged = view.first_frame_logged;
        let search = view.search_highlights();
        let term = view.session.term().lock();
        let grid = term.grid();
        let history = grid.history_size() as i32;
        let screen_lines = grid.screen_lines() as i32;
        let columns = grid.columns();
        let content = term.renderable_content();
        // A copy, so the lock can go before text shaping.
        let colors = *content.colors;
        let selection = content.selection;
        let cursor = content.cursor;
        let cursor_abs = history + cursor.point.line.0;
        let cursor_row = cursor_abs - self.top as i32;
        let rows = if self.fill {
            ((f32::from(bounds.size.height) / f32::from(metrics.line_height)) as usize)
                .min(self.rows)
        } else {
            self.rows
        };
        let cursor_in_view = self.show_cursor && cursor_row >= 0 && (cursor_row as usize) < rows;
        let block_cursor_at =
            (focused && blink_on && cursor_in_view && cursor.shape == CursorShape::Block)
                .then_some((cursor_row as usize, cursor.point.column.0));

        let mut painter = GridPainter::new(bounds.origin, metrics, &colors, Tone::Normal);
        for row in 0..rows {
            let line = self.top as i32 + row as i32 - history;
            if line < -history || line >= screen_lines {
                continue;
            }
            let grid_row = &grid[Line(line)];
            for column in 0..columns {
                let point = GridPoint::new(Line(line), Column(column));
                let state = CellState {
                    selected: selection.is_some_and(|range| range.contains(point)),
                    search_match: search.live_contains(point),
                    under_block_cursor: block_cursor_at == Some((row, column)),
                };
                painter.cell(row, column, &grid_row[Column(column)], state);
            }
        }

        let cell_w = metrics.cell_width;
        let line_h = metrics.line_height;
        let mut cursor_rects = Vec::new();
        let mut cursor_under_text = false;
        if cursor_in_view && cursor.shape != CursorShape::Hidden && marked_text.is_none() {
            let cell = Bounds::new(
                painter.cell_origin(cursor.point.column.0, cursor_row as usize),
                size(cell_w, line_h),
            );
            let thickness = px(CURSOR_THICKNESS);
            if !focused {
                let px1 = px(1.);
                cursor_rects.push(Bounds::new(cell.origin, size(cell_w, px1)));
                cursor_rects.push(Bounds::new(
                    cell.origin + point(px(0.), line_h - px1),
                    size(cell_w, px1),
                ));
                cursor_rects.push(Bounds::new(cell.origin, size(px1, line_h)));
                cursor_rects.push(Bounds::new(
                    cell.origin + point(cell_w - px1, px(0.)),
                    size(px1, line_h),
                ));
            } else if blink_on {
                match cursor.shape {
                    CursorShape::Block | CursorShape::HollowBlock => {
                        cursor_under_text = true;
                        cursor_rects.push(cell);
                    }
                    CursorShape::Beam => {
                        cursor_rects.push(Bounds::new(cell.origin, size(thickness, line_h)))
                    }
                    _ => cursor_rects.push(Bounds::new(
                        cell.origin + point(px(0.), line_h - thickness),
                        size(cell_w, thickness),
                    )),
                }
            }
        }

        let marked = marked_text.filter(|_| cursor_row >= 0).map(|marked| {
            let style = TextRun {
                len: marked.len(),
                font: base_font(),
                color: theme::hsla(palette().fg),
                background_color: Some(theme::hsla(palette().bg1)),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme::hsla(palette().accent)),
                    wavy: false,
                }),
                strikethrough: None,
            };
            let shaped =
                window
                    .text_system()
                    .shape_line(marked.into(), metrics.font_size, &[style], None);
            let at = painter.cell_origin(cursor.point.column.0, cursor_row.max(0) as usize);
            let bounds = Bounds::new(at, size(shaped.width, line_h));
            (at, shaped, bounds)
        });
        drop(term);

        let grid = painter.finish(window);
        let region = HitRegion::Live {
            role: self.role,
            bounds,
            top: self.top,
            rows,
            columns,
        };
        let has_text = grid.has_text;
        self.view
            .update(cx, |view, _| view.hit_regions.push(region));

        if has_text && !first_frame_logged && self.role != LiveRole::Running {
            // Deferred: the startup benchmark quits, which is not allowed mid-frame.
            let view = self.view.clone();
            window.defer(cx, move |window, cx| {
                view.update(cx, |view, cx| view.on_first_frame(window, cx));
            });
        }

        LiveFrame {
            grid,
            cursor: cursor_rects,
            cursor_under_text,
            marked,
            line_height: line_h,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        frame: &mut LiveFrame,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.input {
            window.handle_input(
                &self.focus,
                ElementInputHandler::new(bounds, self.view.clone()),
                cx,
            );
        }
        let accent = theme::hsla(palette().accent);
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            frame.grid.paint_backgrounds(window);
            if frame.cursor_under_text {
                for rect in &frame.cursor {
                    window.paint_quad(fill(*rect, accent));
                }
            }
            frame.grid.paint_text(window, cx);
            if !frame.cursor_under_text {
                for rect in &frame.cursor {
                    window.paint_quad(fill(*rect, accent));
                }
            }
            if let Some((origin, line, bounds)) = &frame.marked {
                window.paint_quad(fill(*bounds, theme::hsla(palette().bg1)));
                let _ = line.paint(
                    *origin,
                    frame.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        });

        if self.input && self.view.read(cx).output_since_input {
            self.view.update(cx, |view, _| view.frame_painted());
        }
    }
}
