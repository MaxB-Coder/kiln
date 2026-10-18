//! Turns terminal cells into backgrounds and shaped text, for both the live grid and frozen
//! blocks.
//!
//! Text is shaped one segment per row (a run of cells without wide characters), with every
//! glyph forced onto the cell grid. GPUI caches shaped lines across frames, so rows that did not
//! change cost almost nothing to redraw.

use gpui::{
    App, Bounds, Font, FontStyle, FontWeight, Hsla, Pixels, Point, ShapedLine, SharedString,
    StrikethroughStyle, TextAlign, TextRun, UnderlineStyle, Window, fill, point, px, size,
};
use kiln_term::engine::term::cell::{Cell, Flags};
use kiln_term::engine::term::color::Colors;
use kiln_term::engine::vte::ansi::{Color, NamedColor, Rgb};

use crate::theme::{self, palette};

/// JetBrains Mono with the Nerd Font symbols patched in (powerline, devicons, Material icons),
/// embedded in the binary. See assets/fonts/README.md for why it is the patched font.
pub const FONT_FAMILY: &str = "JetBrainsMono Nerd Font Mono";

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

/// Rows `first..last` of an element at `bounds` that fall inside the visible area (the
/// current content mask), so long blocks only lay out what can be seen.
pub fn visible_rows(
    bounds: gpui::Bounds<Pixels>,
    line_height: Pixels,
    rows: usize,
    window: &Window,
) -> std::ops::Range<usize> {
    let mask = window.content_mask().bounds;
    let line_h = f32::from(line_height).max(1.);
    let top = f32::from(mask.top() - bounds.top());
    let bottom = f32::from(mask.bottom() - bounds.top());
    let first = (top / line_h).floor().max(0.) as usize;
    let last = ((bottom / line_h).ceil().max(0.) as usize).min(rows);
    first.min(last)..last
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

/// How text in the program's default colour is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Bright: the prompt, the running command, full-screen programs.
    Normal,
    /// Muted: output of finished commands.
    Muted,
}

/// What to draw for one cell besides the cell itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct CellState {
    pub selected: bool,
    pub search_match: bool,
    /// The block cursor sits here: draw the glyph in the background colour.
    pub under_block_cursor: bool,
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

pub struct GridPainter<'a> {
    origin: Point<Pixels>,
    metrics: CellMetrics,
    colors: &'a Colors,
    tone: Tone,
    default_bg: Rgb,
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    segments: Vec<(usize, Segment)>,
    current: Option<Segment>,
    current_row: usize,
    pending_spaces: usize,
    bg_run: Option<(usize, usize, usize, Rgb)>,
    /// Any visible text was drawn.
    pub has_text: bool,
}

impl<'a> GridPainter<'a> {
    pub fn new(
        origin: Point<Pixels>,
        metrics: CellMetrics,
        colors: &'a Colors,
        tone: Tone,
    ) -> Self {
        Self {
            origin,
            metrics,
            colors,
            tone,
            default_bg: theme::resolve(Color::Named(NamedColor::Background), colors),
            backgrounds: Vec::new(),
            segments: Vec::new(),
            current: None,
            current_row: usize::MAX,
            pending_spaces: 0,
            bg_run: None,
            has_text: false,
        }
    }

    pub fn cell_origin(&self, column: usize, row: usize) -> Point<Pixels> {
        self.origin
            + point(
                self.metrics.cell_width * column as f32,
                self.metrics.line_height * row as f32,
            )
    }

    fn flush_bg(&mut self) {
        if let Some((row, column, len, rgb)) = self.bg_run.take() {
            let bounds = Bounds::new(
                self.cell_origin(column, row),
                size(
                    self.metrics.cell_width * len as f32,
                    self.metrics.line_height,
                ),
            );
            self.backgrounds.push((bounds, theme::rgb_to_hsla(rgb)));
        }
    }

    fn flush_segment(&mut self) {
        if let Some(segment) = self.current.take() {
            self.segments.push((self.current_row, segment));
        }
    }

    /// Adds one cell. Cells of a row must come left to right, rows top to bottom.
    pub fn cell(&mut self, row: usize, column: usize, cell: &Cell, state: CellState) {
        let flags = cell.flags;
        if row != self.current_row {
            self.flush_segment();
            self.flush_bg();
            self.current_row = row;
            self.pending_spaces = 0;
        }
        if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            return;
        }

