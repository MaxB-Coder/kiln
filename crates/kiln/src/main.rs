//! Kiln: a calm, cross-platform terminal.

// No console window behind the app on Windows (release builds). The CLI output used by
// `--bench-startup` and `--help` still reaches a console the app was started from.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::time::Instant;

use gpui::{
    App, AppContext as _, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowDecorations, WindowOptions, point, px, size,
};
use kiln::grid_paint::{self, CellMetrics};
use kiln::terminal_view::{PADDING_BOTTOM, PADDING_TOP, PADDING_X};
use kiln::workspace::{TITLEBAR_HEIGHT, Workspace};
use kiln::*;
use kiln_term::ShellCommand;

#[derive(Clone)]
struct Args {
    shell: Option<ShellCommand>,
    /// Quit after the first frame that shows shell output and print the time it took.
    bench_startup: bool,
    /// Open without taking focus from the app in front (for tests and scripts).
    background: bool,
    columns: Option<usize>,
    lines: Option<usize>,
    /// Reopen the last window's sessions.
    restore: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        shell: None,
        bench_startup: false,
        background: false,
        columns: None,
        lines: None,
        restore: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bench-startup" => args.bench_startup = true,
            "--background" => args.background = true,
            "--fresh" => args.restore = false,
            "--cols" => args.columns = it.next().and_then(|v| v.parse().ok()),
            "--rows" => args.lines = it.next().and_then(|v| v.parse().ok()),
            #[cfg(windows)]
            "--wsl" => args.shell = Some(ShellCommand::wsl(None)),
            #[cfg(windows)]
            "--pwsh" => args.shell = Some(ShellCommand::powershell()),
            "--" => {
                let rest: Vec<String> = it.by_ref().collect();
                if let Some((program, rest)) = rest.split_first() {
                    args.shell = Some(ShellCommand::new(program.clone(), rest.iter().cloned()));
                }
            }
            "--version" | "-V" => {
                println!("kiln {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "kiln [--cols N] [--rows N] [--background] [--fresh] [--bench-startup] [--wsl | --pwsh] [-- program args...]"
                );
                std::process::exit(0);
            }
            other => eprintln!("kiln: ignoring unknown argument {other}"),
        }
    }
    args
}

#[cfg(windows)]
fn attach_parent_console() {
    // In the Windows subsystem the app has no console; borrow the one it was started from,
    // if any, so command-line output still shows.
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() {
    let started = Instant::now();
    // SAFETY: first thing in main; no other thread exists yet.
    unsafe { kiln_update::take_appimage_env() };
    #[cfg(windows)]
    attach_parent_console();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args = parse_args();
    let (config, _) = config::load();
    // A given program, or a benchmark, starts with just that; otherwise the last sessions.
    args.restore &= config.window.restore && args.shell.is_none() && !args.bench_startup;
    config::set_current(config);

    gpui_platform::application().run(move |cx: &mut App| {
        cx.set_app_identity("dev.kiln.Kiln", "Kiln");
        grid_paint::load_embedded_fonts(cx);
        bind_keys(cx);
        // A clicked notification brings back the session it came from.
        cx.on_system_notification_response(|response, cx| {
            let Some((window_id, tab_id)) = kiln::workspace::parse_notification_tag(&response.tag)
            else {
                return;
            };
            for handle in cx.windows() {
                if handle.window_id().as_u64() != window_id {
                    continue;
                }
                if let Some(handle) = handle.downcast::<Workspace>() {
                    let _ = handle.update(cx, |workspace, window, cx| {
                        workspace.activate_tab_id(tab_id, window, cx);
                        window.activate_window();
                    });
                }
            }
            cx.activate(true);
        });
        cx.on_action(|_: &Quit, cx| cx.quit());
        let new_window_args = args.clone();
        cx.on_action(move |_: &NewWindow, cx| {
            open_window(
                Args {
                    bench_startup: false,
                    restore: false,
                    ..new_window_args.clone()
                },
                Instant::now(),
                cx,
            )
        });
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.set_menus([
            Menu {
                name: "Kiln".into(),
                items: vec![
                    MenuItem::action("Settings…", EditConfig),
                    MenuItem::separator(),
                    MenuItem::action("Quit Kiln", Quit),
                ],
                disabled: false,
            },
            Menu {
                name: "Shell".into(),
                items: vec![
                    MenuItem::action("New Session", NewSession),
                    MenuItem::action("Split Right", SplitRight),
                    MenuItem::action("New Window", NewWindow),
                    MenuItem::separator(),
                    MenuItem::action("Close Pane", ClosePane),
                    MenuItem::action("Close Window", CloseWindow),
                ],
                disabled: false,
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::action("Copy", Copy),
                    MenuItem::action("Copy Last Output", CopyLastOutput),
                    MenuItem::action("Copy Last Output (Raw)", CopyLastOutputRaw),
                    MenuItem::action("Paste", Paste),
                    MenuItem::action("Select All", SelectAll),
                    MenuItem::separator(),
                    MenuItem::action("Find…", Find),
                ],
                disabled: false,
            },
            Menu {
                name: "View".into(),
                items: vec![
                    MenuItem::action("Command Palette", CommandPaletteToggle),
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Toggle Context Rail", ToggleRail),
                    MenuItem::action("Focus Mode", ToggleFocusMode),
                    MenuItem::separator(),
                    MenuItem::action("Bigger", ZoomIn),
                    MenuItem::action("Smaller", ZoomOut),
                    MenuItem::action("Actual Size", ZoomReset),
                ],
                disabled: false,
            },
        ]);

        open_window(args.clone(), started, cx);
        if !args.bench_startup {
            update::start(cx);
        }
        if !args.background {
            cx.activate(true);
        }
    });
}

