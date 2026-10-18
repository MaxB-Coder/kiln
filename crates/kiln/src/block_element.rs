//! Finished commands in the timeline: their stored output, wrapped to the current width.

use std::cell::{Cell as StdCell, RefCell};
use std::panic::Location;
use std::rc::Rc;

use gpui::{
    App, Bounds, Element, ElementId, Entity, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Style, Window, relative,
};
use kiln_term::Block;
use kiln_term::engine::term::cell::{Cell, Flags};
use kiln_term::engine::term::color::Colors;

use crate::grid_paint::{CellMetrics, CellState, GridPainter, PaintedGrid, Tone};
use crate::terminal_view::{HitRegion, TerminalView};

/// One display row of a block: cells `start..end` of logical line `line`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrappedRow {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// A finished command as the timeline keeps it.
pub struct FrozenBlock {
    pub block: Block,
    pub pinned: StdCell<bool>,
    wrap: RefCell<Option<(usize, Rc<Vec<WrappedRow>>)>>,
}

impl FrozenBlock {
    pub fn new(block: Block) -> Self {
        Self {
            block,
            pinned: StdCell::new(false),
            wrap: RefCell::new(None),
        }
    }

    /// Display rows at `columns` wide (cached per width).
    pub fn rows(&self, columns: usize) -> Rc<Vec<WrappedRow>> {
        if let Some((cols, rows)) = self.wrap.borrow().as_ref()
            && *cols == columns
        {
            return rows.clone();
        }
        let rows = Rc::new(wrap_lines(&self.block.lines, columns.max(2)));
        *self.wrap.borrow_mut() = Some((columns, rows.clone()));
        rows
    }

    pub fn cells(&self, row: WrappedRow) -> &[Cell] {
        &self.block.lines[row.line][row.start..row.end]
    }
}

fn wrap_lines(lines: &[Vec<Cell>], columns: usize) -> Vec<WrappedRow> {
    let mut rows = Vec::new();
    for (line, cells) in lines.iter().enumerate() {
        if cells.is_empty() {
            rows.push(WrappedRow {
                line,
                start: 0,
                end: 0,
            });
            continue;
        }
        let mut start = 0;
        while start < cells.len() {
            let mut end = (start + columns).min(cells.len());
            // Never split a wide character from its spacer.
            if end < cells.len()
                && end > start + 1
                && cells[end - 1].flags.contains(Flags::WIDE_CHAR)
            {
                end -= 1;
            }
            rows.push(WrappedRow { line, start, end });
            start = end;
        }
    }
    rows
}

/// A position in the timeline: block `item`, display row, column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimelinePoint {
    pub item: usize,
    pub row: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct TimelineSelection {
    pub anchor: TimelinePoint,
    pub head: TimelinePoint,
}