        let fg = if self.tone == Tone::Muted
            && cell.fg == Color::Named(NamedColor::Foreground)
            && !flags.contains(Flags::BOLD)
        {
            None
        } else {
            Some(cell.fg)
        };
        let (mut fg, mut bg) = cell_colors(fg, cell.bg, flags, self.colors);
        if state.search_match {
            fg = theme::rgb_from_hex(palette().bg);
            bg = theme::rgb_from_hex(palette().modified);
        }
        if state.selected {
            fg = theme::rgb_from_hex(palette().fg);
            bg = theme::rgb_from_hex(palette().bg3);
        }
        if state.under_block_cursor {
            fg = self.default_bg;
        }
        let width = if flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };

        // Background runs, merged across neighbouring cells of the same colour.
        if bg != self.default_bg {
            match &mut self.bg_run {
                Some((r, start, len, rgb))
                    if *r == row && *start + *len == column && *rgb == bg =>
                {
                    *len += width
                }
                _ => {
                    self.flush_bg();
                    self.bg_run = Some((row, column, width, bg));
                }
            }
        } else {
            self.flush_bg();
        }

        let decorated = flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
        let blank = matches!(cell.c, ' ' | '\t' | '\0') && cell.zerowidth().is_none();
        if blank && !decorated {
            self.pending_spaces += 1;
            return;
        }
        self.has_text = true;

        let style = text_style(fg, flags, cell.underline_color(), self.colors);
        let mut text = String::with_capacity(4);
        text.push(if cell.c == '\0' { ' ' } else { cell.c });
        if let Some(extra) = cell.zerowidth() {
            text.extend(extra);
        }

        if width == 2 {
            // Wide characters get their own segment so the rest of the row stays on grid.
            self.flush_segment();
            let mut segment = Segment::new(column);
            segment.push(&text, &style);
            self.segments.push((row, segment));
            self.pending_spaces = 0;
            return;
        }

        let pending = self.pending_spaces;
        match &mut self.current {
            Some(segment) if pending <= 4 => {
                if pending > 0 {
                    // Gap cells carry no decoration of their own.
                    let gap = TextRun {
                        underline: None,
                        strikethrough: None,
                        font: base_font(),
                        ..style.clone()
                    };
                    segment.push(&" ".repeat(pending), &gap);
                }
            }
            _ => {
                self.flush_segment();
                self.current = Some(Segment::new(column));
            }
        }
        self.pending_spaces = 0;
        if let Some(segment) = self.current.as_mut() {
            segment.push(&text, &style);
        }
    }

    pub fn finish(mut self, window: &Window) -> PaintedGrid {
        self.flush_segment();
        self.flush_bg();
        let text_system = window.text_system().clone();
        let metrics = self.metrics;
        let origin = self.origin;
        let text = self
            .segments
            .into_iter()
            .map(|(row, segment)| {
                let shaped = text_system.shape_line(
                    SharedString::from(segment.text),
                    metrics.font_size,
                    &segment.runs,
                    Some(metrics.cell_width),
                );
                let at = origin
                    + point(
                        metrics.cell_width * segment.column as f32,
                        metrics.line_height * row as f32,
                    );
                (at, shaped)
            })
            .collect();
        PaintedGrid {
            backgrounds: self.backgrounds,
            text,
            line_height: metrics.line_height,
            has_text: self.has_text,
        }
    }
}

pub struct PaintedGrid {
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    text: Vec<(Point<Pixels>, ShapedLine)>,
    line_height: Pixels,
    pub has_text: bool,
}

impl PaintedGrid {
    pub fn paint_backgrounds(&self, window: &mut Window) {
        for (bounds, color) in &self.backgrounds {
            window.paint_quad(fill(*bounds, *color));
        }
    }

    pub fn paint_text(&self, window: &mut Window, cx: &mut App) {
        for (origin, line) in &self.text {
            let _ = line.paint(*origin, self.line_height, TextAlign::Left, None, window, cx);
        }
    }
}

/// Foreground and background for a cell, after bold, dim, inverse and hidden. A `None`
/// foreground means "muted default".
fn cell_colors(fg: Option<Color>, bg: Color, flags: Flags, colors: &Colors) -> (Rgb, Rgb) {
    // Bold text in one of the 8 normal colours uses the bright variant (WezTerm's default).
    let mut fg = match fg {
        None => theme::rgb_from_hex(palette().fg_dim),
        Some(Color::Named(name)) if flags.contains(Flags::BOLD) && (name as usize) < 8 => {
            theme::resolve(Color::Indexed(name as u8 + 8), colors)
        }
        Some(Color::Indexed(index)) if flags.contains(Flags::BOLD) && index < 8 => {
            theme::resolve(Color::Indexed(index + 8), colors)
        }
        Some(other) => theme::resolve(other, colors),
    };
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

fn text_style(fg: Rgb, flags: Flags, underline_color: Option<Color>, colors: &Colors) -> TextRun {
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
