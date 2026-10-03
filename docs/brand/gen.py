"""Generate flint brand art with agy (sandboxed, one image per run) and copy each into this folder."""
import os, re, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
STYLE = ("Brand: 'flint', a native macOS app for coding agents. Palette strictly: near-black #0e0e10, "
         "surfaces #141418/#1b1b20, text #e7e7ea, ONE ember-orange accent #ff8a3d with restrained glow. "
         "Minimal, geometric, premium, calm (Zed/Linear/Warp polish). No purple, no neon cyberpunk, no robots, "
         "no mascots, no lens flare, no watermark, no UI mockup frame.")
MARK = 'THE MARK (must match exactly): a tall vertical gem-like flint shard, a slightly irregular pointed hexagon taller than wide, split into 5-6 flat triangular facets by thin light lines; all facets solid near-black except exactly ONE facet on the lower right which is solid ember orange #ff8a3d. Flat vector style, crisp edges, no gradients beyond a faint glow, no sparks.'
JOBS = {
 "mark-v2": "Logomark only, centered with generous padding, on plain #0e0e10, square. " + MARK + " No text.",
 "wordmark-dark-v2": "Horizontal logo lockup, wide 3:1, on plain #0e0e10: on the left " + MARK + " On the right the lowercase word 'flint' in a clean geometric sans-serif (like the light version) in #e7e7ea. The only text is exactly 'flint'.",
 "icon-v2": "macOS app icon, square 1:1: a rounded-square (squircle) tile in #141418 with a subtle top highlight, centered on a plain #0e0e10 canvas with the standard macOS icon margin. On the tile: " + MARK + " No text.",
 "icon-1024": "macOS app icon, square 1:1, FULL BLEED: the dark squircle tile fills the entire canvas edge to edge, no outer background, no drop shadow, no device mockup. On the tile: a faceted dark flint-stone shard striking one small ember spark. No text.",
 "icon-alt-a": "macOS app icon, square, full bleed dark tile filling the canvas: a single minimal ember spark (four-point star) above a thin faceted stone edge line. Very minimal. No text.",
 "icon-alt-b": "macOS app icon, square, full bleed dark tile filling the canvas: a geometric low-poly flint stone, one facet glowing ember orange from inside. No text.",
 "icon-alt-c": "macOS app icon, square, full bleed dark tile filling the canvas: an abstract lowercase 'f' monogram carved from faceted dark stone with an ember spark at its tip. Only the letter f, nothing else.",
 "mark": "Logomark only on a plain #0e0e10 background, square: a faceted flint shard with a single ember spark, flat-ish vector style, centered with generous padding. No text, no tile.",
 "wordmark-dark": "Horizontal logo lockup, wide 3:1, on plain #0e0e10: the faceted flint-shard-and-spark mark on the left and the lowercase word 'flint' in a clean geometric sans-serif in #e7e7ea on the right. The only text is exactly 'flint'.",
 "wordmark-light": "Horizontal logo lockup, wide 3:1, on plain warm white #f6f5f3: the faceted flint-shard-and-spark mark on the left and the lowercase word 'flint' in a clean geometric sans-serif in near-black #141418 on the right. The only text is exactly 'flint'.",
 "social-preview": "Wide 2:1 banner (GitHub social preview): dark #0e0e10 background with a faint ember glow, centered the flint-shard-and-spark mark above the lowercase word 'flint' in clean geometric sans, lots of negative space. The only text is exactly 'flint'.",
 "readme-hero": "Very wide 3:1 banner: an abstract cinematic scene of a single ember spark arcing over a dark faceted stone surface, shallow depth of field, the left half mostly empty dark space. No text.",
 "feature-agents": "Square illustration: three thin glowing streams (one ember orange, two soft grey) converging into a single minimal dark window frame. Abstract, no logos, no text.",
 "feature-guard": "Square illustration: a looping spiral line being caught by a calm faceted shield shape and straightened into a clean straight line, ember accent. Abstract, no text.",
 "feature-diffs": "Square illustration: abstract horizontal code-line bars on dark, a few bars tinted soft green #4cc38a and soft red #f2555a, crisp and minimal. No readable text.",
 "pattern": "Seamless tileable dark background texture, square: faint faceted flint stone texture on #0e0e10 with a few tiny scattered ember sparks, very subtle and low contrast. No text.",
}

def run(name, prompt):
    full = f"Use ONLY your image generation tool. Do not run any commands, scripts or code, and do not edit files. Generate ONE image. {prompt} {STYLE} After generating, print only the absolute file path of the saved image."
    out = subprocess.run(["agy", "-p", full, "--model", "gemini-3.8-flash-high", "--sandbox"],
                         capture_output=True, text=True, timeout=900).stdout
    paths = re.findall(r"(/Users/[^\s\]\)]+\.(?:png|jpg|jpeg|webp))", out)
    if not paths or not os.path.exists(paths[-1]):
        print(f"{name}: FAILED\n{out[-400:]}", flush=True); return
    src = paths[-1]; dst = os.path.join(HERE, name + os.path.splitext(src)[1])
    shutil.copy(src, dst); print(f"{name}: {dst}", flush=True)

names = sys.argv[1:] or list(JOBS)
for n in names:
    try: run(n, JOBS[n])
    except Exception as e: print(f"{n}: ERROR {e}", flush=True)
print("DONE", flush=True)
