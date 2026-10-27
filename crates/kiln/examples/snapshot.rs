//! Renders Kiln headless and saves screenshots of each feature.
//!
//! Uses GPUI's headless app context with the real text system and the real GPU renderer, and
//! real shells on real PTYs, so the pictures show Kiln as it behaves; no display is needed.
//!
//! Usage: cargo run -p kiln --example snapshot -- [out dir]
//! [phase1|phase2|phase3|prompt|shortcuts|concept]
//! (default: notes/screenshots/phase1, scenario phase1)

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Point, WindowHandle, px, size,
};
use kiln::config::{self, Config};
use kiln::grid_paint::{self, CellMetrics};
use kiln::keymap;
use kiln::terminal_view::TerminalView;
use kiln::workspace::Workspace;

struct Shots {
    cx: HeadlessAppContext,
    window: WindowHandle<Workspace>,
    out: PathBuf,
}

impl Shots {
    /// Lets real time pass (shells run on real threads) while GPUI processes what arrives.
    fn pump(&mut self, duration: Duration) {
        let end = Instant::now() + duration;
        while Instant::now() < end {
            std::thread::sleep(Duration::from_millis(25));
            // The headless app runs on a test clock: move it with real time so timers fire.
            self.cx.advance_clock(Duration::from_millis(25));
            self.cx.run_until_parked();
            self.redraw();
        }
    }

    /// Waits until `done` holds (checked against the focused pane), up to `limit`.
    fn wait_for(&mut self, limit: Duration, done: impl Fn(&TerminalView) -> bool) -> bool {
        let end = Instant::now() + limit;
        while Instant::now() < end {
            self.pump(Duration::from_millis(100));
            let ok = self.cx.update(|cx| {
                self.window
                    .read(cx)
                    .ok()
                    .and_then(|w| w.focused_view())
                    .is_some_and(|pane| done(pane.read(cx)))
            });
            if ok {
                return true;
            }
        }
        false
    }

    fn redraw(&mut self) {
        let window: AnyWindowHandle = self.window.into();
        let _ = self
            .cx
            .update_window(window, |_, window, _| window.refresh());
    }

    fn pane(&mut self) -> Entity<TerminalView> {
        self.cx
            .update(|cx| self.window.read(cx).unwrap().focused_view().unwrap())
    }

    /// Types a command at the prompt and waits for it to finish.
    fn run(&mut self, command: &str, wait: Duration) {
        let pane = self.pane();
        let before = self.cx.update(|cx| pane.read(cx).commands().len());
        self.cx
            .update(|cx| pane.update(cx, |pane, cx| pane.type_text(&format!("{command}\r"), cx)));
        let pane_for_wait = pane.clone();
        let end = Instant::now() + wait;
        while Instant::now() < end {
            self.pump(Duration::from_millis(100));
            let done = self.cx.update(|cx| {
                let p = pane_for_wait.read(cx);
                p.commands().len() > before && p.is_at_prompt()
            });
            if done {
                break;
            }
        }
        self.pump(Duration::from_millis(300));
    }

    fn type_only(&mut self, text: &str) {
        let pane = self.pane();
        self.cx
            .update(|cx| pane.update(cx, |pane, cx| pane.type_text(text, cx)));
    }

    fn action(&mut self, action: Box<dyn gpui::Action>) {
        let window: AnyWindowHandle = self.window.into();
        let _ = self
            .cx
            .update_window(window, |_, window, cx| window.dispatch_action(action, cx));
        self.pump(Duration::from_millis(300));
    }

