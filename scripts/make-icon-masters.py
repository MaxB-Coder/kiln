#!/usr/bin/env python3
"""Writes the app-icon masters, assets/icon/variants/<design>-<colour>.svg: four designs (Kiln,
Flame, Beehive, Line) in seven colours. The tables are the approved mockup's
(notes/concept/kiln-icons.html, `COL` and `ICONS`), on its 100 grid with the tile at 10..90,
scaled to the 1024 macOS grid (X = 100 + (v - 10) * 10.3).

The Kiln design reproduces the hand-made masters it replaced byte for byte, and
kiln-terracotta.svg must stay the same file as assets/icon/kiln.svg (the bundle's icon).
Run it, then scripts/make-icons.sh for the PNGs. `--check` writes nothing and fails if a file
would change. Standard library only.
"""

import sys
from decimal import ROUND_HALF_UP, Decimal
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "icon" / "variants"

# b: tile body gradient, d: door gradient (Kiln), r/ro: rim stroke and opacity (Kiln),
# s: top sheen opacity, fd: flame in the Kiln door (outer, inner), ink: line art (Line),
# fb: flame drawn on the tile (Flame, Line). `about` describes the body in the master's comment.
COLOURS = {
    "terracotta": dict(b=("#ee8a48", "#c25b22"), d=("#2b1a12", "#140c08"), r="#f6a467", ro=0.5, s=0.22,
                       fd=("#ffa457", "#ffe3b5"), ink="#fff3e6", fb=("#fff1dc", "#ffd2a0"),
                       about="fired-clay body (the default)"),
    "black": dict(b=("#3d3834", "#161413"), d=("#0d0b0a", "#070605"), r="#e07b39", ro=0.45, s=0.14,
                  fd=("#e07b39", "#f6c08a"), ink="#e07b39", fb=("#e07b39", "#f6c08a"),
                  about="graphite body"),
    "white": dict(b=("#f8f3ec", "#e1d5c4"), d=("#2b2724", "#161311"), r="#c8b49b", ro=0.7, s=0.35,
                  fd=("#e07b39", "#f6c08a"), ink="#c25b22", fb=("#e07b39", "#f6c08a"),
                  about="glazed bone-white body"),
    "ember": dict(b=("#b0473a", "#6e2219"), d=("#241210", "#120807"), r="#e08a74", ro=0.5, s=0.2,
                  fd=("#ffa457", "#ffe3b5"), ink="#ffe3cf", fb=("#ffb070", "#ffe3b5"),
                  about="deep ember-red body"),
    "slate": dict(b=("#5d6b78", "#323b45"), d=("#161b21", "#0b0e12"), r="#a9b8c6", ro=0.45, s=0.2,
                  fd=("#e07b39", "#f6c08a"), ink="#ee8a48", fb=("#ee8a48", "#f6c08a"),
                  about="blue-grey slate body"),
    "sage": dict(b=("#8a9a78", "#55654a"), d=("#1d2219", "#0e110c"), r="#c5d1b2", ro=0.5, s=0.22,
                 fd=("#e07b39", "#f6c08a"), ink="#fbf3e4", fb=("#f2a35e", "#ffe3b5"),
                 about="sage-green glazed body"),
    "sand": dict(b=("#e3c89a", "#bf9a62"), d=("#2b2016", "#150f0a"), r="#f3dfba", ro=0.6, s=0.3,
                 fd=("#e07b39", "#f6c08a"), ink="#8a3f17", fb=("#c25b22", "#f6c08a"),
                 about="sand-coloured body"),
}
DESIGNS = ["kiln", "flame", "beehive", "line"]

# The prompt's flame (assets/flame/flame-1.svg), on a 12×16 view box.
FLAME_OUTER = ("M6 1 C8 4 10.5 6 10.5 9.5 C10.5 12.8 8.4 15 6 15 C3.6 15 1.5 12.8 1.5 9.5 "
               "C1.5 7 3.2 5.6 4 4 C4.3 5.5 5 6.3 5.6 6.5 C5.6 4.5 5.4 2.8 6 1 Z")
FLAME_INNER = ("M6 8 C7.2 9.4 8 10.4 8 11.8 C8 13.2 7.1 14 6 14 C4.9 14 4 13.2 4 11.8 "
               "C4 10.6 4.8 9.6 5.3 9.2 C5.4 9.9 5.7 10.2 6 10.3 C6 9.4 5.8 8.7 6 8 Z")


