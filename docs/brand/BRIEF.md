# flint brand artwork brief

flint is an open-source native macOS desktop app (Rust + GPUI) for coding agents:
its own fast agent with "harness" guard rules (loop detection, test-before-done),
plus Claude Code and Codex in the same window. Audience: professional developers.
Personality: precise, fast, calm, dark-native. Think Zed / Linear / Warp polish.
Name meaning: flint is the stone you strike to make a spark — the agent is the spark.

Palette (from the app theme, use exactly):
- background near-black #0e0e10, surfaces #141418 / #1b1b20, borders #222228
- text #e7e7ea, muted #9a9aa4
- ONE accent: ember orange #ff8a3d (soft glow rgba(255,138,61,0.14))
- semantic only when needed: green #4cc38a, red #f2555a

Style rules: minimal, geometric, premium; restrained ember glow, subtle grain is
fine; no purple gradients, no neon cyberpunk, no robots, no mascots, no
lens-flare, no clip-art, no stock 3D blobs. Any text rendered in an image must be
exactly the lowercase word "flint" (or none). Prefer no text except where listed.

Deliverables (PNG, save into this folder with these exact filenames):
1. icon-1024.png — macOS app icon 1024x1024: rounded-square (squircle) dark tile,
   a faceted flint-stone shard striking a single ember spark. No text.
2. mark.png — the logomark alone (the flint shard + spark) on transparent or
   pure #0e0e10 background, square, no text.
3. wordmark-dark.png — horizontal lockup: mark + lowercase "flint" in a clean
   geometric sans, light text on #0e0e10, wide (about 3:1).
4. wordmark-light.png — same lockup for light backgrounds (dark text on #f6f5f3).
5. social-preview.png — GitHub social preview 1280x640: dark, mark + "flint" +
   generous negative space, faint ember glow. No other words.
6. readme-hero.png — wide README banner (about 2400x800): abstract scene of a
   spark arcing over a dark faceted stone surface, room on the left for text, no text.
7. feature-agents.png — square illustration: three minimal glowing nodes/streams
   converging into one window frame (own agent + Claude Code + Codex). No text, no logos.
8. feature-guard.png — square illustration: a calm shield-like facet catching
   a loop/spiral and straightening it into a line (the harness guard). No text.
9. feature-diffs.png — square illustration: abstract code lines with a few lines
   glowing green and red, crisp and minimal (reviewing diffs). No text.
10. pattern.png — seamless dark background pattern of faint faceted flint texture
    with tiny ember sparks, for slides/site, 2048x2048.
11. icon-variants.png — a contact sheet of 4 alternative icon concepts (for choice).

Generate each with your image tool, save with the exact filename, then write
NOTES.md listing each file, its prompt, and anything that came out imperfect.
Re-generate any image that has wrong/garbled text, extra words, off-palette color,
or watermark-like artifacts. Do not touch anything outside docs/brand/.