    fn keystroke(&mut self, keys: &str) {
        let window: AnyWindowHandle = self.window.into();
        for key in keys.split(' ') {
            let keystroke = gpui::Keystroke::parse(key).unwrap();
            let _ = self.cx.update_window(window, |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx)
            });
        }
        self.pump(Duration::from_millis(200));
    }

    /// Types into whatever text field has focus (the find bar, the palette).
    fn type_into_field(&mut self, text: &str) {
        for c in text.chars() {
            self.keystroke(&c.to_string());
        }
    }

    fn mouse_move(&mut self, x: f32, y: f32) {
        let window: AnyWindowHandle = self.window.into();
        let event = PlatformInput::MouseMove(MouseMoveEvent {
            position: Point::new(px(x), px(y)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        });
        let _ = self
            .cx
            .update_window(window, |_, window, cx| window.dispatch_event(event, cx));
        self.pump(Duration::from_millis(200));
    }

    fn click(&mut self, x: f32, y: f32, modifiers: Modifiers) {
        let window: AnyWindowHandle = self.window.into();
        let position = Point::new(px(x), px(y));
        for event in [
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: 1,
            }),
        ] {
            let _ = self
                .cx
                .update_window(window, |_, window, cx| window.dispatch_event(event, cx));
        }
        self.pump(Duration::from_millis(200));
    }

    /// Scrolls whatever is under (x, y) by `dy` points (negative: down the page).
    fn scroll(&mut self, x: f32, y: f32, dy: f32) {
        let window: AnyWindowHandle = self.window.into();
        let event = PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
            position: Point::new(px(x), px(y)),
            delta: gpui::ScrollDelta::Pixels(Point::new(px(0.), px(dy))),
            modifiers: Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
        });
        let _ = self
            .cx
            .update_window(window, |_, window, cx| window.dispatch_event(event, cx));
        self.pump(Duration::from_millis(300));
    }

    /// A mouse click (`count` 2: a double-click) at a point in the window.
    fn click_button(&mut self, x: f32, y: f32, button: gpui::MouseButton, count: usize) {
        let window: AnyWindowHandle = self.window.into();
        for n in 1..=count {
            let position = Point::new(px(x), px(y));
            let down = PlatformInput::MouseDown(gpui::MouseDownEvent {
                button,
                position,
                modifiers: Modifiers::default(),
                click_count: n,
                first_mouse: false,
            });
            let up = PlatformInput::MouseUp(gpui::MouseUpEvent {
                button,
                position,
                modifiers: Modifiers::default(),
                click_count: n,
            });
            let _ = self.cx.update_window(window, |_, window, cx| {
                window.dispatch_event(down, cx);
                window.dispatch_event(up, cx);
            });
            self.pump(Duration::from_millis(60));
        }
        self.pump(Duration::from_millis(300));
    }

    fn titles(&mut self) -> Vec<String> {
        let window = self.window;
        self.cx
            .update(|cx| window.update(cx, |w, _, cx| w.session_titles(cx)))
            .unwrap()
    }

    fn workspace<R>(&mut self, f: impl FnOnce(&Workspace) -> R) -> R {
        self.cx.update(|cx| f(self.window.read(cx).unwrap()))
    }

    fn shot(&mut self, name: &str) {
        self.pump(Duration::from_millis(200));
        let window: AnyWindowHandle = self.window.into();
        match self.cx.capture_screenshot(window) {
            Ok(image) => {
                let path = self.out.join(format!("{name}.png"));
                image.save(&path).unwrap();
                println!("saved {}", path.display());
            }
            Err(err) => eprintln!("{name}: {err:#}"),
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let scenario = std::env::args().nth(2).unwrap_or_else(|| "phase1".into());
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("notes/screenshots/{scenario}")));
    std::fs::create_dir_all(&out).unwrap();
    // Saved sessions go to a scratch file, never the real one.
    let state_file =
        std::env::temp_dir().join(format!("kiln-snapshot-{}.json", std::process::id()));
    // SAFETY: no other thread runs yet.
    unsafe { std::env::set_var("KILN_STATE_FILE", &state_file) };
    // So is the config: Settings writes kiln.toml, and this must never be the user's.
    let config_home =
        std::env::temp_dir().join(format!("kiln-snapshot-config-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config_home);
    // SAFETY: as above.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &config_home) };
    // Kiln opens the way it does after a restart: one saved session, whose shell starts in its
    // folder (nothing is typed to get there).
    let start = if scenario == "phase2" || scenario == "concept" {
        demo_project()
    } else {
        std::env::current_dir().unwrap()
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let mut tabs = vec![serde_json::json!({ "panes": [start.to_string_lossy()], "focused": 0 })];
    if scenario == "prompt" {
        // A second session in the home folder, for the sidebar's Home group.
        tabs.push(serde_json::json!({ "panes": [home], "focused": 0 }));
    }
    let saved = serde_json::json!({
        "version": 1,
        "window": { "sidebar": true, "rail": false, "active": 0, "tabs": tabs },
        "projects": []
    });
    std::fs::write(&state_file, saved.to_string()).unwrap();

    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(()),
        gpui_platform::current_headless_renderer,
    );
    cx.allow_parking();
    cx.update(|cx| grid_paint::load_embedded_fonts(cx));
    // The app's own key bindings, as main registers them.
    cx.update(|cx| cx.bind_keys(keymap::bindings(keymap::KeyStyle::current())));
    config::set_current(Config {
        // Short, so the phase 2 scenario can show a finished command's attention dot.
        notify_after_secs: 2,
        // A blinking cursor would make the pictures depend on timing.
        cursor_blink: false,
        ..Config::default()
    });

    let window = cx
        .open_window(size(px(1180.), px(720.)), |window, cx| {
            let metrics = CellMetrics::measure(px(13.), window);
            cx.new(|cx| Workspace::new(None, metrics, false, true, Instant::now(), window, cx))
        })
        .unwrap();
    let mut s = Shots { cx, window, out };
    // As if it were the window in front: the flame only moves there.
    let handle: AnyWindowHandle = s.window.into();
    let _ =
        s.cx.update_window(handle, |_, window, _| window.activate_window());

    // 1. The prompt in the input bar, the sidebar with one session.
    let ready = s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    println!("shell ready: {ready}");
    if scenario == "prompt-switch" {
        prompt_switch(&mut s);
        let _ = std::fs::remove_file(&state_file);
        let _ = std::fs::remove_dir_all(&config_home);
        println!("done");
        return;
    }
    if scenario == "settings" {
        settings(&mut s);
        let _ = std::fs::remove_file(&state_file);
        let _ = std::fs::remove_dir_all(&config_home);
        println!("done");
        return;
    }
    if scenario == "bench-flame" {
        bench_flame(&mut s);
        let _ = std::fs::remove_file(&state_file);
        return;
    }
    if scenario == "concept" {
        concept(&mut s);
        let _ = std::fs::remove_file(&state_file);
        println!("done");
        return;
    }
    if scenario == "quiet" {
        quiet(&mut s);
        let _ = std::fs::remove_file(&state_file);
        let _ = std::fs::remove_dir_all(&config_home);
        println!("done");
        return;
    }
    if scenario == "sidebar" {
        sidebar(&mut s, &state_file);
        let _ = std::fs::remove_file(&state_file);
        let _ = std::fs::remove_dir_all(&config_home);
        println!("done");
        return;
    }
    if scenario == "shortcuts" {
        shortcuts(&mut s);
        let _ = std::fs::remove_file(&state_file);
        println!("done");
        return;
    }
    if scenario == "prompt" {
        prompt(&mut s);
        let _ = std::fs::remove_file(&state_file);
        println!("done");
        return;
    }
    if scenario == "phase3" {
        phase3(&mut s);
        let _ = std::fs::remove_file(&state_file);
        println!("done");
        return;
    }
    if scenario == "phase2" {
        phase2(&mut s);
        let _ = std::fs::remove_file(&state_file);
        println!("done");
        return;
    }
    s.shot("01-prompt");

    // 2. A few blocks: listing, git, a failure.
    s.run("ls -la", Duration::from_secs(5));
    s.run("git --no-pager log --oneline -6", Duration::from_secs(5));
    s.run("ls /does-not-exist", Duration::from_secs(5));
    s.run(
        "printf '\\033[32mok\\033[0m  colours stay in output, plain text is muted\\n'",
        Duration::from_secs(5),
    );
    s.shot("02-blocks");
    s.run("clear", Duration::from_secs(5));
    s.run("bench/sample.sh", Duration::from_secs(5));
    s.shot("02b-rendering-sample");
    if std::env::var_os("KILN_SNAPSHOT_QUICK").is_some() {
        return;
    }

    // 3. Hover actions on a block header (Copy, Rerun, Pin, Open log).
    s.mouse_move(700., 320.);
    s.shot("03-hover-actions");
    s.mouse_move(5., 700.);

    // 4. A running command: live output and a green dot.
    s.type_only("for i in 1 2 3 4 5 6; do echo \"tick $i\"; sleep 1; done\r");
    s.pump(Duration::from_millis(2600));
    s.shot("04-running");
    s.wait_for(Duration::from_secs(10), |p| p.is_at_prompt());
    s.pump(Duration::from_millis(400));

    // 5. Find.
    s.action(Box::new(kiln::Find));
    s.type_into_field("kiln");
    s.shot("05-find");
    s.keystroke("escape");

    // 6. Command palette.
    s.action(Box::new(kiln::CommandPaletteToggle));
    s.type_into_field("split");
    s.shot("06-palette");
    s.keystroke("escape");

    // 7. Copy last output as a fenced block (saved as text next to the screenshots).
    s.action(Box::new(kiln::CopyLastOutput));
    let copied =
        s.cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    if let Some(text) = copied {
        std::fs::write(s.out.join("07-copy-last-output.md"), &text).unwrap();
        println!("copied:\n{text}");
    }

    // 8. Split right.
    s.action(Box::new(kiln::SplitRight));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.run("echo 'second pane'", Duration::from_secs(5));
    s.shot("08-split");

    // 9. More sessions, grouped by project in the sidebar.
    s.action(Box::new(kiln::NewSession));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.run("cd ~ && echo home", Duration::from_secs(5));
    s.action(Box::new(kiln::NewSession));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.type_only("sleep 30\r");
    s.pump(Duration::from_millis(800));
    s.shot("09-sessions");
    s.type_only("\x03");
    s.pump(Duration::from_millis(500));

    // 10. A full-screen program takes the whole pane.
    s.action(Box::new(kiln::SelectSession1));
    s.type_only("nvim crates/kiln/src/lib.rs\r");
    s.pump(Duration::from_millis(3500));
    s.shot("10-fullscreen-nvim");
    s.type_only("\x1b:qa!\r");
    s.pump(Duration::from_millis(800));

    // 11. The Windows title strip (controls at top right, `>_` mark), previewed on this machine.
    s.action(Box::new(kiln::ToggleSidebar));
    // SAFETY: only this thread reads the variable (in the next render).
    unsafe { std::env::set_var("KILN_CHROME", "windows") };
    s.pump(Duration::from_millis(400));
    s.shot("11-windows-chrome-preview");
    s.action(Box::new(kiln::ToggleSidebar));
    s.shot("12-windows-chrome-with-sidebar");
    unsafe { std::env::remove_var("KILN_CHROME") };

    println!("done");
}

/// Phase 2: the context rail, focus mode, tints, attention, projects and restored sessions.
fn phase2(s: &mut Shots) {
    s.run("git --no-pager log --oneline", Duration::from_secs(5));

    // 13. The rail: the dev server's process and port, the changes, node, the session.
    // The demo's dev server; machines without node serve the folder with python instead.
    let has = |program: &str| {
        std::process::Command::new(program)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let server = if has("node") {
        "node src/server.js"
    } else if has("python3") {
        "python3 -m http.server 4173 --bind 127.0.0.1"
    } else {
        "sleep 30"
    };
    s.type_only(&format!("{server}\r"));
    s.pump(Duration::from_millis(2500));
    s.action(Box::new(kiln::ToggleRail));
    s.pump(Duration::from_millis(2500));
    s.shot("13-rail");
    s.type_only("\x03");
    s.wait_for(Duration::from_secs(5), |p| p.is_at_prompt());

    // 14. Focus mode: one pane, nothing else.
    s.action(Box::new(kiln::SplitRight));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.run("git --no-pager diff --stat", Duration::from_secs(5));
    s.action(Box::new(kiln::ToggleFocusMode));
    s.shot("14-focus-mode");
    s.action(Box::new(kiln::ToggleFocusMode));

    // 15–16. Tints. `ssh` is a stand-in function here: nothing connects anywhere.
    s.action(Box::new(kiln::NewSession));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.run(
        "ssh() { printf 'Last login: Mon Oct 19 09:12 from 10.0.4.2\\n%s:~$ \\n' \"${1#*@}\"; sleep 30; }",
        Duration::from_secs(5),
    );
    s.type_only("ssh deploy@db.prod.example.com\r");
    s.pump(Duration::from_millis(2500));
    s.shot("15-production-tint");
    s.type_only("\x03");
    s.wait_for(Duration::from_secs(5), |p| p.is_at_prompt());
    s.type_only("ssh build@build-box\r");
    s.pump(Duration::from_millis(1500));
    s.shot("16-remote-tint");
    s.type_only("\x03");
    s.wait_for(Duration::from_secs(5), |p| p.is_at_prompt());

    // 17. A long command finishes in another session: an orange dot until it is shown.
    s.type_only("sleep 3; echo built\r");
    s.pump(Duration::from_millis(300));
    s.action(Box::new(kiln::SelectSession1));
    s.pump(Duration::from_millis(4000));
    s.shot("17-attention");

    // 18. Projects in the palette (saved every two seconds).
    s.pump(Duration::from_millis(2200));
    s.action(Box::new(kiln::CommandPaletteToggle));
    s.type_into_field("acme");
    s.shot("18-palette-projects");
    s.keystroke("escape");

    // 19. A new window restores the saved sessions: folders, splits, the rail.
    s.pump(Duration::from_millis(2200));
    let restored =
        s.cx.open_window(size(px(1180.), px(720.)), |window, cx| {
            let metrics = CellMetrics::measure(px(13.), window);
            cx.new(|cx| Workspace::new(None, metrics, false, true, Instant::now(), window, cx))
        })
        .unwrap();
    s.window = restored;
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.pump(Duration::from_millis(2500));
    s.shot("19-restored");

    // 20. Windows chrome with the rail open: the window controls sit at the top of the rail.
    // SAFETY: only this thread reads the variable (in the next render).
    unsafe { std::env::set_var("KILN_CHROME", "windows") };
    s.pump(Duration::from_millis(400));
    s.shot("20-windows-chrome-with-rail");
    unsafe { std::env::remove_var("KILN_CHROME") };
}

/// The keyboard shortcuts pop-up: all of it, filtered by name and by keys, the Windows chrome
/// preview with the Windows and Linux bindings, and a narrow window (one column).
fn shortcuts(s: &mut Shots) {
    s.run("echo hello", Duration::from_secs(5));
    // Opened with its keys, through the keymap, like a person would.
    s.keystroke("cmd-/");
    s.shot("s1-all");
    s.type_into_field("split");
    s.shot("s2-filter-name");
    s.keystroke("backspace backspace backspace backspace backspace");
    s.type_into_field("cmd+d");
    s.shot("s3-filter-keys");
    // The same keys close it; Esc too.
    let open = |s: &mut Shots| {
        s.cx.update(|cx| s.window.read(cx).unwrap().shortcuts_open())
    };
    assert!(open(s), "⌘/ opened the shortcuts");
    s.keystroke("cmd-/");
    assert!(!open(s), "⌘/ closes the shortcuts again");
    s.keystroke("cmd-/");
    assert!(open(s));
    s.keystroke("escape");
    assert!(!open(s), "Esc closes the shortcuts");

    // The Windows preview: its chrome, and the bindings Windows and Linux get.
    // SAFETY: only this thread reads the variable (in the next render).
    unsafe { std::env::set_var("KILN_CHROME", "windows") };
    s.cx.update(|cx| {
        cx.clear_key_bindings();
        cx.bind_keys(keymap::bindings(keymap::KeyStyle::Pc));
    });
    s.keystroke("ctrl-shift-/");
    s.shot("s4-windows-preview");
    s.type_into_field("ctrl+shift+d");
    s.shot("s5-windows-filter-keys");
    s.keystroke("escape");
    unsafe { std::env::remove_var("KILN_CHROME") };
    s.cx.update(|cx| {
        cx.clear_key_bindings();
        cx.bind_keys(keymap::bindings(keymap::KeyStyle::current()));
    });

    // A narrow window: one column.
    let narrow =
        s.cx.open_window(size(px(640.), px(720.)), |window, cx| {
            let metrics = CellMetrics::measure(px(13.), window);
            cx.new(|cx| Workspace::new(None, metrics, false, true, Instant::now(), window, cx))
        })
        .unwrap();
    s.window = narrow;
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.action(Box::new(kiln::ToggleShortcuts));
    s.shot("s6-narrow");
    // The rest of the list scrolls into view.
    s.scroll(320., 500., -2000.);
    s.shot("s7-narrow-scrolled");
}

/// The input box: the prompt, after a failure, with a second line, with a completion menu, and
/// the sidebar's Home group.
fn prompt(s: &mut Shots) {
    s.run("echo hello", Duration::from_secs(5));
    s.shot("p1-prompt");
    s.run("ls /does-not-exist", Duration::from_secs(5));
    s.shot("p2-failed");
    // A command over two lines: the shell waits for the rest.
    s.type_only("for i in 1 2 3; do\r");
    s.pump(Duration::from_millis(600));
    s.type_only("echo $i");
    s.pump(Duration::from_millis(600));
    s.shot("p3-multiline");
    s.type_only("\x03");
    s.wait_for(Duration::from_secs(5), |p| p.is_at_prompt());
    s.pump(Duration::from_millis(400));
    // A completion menu.
    s.type_only("ls crates/kiln/s\t\t");
    s.pump(Duration::from_millis(1200));
    s.shot("p4-completion");
    s.type_only("\x03");
    s.wait_for(Duration::from_secs(5), |p| p.is_at_prompt());
    s.pump(Duration::from_millis(400));
    // The session in the home folder.
    s.action(Box::new(kiln::SelectSession2));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.pump(Duration::from_millis(800));
    s.shot("p5-home");
    // `[shell] prompt = "shell"`: the shell's own prompt, inside the same box.
    let mut config = config::current();
    config.shell.prompt = config::PromptStyle::Shell;
    config::set_current(config);
    s.action(Box::new(kiln::NewSession));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.pump(Duration::from_millis(800));
    s.shot("p6-shell-prompt");
}

/// Phase 3: an update on offer, in the sidebar and the palette. (The offer is made up here;
/// nothing is downloaded.)
fn phase3(s: &mut Shots) {
    let update = kiln_update::Update {
        config: Default::default(),
        body: Some("Kiln 2026.10.26".into()),
        current_version: "2026.10.19".into(),
        version: "2026.10.26".into(),
        date: None,
        target: "macos".into(),
        extract_path: PathBuf::from("/Applications/Kiln.app"),
        download_url: "https://example.com/Kiln.app.tar.gz".parse().unwrap(),
        signature: String::new(),
        timeout: None,
        headers: Default::default(),
        format: kiln_update::UpdateFormat::App,
    };
    s.cx.update(|cx| {
        kiln::update::set_state(kiln::update::UpdateState::Available(Arc::new(update)), cx)
    });
    s.run("echo 'a new version is on offer'", Duration::from_secs(5));
    s.shot("21-update-available");
    s.action(Box::new(kiln::CommandPaletteToggle));
    s.shot("22-palette-update");
    s.keystroke("escape");
}

/// Each state of `notes/concept/kiln-concept.html`, for `notes/concept-check.md`: on macOS, and
/// again in the Windows chrome preview with the Windows and Linux key bindings.
fn concept(s: &mut Shots) {
    let clean = clean_project();
    let rust = rust_project();
    let loose = std::env::temp_dir().join("kiln-demo").join("scratch");
    std::fs::create_dir_all(&loose).unwrap();
    s.run("git --no-pager log --oneline", Duration::from_secs(5));
    s.run("ls src", Duration::from_secs(5));
    s.run("git status --short", Duration::from_secs(5));
    // The rail's runtimes and git refresh in the background.
    s.pump(Duration::from_millis(2500));

    // 1. The header's pills: a repository with changes, a clean one, a plain folder, Home;
    // the runtime only where the project uses it.
    s.shot("01-header-repo");
    s.run(&format!("cd {}", clean.display()), Duration::from_secs(5));
    s.pump(Duration::from_millis(1500));
    s.shot("01b-header-clean");
    s.run(&format!("cd {}", loose.display()), Duration::from_secs(5));
    s.pump(Duration::from_millis(1500));
    s.shot("01c-header-plain-folder");
    s.run("cd ~", Duration::from_secs(5));
    s.pump(Duration::from_millis(1500));
    s.shot("01d-header-home");
    s.run(&format!("cd {}", rust.display()), Duration::from_secs(5));
    s.pump(Duration::from_millis(2500));
    s.shot("02-runtime-rust");
    s.run("cd ../acme-web", Duration::from_secs(5));
    s.pump(Duration::from_millis(1500));

    // 2. The flame: its four frames (from the focused pane, as they come), after a failure,
    // holding still without focus or with reduced motion, and in split panes.
    flame(s);

    // 3. Focus mode, with something running in another session.
    s.action(Box::new(kiln::NewSession));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.type_only("sleep 60\r");
    s.pump(Duration::from_millis(600));
    s.action(Box::new(kiln::SelectSession1));
    s.action(Box::new(kiln::ToggleFocusMode));
    s.shot("03-focus-mode");
    s.action(Box::new(kiln::ToggleFocusMode));

    // 4–6. Selecting commands, extending, copying several; the copy note.
    s.run("node --version || echo 'no node'", Duration::from_secs(5));
    s.keystroke("cmd-up cmd-up");
    s.shot("04-select-command");
    s.keystroke("cmd-shift-up");
    s.shot("05-extend-selection");
    s.keystroke("cmd-c");
    s.shot("06-copy-note");
    let copied =
        s.cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    if let Some(text) = copied {
        std::fs::write(s.out.join("05-multi-copy.md"), &text).unwrap();
    }
    s.pump(Duration::from_millis(2800));
    s.shot("06b-note-gone");

    // 7. A pinned command stays at the top through new commands and `clear`.
    let pane = s.pane();
    let status = s.cx.update(|cx| {
        let view = pane.read(cx);
        view.timeline()
            .iter()
            .rposition(|(_, command)| command == "git status --short")
            .unwrap()
    });
    s.cx.update(|cx| pane.update(cx, |p, cx| p.toggle_pin(status, cx)));
    s.run("clear", Duration::from_secs(5));
    s.run("ls", Duration::from_secs(5));
    s.run("git --no-pager diff --stat", Duration::from_secs(5));
    s.shot("07-pinned");
    s.mouse_move(700., 60.);
    s.shot("07b-pinned-hover");
    let id = s.cx.update(|cx| pane.read(cx).timeline()[0].0);
    s.cx.update(|cx| pane.update(cx, |p, cx| p.toggle_pin_open(id, cx)));
    s.shot("07c-pinned-open");
    s.cx.update(|cx| pane.update(cx, |p, cx| p.toggle_pin_open(id, cx)));
    s.mouse_move(5., 700.);

    // 8. Find.
    s.action(Box::new(kiln::Find));
    s.type_into_field("auth");
    s.keystroke("enter");
    s.shot("08-find");
    s.keystroke("escape");

    // 9. The palette: everything, then a query.
    s.pump(Duration::from_millis(2200));
    s.keystroke("cmd-k");
    s.shot("09-palette");
    s.type_into_field("git");
    s.shot("09b-palette-query");
    s.keystroke("escape");

    // 10. The sidebar's hint opens the palette.
    s.click(24., 707., Modifiers::default());
    assert!(
        s.workspace(|w| w.palette_open()),
        "a click on the sidebar hint opens the palette"
    );
    s.shot("10-sidebar-hint");
    s.keystroke("escape");

    // 11. The shortcuts pop-up lists the new bindings.
    s.keystroke("cmd-/");
    s.type_into_field("select");
    s.shot("11-shortcuts-select");
    s.keystroke("escape");

    // The Windows chrome preview, with the bindings Windows and Linux get.
    // SAFETY: only this thread reads the variable (in the next render).
    unsafe { std::env::set_var("KILN_CHROME", "windows") };
    s.cx.update(|cx| {
        cx.clear_key_bindings();
        cx.bind_keys(keymap::bindings(keymap::KeyStyle::Pc));
    });
    s.pump(Duration::from_millis(400));
    s.shot("w01-header-pills");
    s.keystroke("ctrl-shift-up");
    s.keystroke("ctrl-alt-shift-up");
    s.shot("w04-select");
    s.keystroke("escape");
    s.action(Box::new(kiln::ToggleFocusMode));
    s.shot("w03-focus-mode");
    s.action(Box::new(kiln::ToggleFocusMode));
    s.keystroke("ctrl-shift-f");
    s.type_into_field("auth");
    s.shot("w08-find");
    s.keystroke("escape");
    s.keystroke("ctrl-shift-k");
    s.shot("w09-palette");
    s.keystroke("escape");
    s.keystroke("ctrl-shift-/");
    s.type_into_field("select");
    s.shot("w11-shortcuts-select");
    s.keystroke("escape");
    unsafe { std::env::remove_var("KILN_CHROME") };
    s.cx.update(|cx| {
        cx.clear_key_bindings();
        cx.bind_keys(keymap::bindings(keymap::KeyStyle::current()));
    });
    s.type_only("\x03");
}

/// The Settings pop-up: as it opens, driven by the keyboard, the icon picked, a file it must
/// not write over, and the Windows preview. Saves kiln.toml before and after, to show that only
/// the changed lines change.
fn settings(s: &mut Shots) {
    let path = kiln::config::config_path().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let user = "# My Kiln settings\n\
        font_size = 13    # comfortable on this screen\n\
        \n\
        [shell]\n\
        program = \"/bin/zsh\" # not in Settings: stays as it is\n\
        \n\
        [window]\n\
        sidebar = true\n";
    std::fs::write(&path, user).unwrap();
    std::fs::write(s.out.join("s00-kiln-before.toml"), user).unwrap();
    s.run("git --no-pager log --oneline -3", Duration::from_secs(5));
    let open = |s: &mut Shots| s.workspace(|w| w.settings_open());

    s.keystroke("cmd-,");
    assert!(open(s), "⌘, opens Settings");
    s.shot("s01-settings");

    // The keyboard: Tab lights the first control, the arrows change it.
    s.keystroke("tab");
    s.shot("s02-keyboard");
    s.keystroke("right");
    s.shot("s03-icon-flame");
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("icon = \"flame\""), "{written}");
    // Font size up, the cursor's blink off, the prompt switched, notifications later.
    s.keystroke("tab right");
    s.keystroke("tab space");
    s.keystroke("tab tab right");
    s.keystroke("tab tab right right");
    s.shot("s04-changed");
    std::fs::copy(&path, s.out.join("s04-kiln-after.toml")).unwrap();
    s.keystroke("escape");
    assert!(!open(s), "Esc closes Settings");

    // A file Kiln cannot read: shown, and left alone.
    let broken = "font_size = = 13\n";
    std::fs::write(&path, broken).unwrap();
    s.keystroke("cmd-,");
    s.shot("s05-broken-file");
    // Only "Open config file" takes the keyboard; the arrows change nothing.
    s.keystroke("tab right left");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        broken,
        "a broken file is not written"
    );
    s.keystroke("escape");

    // Themes: each card applies at once, the pop-up included.
    std::fs::write(&path, user).unwrap();
    s.keystroke("cmd-,");
    let view = s.workspace(|w| w.settings_view()).unwrap();
    for (i, (name, _)) in kiln::settings_theme::CARDS.iter().enumerate() {
        if *name == kiln::theme::CUSTOM {
            continue;
        }
        s.cx.update(|cx| view.update(cx, |v, cx| v.choose_theme(name, cx)));
        s.pump(Duration::from_millis(600));
        s.shot(&format!("t{i}-{name}"));
    }
    // Custom from Everforest: the file gets name, base and every colour.
    s.cx.update(|cx| view.update(cx, |v, cx| v.choose_theme("everforest", cx)));
    s.cx.update(|cx| view.update(cx, |v, cx| v.choose_theme(kiln::theme::CUSTOM, cx)));
    s.pump(Duration::from_millis(600));
    s.shot("c1-custom");
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("name = \"custom\"") && written.contains("base = \"everforest\""));
    assert!(
        written.contains("# My Kiln settings"),
        "the user's comment survives"
    );
    std::fs::copy(&path, s.out.join("c1-kiln-custom.toml")).unwrap();
    // The picker, on the accent.
    s.cx.update(|cx| {
        view.update(cx, |v, cx| {
            v.open_picker(kiln::settings::Target::Colour(9), cx)
        })
    });
    s.pump(Duration::from_millis(400));
    // Down to the picker and the terminal colours.
    s.scroll(590., 500., -420.);
    s.shot("c2-custom-picker");
    s.scroll(590., 500., 2000.);
    // A pink accent, then Nord: exactly Nord, nothing left over.
    s.cx.update(|cx| {
        view.update(cx, |v, cx| {
            v.set(kiln::settings::Change::Colour(9, 0xff3d8b), cx)
        })
    });
    s.cx.update(|cx| view.update(cx, |v, cx| v.choose_theme("nord", cx)));
    s.pump(Duration::from_millis(600));
    s.shot("c3-preset-after-custom");
    let nord = kiln::theme::preset_by_id("nord").unwrap().palette;
    let on_file = || kiln::config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        on_file().palette_for(true),
        nord,
        "a preset is exactly itself"
    );
    assert_eq!(kiln::theme::palette(), nord, "and on screen");
    std::fs::copy(&path, s.out.join("c3-kiln-nord.toml")).unwrap();
    // Custom again: the pink accent comes back.
    s.cx.update(|cx| view.update(cx, |v, cx| v.choose_theme(kiln::theme::CUSTOM, cx)));
    s.pump(Duration::from_millis(600));
    s.shot("c4-custom-again");
    assert_eq!(on_file().palette_for(true).accent, 0xff3d8b);
    assert_eq!(kiln::theme::palette().accent, 0xff3d8b);
    s.cx.update(|cx| view.update(cx, |v, cx| v.choose_theme("kiln-dark", cx)));
    s.keystroke("escape");

    // The app icon row: four designs and Custom; each design keeps its own colour.
    std::fs::write(&path, user).unwrap();
    s.keystroke("cmd-,");
    assert!(open(s));
    let view = s.workspace(|w| w.settings_view()).unwrap();
    use kiln::config::{AppIcon, IconColour, IconDesign};
    let on_file = |path: &std::path::Path| {
        kiln::config::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    s.shot("i1-icon-row");
    // Clicking the chosen design shows its colours.
    s.cx.update(|cx| view.update(cx, |v, cx| v.click_icon(IconDesign::Kiln, cx)));
    s.pump(Duration::from_millis(300));
    s.shot("i2-colour-popup");
    for (i, (design, colour)) in [
        (IconDesign::Kiln, IconColour::Sage),
        (IconDesign::Beehive, IconColour::Slate),
        (IconDesign::Line, IconColour::Ember),
    ]
    .into_iter()
    .enumerate()
    {
        if on_file(&path).icon.design != design {
            s.cx.update(|cx| view.update(cx, |v, cx| v.click_icon(design, cx)));
        }
        s.cx.update(|cx| {
            view.update(cx, |v, cx| {
                v.set(kiln::settings::Change::IconColour(design, colour), cx)
            })
        });
        s.pump(Duration::from_millis(400));
        s.shot(&format!("i{}-{}-{}", i + 3, design.key(), colour.key()));
        assert_eq!(on_file(&path).app_icon(), AppIcon::Drawn(design, colour));
    }
    // Back to Kiln: its own colour again. Esc closes the pop-up, not Settings.
    s.cx.update(|cx| view.update(cx, |v, cx| v.click_icon(IconDesign::Kiln, cx)));
    assert_eq!(
        on_file(&path).app_icon(),
        AppIcon::Drawn(IconDesign::Kiln, IconColour::Sage)
    );
    s.keystroke("escape");
    assert!(open(s), "Esc closes the colour pop-up first");
    std::fs::copy(&path, s.out.join("i5-kiln.toml")).unwrap();
    // Custom: a wide picture, cropped from the middle and copied into Kiln's folder.
    let pictures =
        std::env::temp_dir().join(format!("kiln-snapshot-{}-pictures", std::process::id()));
    std::fs::create_dir_all(&pictures).unwrap();
    let picture = pictures.join("sunset.png");
    image::RgbaImage::from_fn(900, 600, |x, y| {
        let (dx, dy) = (x as f32 - 450., y as f32 - 330.);
        if dx * dx + dy * dy < 150. * 150. {
            image::Rgba([255, 214, 120, 255])
        } else if y > 420 {
            image::Rgba([40, 60, 110, 255])
        } else {
            let t = y as f32 / 420.;
            image::Rgba([
                (240. - 80. * t) as u8,
                (110. - 40. * t) as u8,
                (90. + 60. * t) as u8,
                255,
            ])
        }
    })
    .save(&picture)
    .unwrap();
    s.cx.update(|cx| view.update(cx, |v, cx| v.use_image(picture.clone(), cx)));
    s.pump(Duration::from_millis(1500));
    s.shot("i6-custom");
    let config = on_file(&path);
    let Some(copy) = config.icon_path.clone() else {
        panic!("no icon_path: {config:?}")
    };
    assert_eq!(config.app_icon(), AppIcon::Custom(copy.clone()));
    assert!(
        copy.starts_with(path.parent().unwrap()) && copy.exists(),
        "{copy:?}"
    );
    std::fs::copy(&path, s.out.join("i6-kiln.toml")).unwrap();
    // Not an image: refused, and nothing changes.
    let fake = pictures.join("notes.png");
    std::fs::write(&fake, "#!/bin/sh\necho hi\n").unwrap();
    s.cx.update(|cx| view.update(cx, |v, cx| v.use_image(fake.clone(), cx)));
    s.pump(Duration::from_millis(800));
    s.shot("i7-custom-refused");
    assert_eq!(on_file(&path).icon_path, Some(copy.clone()));
    // Remove: Kiln, in its colour; Kiln's copy goes.
    s.cx.update(|cx| view.update(cx, |v, cx| v.click_icon(IconDesign::Kiln, cx)));
    s.cx.update(|cx| {
        view.update(cx, |v, cx| {
            v.set(kiln::settings::Change::Icon(IconDesign::Custom), cx)
        })
    });
    s.cx.update(|cx| view.update(cx, |v, cx| v.remove_image(cx)));
    s.pump(Duration::from_millis(300));
    assert_eq!(
        on_file(&path).app_icon(),
        AppIcon::Drawn(IconDesign::Kiln, IconColour::Sage)
    );
    assert!(!copy.exists());
    let _ = std::fs::remove_dir_all(&pictures);
    s.keystroke("escape");

    // The Windows preview: no Option row, the Windows keys.
    std::fs::write(&path, user).unwrap();
    // SAFETY: only this thread reads the variable (in the next render).
    unsafe { std::env::set_var("KILN_CHROME", "windows") };
    s.cx.update(|cx| {
        cx.clear_key_bindings();
        cx.bind_keys(keymap::bindings(keymap::KeyStyle::Pc));
    });
    s.keystroke("ctrl-shift-,");
    s.shot("w-settings");
    s.keystroke("escape");
    unsafe { std::env::remove_var("KILN_CHROME") };
    s.cx.update(|cx| {
        cx.clear_key_bindings();
        cx.bind_keys(keymap::bindings(keymap::KeyStyle::current()));
    });
}

