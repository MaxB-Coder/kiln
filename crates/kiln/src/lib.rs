//! Kiln: a calm, cross-platform terminal. The library holds the app; `main.rs` starts it, and
//! the `snapshot` example renders it headless for visual checks.

pub mod block_element;
pub mod box_drawing;
pub mod config;
pub mod context;
pub mod copy;
pub mod git;
pub mod grid_paint;
pub mod live_element;
pub mod logs;
pub mod palette;
pub mod rail;
pub mod remote;
pub mod search;
pub mod state;
pub mod terminal_view;
pub mod text_input;
pub mod theme;
pub mod workspace;

use gpui::actions;

actions!(
    kiln,
    [
        Quit,
        NewWindow,
        CloseWindow,
        NewSession,
        ClosePane,
        SplitRight,
        FocusNextPane,
        FocusPrevPane,
        NextSession,
        PrevSession,
        SelectSession1,
        SelectSession2,
        SelectSession3,
        SelectSession4,
        SelectSession5,
        SelectSession6,
        SelectSession7,
        SelectSession8,
        SelectSession9,
        ToggleSidebar,
        ToggleRail,
        ToggleFocusMode,
        Find,
        CommandPaletteToggle,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        EditConfig,
        Copy,
        CopyLastOutput,
        CopyLastOutputRaw,
        Paste,
        SelectAll,
        ScrollPageUp,
        ScrollPageDown,
        ScrollToTop,
        ScrollToBottom,
    ]
);
