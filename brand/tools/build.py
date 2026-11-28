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

from mark import symbol_group, paint, BOX, STEM_W, TOP, BOTTOM, f as fmt
from outline import word
from palette import (GREEN, AMBER, NEUTRAL, SEMANTIC, AMAN_GREEN, MINT, INK, PAPER, HIGHLIGHT,
                     ICON_GRADIENT, LOGO_ON_LIGHT, LOGO_ON_DARK)

ROOT = Path(__file__).resolve().parent.parent.parent
FONTS = Path(__file__).resolve().parent / ".fonts"
FONT_URLS = {
    "ReadexPro.ttf": "https://raw.githubusercontent.com/google/fonts/main/ofl/readexpro/ReadexPro%5BHEXP%2Cwght%5D.ttf",
}

WORD_LATIN = "amanfold"
WORD_ARABIC = "أمانفولد"
LATIN = dict(wght=600, tracking=-12, single_storey_a=True, kern={"nf": -6, "fo": -10, "ld": 4})
ARABIC = dict(wght=540)           # a little lighter, so it matches the Latin optically
ARABIC_SCALE = 0.9
GAP = STEM_W                      # space between symbol and name: one stem width
STEM_H = BOTTOM - TOP             # the symbol's stem is as tall as the ascenders of f, l and d
ONE = lambda c: dict(body=c, flap=None, text=c)


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


def svg(w, h, body, title, vb=None):
    vb = vb or f"0 0 {fmt(w)} {fmt(h)}"
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{fmt(w)}" height="{fmt(h)}" '
            f'viewBox="{vb}" role="img" aria-label="{title}"><title>{title}</title>{body}</svg>\n')


_words = {}


def words():
    if not _words:
        fp = font("ReadexPro.ttf")
        _words["lat"] = word(fp, WORD_LATIN, **LATIN)
        _words["ara"] = word(fp, WORD_ARABIC, **ARABIC)
    return _words["lat"], _words["ara"]


# ---------------------------------------------------------------- symbol and lockups
def mark(colors, kind=None, classes=False):
    """The symbol cropped to its own bounding box, 50 x 48 units."""
    x0, y0, x1, y1 = BOX
    return x1 - x0, y1 - y0, f'<g transform="translate({-x0} {-y0})">{symbol_group(colors["body"], colors["flap"], kind, classes)}</g>'


def lockup(colors, arabic=False, classes=False):
    """Symbol and name on one line. Latin: symbol on the left. Arabic: symbol on the right, never mirrored."""
    lat, ara = words()
    w = ara if arabic else lat
    s = STEM_H / lat["ascender"] * (ARABIC_SCALE if arabic else 1)
    bx0, by0, bx1, by1 = (v * s for v in w["bbox"])
    tw = bx1 - bx0
    x0, y0, x1, y1 = BOX
    top, bottom = min(y0, BOTTOM + by0), max(y1, BOTTOM + by1)
    sym = symbol_group(colors["body"], colors["flap"], None, classes)
    t = paint(colors["text"], "mt" if classes else None)
    if arabic:
        W = tw + GAP + (x1 - x0)
        body = (f'<path {t} transform="translate({fmt(-bx0)} {fmt(BOTTOM - top)}) scale({s:.5f})" d="{w["d"]}"/>'
                f'<g transform="translate({fmt(tw + GAP - x0)} {fmt(-top)})">{sym}</g>')
    else:
        W = (x1 - x0) + GAP + tw
        body = (f'<g transform="translate({-x0} {fmt(-top)})">{sym}</g>'
                f'<path {t} transform="translate({fmt(x1 - x0 + GAP - bx0)} {fmt(BOTTOM - top)}) scale({s:.5f})" d="{w["d"]}"/>')
    return W, bottom - top, body


def wordmark_only(arabic, color, classes=False):
    lat, ara = words()
    w = ara if arabic else lat
    s = STEM_H / lat["ascender"] * (ARABIC_SCALE if arabic else 1)
    x0, top, x1, bottom = (v * s for v in w["bbox"])
    t = paint(color, "mt" if classes else None)
    return x1 - x0, bottom - top, f'<path {t} transform="translate({fmt(-x0)} {fmt(-top)}) scale({s:.5f})" d="{w["d"]}"/>'


