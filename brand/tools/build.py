"""Build every Amanfold brand asset from code.

Run from the repository root:

    pip install fonttools uharfbuzz
    python brand/tools/build.py

Outputs SVG logos, app icons, favicon and design tokens into brand/.
PNG exports are made by brand/tools/render-png.js (needs Playwright).
Fonts (SIL Open Font License) are downloaded into brand/tools/.fonts on first run.
"""
import json
import os
import urllib.request
from pathlib import Path

from outline import outline
from palette import OASIS, SAFFRON, MIST, SEMANTIC, MARK_ON_LIGHT, MARK_ON_DARK

ROOT = Path(__file__).resolve().parent.parent.parent
FONTS = Path(__file__).resolve().parent / ".fonts"
FONT_URLS = {
    "ReadexPro.ttf": "https://raw.githubusercontent.com/google/fonts/main/ofl/readexpro/ReadexPro%5BHEXP%2Cwght%5D.ttf",
}

WORD_LATIN = "amanfold"
WORD_ARABIC = "أمانفولد"
WORD_WEIGHT = 500
WORD_TRACKING = -12  # thousandths of an em


def font(name):
    FONTS.mkdir(exist_ok=True)
    p = FONTS / name
    if not p.exists():
        urllib.request.urlretrieve(FONT_URLS[name], p)
    return str(p)


def write(rel, text):
    p = ROOT / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")
    return p


# ---------------------------------------------------------------- the mark
# 64-unit grid. A square sheet with its four corners folded to the centre,
# held shut by a round seal. A 3-unit ring separates the seal from the folds.
def _flaps():
    """Four triangles with a 3-unit gap along each fold line and softly rounded corners
    (drawn as an inset polygon with a 2-unit round-joined stroke)."""
    g = 1.5 * 2 ** 0.5 + 2 ** 0.5          # half-gap plus stroke radius, measured along x
    top = [(9 + g, 9), (55 - g, 9), (32, 32 - g)]
    rot = lambda pts, k: [(pts_x, pts_y) for pts_x, pts_y in pts] if k == 0 else rot([(64 - y, x) for x, y in pts], k - 1)
    names = ["top", "right", "bottom", "left"]
    return {n: "M" + " ".join(f"{fmt(x)} {fmt(y)}" for x, y in rot(top, i)) + "z" for i, n in enumerate(names)}


RING_CLIP = "M0 0h64v64H0zM32 20.5a11.5 11.5 0 1 0 0 23 11.5 11.5 0 1 0 0-23z"
SEAL = (32, 32, 8.5)


def mark_group(L, D, S, uid="m"):
    """SVG fragment for the mark on the 64 grid (no outer <svg>)."""
    cx, cy, r = SEAL
    F = _flaps()
    flap = lambda n, c: f'<path fill="{c}" stroke="{c}" stroke-width="2" stroke-linejoin="round" d="{F[n]}"/>'
    return (
        f'<clipPath id="{uid}-ring"><path clip-rule="evenodd" d="{RING_CLIP}"/></clipPath>'
        f'<g clip-path="url(#{uid}-ring)">{flap("top", L)}{flap("right", D)}{flap("bottom", L)}{flap("left", D)}</g>'
        f'<circle fill="{S}" cx="{cx}" cy="{cy}" r="{r}"/>'
    )


def svg(w, h, body, title, vb=None):
    vb = vb or f"0 0 {fmt(w)} {fmt(h)}"
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{fmt(w)}" height="{fmt(h)}" '
            f'viewBox="{vb}" role="img" aria-label="{title}"><title>{title}</title>{body}</svg>\n')


def fmt(v):
    return ("%.2f" % v).rstrip("0").rstrip(".")


# ---------------------------------------------------------------- lockups
def words():
    f = font("ReadexPro.ttf")
    lat = outline(f, WORD_LATIN, WORD_WEIGHT, 100, WORD_TRACKING)
    ara = outline(f, WORD_ARABIC, WORD_WEIGHT, 100, 0)
    return lat, ara


