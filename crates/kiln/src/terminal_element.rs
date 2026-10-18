//! Draws the engine's grid: background runs, text runs, selection, cursor and IME text.
//!
//! Text is shaped one segment per row (a run of cells without wide characters), with every
//! glyph forced onto the cell grid. GPUI caches shaped lines across frames, so rows that did not
//! change cost almost nothing to redraw.

use std::panic::Location;

use gpui::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, FocusHandle, Font, FontStyle,
    FontWeight, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, Point,
    ShapedLine, SharedString, StrikethroughStyle, Style, TextAlign, TextRun, UnderlineStyle,
    Window, fill, point, px, relative, size,
};
use kiln_term::GridSize;
use kiln_term::engine::term::cell::Flags;
use kiln_term::engine::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

use crate::terminal_view::{GridLayout, TerminalView};
use crate::theme::{self, PALETTE};

/// JetBrains Mono with the Nerd Font symbols patched in (powerline, devicons, Material icons),
/// embedded in the binary. See assets/fonts/README.md for why it is the patched font.
pub const FONT_FAMILY: &str = "JetBrainsMono Nerd Font Mono";

/// Window padding around the grid, in points (same as the WezTerm config).
pub const PADDING_X: f32 = 14.;
pub const PADDING_TOP: f32 = 10.;
pub const PADDING_BOTTOM: f32 = 8.;

/// Underline cursor thickness in points.
const CURSOR_THICKNESS: f32 = 2.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    pub font_size: Pixels,
    pub cell_width: Pixels,
    pub line_height: Pixels,
}

impl CellMetrics {
    /// Measures the font and snaps the cell to whole device pixels so box drawing lines up.
    pub fn measure(font_size: Pixels, window: &Window) -> Self {
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&base_font());
        let advance = text_system
            .advance(font_id, font_size, 'm')
            .map(|size| size.width)
            .unwrap_or(font_size * 0.6);
        let ascent = text_system.ascent(font_id, font_size);
        let descent = text_system.descent(font_id, font_size);
        let scale = window.scale_factor();
        let snap = |v: f32| px((v * scale).round() / scale);
        Self {
            font_size,
            cell_width: snap(f32::from(advance)),
            line_height: snap(f32::from(ascent) + f32::from(descent).abs()),
        }
    }
}

pub fn base_font() -> Font {
    gpui::font(FONT_FAMILY)
}

/// Registers the fonts embedded in the binary, so nothing needs installing on any platform.
pub fn load_embedded_fonts(cx: &App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFontMono-Regular.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFontMono-Bold.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFontMono-Italic.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../assets/fonts/JetBrainsMonoNerdFontMono-BoldItalic.ttf")
            .as_slice()
            .into(),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        log::error!("could not load embedded fonts: {err:#}");
    }
}

pub struct TerminalElement {
    view: Entity<TerminalView>,
    focus: FocusHandle,
}

impl TerminalElement {
    pub fn new(view: Entity<TerminalView>, focus: FocusHandle) -> Self {
        Self { view, focus }
    }
}

pub struct Frame {
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    text: Vec<(Point<Pixels>, ShapedLine)>,
    cursor: Vec<Bounds<Pixels>>,
    marked: Option<(Point<Pixels>, ShapedLine, Bounds<Pixels>)>,
    line_height: Pixels,
}

/// Cells of one row that are shaped together.
struct Segment {
    column: usize,
    text: String,
    runs: Vec<TextRun>,
}

impl Segment {
    fn new(column: usize) -> Self {
        Self {
            column,
            text: String::new(),
            runs: Vec::new(),
        }
    }