/// The prompt setting switched from Settings: the open session takes it at its next prompt,
/// a new one starts with it, and back again. Uses the user's own shell setup (Starship, if
/// they have it), as a person would see it.
fn prompt_switch(s: &mut Shots) {
    use kiln::config::PromptStyle;
    use kiln::settings::Change;
    // Every few milliseconds while prompts redraw: the input box never shows the lines above
    // the one typed on (the Starship pills: folder, branch).
    let watch = |s: &mut Shots, what: &str| {
        let end = Instant::now() + Duration::from_millis(2500);
        let mut samples = 0;
        while Instant::now() < end {
            s.pump(Duration::from_millis(5));
            let text = s.cx.update(|cx| {
                s.window
                    .read(cx)
                    .ok()
                    .and_then(|w| w.focused_view())
                    .map(|pane| pane.read(cx).input_box_text())
            });
            let text = text.unwrap_or_default();
            assert!(
                !text.contains("main") && !text.contains('\u{e0b6}'),
                "{what}: the pills showed: {text:?}"
            );
            samples += 1;
        }
        println!("{what}: {samples} samples, no pills");
    };
    let set = |s: &mut Shots, style: PromptStyle| {
        s.keystroke("cmd-,");
        let view = s.workspace(|w| w.settings_view()).unwrap();
        s.cx.update(|cx| view.update(cx, |v, cx| v.set(Change::Prompt(style), cx)));
        s.keystroke("escape");
        watch(s, &format!("switching to {style:?}"));
    };
    s.run("echo first session", Duration::from_secs(5));
    s.shot("ps1-kiln");
    // Switched while the prompt waits, nothing typed: it is redrawn at once.
    set(s, PromptStyle::Shell);
    assert!(
        s.wait_for(Duration::from_secs(10), |p| !p.shows_kiln_prompt()),
        "the open session switches"
    );
    s.pump(Duration::from_millis(2500));
    s.shot("ps2-open-session-shell");
    s.action(Box::new(kiln::NewSession));
    watch(s, "a new session starting");
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.pump(Duration::from_millis(2500));
    assert!(
        s.wait_for(Duration::from_secs(5), |p| !p.shows_kiln_prompt()),
        "a new session"
    );
    s.shot("ps3-new-session-shell");
    set(s, PromptStyle::Kiln);
    assert!(s.wait_for(Duration::from_secs(10), |p| p.shows_kiln_prompt()));
    s.pump(Duration::from_millis(2500));
    s.shot("ps4-new-session-kiln");
    s.action(Box::new(kiln::SelectSession1));
    assert!(
        s.wait_for(Duration::from_secs(10), |p| p.shows_kiln_prompt()),
        "the other open session"
    );
    s.pump(Duration::from_millis(2500));
    s.shot("ps5-open-session-kiln");
    // Something typed: left alone, and the next prompt follows the setting.
    s.keystroke("e c h o space k e p t");
    s.pump(Duration::from_millis(500));
    set(s, PromptStyle::Shell);
    s.pump(Duration::from_millis(2500));
    assert!(
        s.wait_for(Duration::from_secs(1), |p| p.shows_kiln_prompt()),
        "typing is not interrupted"
    );
    s.shot("ps6-typed-waits");
    s.keystroke("enter");
    watch(s, "the command's next prompt");
    assert!(s.wait_for(Duration::from_secs(10), |p| !p.shows_kiln_prompt()));
    s.pump(Duration::from_millis(2500));
    s.shot("ps7-after-enter-shell");
    set(s, PromptStyle::Kiln);
}