def stacked(colors, classes=False):
    """Symbol centred above the Latin and Arabic names: for splash screens and stickers."""
    lat, ara = words()
    k = 3                                      # symbol scale
    s = STEM_H / lat["ascender"] * 1.15
    sa = s * ARABIC_SCALE
    lx0, lt, lx1, lb = (v * s for v in lat["bbox"])
    ax0, at, ax1, ab = (v * sa for v in ara["bbox"])
    x0, y0, x1, y1 = BOX
    mw, mh = (x1 - x0) * k, (y1 - y0) * k
    lw, aw = lx1 - lx0, ax1 - ax0
    W = max(mw, lw, aw)
    yl = mh + 22 - lt
    ya = yl + lb + 16 - at
    t = paint(colors["text"], "mt" if classes else None)
    body = (f'<g transform="translate({fmt((W - mw) / 2 - x0 * k)} {fmt(-y0 * k)}) scale({k})">{symbol_group(colors["body"], colors["flap"], None, classes)}</g>'
            f'<path {t} transform="translate({fmt((W - lw) / 2 - lx0)} {fmt(yl)}) scale({s:.5f})" d="{lat["d"]}"/>'
            f'<path {t} transform="translate({fmt((W - aw) / 2 - ax0)} {fmt(ya)}) scale({sa:.5f})" d="{ara["d"]}"/>')
    return W, ya + ab, body


# ---------------------------------------------------------------- app icons
# The symbol sits at 62% of the tile and 1.6 units right of centre: the heavy stem makes a
# mathematically centred symbol look as if it leans left.
ICON_PLACE = "translate(32.6 32) scale(.62) translate(-31 -32)"


def tile(uid, rounded):
    rx = 14.4 if rounded else 0
    a, b = ICON_GRADIENT
    return (f'<linearGradient id="{uid}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{a}"/>'
            f'<stop offset="1" stop-color="{b}"/></linearGradient><rect width="64" height="64" rx="{rx}" fill="url(#{uid})"/>')


def app_icon(size=1024, rounded=False, small=False, uid="ai"):
    """Full-bleed square (stores apply their own mask) or a rounded preview. small=True uses the small drawing."""
    sym = symbol_group("#FFFFFF", None, "small") if small else symbol_group("#FFFFFF", MINT)
    return svg(size, size, tile(uid, rounded) + f'<g transform="{ICON_PLACE}">{sym}</g>', "Amanfold app icon", "0 0 64 64")


def android_layer(inner, title):
    # 108 x 108 canvas; the symbol stays inside the 66 unit safe zone
    return svg(108, 108, inner, title, "0 0 108 108")


def android_foreground():
    return android_layer(f'<g transform="translate(55 54) scale(.9) translate(-31 -32)">{symbol_group("#FFFFFF", MINT)}</g>',
                         "Amanfold adaptive icon foreground")


def android_background():
    a, b = ICON_GRADIENT
    return android_layer(f'<linearGradient id="ab" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{a}"/>'
                         f'<stop offset="1" stop-color="{b}"/></linearGradient><rect width="108" height="108" fill="url(#ab)"/>',
                         "Amanfold adaptive icon background")


def android_monochrome():
    return android_layer(f'<g transform="translate(55 54) scale(.9) translate(-31 -32)">{symbol_group("#000000", None)}</g>',
                         "Amanfold themed icon")


def favicon():
    """Bare symbol, small drawing. Aman Green on light browser themes, white on dark ones."""
    from mark import symbol
    b, _ = symbol("small")
    x0, y0, x1, y1 = BOX
    style = f'<style>path{{fill:{AMAN_GREEN}}}@media (prefers-color-scheme:dark){{path{{fill:#FFFFFF}}}}</style>'
    return svg(64, 64, style + f'<path fill-rule="evenodd" d="{b}"/>', "Amanfold", "1 2 60 60")


def safari_pinned():
    return svg(64, 64, symbol_group("#000000", None, "small"), "Amanfold", "1 2 60 60")


