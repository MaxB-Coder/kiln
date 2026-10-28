//! The widths of the sidebar and the context rail: each can be dragged by its inner edge,
//! within its range, and the terminal between them always keeps room for `MIN_COLUMNS`
//! (the panels give way first when the window is small).

/// The sidebar's width: least, default, most.
pub const SIDEBAR: Range = Range {
    min: 160.,
    default: 220.,
    max: 420.,
};
/// The context rail's.
pub const RAIL: Range = Range {
    min: 180.,
    default: 280.,
    max: 480.,
};
/// Columns the terminal keeps however the panels are dragged.
pub const MIN_COLUMNS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

impl Range {
    pub fn clamp(&self, width: f32) -> f32 {
        width.clamp(self.min, self.max)
    }
}

/// The widths the shown panels get in a window `window` wide, from the widths wanted (`None`:
/// the panel is hidden), leaving the terminal at least `terminal` px. Each is first held to its
/// range; then, while the terminal is short of room, the rail gives way, then the sidebar,
/// each down to its least width. (A window too small even then squeezes the terminal.)
pub fn widths(
    window: f32,
    sidebar: Option<f32>,
    rail: Option<f32>,
    terminal: f32,
) -> (Option<f32>, Option<f32>) {
    let mut sidebar = sidebar.map(|w| SIDEBAR.clamp(w));
    let mut rail = rail.map(|w| RAIL.clamp(w));
    let short = |sidebar: Option<f32>, rail: Option<f32>| {
        terminal - (window - sidebar.unwrap_or(0.) - rail.unwrap_or(0.))
    };
    if let Some(width) = rail.as_mut() {
        let over = short(sidebar, Some(*width));
        if over > 0. {
            *width = (*width - over).max(RAIL.min);
        }
    }
    if let Some(width) = sidebar.as_mut() {
        let over = short(Some(*width), rail);
        if over > 0. {
            *width = (*width - over).max(SIDEBAR.min);
        }
    }
    (sidebar, rail)
}

/// The room the terminal needs for `MIN_COLUMNS` of `cell` px, with its padding.
pub fn terminal_min(cell: f32) -> f32 {
    MIN_COLUMNS as f32 * cell + 2. * crate::terminal_view::PADDING_X
}

#[cfg(test)]
mod tests {
    use super::*;

    const TERMINAL: f32 = 400.;

    #[test]
    fn widths_stay_in_their_ranges() {
        let wide = 2000.;
        assert_eq!(
            widths(wide, Some(100.), Some(100.), TERMINAL),
            (Some(160.), Some(180.))
        );
        assert_eq!(
            widths(wide, Some(900.), Some(900.), TERMINAL),
            (Some(420.), Some(480.))
        );
        assert_eq!(
            widths(wide, Some(250.), Some(300.), TERMINAL),
            (Some(250.), Some(300.))
        );
        // A hidden panel takes nothing.
        assert_eq!(widths(wide, None, Some(300.), TERMINAL), (None, Some(300.)));
    }

    #[test]
    fn the_terminal_keeps_its_columns_and_the_rail_gives_way_first() {
        // 1000 px: 420 + 480 would leave 100 for the terminal; it needs 400.
        let (sidebar, rail) = widths(1000., Some(420.), Some(480.), TERMINAL);
        assert_eq!(rail, Some(180.), "the rail goes to its least first");
        assert_eq!(sidebar, Some(420.), "then there is room");
        let (sidebar, rail) = widths(900., Some(420.), Some(480.), TERMINAL);
        assert_eq!(rail, Some(180.));
        assert_eq!(sidebar, Some(320.), "the sidebar gives the rest");
        assert_eq!(900. - 320. - 180., TERMINAL);
        // Too small for both at their least: they stay there, and the terminal is squeezed.
        assert_eq!(
            widths(600., Some(300.), Some(300.), TERMINAL),
            (Some(160.), Some(180.))
        );
        // Without a rail, the sidebar alone gives way.
        assert_eq!(widths(700., Some(420.), None, TERMINAL), (Some(300.), None));
    }

    #[test]
    fn forty_columns_of_terminal() {
        assert_eq!(terminal_min(8.), 40. * 8. + 40.);
    }
}