def lockup(word, colors, text_color, rtl=False, uid="lk", arabic=False):
    """Mark + wordmark on one line. The mark's art is 48 units tall; Latin cap height is 62% of it.
    Arabic is set 12% smaller so its letter bodies match the Latin x-height optically."""
    M = 48
    scale = (M * 0.62) / word["cap"] * (0.88 if arabic else 1)
    gap = M * 0.28
    x0, top, x1, bottom = (v * scale for v in word["bbox"])
    tw = x1 - x0
    base = M / 2 + M * 0.31                   # Latin cap height centred on the seal
    y_min = min(0, base + top)
    y_max = max(M, base + bottom)
    W, H = M + gap + tw, y_max - y_min
    mx, tx = (0, M + gap - x0) if not rtl else (tw + gap, -x0)
    body = (f'<g transform="translate(0 {fmt(-y_min)})">'
            f'<g transform="translate({fmt(mx - 8)} -8)">{mark_group(**colors, uid=uid)}</g>'
            f'<path fill="{text_color}" transform="translate({fmt(tx)} {fmt(base)}) scale({scale:.4f})" d="{word["d"]}"/></g>')
    return W, H, body


def wordmark_only(word, color):
    scale = 32 / word["cap"]
    x0, top, x1, bottom = (v * scale for v in word["bbox"])
    W, H = x1 - x0, bottom - top
    body = f'<path fill="{color}" transform="translate({fmt(-x0)} {fmt(-top)}) scale({scale:.4f})" d="{word["d"]}"/>'
    return W, H, body


def stacked(lat, ara, colors, text_color, uid="st"):
    """Mark centred above the Latin and Arabic names: for splash screens and stickers."""
    M = 150                                   # visual mark size
    s = 34 / lat["cap"]
    sa = s * 0.88
    lx0, lt, lx1, lb = (v * s for v in lat["bbox"])
    ax0, at, ax1, ab = (v * sa for v in ara["bbox"])
    lw, aw = lx1 - lx0, ax1 - ax0
    W = max(M, lw, aw)
    y1 = M + 26 - lt                          # Latin baseline: 26 below the mark
    y2 = y1 + lb + 20 - at                    # Arabic baseline: 20 below the Latin
    H = y2 + ab
    k = M / 48
    body = (f'<g transform="translate({fmt((W - M) / 2 - 8 * k)} {fmt(-8 * k)}) scale({fmt(k)})">{mark_group(**colors, uid=uid)}</g>'
            f'<path fill="{text_color}" transform="translate({fmt((W - lw) / 2 - lx0)} {fmt(y1)}) scale({s:.4f})" d="{lat["d"]}"/>'
            f'<path fill="{text_color}" transform="translate({fmt((W - aw) / 2 - ax0)} {fmt(y2)}) scale({sa:.4f})" d="{ara["d"]}"/>')
    return W, H, body


# ---------------------------------------------------------------- app icons
def app_icon(size=1024, rounded=False, uid="ai"):
    """Full-bleed square (stores apply their own mask) or a rounded preview."""
    bg = OASIS[800]
    rect = f'<rect width="64" height="64" rx="{14.2 if rounded else 0}" fill="{bg}"/>'
    # subtle top light so the tile is not flat
    grad = (f'<linearGradient id="{uid}-g" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="#FFFFFF" stop-opacity=".07"/><stop offset="1" stop-color="#000000" stop-opacity=".10"/></linearGradient>'
            f'<rect width="64" height="64" rx="{14.2 if rounded else 0}" fill="url(#{uid}-g)"/>')
    inner = f'<g transform="translate(13.44 13.44) scale(.58)">{mark_group(**MARK_ON_DARK, uid=uid)}</g>'
    return svg(size, size, rect + grad + inner, "Amanfold app icon", "0 0 64 64")