# ---------------------------------------------------------------- tokens
def tokens():
    t = {
        "color": {
            "green": {str(k): v for k, v in GREEN.items()},
            "amber": {str(k): v for k, v in AMBER.items()},
            "neutral": {str(k): v for k, v in NEUTRAL.items()},
            "brand": {"aman-green": AMAN_GREEN, "mint": MINT, "ink": INK, "paper": PAPER, "highlight": HIGHLIGHT},
            "semantic": SEMANTIC,
        },
        "theme": {
            "light": {
                "bg": PAPER, "surface": NEUTRAL[0], "surface-sunken": NEUTRAL[100], "border": NEUTRAL[200],
                "text": INK, "text-muted": NEUTRAL[600], "primary": AMAN_GREEN, "primary-hover": GREEN[700],
                "on-primary": NEUTRAL[0], "link": AMAN_GREEN, "accent": HIGHLIGHT, "on-accent": INK, "focus": GREEN[500],
                "success": SEMANTIC["success"]["light"], "warning": SEMANTIC["warning"]["light"],
                "danger": SEMANTIC["danger"]["light"], "info": SEMANTIC["info"]["light"],
            },
            "dark": {
                "bg": NEUTRAL[950], "surface": "#0F231D", "surface-sunken": "#06100D", "border": "#1E3A31",
                "text": "#EAF2EE", "text-muted": "#A3B5AE", "primary": GREEN[400], "primary-hover": GREEN[300],
                "on-primary": INK, "link": MINT, "accent": HIGHLIGHT, "on-accent": INK, "focus": GREEN[300],
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
        "shadow": {"card": "0 1px 2px rgb(8 20 16 / .06), 0 4px 16px rgb(8 20 16 / .06)",
                   "pop": "0 8px 32px rgb(8 20 16 / .18)"},
        "motion": {"fold": "cubic-bezier(.2,.8,.2,1)", "duration-fast": "120ms", "duration": "220ms"},
    }
    css = ["/* Amanfold design tokens. Generated by brand/tools/build.py; edit palette.py, not this file. */", ":root {"]
    for fam in ("green", "amber", "neutral", "brand"):
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
    black, white = ONE("#000000"), ONE("#FFFFFF")
    variants = {"": LOGO_ON_LIGHT, "-reversed": LOGO_ON_DARK, "-black": black, "-white": white}
    for suffix, colors in variants.items():
        write(f"brand/logo/amanfold-mark{suffix}.svg", svg(*mark(colors), "Amanfold"))
        write(f"brand/logo/amanfold-logo{suffix}.svg", svg(*lockup(colors), "Amanfold"))
        write(f"brand/logo/amanfold-logo-arabic{suffix}.svg", svg(*lockup(colors, arabic=True), "أمانفولد"))
        write(f"brand/logo/amanfold-logo-stacked{suffix}.svg", svg(*stacked(colors), "Amanfold أمانفولد"))
    # small drawing, for 24 px and below
    write("brand/logo/amanfold-mark-small.svg", svg(*mark(dict(body=AMAN_GREEN, flap=None), "small"), "Amanfold"))
    write("brand/logo/amanfold-mark-small-white.svg", svg(*mark(dict(body="#FFFFFF", flap=None), "small"), "Amanfold"))
    write("brand/logo/amanfold-wordmark-latin.svg", svg(*wordmark_only(False, INK), "Amanfold"))
    write("brand/logo/amanfold-wordmark-arabic.svg", svg(*wordmark_only(True, INK), "أمانفولد"))

    write("brand/app-icon/app-icon-1024.svg", app_icon(1024))
    write("brand/app-icon/app-icon-rounded.svg", app_icon(512, rounded=True))
    write("brand/app-icon/app-icon-small.svg", app_icon(64, rounded=True, small=True, uid="as"))
    write("brand/app-icon/android-adaptive-foreground.svg", android_foreground())
    write("brand/app-icon/android-adaptive-background.svg", android_background())
    write("brand/app-icon/android-monochrome.svg", android_monochrome())
    write("brand/app-icon/favicon.svg", favicon())
    write("brand/app-icon/safari-pinned-tab.svg", safari_pinned())
    tokens()
    print("built brand assets in", ROOT / "brand")


if __name__ == "__main__":
    main()