/// What one flame frame costs: the workspace rendered, laid out and painted again (the panes
/// come from the cache), as each 150 ms tick asks for; then the same with the scene also
/// rendered on the GPU and read back (more than presenting costs). Prints the time per frame and what that is at the flame's rate.
/// Build with --release.
fn bench_flame(s: &mut Shots) {
    s.run("git --no-pager log --oneline", Duration::from_secs(5));
    s.run("ls -la", Duration::from_secs(5));
    s.pump(Duration::from_millis(2500));
    let window: AnyWindowHandle = s.window.into();
    let per_second = 1000. / kiln::flame::FRAME_EVERY.as_millis() as f64;
    let workspace = s.window;
    for gpu in [false, true] {
        let frame = |s: &mut Shots| {
            // What a tick does: the workspace redraws.
            let _ =
                s.cx.update(|cx| workspace.update(cx, |_, _, cx| cx.notify()));
            let start = Instant::now();
            if gpu {
                let _ = s.cx.capture_screenshot(window);
            } else {
                let _ =
                    s.cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx));
            }
            start.elapsed()
        };
        for _ in 0..30 {
            frame(s);
        }
        let mut times: Vec<Duration> = (0..300).map(|_| frame(s)).collect();
        times.sort();
        let mean = times.iter().sum::<Duration>() / times.len() as u32;
        println!(
            "{}: median {:.2} ms, mean {:.2} ms, p95 {:.2} ms; at {:.1} frames/s: {:.2}% of one core",
            if gpu {
                "frame drawn, rendered and read back"
            } else {
                "frame drawn (render, layout, paint)"
            },
            times[times.len() / 2].as_secs_f64() * 1000.,
            mean.as_secs_f64() * 1000.,
            times[times.len() * 95 / 100].as_secs_f64() * 1000.,
            per_second,
            mean.as_secs_f64() * per_second * 100.,
        );
    }
}