def tenths(v):
    """To one decimal, halves rounded up (303.85 -> 303.9), as the first masters were."""
    return str(Decimal(v).quantize(Decimal("0.1"), rounding=ROUND_HALF_UP))


def x(v):
    """A mockup coordinate on the 1024 grid."""
    return tenths(100 + (Decimal(repr(v)) - 10) * Decimal("10.3"))


def length(v):
    """A mockup length on the 1024 grid."""
    return tenths(Decimal(repr(v)) * Decimal("10.3"))


def num(v):
    """An opacity as the mockup writes it (0.5, 0.45)."""
    return f"{v:g}"


def gradient(id_, a, b):
    return (f'<linearGradient id="{id_}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" '
            f'stop-color="{a}"/><stop offset="1" stop-color="{b}"/></linearGradient>')


def sheen_def(c):
    return ('<radialGradient id="sheen" cx="50%" cy="0%" r="75%"><stop offset="0" stop-color="#ffffff" '
            f'stop-opacity="{num(c["s"])}"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/>'
            '</radialGradient>')


TILE = '<rect x="100" y="100" width="824" height="824" rx="185" fill="url(#{})"/>'


def flame(colours, tx, ty, scale, inner=True):
    """The flame at (tx, ty) on the mockup grid, `scale` times its view box, on the 1024 grid."""
    lines = [f'  <g transform="translate({x(tx)} {x(ty)}) scale({scale * 10.3:.2f})">',
             f'    <path fill="{colours[0]}" d="{FLAME_OUTER}"/>']
    if inner:
        lines.append(f'    <path fill="{colours[1]}" d="{FLAME_INNER}"/>')
    lines.append("  </g>")
    return lines


def kiln(name, c):
    door = f"M{x(22)} {x(90)} V{x(53)} A{length(28)} {length(28)} 0 0 1 {x(78)} {x(53)} V{x(90)} Z"
    rim = (f"M{x(20.5)} {x(90)} V{x(53)} A{length(29.5)} {length(29.5)} 0 0 1 {x(79.5)} {x(53)} "
           f"V{x(90)}")
    return [
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">',
        f'  <!-- Kiln app icon, "{name}": {c["about"]}. Wide door (56% of the tile) with the prompt\'s',
        "       flame (assets/flame/flame-1.svg) on the hearth. macOS icon grid: 824 px body on 1024.",
        "       Source for scripts/make-icons.sh; all three variants share this geometry. -->",
        "  <defs>",
        "    " + gradient("body", *c["b"]),
        "    " + gradient("door", *c["d"]),
        '    <radialGradient id="hearth" cx="50%" cy="88%" r="65%"><stop offset="0" stop-color="#ff9a4a" '
        'stop-opacity="0.8"/><stop offset="0.55" stop-color="#ff7a2e" stop-opacity="0.2"/><stop offset="1" '
        'stop-color="#ff7a2e" stop-opacity="0"/></radialGradient>',
        "    " + sheen_def(c),
        '    <clipPath id="tile"><rect x="100" y="100" width="824" height="824" rx="185"/></clipPath>',
        f'    <clipPath id="opening"><path d="{door}"/></clipPath>',
        "  </defs>",
        "  " + TILE.format("body"),
        '  <g clip-path="url(#tile)">',
        f'    <path d="{door}" fill="url(#door)"/>',
        f'    <g clip-path="url(#opening)"><rect x="{x(12)}" y="{x(20)}" width="{length(76)}" '
        f'height="{length(75)}" fill="url(#hearth)"/></g>',
        f'    <path d="{rim}" fill="none" stroke="{c["r"]}" stroke-opacity="{num(c["ro"])}" '
        f'stroke-width="{length(1.3)}"/>',
        "  </g>",
        "  " + TILE.format("sheen"),
        *flame(c["fd"], 36.2, 50, 2.3),
        "</svg>",
    ]


def header(design, name, c, what):
    return [
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">',
        f'  <!-- Kiln app icon, {design} design, "{name}": {c["about"]}. {what}',
        "       Written by scripts/make-icon-masters.py from notes/concept/kiln-icons.html; the",
        "       drawing is on the mockup's 100 grid (tile at 10..90), scaled onto the 1024 grid. -->",
    ]


# The mockup's 100 grid placed on the 1024 one.
GRID = 'transform="translate(100 100) scale(10.3) translate(-10 -10)"'