    fn push(&mut self, text: &str, style: &TextRun) {
        self.text.push_str(text);
        if let Some(last) = self.runs.last_mut()
            && last.font == style.font
            && last.color == style.color
            && last.underline == style.underline
            && last.strikethrough == style.strikethrough
        {
            last.len += text.len();
            return;
        }
        self.runs.push(TextRun {
            len: text.len(),
            ..style.clone()
        });
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Frame;

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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
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
    ) -> Frame {
        // Re-measure when the window moves to a screen with a different scale.
        let font_size = self.view.read(cx).metrics.font_size;
        let metrics = CellMetrics::measure(font_size, window);
        let origin = bounds.origin + point(px(PADDING_X), px(PADDING_TOP));
        let usable_w = f32::from(bounds.size.width) - 2. * PADDING_X;
        let usable_h = f32::from(bounds.size.height) - PADDING_TOP - PADDING_BOTTOM;
        let columns = ((usable_w / f32::from(metrics.cell_width)) as usize).max(2);
        let lines = ((usable_h / f32::from(metrics.line_height)) as usize).max(1);
        let scale = window.scale_factor();
        let grid_size = GridSize {
            columns,
            lines,
            cell_width: (f32::from(metrics.cell_width) * scale) as u16,
            cell_height: (f32::from(metrics.line_height) * scale) as u16,
        };
        self.view.update(cx, |view, _| {
            view.metrics = metrics;
            view.session.resize(grid_size);
            view.layout = Some(GridLayout {
                origin,
                metrics,
                columns,
                lines,
            });
        });

        let focused = self.focus.is_focused(window);
        let view = self.view.read(cx);
        let blink_on = view.cursor_blink_on;
        let marked_text = view.marked_text.clone();
        let term = view.session.term().lock();
        let content = term.renderable_content();
        let display_offset = content.display_offset as i32;
        let colors = content.colors;
        let selection = content.selection;
        let cursor = content.cursor;
        let default_bg = theme::resolve(Color::Named(NamedColor::Background), colors);
        let cell_w = metrics.cell_width;
        let line_h = metrics.line_height;

        let cell_origin = |column: usize, line: usize| {
            origin + point(cell_w * column as f32, line_h * line as f32)
        };

        // Block cursor: the glyph under it is drawn in the background colour.
        let cursor_line = cursor.point.line.0 + display_offset;
        let block_cursor_at = (focused && blink_on && cursor.shape == CursorShape::Block)
            .then_some((cursor_line, cursor.point.column.0));

        let mut backgrounds: Vec<(Bounds<Pixels>, Hsla)> = Vec::new();
        let mut segments: Vec<(usize, Segment)> = Vec::new();
        let mut current: Option<Segment> = None;
        let mut current_line = usize::MAX;
        let mut pending_spaces = 0usize;
        let mut bg_run: Option<(usize, usize, usize, Rgb)> = None; // line, start col, len, colour
        let mut has_output = false;

        let flush_bg = |bg_run: &mut Option<(usize, usize, usize, Rgb)>,
                        backgrounds: &mut Vec<(Bounds<Pixels>, Hsla)>| {
            if let Some((line, column, len, rgb)) = bg_run.take() {
                let bounds =
                    Bounds::new(cell_origin(column, line), size(cell_w * len as f32, line_h));
                backgrounds.push((bounds, theme::rgb_to_hsla(rgb)));
            }
        };

        for indexed in content.display_iter {
            let line = indexed.point.line.0 + display_offset;
            if line < 0 || line as usize >= lines {
                continue;
            }
            let line = line as usize;
            let column = indexed.point.column.0;
            let cell = indexed.cell;
            let flags = cell.flags;

            if line != current_line {
                if let Some(segment) = current.take() {
                    segments.push((current_line, segment));
                }
                flush_bg(&mut bg_run, &mut backgrounds);
                current_line = line;
                pending_spaces = 0;
            }
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }

            let (fg, bg) = cell_colors(cell.fg, cell.bg, flags, colors);
            let selected = selection.is_some_and(|range| range.contains(indexed.point));
            let (mut fg, bg) = if selected {
                (
                    theme::rgb_from_hex(PALETTE.fg),
                    theme::rgb_from_hex(PALETTE.bg3),
                )
            } else {
                (fg, bg)
            };
            if block_cursor_at == Some((line as i32, column)) {
                fg = default_bg;
            }
            let width = if flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };

            // Background runs, merged across neighbouring cells of the same colour.
            if bg != default_bg {
                match &mut bg_run {
                    Some((l, start, len, rgb))
                        if *l == line && *start + *len == column && *rgb == bg =>
                    {
                        *len += width
                    }
                    _ => {
                        flush_bg(&mut bg_run, &mut backgrounds);
                        bg_run = Some((line, column, width, bg));
                    }
                }
            } else {
                flush_bg(&mut bg_run, &mut backgrounds);
            }

            let decorated = flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
            let blank = matches!(cell.c, ' ' | '\t' | '\0') && cell.zerowidth().is_none();
            if blank && !decorated {
                pending_spaces += 1;
                continue;
            }
            has_output = true;

            let style = text_style(fg, flags, cell.underline_color(), colors);
            let mut text = String::with_capacity(4);
            text.push(if cell.c == '\0' { ' ' } else { cell.c });
            if let Some(extra) = cell.zerowidth() {
                text.extend(extra);
            }

            if width == 2 {
                // Wide characters get their own segment so the rest of the row stays on grid.
                if let Some(segment) = current.take() {
                    segments.push((line, segment));
                }
                let mut segment = Segment::new(column);
                segment.push(&text, &style);
                segments.push((line, segment));
                pending_spaces = 0;
                continue;
            }