fn bind_keys(cx: &mut App) {
    // Every ⌘ shortcut on macOS is Ctrl+Shift on Windows and Linux.
    #[cfg(target_os = "macos")]
    let primary = "cmd";
    #[cfg(not(target_os = "macos"))]
    let primary = "ctrl-shift";
    let k = |keys: &str| format!("{primary}-{keys}");

    let terminal = Some("Terminal");
    cx.bind_keys([
        KeyBinding::new(&k("c"), Copy, terminal),
        KeyBinding::new(&k("v"), Paste, terminal),
        KeyBinding::new(&k("a"), SelectAll, terminal),
        KeyBinding::new(&k("alt-c"), CopyLastOutputRaw, terminal),
        KeyBinding::new(&k("n"), NewWindow, None),
        KeyBinding::new(&k("t"), NewSession, None),
        KeyBinding::new(&k("w"), ClosePane, None),
        KeyBinding::new(&k("d"), SplitRight, None),
        KeyBinding::new(&k("]"), FocusNextPane, None),
        KeyBinding::new(&k("["), FocusPrevPane, None),
        KeyBinding::new(&k("b"), ToggleSidebar, None),
        KeyBinding::new(&k("i"), ToggleRail, None),
        KeyBinding::new(&k("."), ToggleFocusMode, None),
        KeyBinding::new(&k("f"), Find, None),
        KeyBinding::new(&k("k"), CommandPaletteToggle, None),
        KeyBinding::new(&k("p"), CommandPaletteToggle, None),
        KeyBinding::new(&k(","), EditConfig, None),
        KeyBinding::new(&k("q"), Quit, None),
        KeyBinding::new(&k("="), ZoomIn, None),
        KeyBinding::new(&k("-"), ZoomOut, None),
        KeyBinding::new(&k("0"), ZoomReset, None),
        KeyBinding::new("shift-pageup", ScrollPageUp, terminal),
        KeyBinding::new("shift-pagedown", ScrollPageDown, terminal),
        KeyBinding::new("shift-home", ScrollToTop, terminal),
        KeyBinding::new("shift-end", ScrollToBottom, terminal),
    ]);

    #[cfg(target_os = "macos")]
    cx.bind_keys([
        // ⌘⇧C: last output as a fenced block. Elsewhere Ctrl+Shift+C covers it (the selection,
        // or the last output when nothing is selected).
        KeyBinding::new("cmd-shift-c", CopyLastOutput, terminal),
        KeyBinding::new("cmd-shift-]", NextSession, None),
        KeyBinding::new("cmd-shift-[", PrevSession, None),
        KeyBinding::new("cmd-1", SelectSession1, None),
        KeyBinding::new("cmd-2", SelectSession2, None),
        KeyBinding::new("cmd-3", SelectSession3, None),
        KeyBinding::new("cmd-4", SelectSession4, None),
        KeyBinding::new("cmd-5", SelectSession5, None),
        KeyBinding::new("cmd-6", SelectSession6, None),
        KeyBinding::new("cmd-7", SelectSession7, None),
        KeyBinding::new("cmd-8", SelectSession8, None),
        KeyBinding::new("cmd-9", SelectSession9, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("shift-insert", Paste, terminal),
        KeyBinding::new("ctrl-tab", NextSession, None),
        KeyBinding::new("ctrl-shift-tab", PrevSession, None),
        KeyBinding::new("ctrl-alt-1", SelectSession1, None),
        KeyBinding::new("ctrl-alt-2", SelectSession2, None),
        KeyBinding::new("ctrl-alt-3", SelectSession3, None),
        KeyBinding::new("ctrl-alt-4", SelectSession4, None),
        KeyBinding::new("ctrl-alt-5", SelectSession5, None),
        KeyBinding::new("ctrl-alt-6", SelectSession6, None),
        KeyBinding::new("ctrl-alt-7", SelectSession7, None),
        KeyBinding::new("ctrl-alt-8", SelectSession8, None),
        KeyBinding::new("ctrl-alt-9", SelectSession9, None),
    ]);
}

fn open_window(args: Args, started: Instant, cx: &mut App) {
    let config = config::current();
    let columns = args.columns.unwrap_or(config.window.columns);
    let lines = args.lines.unwrap_or(config.window.rows);
    let font_size = px(config.font_size.clamp(6., 72.));
    let sidebar = if config.window.sidebar { 220. } else { 0. };

    // Size the window for the initial grid, from unsnapped metrics (the panes snap later).
    let text_system = cx.text_system().clone();
    let font_id = text_system.resolve_font(&grid_paint::base_font());
    let cell_w = text_system
        .advance(font_id, font_size, 'm')
        .map(|s| s.width)
        .unwrap_or(px(8.));
    let line_h =
        text_system.ascent(font_id, font_size) + text_system.descent(font_id, font_size).abs();
    let window_size = |cell_w: gpui::Pixels, line_h: gpui::Pixels| {
        size(
            cell_w * columns as f32 + px(2. * PADDING_X + sidebar),
            line_h * lines as f32 + px(PADDING_TOP + PADDING_BOTTOM) + TITLEBAR_HEIGHT,
        )
    };
    let bounds = Bounds::centered(None, window_size(cell_w, line_h), cx);

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Kiln".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(11.))),
        }),
        app_id: Some("dev.kiln.Kiln".into()),
        focus: !args.background,
        window_min_size: Some(size(px(360.), px(200.))),
        // Linux: Kiln draws its own title bar, so it looks the same on GNOME, KDE and X11.
        window_decorations: Some(if cfg!(target_os = "linux") {
            WindowDecorations::Client
        } else {
            WindowDecorations::Server
        }),
        ..Default::default()
    };

    let result = cx.open_window(options, move |window, cx| {
        let metrics = CellMetrics::measure(font_size, window);
        // Cells snap to device pixels; size the window for the snapped cell so the requested
        // columns and rows fit exactly.
        window.resize(window_size(metrics.cell_width, metrics.line_height));
        cx.new(|cx| {
            Workspace::new(
                args.shell.clone(),
                metrics,
                args.bench_startup,
                args.restore,
                started,
                window,
                cx,
            )
        })
    });
    if let Err(err) = result {
        log::error!("could not open a window: {err:#}");
    }
}
