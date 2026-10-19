//! Renders Kiln headless and saves screenshots of each feature.
//!
//! Uses GPUI's headless app context with the real text system and the real GPU renderer, and
//! real shells on real PTYs, so the pictures show Kiln as it behaves; no display is needed.
//!
//! Usage: cargo run -p kiln --example snapshot -- [out dir] [phase1|phase2]
//! (default: notes/screenshots/phase1, scenario phase1)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, Modifiers, MouseMoveEvent,
    PlatformInput, Point, WindowHandle, px, size,
};
use kiln::config::{self, Config};
use kiln::grid_paint::{self, CellMetrics};
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
    let project = std::env::current_dir().unwrap();

    let text_system = gpui_platform::current_platform(true).text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(()),
        gpui_platform::current_headless_renderer,
    );
    cx.allow_parking();
    cx.update(|cx| grid_paint::load_embedded_fonts(cx));
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
            cx.new(|cx| Workspace::new(None, metrics, false, false, Instant::now(), window, cx))
        })
        .unwrap();
    let mut s = Shots { cx, window, out };

    // 1. The prompt in the input bar, the sidebar with one session.
    let ready = s.wait_for(Duration::from_secs(15), |p| p.is_at_prompt());
    println!("shell ready: {ready}");
    if scenario == "phase2" {
        phase2(&mut s);
        let _ = std::fs::remove_file(&state_file);
        println!("done");
        return;
    }
    s.run(
        &format!("cd {}", shell_quote(&project)),
        Duration::from_secs(5),
    );
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
    let demo = demo_project();
    s.run(
        &format!("cd {}", shell_quote(&demo)),
        Duration::from_secs(5),
    );
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

/// A small git project with a few changes, in a scratch folder.
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

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