impl TimelineSelection {
    pub fn ordered(&self) -> (TimelinePoint, TimelinePoint) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn contains(&self, point: TimelinePoint) -> bool {
        let (start, end) = self.ordered();
        start <= point && point <= end
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Selected text, joining soft-wrapped rows without a newline.
    pub fn text(&self, blocks: &[Rc<FrozenBlock>], columns: usize) -> String {
        let (start, end) = self.ordered();
        let mut out = String::new();
        let last_item = end.item.min(blocks.len().saturating_sub(1));
        for (item, block) in blocks
            .iter()
            .enumerate()
            .take(last_item + 1)
            .skip(start.item)
        {
            let rows = block.rows(columns);
            let first = if item == start.item { start.row } else { 0 };
            let last = if item == end.item {
                end.row
            } else {
                rows.len().saturating_sub(1)
            };
            if item > start.item {
                out.push('\n');
            }
            for r in first..=last.min(rows.len().saturating_sub(1)) {
                let row = rows[r];
                let from = if item == start.item && r == start.row {
                    start.column
                } else {
                    0
                };
                let to = if item == end.item && r == end.row {
                    end.column + 1
                } else {
                    usize::MAX
                };
                let cells = block.cells(row);
                for (column, cell) in cells.iter().enumerate() {
                    if column < from || column >= to {
                        continue;
                    }
                    if cell.flags.intersects(Flags::WIDE_CHAR_SPACER) {
                        continue;
                    }
                    out.push(cell.c);
                    if let Some(extra) = cell.zerowidth() {
                        out.extend(extra);
                    }
                }
                let continues = rows.get(r + 1).is_some_and(|next| next.line == row.line);
                if r < last && !continues {
                    // Trailing blanks were trimmed when the block was stored.
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    out.push('\n');
                }
            }
        }
        out
    }
}

/// Draws a block's output.
pub struct BlockBody {
    view: Entity<TerminalView>,
    item: usize,
    block: Rc<FrozenBlock>,
    columns: usize,
    metrics: CellMetrics,
}

impl BlockBody {
    pub fn new(
        view: Entity<TerminalView>,
        item: usize,
        block: Rc<FrozenBlock>,
        columns: usize,
        metrics: CellMetrics,
    ) -> Self {
        Self {
            view,
            item,
            block,
            columns,
            metrics,
        }
    }
}

impl IntoElement for BlockBody {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BlockBody {
    type RequestLayoutState = ();
    type PrepaintState = PaintedGrid;

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
        let rows = self.block.rows(self.columns).len();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (self.metrics.line_height * rows as f32).into();
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
    ) -> PaintedGrid {
        let rows = self.block.rows(self.columns);
        let view = self.view.read(cx);
        let selection = view.timeline_selection;
        let search = view.search_highlights();
        // Frozen output uses the default palette: programs' palette changes (OSC 4) were for
        // the live screen at the time.
        let colors = Colors::default();
        let mut painter = GridPainter::new(bounds.origin, self.metrics, &colors, Tone::Muted)
            .with_scale(window.scale_factor());
        let visible =
            crate::grid_paint::visible_rows(bounds, self.metrics.line_height, rows.len(), window);
        for (r, row) in rows
            .iter()
            .enumerate()
            .skip(visible.start)
            .take(visible.len())
        {
            for (column, cell) in self.block.cells(*row).iter().enumerate() {
                let point = TimelinePoint {
                    item: self.item,
                    row: r,
                    column,
                };
                let state = CellState {
                    selected: selection.is_some_and(|s| !s.is_empty() && s.contains(point)),
                    search_match: search.block_contains(self.item, row.line, row.start + column),
                    under_block_cursor: false,
                };
                painter.cell(r, column, cell, state);
            }
        }
        let region = HitRegion::Block {
            item: self.item,
            bounds,
            rows: rows.clone(),
        };
        self.view
            .update(cx, |view, _| view.hit_regions.push(region));
        painter.finish(window)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        grid: &mut PaintedGrid,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            grid.paint_backgrounds(window);
            grid.paint_text(window, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> Vec<Cell> {
        text.chars()
            .map(|c| Cell {
                c,
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn wraps_long_lines_and_keeps_empty_ones() {
        let rows = wrap_lines(&[line("abcdef"), line(""), line("xy")], 4);
        assert_eq!(
            rows,
            vec![
                WrappedRow {
                    line: 0,
                    start: 0,
                    end: 4
                },
                WrappedRow {
                    line: 0,
                    start: 4,
                    end: 6
                },
                WrappedRow {
                    line: 1,
                    start: 0,
                    end: 0
                },
                WrappedRow {
                    line: 2,
                    start: 0,
                    end: 2
                },
            ]
        );
    }

    #[test]
    fn does_not_split_wide_characters() {
        let mut cells = line("abc");
        cells.push(Cell {
            c: '宽',
            flags: Flags::WIDE_CHAR,
            ..Default::default()
        });
        cells.push(Cell {
            c: ' ',
            flags: Flags::WIDE_CHAR_SPACER,
            ..Default::default()
        });
        let rows = wrap_lines(&[cells], 4);
        assert_eq!(
            rows[0],
            WrappedRow {
                line: 0,
                start: 0,
                end: 3
            }
        );
        assert_eq!(
            rows[1],
            WrappedRow {
                line: 0,
                start: 3,
                end: 5
            }
        );
    }
}