/// The flame's states, and a contact sheet of its frames as Kiln draws them.
fn flame(s: &mut Shots) {
    let pane = s.pane();
    let frame = |s: &mut Shots, pane: &Entity<TerminalView>| {
        let workspace = s.window;
        s.cx.update(|cx| workspace.read(cx).unwrap().flame_frame_of(pane))
    };
    s.shot("12-flame-ready");
    // The frames, one by one: each crop is the flame and a little around it.
    let mut crops = Vec::new();
    for want in 0..kiln::flame::FRAMES {
        let end = Instant::now() + Duration::from_secs(5);
        while frame(s, &pane) != want && Instant::now() < end {
            s.cx.advance_clock(Duration::from_millis(5));
            s.cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            frame(s, &pane),
            want,
            "the flame moves on to frame {}",
            want + 1
        );
        s.redraw();
        let (bounds, _) = s.cx.update(|cx| pane.read(cx).flame()).unwrap();
        let window: AnyWindowHandle = s.window.into();
        let image = s.cx.capture_screenshot(window).unwrap();
        let scale = image.width() as f32 / 1180.;
        // A little room around it, short of the cursor two cells on.
        let (pad_x, pad_y) = (2., 4.);
        let x = ((f32::from(bounds.origin.x) - pad_x) * scale) as u32;
        let y = ((f32::from(bounds.origin.y) - pad_y) * scale) as u32;
        let w = ((f32::from(bounds.size.width) + 2. * pad_x) * scale) as u32;
        let h = ((f32::from(bounds.size.height) + 2. * pad_y) * scale) as u32;
        crops.push(image::imageops::crop_imm(&image, x, y, w, h).to_image());
    }
    // Side by side, four times bigger, with a gap.
    let zoom = 4;
    let (cw, ch) = (crops[0].width() * zoom, crops[0].height() * zoom);
    let gap = 24;
    let mut sheet = image::RgbaImage::from_pixel(
        cw * crops.len() as u32 + gap * (crops.len() as u32 + 1),
        ch + 2 * gap,
        image::Rgba([0x32, 0x30, 0x2f, 0xff]),
    );
    for (i, crop) in crops.iter().enumerate() {
        let big = image::imageops::resize(crop, cw, ch, image::imageops::FilterType::Nearest);
        image::imageops::overlay(
            &mut sheet,
            &big,
            (gap + i as u32 * (cw + gap)) as i64,
            gap as i64,
        );
    }
    sheet.save(s.out.join("12a-flame-frames.png")).unwrap();

    // A failed command: muted red.
    s.run("false", Duration::from_secs(5));
    s.shot("12b-flame-failed");
    s.run("true", Duration::from_secs(5));

    // Without the keyboard (the palette has it), the flame holds frame 1.
    s.keystroke("cmd-k");
    s.pump(Duration::from_millis(800));
    assert_eq!(frame(s, &pane), 0, "the flame holds still without focus");
    s.keystroke("escape");

    // Reduced motion: frame 1.
    kiln::flame::force_reduced_motion(true);
    s.pump(Duration::from_millis(800));
    assert_eq!(
        frame(s, &pane),
        0,
        "the flame holds still with reduced motion"
    );
    s.shot("12c-flame-reduced-motion");
    kiln::flame::force_reduced_motion(false);

    // Split panes: each has its own header and flame; only the focused one moves.
    s.action(Box::new(kiln::SplitRight));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.pump(Duration::from_millis(1500));
    let right = s.pane();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..12 {
        s.pump(Duration::from_millis(100));
        seen.insert(frame(s, &right));
        assert_eq!(frame(s, &pane), 0, "the other pane's flame holds still");
    }
    assert!(seen.len() > 1, "the focused pane's flame moves: {seen:?}");
    s.shot("12d-flame-split");
    s.action(Box::new(kiln::ClosePane));
    s.pump(Duration::from_millis(500));
}

