//! Find in a pane (⌘F / Ctrl+Shift+F): matches in finished blocks and in the live grid.

use std::collections::HashMap;
use std::rc::Rc;

use kiln_term::engine::grid::Dimensions;
use kiln_term::engine::index::{Column, Line, Point as GridPoint};
use kiln_term::engine::term::cell::{Cell, Flags};

use crate::block_element::FrozenBlock;

/// One match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    /// In a finished block's command line: byte range `start..end`.
    Header {
        item: usize,
        start: usize,
        end: usize,
    },
    /// In a finished block: logical line and cell range `start..end`.
    Block {
        item: usize,
        line: usize,
        start: usize,
        end: usize,
    },
    /// In the live grid, inclusive cell range on one row.
    Live {
        line: Line,
        start: Column,
        end: Column,
    },
}

/// Matches in one block: (logical line, first cell, end cell).
type BlockRanges = HashMap<usize, Vec<(usize, usize, usize)>>;

/// What the elements need to highlight matches. Cheap to clone.
#[derive(Clone, Default)]
pub struct SearchHighlights {
    live: Rc<Vec<(Line, Column, Column)>>,
    blocks: Rc<BlockRanges>,
    headers: Rc<HashMap<usize, Vec<(usize, usize)>>>,
}

impl SearchHighlights {
    pub fn from_matches(matches: &[Match]) -> Self {
        let mut live = Vec::new();
        let mut blocks = BlockRanges::new();
        let mut headers: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for m in matches {
            match *m {
                Match::Header { item, start, end } => {
                    headers.entry(item).or_default().push((start, end))
                }
                Match::Block {
                    item,
                    line,
                    start,
                    end,
                } => blocks.entry(item).or_default().push((line, start, end)),
                Match::Live { line, start, end } => live.push((line, start, end)),
            }
        }
        Self {
            live: Rc::new(live),
            blocks: Rc::new(blocks),
            headers: Rc::new(headers),
        }
    }

    pub fn live_contains(&self, point: GridPoint) -> bool {
        self.live.iter().any(|&(line, start, end)| {
            line == point.line && start <= point.column && point.column <= end
        })
    }

    /// Byte ranges of matches in a block's command line.
    pub fn header_ranges(&self, item: usize) -> &[(usize, usize)] {
        self.headers.get(&item).map_or(&[], Vec::as_slice)
    }

    pub fn block_contains(&self, item: usize, line: usize, cell: usize) -> bool {
        self.blocks.get(&item).is_some_and(|ranges| {
            ranges
                .iter()
                .any(|&(l, start, end)| l == line && start <= cell && cell < end)
        })
    }
}

/// Text of a row of cells, plus the cell index of each char.
fn row_text(cells: &[Cell]) -> (String, Vec<usize>) {
    let mut text = String::with_capacity(cells.len());
    let mut index = Vec::with_capacity(cells.len());
    for (i, cell) in cells.iter().enumerate() {
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        for c in cell.c.to_lowercase() {
            text.push(c);
            index.push(i);
        }
    }
    (text, index)
}

/// Case-insensitive matches of `query` in `cells`, as cell ranges `start..end`.
fn find_in_cells(cells: &[Cell], query: &str) -> Vec<(usize, usize)> {
    let (text, index) = row_text(cells);
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = query.to_lowercase().chars().collect();
    let mut out = Vec::new();
    if needle.is_empty() || chars.len() < needle.len() {
        return out;
    }
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()] == needle[..] {
            let start = index[i];
            let end = index[i + needle.len() - 1] + 1;
            out.push((start, end));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Case-insensitive matches of `query` in `text`, as byte ranges.
fn find_in_text(text: &str, query: &str) -> Vec<(usize, usize)> {
    let needle: Vec<char> = query.to_lowercase().chars().collect();
    let chars: Vec<(usize, char)> = text
        .char_indices()
        .flat_map(|(i, c)| c.to_lowercase().map(move |l| (i, l)))
        .collect();
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()]
            .iter()
            .map(|(_, c)| *c)
            .eq(needle.iter().copied())
        {
            let (last_at, _) = chars[i + needle.len() - 1];
            let end = last_at + text[last_at..].chars().next().map_or(0, char::len_utf8);
            out.push((chars[i].0, end));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// All matches, oldest first.
pub fn find(
    query: &str,
    blocks: &[Rc<FrozenBlock>],
    term: &kiln_term::EngineTerm,
    live_top: usize,
) -> Vec<Match> {
    let mut matches = Vec::new();
    if query.is_empty() {
        return matches;
    }
    for (item, block) in blocks.iter().enumerate() {
        for (start, end) in find_in_text(&block.block.command, query) {
            matches.push(Match::Header { item, start, end });
        }
        for (line, cells) in block.block.lines.iter().enumerate() {
            for (start, end) in find_in_cells(cells, query) {
                matches.push(Match::Block {
                    item,
                    line,
                    start,
                    end,
                });
            }
        }
    }
    let grid = term.grid();
    let history = grid.history_size() as i32;
    let columns = grid.columns();
    let first = (live_top as i32 - history).max(-history);
    for line in first..grid.screen_lines() as i32 {
        let row = &grid[Line(line)];
        let cells: Vec<Cell> = (0..columns).map(|c| row[Column(c)].clone()).collect();
        for (start, end) in find_in_cells(&cells, query) {
            matches.push(Match::Live {
                line: Line(line),
                start: Column(start),
                end: Column(end - 1),
            });
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(text: &str) -> Vec<Cell> {
        text.chars()
            .map(|c| Cell {
                c,
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn finds_matches_in_command_lines() {
        assert_eq!(
            find_in_text("cd ~/Kiln && kiln", "kiln"),
            vec![(5, 9), (13, 17)]
        );
        assert_eq!(find_in_text("café", "É"), vec![(3, 5)]);
    }

    #[test]
    fn finds_case_insensitive_matches() {
        assert_eq!(
            find_in_cells(&cells("Error: error ERR"), "err"),
            vec![(0, 3), (7, 10), (13, 16)]
        );
        assert!(find_in_cells(&cells("abc"), "").is_empty());
    }
}
