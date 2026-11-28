"""Amanfold UI icon set: 24 unit grid, 1.75 stroke, round caps and joins.
Containers (cards, notes, identities) carry the brand's folded top-right corner.
Run from the repository root: python brand/tools/icons.py"""
from pathlib import Path

FOLD_BOX = "M3 7a2 2 0 0 1 2-2h11l5 5v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"
FOLD_TAB = "M16 5v3a2 2 0 0 0 2 2h3"
ICONS = {
    "login": ["c8 15 4", "M11 12l8-8", "M16 7l2.5 2.5", "M13.5 9.5l2 2"],
    "card": [FOLD_BOX, FOLD_TAB, "M3 10h9", "M7 15h3"],
    "note": ["M6 3h8l5 5v11a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M14 3v3a2 2 0 0 0 2 2h3", "M8 13h8", "M8 17h5"],
    "identity": [FOLD_BOX, FOLD_TAB, "c8.5 11 2", "M5.5 16c.6-1.3 1.7-2 3-2s2.4.7 3 2", "M14 13h3.5", "M14 16h3.5"],
    "folder": ["M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"],
    "search": ["c11 11 6.5", "M20 20l-4.4-4.4"],
    "add": ["M12 5v14", "M5 12h14"],
    "copy": ["M10 8h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z", "M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"],
    "show": ["M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z", "c12 12 3"],
    "hide": ["M3 3l18 18", "M10.5 5.6A9 9 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a16 16 0 0 1-2.7 3.5", "M6.7 7.2C4 9 2.5 12 2.5 12S6 18.5 12 18.5a9 9 0 0 0 4.6-1.3", "M9.9 9.9a3 3 0 0 0 4.2 4.2"],
    "generate": ["M12 3v4", "M12 17v4", "M3 12h4", "M17 12h4", "M6 6l2.5 2.5", "M15.5 15.5L18 18", "M18 6l-2.5 2.5", "M8.5 15.5L6 18"],
    "share": ["c18 5 2.5", "c6 12 2.5", "c18 19 2.5", "M8.2 10.8l7.6-4.4", "M8.2 13.2l7.6 4.4"],
    "family": ["c9 8 3", "c17 9.5 2.2", "M3.5 19c.6-3 2.8-5 5.5-5s4.9 2 5.5 5", "M15.5 14.2c2.4-.3 4.4 1.3 5 4.3"],
    "team": ["c12 8 3", "c5 10 2", "c19 10 2", "M7 19c.5-2.8 2.5-4.5 5-4.5s4.5 1.7 5 4.5", "M2.5 17.5c.3-1.8 1.4-3 3-3.2", "M21.5 17.5c-.3-1.8-1.4-3-3-3.2"],
    "settings": ["M4 7h9", "M17 7h3", "M4 17h3", "M11 17h9", "c15 7 2", "c9 17 2"],
    "sync": ["M20 11a8 8 0 0 0-14.3-4.3L4 8.5", "M4 4v4.5h4.5", "M4 13a8 8 0 0 0 14.3 4.3L20 15.5", "M20 20v-4.5h-4.5"],
    "warning": ["M10.3 4.6a2 2 0 0 1 3.4 0l7.4 12.8a2 2 0 0 1-1.7 3H4.6a2 2 0 0 1-1.7-3z", "M12 9.5v4", "M12 17h.01"],
    "done": ["c12 12 9", "M8 12.5l2.7 2.7L16 9.8"],
    "delete": ["M4 7h16", "M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2", "M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12", "M10 11v6", "M14 11v6"],
    "website": ["c12 12 9", "M3 12h18", "M12 3c2.5 2.5 3.8 5.5 3.8 9s-1.3 6.5-3.8 9c-2.5-2.5-3.8-5.5-3.8-9S9.5 5.5 12 3z"],
    "one-time-code": ["c12 13 8", "M12 9v4l2.5 2.5", "M9.5 2.5h5"],
    "recovery-kit": ["c12 12 9", "c12 12 3.5", "M5.6 5.6l3.9 3.9", "M14.5 14.5l3.9 3.9", "M18.4 5.6l-3.9 3.9", "M9.5 14.5l-3.9 3.9"],
    "sealed": ["M6 4h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z", "M4.6 4.6l4.8 4.8", "M19.4 4.6l-4.8 4.8", "M4.6 19.4l4.8-4.8", "M19.4 19.4l-4.8-4.8", "c12 12 2.6"],
    "lookalike": ["c10.5 10.5 7", "M3.5 10.5h14", "M10.5 3.5c1.9 1.9 2.9 4.3 2.9 7", "M10.5 3.5c-1.9 1.9-2.9 4.3-2.9 7s1 5.1 2.9 7", "M18.5 15v3", "M18.5 21h.01"],
}


def el(p):
    if p.startswith("c"):
        cx, cy, r = p[1:].split()
        return f'<circle cx="{cx}" cy="{cy}" r="{r}"/>'
    return f'<path d="{p}"/>'


ATTR = 'fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"'


def icon_svg(name):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" {ATTR} '
            f'aria-hidden="true">{"".join(el(p) for p in ICONS[name])}</svg>\n')


def main():
    root = Path(__file__).resolve().parent.parent / "icons"
    root.mkdir(exist_ok=True)
    for n in ICONS:
        (root / f"{n}.svg").write_text(icon_svg(n))
    sprite = "".join(f'<symbol id="af-{n}" viewBox="0 0 24 24">{"".join(el(p) for p in ICONS[n])}</symbol>' for n in ICONS)
    (root / "sprite.svg").write_text(f'<svg xmlns="http://www.w3.org/2000/svg" {ATTR} style="display:none">{sprite}</svg>\n')
    print(f"wrote {len(ICONS)} icons to {root}")


if __name__ == "__main__":
    main()