/// A Rust project (in git, nothing changed).
fn rust_project() -> PathBuf {
    let root = std::env::temp_dir().join("kiln-demo").join("rust-cli");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"rust-cli\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=Demo", "-c", "user.email=demo@example.com"])
            .args(args)
            .current_dir(&root)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "Start"]);
    root
}

/// A git project with nothing changed.
fn clean_project() -> PathBuf {
    let root = std::env::temp_dir().join("kiln-demo").join("notes-site");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("index.md"), "# Notes\n").unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=Demo", "-c", "user.email=demo@example.com"])
            .args(args)
            .current_dir(&root)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "Start the notes"]);
    root
}

/// A small git project with a few changes, in a scratch folder.
/// Renaming sessions and projects, and project colours, by keyboard and by mouse.
fn sidebar(s: &mut Shots, state_file: &std::path::Path) {
    use gpui::MouseButton;
    let path = kiln::config::config_path().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let demo = demo_project();
    // A colour as configs wrote it before the named colours.
    let demo_key = demo.to_string_lossy().to_string();
    // Kiln knows the project by its folder with links resolved (/private/var on macOS).
    let demo_root = std::fs::canonicalize(&demo)
        .unwrap()
        .to_string_lossy()
        .to_string();
    std::fs::write(
        &path,
        format!("cursor_blink = false\n\n[projects.\"{demo_key}\"]\ncolor = \"#83a598\"\n"),
    )
    .unwrap();
    let on_file = |path: &std::path::Path| {
        kiln::config::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    s.pump(Duration::from_millis(1500));
    s.run("echo first session", Duration::from_secs(5));
    s.action(Box::new(kiln::NewSession));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.run(&format!("cd {}", demo.display()), Duration::from_secs(5));
    s.run("git --no-pager log --oneline -2", Duration::from_secs(5));
    s.pump(Duration::from_millis(1500));
    // No name at the top left of the panes; "⌘/ Shortcuts" at the bottom of the sidebar; the
    // demo project's dot and pill in its old "#rrggbb" colour.
    s.shot("sb1-sidebar");

    // ⌘R: the active session's name, edited in place.
    s.keystroke(if cfg!(target_os = "macos") {
        "cmd-r"
    } else {
        "ctrl-shift-r"
    });
    assert_eq!(s.workspace(|w| w.editing()), (true, false));
    s.shot("sb2-rename-session");
    s.keystroke("ctrl-u d e p l o y s enter");
    assert_eq!(s.titles()[1], "deploys");
    // It stays when the folder changes, and is saved with the sessions.
    s.run("cd ..", Duration::from_secs(5));
    s.pump(Duration::from_millis(2600));
    assert_eq!(s.titles()[1], "deploys");
    let saved: kiln::state::State =
        serde_json::from_str(&std::fs::read_to_string(state_file).unwrap()).unwrap();
    let tabs = saved.window.unwrap().tabs;
    assert_eq!(tabs[1].name.as_deref(), Some("deploys"), "{tabs:?}");
    s.run(&format!("cd {}", demo.display()), Duration::from_secs(5));
    s.shot("sb3-session-renamed");
    // Esc keeps the old name; an empty name gives the automatic one back.
    s.keystroke("f2 x escape");
    assert_eq!(s.titles()[1], "deploys");
    s.keystroke("f2 ctrl-u enter");
    assert_ne!(s.titles()[1], "deploys");

    // ⌘⇧R: the project's name, in the config.
    s.keystroke(if cfg!(target_os = "macos") {
        "cmd-shift-r"
    } else {
        "ctrl-alt-shift-r"
    });
    s.keystroke("ctrl-u A c m e space W e b enter");
    let name = kiln::state::custom_project_name(&demo_root, &on_file(&path));
    assert_eq!(name.as_deref(), Some("Acme Web"));
    s.pump(Duration::from_millis(500));
    s.shot("sb4-project-renamed");

    // The colour pop-up from the palette, all by keyboard. It opens on the custom field (the
    // project's old "#rrggbb"); ↑ goes to the second row of colours, → → to Blue; Enter picks.
    s.keystroke(if cfg!(target_os = "macos") {
        "cmd-k"
    } else {
        "ctrl-shift-k"
    });
    s.keystroke("P r o j e c t space c o l o u r");
    s.pump(Duration::from_millis(300));
    s.keystroke("enter");
    s.pump(Duration::from_millis(300));
    assert_eq!(
        s.workspace(|w| w.editing()),
        (false, true),
        "the pop-up opens"
    );
    s.keystroke("up right right");
    s.shot("sb5-colour-popup");
    s.keystroke("enter");
    assert_eq!(s.workspace(|w| w.editing()), (false, false));
    let config = on_file(&path);
    let project = kiln::state::project_config(&demo_root, &config).unwrap().1;
    assert_eq!(project.color.as_deref(), Some("blue"));
    let blue = 0x5f78d9;
    assert_eq!(kiln::state::project_color(&demo_root, &config), blue);
    s.pump(Duration::from_millis(500));
    s.shot("sb6-project-blue");

    // The mouse: right-click a project for Rename and Colour…, double-click a session.
    let header_y = s.workspace(|w| w.sidebar_rows())[1].0;
    s.click_button(60., header_y, MouseButton::Right, 1);
    s.shot("sb7-context-menu");
    // The menu takes the keys: ↓ Enter is Colour…, and its pop-up then has them.
    s.keystroke("down enter");
    assert_eq!(
        s.workspace(|w| w.editing()),
        (false, true),
        "Colour… opens the pop-up"
    );
    s.keystroke("escape");
    assert_eq!(
        s.workspace(|w| w.editing()),
        (false, false),
        "Esc closes it"
    );
    let session_y = s.workspace(|w| w.sidebar_rows())[1].1;
    s.click_button(80., session_y, MouseButton::Left, 2);
    assert_eq!(
        s.workspace(|w| w.editing()),
        (true, false),
        "double-click renames"
    );
    s.shot("sb8-double-click");
    s.keystroke("escape");

    // The shortcuts hint opens the pop-up, which lists the new keys.
    let hint_y = s.workspace(|w| w.sidebar_rows())[0].2;
    s.click_button(40., hint_y, MouseButton::Left, 1);
    s.shot("sb9-shortcuts");
    s.keystroke("escape");
    std::fs::copy(&path, s.out.join("sb-kiln.toml")).unwrap();

    // Project colours as tinted cards, in Kiln Dark and Kiln Light: kiln in orange, the demo
    // project in a custom "#rrggbb" typed in the pop-up, and a third, Home, left on Auto.
    s.action(Box::new(kiln::NewSession));
    s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    s.run("cd ~", Duration::from_secs(5));
    s.action(Box::new(kiln::SelectSession1));
    s.action(Box::new(kiln::ProjectColour));
    s.keystroke("down right right");
    assert_eq!(s.workspace(|w| w.editing()), (false, true));
    s.keystroke("enter");
    s.action(Box::new(kiln::SelectSession2));
    s.action(Box::new(kiln::ProjectColour));
    // ↓ from Blue's row is the custom field; typing goes there, with a preview. Magenta's own
    // hex points at the Magenta swatch (thin ring, its name below) and is saved by name.
    s.keystroke("down ctrl-u");
    s.keystroke("# c 4 5 a a 8");
    s.pump(Duration::from_millis(300));
    s.shot("sc1-dark-typed-hex");
    s.keystroke("enter");
    let colour = || {
        let config = on_file(&path);
        kiln::state::project_config(&demo_root, &config)
            .unwrap()
            .1
            .color
            .clone()
    };
    assert_eq!(colour().as_deref(), Some("magenta"));
    // Opened again: the accent ring on the current colour and its name below; Custom empty.
    s.action(Box::new(kiln::ProjectColour));
    s.pump(Duration::from_millis(300));
    s.shot("sc3-dark-popup");
    // A colour outside the sixteen stays a "#rrggbb".
    s.keystroke("down ctrl-u");
    s.keystroke("# 2 f 8 f 6 e enter");
    assert_eq!(colour().as_deref(), Some("#2f8f6e"));
    s.pump(Duration::from_millis(800));
    s.shot("sc2-dark-cards");
    // Kiln Light.
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{text}\n[theme]\nname = \"kiln-light\"\n")).unwrap();
    s.pump(Duration::from_millis(2000));
    s.shot("sc4-light-cards");
    s.action(Box::new(kiln::ProjectColour));
    s.keystroke("up right right right");
    s.pump(Duration::from_millis(300));
    s.shot("sc5-light-popup");
    s.keystroke("escape");
    std::fs::copy(&path, s.out.join("sc-kiln.toml")).unwrap();

    // The context rail: its first section in the header row.
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("name = \"kiln-light\"", "name = \"kiln-dark\""),
    )
    .unwrap();
    s.pump(Duration::from_millis(2000));
    s.action(Box::new(kiln::ToggleRail));
    s.pump(Duration::from_millis(2500));
    s.shot("rb2-rail-after");
}

