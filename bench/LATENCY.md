# Keystroke-to-screen latency

This one needs you: measuring it end to end means injecting keystrokes and reading the screen,
which needs Accessibility and Screen Recording permission for the tool doing it.

## 1. Kiln's own share (no permissions needed)

Start Kiln with tracing on, run `cat`, and type a few dozen characters:

```bash
KILN_TRACE_LATENCY=1 RUST_LOG=kiln=info target/release/Kiln.app/Contents/MacOS/kiln -- /bin/cat
```

Each keystroke logs `latency: keystroke to frame painted N ms`: from Kiln receiving the key to
the end of painting the frame that shows the echo. It leaves out the OS input path, the wait for
the next display refresh and the display itself (together roughly 10–25 ms on a 60 Hz panel).

## 2. Kiln against WezTerm (end to end)

Use [Typometer](https://github.com/pavelfatin/typometer) (needs Java), the usual tool for this:

1. In both terminals, set the same window size, run `cat`, and turn the cursor blink off for the
   test (Kiln: blink stops while typing; WezTerm: `cursor_blink_rate = 0` in a test config).
2. Start Typometer, give it Accessibility and Screen Recording permission when macOS asks.
3. Benchmark each terminal with the same settings (200 samples, 0 ms pause is fine) and note the
   minimum, average and standard deviation.

No-install alternative: film the keyboard and screen with a phone in slow motion (240 fps), press
a key 20 times per app, and count frames from key-down to the character appearing (one frame is
about 4 ms).

Put the numbers in `notes/phase0.md` under "Keystroke latency".
