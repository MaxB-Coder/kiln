# Bundled fonts

Kiln embeds its font in the binary, so text looks the same on macOS, Windows and Linux with
nothing installed. WezTerm does the same.

| File | Source | License |
| --- | --- | --- |
| `JetBrainsMonoNerdFontMono-{Regular,Bold,Italic,BoldItalic}.ttf` | [Nerd Fonts v3.5.1](https://github.com/ryanoasis/nerd-fonts/releases/tag/v3.5.1) `JetBrainsMono.tar.xz` (sha256 `04d5e8f903693f9dd13e16f867e994834e681eb3c72c0d337a770dcda09010cf`): JetBrains Mono 2.304 patched with the Nerd Font symbols | JetBrains Mono: SIL OFL 1.1 (`JetBrainsMono-OFL.txt`). Nerd Font glyphs: MIT and others (`NerdFonts-LICENSE`) |

Why the patched font and not plain JetBrains Mono plus a symbols-only fallback (what WezTerm
does): GPUI refuses to load a font without an `m` glyph, and on macOS its fallback list only
finds fonts registered with CoreText, never fonts embedded in the binary. The patched font
avoids both.

Renovate cannot bump these. Check Nerd Fonts and JetBrains Mono when cutting a release.