/// The "Quiet" clean-up: the whole window with the rail, idle and then busy (a running dev
/// server and a failed command), in Kiln Dark and Kiln Light. `KILN_QUIET_STAGE` ("before" or
/// "after") prefixes the pictures.
fn quiet(s: &mut Shots) {
    let stage = std::env::var("KILN_QUIET_STAGE").unwrap_or_else(|_| "after".into());
    let path = kiln::config::config_path().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let theme = |s: &mut Shots, name: &str| {
        std::fs::write(
            &path,
            format!("cursor_blink = false\n\n[theme]\nname = \"{name}\"\n"),
        )
        .unwrap();
        s.pump(Duration::from_millis(2000));
    };
    theme(s, "kiln-dark");
    let demo = demo_project();
    let home = std::env::var("HOME").unwrap_or_default();
    s.run(&format!("cd {}", demo.display()), Duration::from_secs(5));
    s.run("git --no-pager log --oneline -2", Duration::from_secs(5));
    s.run("ls src", Duration::from_secs(5));
    // A path under the home folder, typed in full; and a command that takes over a second.
    s.run(&format!("ls -d {home}/.config"), Duration::from_secs(5));
    s.run("sleep 1.2", Duration::from_secs(5));
    s.action(Box::new(kiln::ToggleRail));
    s.pump(Duration::from_millis(2500));
    s.shot(&format!("{stage}-idle-dark"));
    theme(s, "kiln-light");
    s.pump(Duration::from_millis(500));
    s.shot(&format!("{stage}-idle-light"));

    // Busy: a failed command, then the dev server running.
    theme(s, "kiln-dark");
    s.run("ls missing-folder", Duration::from_secs(5));
    let has_node = std::process::Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|st| st.success());
    let server = if has_node {
        "node src/server.js"
    } else {
        "python3 -m http.server 4173 --bind 127.0.0.1"
    };
    s.type_only(&format!("{server}\r"));
    s.pump(Duration::from_millis(3000));
    s.shot(&format!("{stage}-busy-dark"));
    theme(s, "kiln-light");
    s.pump(Duration::from_millis(1000));
    s.shot(&format!("{stage}-busy-light"));
    s.type_only("\x03");
    s.wait_for(Duration::from_secs(5), |p| p.is_at_prompt());
}

fn demo_project() -> PathBuf {
    let root = std::env::temp_dir().join("kiln-demo").join("acme-web");
    let _ = std::fs::remove_dir_all(&root);
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "package.json",
        "{\n  \"name\": \"acme-web\",\n  \"scripts\": { \"dev\": \"node src/server.js\" }\n}\n",
    );
    write("README.md", "# acme-web\n");
    write(
        "src/server.js",
        "const http = require('http');\nconst routes = require('./routes');\n\
         http.createServer(routes).listen(4173, '127.0.0.1', () =>\n  \
         console.log('listening on http://localhost:4173'));\n",
    );
    write(
        "src/routes.js",
        "module.exports = (req, res) => res.end('ok');\n",
    );
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=Demo", "-c", "user.email=demo@example.com"])
            .args(args)
            .current_dir(&root)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "Serve the home page"]);
    write(
        "src/routes.js",
        "module.exports = (req, res) => {\n  if (req.url === '/health') return res.end('up');\n  res.end('ok');\n};\n",
    );
    write("src/auth.js", "module.exports = () => true;\n");
    write("docs/deploy.md", "# Deploy\n");
    git(&["add", "docs/deploy.md"]);
    std::fs::remove_file(root.join("README.md")).unwrap();
    root
}