            match &mut current {
                Some(segment) if pending_spaces <= 4 => {
                    if pending_spaces > 0 {
                        // Gap cells carry no decoration of their own.
                        let gap = TextRun {
                            underline: None,
                            strikethrough: None,
                            font: base_font(),
                            ..style.clone()
                        };
                        segment.push(&" ".repeat(pending_spaces), &gap);
                    }
                }
                _ => {
                    if let Some(segment) = current.take() {
                        segments.push((line, segment));
                    }
                    current = Some(Segment::new(column));
                }
            }
            pending_spaces = 0;
            current.as_mut().unwrap().push(&text, &style);
        }
        if let Some(segment) = current.take() {
            segments.push((current_line, segment));
        }
        flush_bg(&mut bg_run, &mut backgrounds);

        // Cursor.
        let mut cursor_rects = Vec::new();
        let cursor_visible = cursor.shape != CursorShape::Hidden
            && cursor_line >= 0
            && (cursor_line as usize) < lines
            && marked_text.is_none();
        if cursor_visible {
            let cell = Bounds::new(
                cell_origin(cursor.point.column.0, cursor_line as usize),
                size(cell_w, line_h),
            );
            let thickness = px(CURSOR_THICKNESS);
            if !focused {
                // Hollow box when the window is not focused.
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
                cursor_rects.push(match cursor.shape {
                    CursorShape::Block => cell,
                    CursorShape::Beam => Bounds::new(cell.origin, size(thickness, line_h)),
                    CursorShape::Underline => Bounds::new(
                        cell.origin + point(px(0.), line_h - thickness),
                        size(cell_w, thickness),
                    ),
                    CursorShape::HollowBlock | CursorShape::Hidden => cell,
                });
            }
        }

        drop(term);

        // Shape text.
        let text_system = window.text_system().clone();
        let text = segments
            .into_iter()
            .map(|(line, segment)| {
                let shaped = text_system.shape_line(
                    SharedString::from(segment.text),
                    metrics.font_size,
                    &segment.runs,
                    Some(cell_w),
                );
                (cell_origin(segment.column, line), shaped)
            })
            .collect();

        // Text the input method is composing, drawn over the cursor cell.
        let marked = marked_text.filter(|_| cursor_line >= 0).map(|marked| {
            let style = TextRun {
                len: marked.len(),
                font: base_font(),
                color: theme::hsla(PALETTE.fg),
                background_color: Some(theme::hsla(PALETTE.bg1)),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme::hsla(PALETTE.accent)),
                    wavy: false,
                }),
                strikethrough: None,
            };
            let shaped = text_system.shape_line(marked.into(), metrics.font_size, &[style], None);
            let at = cell_origin(cursor.point.column.0, cursor_line.max(0) as usize);
            let bounds = Bounds::new(at, size(shaped.width, line_h));
            (at, shaped, bounds)
        });

        if has_output && !self.view.read(cx).first_frame_logged {
            // Deferred: the startup benchmark quits, which is not allowed mid-frame.
            let view = self.view.clone();
            window.defer(cx, move |window, cx| {
                view.update(cx, |view, cx| view.on_first_frame(window, cx));
            });
        }

        Frame {
            backgrounds,
            text,
            cursor: cursor_rects,
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
        frame: &mut Frame,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );

        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for (bounds, color) in &frame.backgrounds {
                window.paint_quad(fill(*bounds, *color));
            }
            // Block cursor goes under the text (the glyph is recoloured); other shapes go over it.
            let accent = theme::hsla(PALETTE.accent);
            let cursor_under = frame.cursor.len() == 1
                && frame.cursor[0].size.height == frame.line_height
                && frame.cursor[0].size.width > px(CURSOR_THICKNESS);
            if cursor_under {
                window.paint_quad(fill(frame.cursor[0], accent));
            }
            for (origin, line) in &frame.text {
                let _ = line.paint(
                    *origin,
                    frame.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            if !cursor_under {
                for rect in &frame.cursor {
                    window.paint_quad(fill(*rect, accent));
                }
            }
            if let Some((origin, line, bounds)) = &frame.marked {
                window.paint_quad(fill(*bounds, theme::hsla(PALETTE.bg1)));
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
    }
}

/// Foreground and background for a cell, after bold, dim, inverse and hidden.
fn cell_colors(
    fg: Color,
    bg: Color,
    flags: Flags,
    colors: &kiln_term::engine::term::color::Colors,
) -> (Rgb, Rgb) {
    // Bold text in one of the 8 normal colours uses the bright variant (WezTerm's default).
    let fg = match fg {
        Color::Named(name) if flags.contains(Flags::BOLD) && (name as usize) < 8 => {
            Color::Indexed(name as u8 + 8)
        }
        Color::Indexed(index) if flags.contains(Flags::BOLD) && index < 8 => {
            Color::Indexed(index + 8)
        }
        other => other,
    };
    let mut fg = theme::resolve(fg, colors);
    let mut bg = theme::resolve(bg, colors);
    if flags.contains(Flags::DIM) {
        fg = theme::dim(fg);
    }
    if flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    (fg, bg)
}

fn text_style(
    fg: Rgb,
    flags: Flags,
    underline_color: Option<Color>,
    colors: &kiln_term::engine::term::color::Colors,
) -> TextRun {
    let mut font = base_font();
    if flags.contains(Flags::BOLD) {
        font.weight = FontWeight::BOLD;
    }
    if flags.contains(Flags::ITALIC) {
        font.style = FontStyle::Italic;
    }
    let color = theme::rgb_to_hsla(fg);
    let underline = flags
        .intersects(Flags::ALL_UNDERLINES)
        .then(|| UnderlineStyle {
            thickness: px(1.),
            color: Some(
                underline_color.map_or(color, |c| theme::rgb_to_hsla(theme::resolve(c, colors))),
            ),
            wavy: flags.contains(Flags::UNDERCURL),
        });
    let strikethrough = flags
        .contains(Flags::STRIKEOUT)
        .then(|| StrikethroughStyle {
            thickness: px(1.),
            color: Some(color),
        });
    TextRun {
        len: 0,
        font,
        color,
        background_color: None,
        underline,
        strikethrough,
    }
}