def android_foreground():
    # 108x108 canvas, mark kept inside the 66 unit safe zone
    s = 50 / 64
    inner = f'<g transform="translate({fmt((108 - 50) / 2)} {fmt((108 - 50) / 2)}) scale({s:.4f})">{mark_group(**MARK_ON_DARK, uid="af")}</g>'
    return svg(108, 108, inner, "Amanfold adaptive icon foreground", "0 0 108 108")


def android_background():
    return svg(108, 108, f'<rect width="108" height="108" fill="{OASIS[800]}"/>', "Amanfold adaptive icon background", "0 0 108 108")


def android_monochrome():
    s = 50 / 64
    inner = f'<g transform="translate(29 29) scale({s:.4f})">{mark_group("#000", "#000", "#000", uid="am")}</g>'
    return svg(108, 108, inner, "Amanfold themed icon", "0 0 108 108")


def favicon():
    # Rounded tile so it reads on light and dark browser tabs alike
    return app_icon(32, rounded=True, uid="fv")


def safari_pinned():
    return svg(64, 64, mark_group("#000", "#000", "#000", uid="sp"), "Amanfold")


# ---------------------------------------------------------------- tokens
def tokens():
    t = {
        "color": {
            "oasis": {str(k): v for k, v in OASIS.items()},
            "saffron": {str(k): v for k, v in SAFFRON.items()},
            "mist": {str(k): v for k, v in MIST.items()},
            "semantic": SEMANTIC,
        },
        "theme": {
            "light": {
                "bg": MIST[50], "surface": MIST[0], "surface-sunken": MIST[100], "border": MIST[200],
                "text": MIST[900], "text-muted": MIST[600], "primary": OASIS[800], "primary-hover": OASIS[700],
                "on-primary": MIST[0], "accent": SAFFRON[500], "on-accent": MIST[950], "focus": OASIS[500],
                "success": SEMANTIC["success"]["light"], "warning": SEMANTIC["warning"]["light"],
                "danger": SEMANTIC["danger"]["light"], "info": SEMANTIC["info"]["light"],
            },
            "dark": {
                "bg": MIST[950], "surface": "#112220", "surface-sunken": "#081210", "border": "#22403C",
                "text": "#E6EFEC", "text-muted": "#9DB2AD", "primary": OASIS[300], "primary-hover": OASIS[200],
                "on-primary": OASIS[950], "accent": SAFFRON[400], "on-accent": MIST[950], "focus": OASIS[300],
                "success": SEMANTIC["success"]["dark"], "warning": SEMANTIC["warning"]["dark"],
                "danger": SEMANTIC["danger"]["dark"], "info": SEMANTIC["info"]["dark"],
            },
        },
        "font": {
            "display": "'Readex Pro', 'IBM Plex Sans Arabic', system-ui, sans-serif",
            "text": "'IBM Plex Sans', 'IBM Plex Sans Arabic', system-ui, sans-serif",
            "mono": "'IBM Plex Mono', ui-monospace, monospace",
        },
        "type-scale": {"xs": "12px", "sm": "14px", "md": "16px", "lg": "18px", "xl": "22px",
                        "2xl": "28px", "3xl": "36px", "4xl": "48px"},
        "space": {str(i): f"{v}px" for i, v in enumerate([0, 4, 8, 12, 16, 20, 24, 32, 40, 48, 64])},
        "radius": {"sm": "6px", "md": "10px", "lg": "14px", "xl": "20px", "full": "999px"},
        "shadow": {"card": "0 1px 2px rgb(6 27 26 / .06), 0 4px 16px rgb(6 27 26 / .06)",
                   "pop": "0 8px 32px rgb(6 27 26 / .18)"},
        "motion": {"fold": "cubic-bezier(.2,.8,.2,1)", "duration-fast": "120ms", "duration": "220ms"},
    }
    css = ["/* Amanfold design tokens. Generated by brand/tools/build.py; edit palette.py, not this file. */", ":root {"]
    for fam in ("oasis", "saffron", "mist"):
        for k, v in t["color"][fam].items():
            css.append(f"  --af-{fam}-{k}: {v};")
    for k, v in t["font"].items():
        css.append(f"  --af-font-{k}: {v};")
    for k, v in t["type-scale"].items():
        css.append(f"  --af-text-{k}: {v};")
    for k, v in t["space"].items():
        css.append(f"  --af-space-{k}: {v};")
    for k, v in t["radius"].items():
        css.append(f"  --af-radius-{k}: {v};")
    for k, v in t["shadow"].items():
        css.append(f"  --af-shadow-{k}: {v};")
    for k, v in t["motion"].items():
        css.append(f"  --af-motion-{k}: {v};")
    for k, v in t["theme"]["light"].items():
        css.append(f"  --af-{k}: {v};")
    css.append("}")
    dark = "\n".join(f"  --af-{k}: {v};" for k, v in t["theme"]["dark"].items())
    css.append("@media (prefers-color-scheme: dark) {\n  :root:not([data-theme=\"light\"]) {\n" +
               dark.replace("\n  ", "\n    ").replace("  --", "    --", 1) + "\n    color-scheme: dark;\n  }\n}")
    css.append(":root[data-theme=\"dark\"] {\n" + dark + "\n  color-scheme: dark;\n}")
    write("brand/tokens/tokens.json", json.dumps(t, indent=2, ensure_ascii=False) + "\n")
    write("brand/tokens/tokens.css", "\n".join(css) + "\n")