def flame_design(name, c):
    return [
        *header("Flame", name, c, "The prompt's flame, large, over a warm glow."),
        "  <defs>",
        "    " + gradient("body", *c["b"]),
        '    <radialGradient id="glow" cx="50%" cy="62%" r="45%"><stop offset="0" stop-color="#ff8a3a" '
        'stop-opacity="0.45"/><stop offset="1" stop-color="#ff7a2e" stop-opacity="0"/></radialGradient>',
        "    " + sheen_def(c),
        "  </defs>",
        "  " + TILE.format("body"),
        "  " + TILE.format("glow"),
        "  " + TILE.format("sheen"),
        *flame(c["fb"], 30.5, 22, 3.25),
        "</svg>",
    ]


def beehive(name, c):
    # The kiln itself is always brick; only the tile takes the colour. A soft shadow under the
    # dome keeps it apart from the warm tiles (terracotta, ember, sand).
    return [
        *header("Beehive", name, c, "A brick beehive kiln, the same on every colour."),
        "  <defs>",
        "    " + gradient("body", *c["b"]),
        "    " + gradient("brick", "#d9774a", "#9a4426"),
        '    <radialGradient id="mouth" cx="50%" cy="80%" r="60%"><stop offset="0" stop-color="#ffd08a"/>'
        '<stop offset="0.6" stop-color="#ff8a3a"/><stop offset="1" stop-color="#b8451c"/></radialGradient>',
        "    " + sheen_def(c),
        "  </defs>",
        "  " + TILE.format("body"),
        "  " + TILE.format("sheen"),
        f"  <g {GRID}>",
        '    <path d="M21 77 C21 50 32.5 31 50 31 C67.5 31 79 50 79 77 Z" fill="#2a120a" opacity="0.35"/>',
        '    <rect x="46" y="20" width="8" height="14" rx="1.5" fill="url(#brick)"/>',
        '    <path d="M22 76 C22 50 33 32 50 32 C67 32 78 50 78 76 Z" fill="url(#brick)"/>',
        '    <path d="M27 52 H73 M24 64 H76 M33 41 H67" stroke="#7a3318" stroke-opacity="0.45" stroke-width="1.2"/>',
        '    <path d="M41 76 V66 A9 9 0 0 1 59 66 V76 Z" fill="url(#mouth)"/>',
        '    <path d="M18 76 H82" stroke="#7a3318" stroke-width="2" stroke-linecap="round"/>',
        "  </g>",
        "</svg>",
    ]


def line(name, c):
    return [
        *header("Line", name, c, "The kiln's arch drawn in one line, with a small flame."),
        "  <defs>",
        "    " + gradient("body", *c["b"]),
        "    " + sheen_def(c),
        "  </defs>",
        "  " + TILE.format("body"),
        "  " + TILE.format("sheen"),
        f"  <g {GRID}>",
        f'    <path d="M28 76 V52 A22 22 0 0 1 72 52 V76" fill="none" stroke="{c["ink"]}" stroke-width="4.2" '
        'stroke-linecap="round"/>',
        f'    <path d="M22 76 H78" stroke="{c["ink"]}" stroke-width="4.2" stroke-linecap="round"/>',
        "  </g>",
        *flame(c["fb"], 41.6, 50, 1.4, inner=False),
        "</svg>",
    ]


BUILD = {"kiln": kiln, "flame": flame_design, "beehive": beehive, "line": line}


def masters():
    """Every master: file name -> text."""
    return {
        f"{design}-{name}.svg": "\n".join(BUILD[design](name, c)) + "\n"
        for design in DESIGNS
        for name, c in COLOURS.items()
    }


def main():
    check = "--check" in sys.argv[1:]
    changed = []
    for file, text in masters().items():
        path = OUT / file
        if path.exists() and path.read_text() == text:
            continue
        changed.append(file)
        if not check:
            path.write_text(text)
    kiln_svg = ROOT / "assets" / "icon" / "kiln.svg"
    if kiln_svg.read_text() != masters()["kiln-terracotta.svg"]:
        print("make-icon-masters: assets/icon/kiln.svg differs from kiln-terracotta.svg", file=sys.stderr)
        return 1
    if check and changed:
        print("make-icon-masters: out of date: " + ", ".join(changed), file=sys.stderr)
        return 1
    print(f"{len(masters())} masters" + (f", wrote {len(changed)}" if changed and not check else ", unchanged"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
