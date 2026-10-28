//! Kiln: a calm, cross-platform terminal. The library holds the app; `main.rs` starts it, and
//! the `snapshot` example renders it headless for visual checks.

pub mod app_icon;
pub mod block_element;
pub mod box_drawing;
pub mod config;
pub mod config_write;
pub mod context;
pub mod copy;
pub mod custom_icon;
pub mod flame;
pub mod git;
pub mod grid_paint;
pub mod keymap;
pub mod live_element;
pub mod logs;
pub mod naming;
pub mod palette;
pub mod panels;
pub mod pills;
pub mod rail;
pub mod remote;
pub mod search;
pub mod settings;
pub mod settings_theme;
pub mod shortcuts;
pub mod state;
pub mod terminal_view;
pub mod text_input;
pub mod theme;
pub mod update;
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
        ToggleShortcuts,
        RenameSession,
        RenameProject,
        ProjectColour,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        EditConfig,
        OpenConfigFile,
        InstallUpdate,
        Copy,
        CopyLastOutput,
        CopyLastOutputRaw,
        SelectPrevCommand,
        SelectNextCommand,
        ExtendSelectionUp,
        ExtendSelectionDown,
        Paste,
        SelectAll,
        ScrollPageUp,
        ScrollPageDown,
        ScrollToTop,
        ScrollToBottom,
    ]
);