# ---------------------------------------------------------------- main
def main():
    lat, ara = words()
    ink, paper = MIST[900], MIST[0]

    write("brand/logo/amanfold-mark.svg", svg(64, 64, mark_group(**MARK_ON_LIGHT), "Amanfold"))
    write("brand/logo/amanfold-mark-reversed.svg", svg(64, 64, mark_group(**MARK_ON_DARK), "Amanfold"))
    write("brand/logo/amanfold-mark-black.svg", svg(64, 64, mark_group("#000", "#000", "#000"), "Amanfold"))
    write("brand/logo/amanfold-mark-white.svg", svg(64, 64, mark_group("#FFF", "#FFF", "#FFF"), "Amanfold"))

    variants = {
        "": (MARK_ON_LIGHT, ink),
        "-reversed": (MARK_ON_DARK, paper),
        "-black": (dict(L="#000", D="#000", S="#000"), "#000"),
        "-white": (dict(L="#FFF", D="#FFF", S="#FFF"), "#FFF"),
    }
    for suffix, (colors, tc) in variants.items():
        w, h, b = lockup(lat, colors, tc)
        write(f"brand/logo/amanfold-logo{suffix}.svg", svg(w, h, b, "Amanfold"))
        w, h, b = lockup(ara, colors, tc, rtl=True, arabic=True)
        write(f"brand/logo/amanfold-logo-arabic{suffix}.svg", svg(w, h, b, "أمانفولد"))
        w, h, b = stacked(lat, ara, colors, tc)
        write(f"brand/logo/amanfold-logo-stacked{suffix}.svg", svg(w, h, b, "Amanfold أمانفولد"))
    for name, word in (("latin", lat), ("arabic", ara)):
        w, h, b = wordmark_only(word, ink)
        write(f"brand/logo/amanfold-wordmark-{name}.svg", svg(w, h, b, "Amanfold"))

    write("brand/app-icon/app-icon-1024.svg", app_icon(1024))
    write("brand/app-icon/app-icon-rounded.svg", app_icon(512, rounded=True))
    write("brand/app-icon/android-adaptive-foreground.svg", android_foreground())
    write("brand/app-icon/android-adaptive-background.svg", android_background())
    write("brand/app-icon/android-monochrome.svg", android_monochrome())
    write("brand/app-icon/favicon.svg", favicon())
    write("brand/app-icon/safari-pinned-tab.svg", safari_pinned())
    tokens()
    print("built brand assets in", ROOT / "brand")


if __name__ == "__main__":
    main()
