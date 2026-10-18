//! Kiln: a calm, cross-platform terminal. Phase 0 spike.

mod terminal_element;
mod terminal_view;
mod theme;
mod workspace;

use std::time::Instant;

use gpui::{
    App, AppContext as _, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds,
    WindowOptions, actions, point, px, size,
};
use kiln_term::{GridSize, SessionOptions, ShellCommand};

use crate::terminal_element::{CellMetrics, PADDING_BOTTOM, PADDING_TOP, PADDING_X};
use crate::workspace::{TITLEBAR_HEIGHT, Workspace};

actions!(
    kiln,
    [
        Quit,
        NewWindow,
        CloseWindow,
        Copy,
        Paste,
        SelectAll,
        ScrollPageUp,
        ScrollPageDown,
        ScrollToTop,
        ScrollToBottom,
    ]
);

/// Font size in points, as in the WezTerm config.
const FONT_SIZE: f32 = 13.;
/// Initial grid, as in the WezTerm config.
const INITIAL_COLUMNS: usize = 220;
const INITIAL_LINES: usize = 50;

#[derive(Clone)]
struct Args {
    shell: Option<ShellCommand>,
    /// Quit after the first frame that shows shell output and print the time it took.
    bench_startup: bool,
    columns: usize,
    lines: usize,
}

fn parse_args() -> Args {
    let mut args = Args {
        shell: None,
        bench_startup: false,
        columns: INITIAL_COLUMNS,
        lines: INITIAL_LINES,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bench-startup" => args.bench_startup = true,
            "--cols" => {
                args.columns = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(args.columns)
            }
            "--rows" => args.lines = it.next().and_then(|v| v.parse().ok()).unwrap_or(args.lines),
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
            "--help" | "-h" => {
                println!(
                    "kiln [--cols N] [--rows N] [--bench-startup] [--wsl | --pwsh] [-- program args...]"
                );
                std::process::exit(0);
            }
            other => eprintln!("kiln: ignoring unknown argument {other}"),
        }
    }
    args
}

fn main() {
    let started = Instant::now();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args = parse_args();

    gpui_platform::application().run(move |cx: &mut App| {
        terminal_element::load_embedded_fonts(cx);
        bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let new_window_args = args.clone();
        cx.on_action(move |_: &NewWindow, cx| {
            open_window(
                Args {
                    bench_startup: false,
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
                items: vec![MenuItem::action("Quit Kiln", Quit)],
                disabled: false,
            },
            Menu {
                name: "Shell".into(),
                items: vec![
                    MenuItem::action("New Window", NewWindow),
                    MenuItem::action("Close Window", CloseWindow),
                ],
                disabled: false,
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::action("Copy", Copy),
                    MenuItem::action("Paste", Paste),
                    MenuItem::action("Select All", SelectAll),
                ],
                disabled: false,
            },
        ]);

        open_window(args.clone(), started, cx);
        cx.activate(true);
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
        KeyBinding::new(&k("n"), NewWindow, None),
        KeyBinding::new(&k("w"), CloseWindow, None),
        KeyBinding::new(&k("q"), Quit, None),
        KeyBinding::new("shift-pageup", ScrollPageUp, terminal),
        KeyBinding::new("shift-pagedown", ScrollPageDown, terminal),
        KeyBinding::new("shift-home", ScrollToTop, terminal),
        KeyBinding::new("shift-end", ScrollToBottom, terminal),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("shift-insert", Paste, terminal)]);
}

fn open_window(args: Args, started: Instant, cx: &mut App) {
    // Size the window for the initial grid, from unsnapped metrics (the element snaps later).
    let text_system = cx.text_system().clone();
    let font_id = text_system.resolve_font(&terminal_element::base_font());
    let font_size = px(FONT_SIZE);
    let cell_w = text_system
        .advance(font_id, font_size, 'm')
        .map(|s| s.width)
        .unwrap_or(px(8.));
    let line_h =
        text_system.ascent(font_id, font_size) + text_system.descent(font_id, font_size).abs();
    let width = cell_w * args.columns as f32 + px(2. * PADDING_X);
    let height = line_h * args.lines as f32 + px(PADDING_TOP + PADDING_BOTTOM) + TITLEBAR_HEIGHT;
    let bounds = Bounds::centered(None, size(width, height), cx);

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Kiln".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.), px(11.))),
        }),
        app_id: Some("dev.kiln.Kiln".into()),
        window_min_size: Some(size(px(320.), px(160.))),
        ..Default::default()
    };

    let result = cx.open_window(options, move |window, cx| {
        let metrics = CellMetrics::measure(font_size, window);
        let mut session = SessionOptions::default();
        if let Some(shell) = args.shell.clone() {
            session.shell = shell;
        }
        let grid = GridSize {
            columns: args.columns,
            lines: args.lines,
            cell_width: (f32::from(metrics.cell_width) * window.scale_factor()) as u16,
            cell_height: (f32::from(metrics.line_height) * window.scale_factor()) as u16,
        };
        cx.new(|cx| {
            Workspace::new(
                session,
                grid,
                metrics,
                args.bench_startup,
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
