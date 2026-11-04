//! Kiln: a calm, cross-platform terminal.

// No console window behind the app on Windows (release builds). The CLI output used by
// `--bench-startup` and `--help` still reaches a console the app was started from.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::time::Instant;

use gpui::{
    App, AppContext as _, Bounds, Menu, MenuItem, TitlebarOptions, WindowBounds, WindowDecorations,
    WindowOptions, point, px, size,
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
            // The flame moves even without focus, so its cost can be measured on a window in
            // the background (see notes/concept-check.md).
            "--bench-flame" => flame::force_animation(true),
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
                    "kiln [--cols N] [--rows N] [--background] [--fresh] [--bench-startup] [--bench-flame] [--wsl | --pwsh] [-- program args...]"
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
            // Beside the window in front, at its size.
            let place = cx
                .active_window()
                .and_then(|w| {
                    w.update(cx, |_, window, cx| {
                        (
                            window.window_bounds().get_bounds(),
                            window.display(cx).map(|d| d.id()),
                        )
                    })
                    .ok()
                })
                .map_or(Place::Launch, |(bounds, display)| {
                    Place::Beside(bounds, display)
                });
            open_window(
                Args {
                    bench_startup: false,
                    restore: false,
                    ..new_window_args.clone()
                },
                place,
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
                    MenuItem::action("Focus Context Rail", FocusRail),
                    MenuItem::action("Focus Mode", ToggleFocusMode),
                    MenuItem::separator(),
                    MenuItem::action("Bigger", ZoomIn),
                    MenuItem::action("Smaller", ZoomOut),
                    MenuItem::action("Actual Size", ZoomReset),
                ],
                disabled: false,
            },
            Menu {
                name: "Help".into(),
                items: vec![MenuItem::action("Keyboard Shortcuts", ToggleShortcuts)],
                disabled: false,
            },
        ]);

        open_window(args.clone(), Place::Launch, started, cx);
        if !args.bench_startup {
            update::start(cx);
        }
        if !args.background {
            cx.activate(true);
        }
    });
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys(kiln::keymap::bindings(kiln::keymap::KeyStyle::current()));
}

/// Where a window opens: the first one as the setting and the last window say, a new one
/// beside the window in front.
enum Place {
    Launch,
    Beside(Bounds<gpui::Pixels>, Option<gpui::DisplayId>),
}

fn open_window(args: Args, place: Place, started: Instant, cx: &mut App) {
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
    // A size asked for on the command line, or a benchmark: exactly that, centred.
    let exact = args.columns.is_some() || args.lines.is_some() || args.bench_startup;
    let open = config.window.open;
    let (window_bounds, display_id, sized_by_grid) = if exact {
        let bounds = Bounds::centered(None, window_size(cell_w, line_h), cx);
        (WindowBounds::Windowed(bounds), None, true)
    } else {
        match place {
            Place::Launch => {
                // The main display first.
                let mut displays = cx.displays();
                if let Some(main) = cx.primary_display() {
                    displays.sort_by_key(|d| d.id() != main.id());
                }
                let screens: Vec<placement::Screen> = displays
                    .iter()
                    .map(|d| placement::Screen {
                        uuid: d.uuid().ok().map(|u| u.to_string()),
                        bounds: d.visible_bounds(),
                    })
                    .collect();
                let saved = state::current().window.and_then(|w| w.placement);
                let (bounds, screen) =
                    placement::initial(saved.as_ref(), open, &screens, window_size(cell_w, line_h));
                let windowed_fresh = saved.is_none() && matches!(bounds, WindowBounds::Windowed(_));
                (bounds, displays.get(screen).map(|d| d.id()), windowed_fresh)
            }
            Place::Beside(current, display) => {
                let area = display
                    .and_then(|id| cx.find_display(id))
                    .map(|d| d.visible_bounds());
                (placement::beside(current, open, area), display, false)
            }
        }
    };

    let options = WindowOptions {
        window_bounds: Some(window_bounds),
        display_id,
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
        // Cells snap to device pixels; size a window opened for a number of columns and rows
        // for the snapped cell so they fit exactly. Restored windows keep their size.
        if sized_by_grid {
            window.resize(window_size(metrics.cell_width, metrics.line_height));
        }
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
