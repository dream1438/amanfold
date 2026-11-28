"""Geometry of the Amanfold symbol: a lowercase "a" whose counter is a keyhole, with a folded top corner.

64 unit grid. The bowl is a circle of radius 24 centred on (30, 32). The stem is 12 units wide and
runs from x 44 to 56; its flat ends sit 0.8 units inside the bowl so both look the same height.
The fold spans the full stem width. The keyhole's head is one third of the bowl's diameter.

Three drawings:
  full    two colours: body plus a lighter folded corner (32 px and above)
  crease  one colour: the fold is a knocked-out line (print, stamps, themed icons)
  small   one colour: plain cut corner and a larger keyhole (24 px and below)
"""
from math import sqrt, hypot

CX, CY, R = 30, 32, 24          # bowl
STEM_R, STEM_W = 56, 12         # stem right edge, stem width
TOP, BOTTOM = 8.8, 55.2         # flat ends of the stem
RADIUS = 2                      # outer corner radius
KEYHOLE = dict(kx=30, ky=27, kr=8, nh=3.6, bh=6.3, kb=47, kc=1.5)
KEYHOLE_SMALL = dict(kx=30, ky=27, kr=9.2, nh=4.5, bh=7.2, kb=47.8, kc=1.5)
BOX = (6, 8, 56, 56)            # the symbol's bounding box: x0, y0, x1, y1


def f(v):
    return ("%.2f" % v).rstrip("0").rstrip(".")


def keyhole(kx, ky, kr, nh, bh, kb, kc):
    """Round head (centre kx,ky radius kr), flaring to half-width bh at y=kb; nh is the neck half-width."""
    ny = ky + sqrt(kr * kr - nh * nh)
    dx, dy = -(bh - nh), kb - ny
    length = hypot(dx, dy)
    ux, uy = dx / length, dy / length
    ax, ay = kx - bh - ux * kc, kb - uy * kc
    return (f"M{f(kx)} {f(ky - kr)}a{f(kr)} {f(kr)} 0 0 0 {f(-nh)} {f(ny - (ky - kr))}"
            f"L{f(ax)} {f(ay)}Q{f(kx - bh)} {f(kb)} {f(kx - bh + kc)} {f(kb)}H{f(kx + bh - kc)}"
            f"Q{f(kx + bh)} {f(kb)} {f(kx + bh + ux * kc)} {f(ay)}"
            f"L{f(kx + nh)} {f(ny)}A{f(kr)} {f(kr)} 0 0 0 {f(kx)} {f(ky - kr)}Z")


def symbol(kind="full"):
    """Return (body path, flap path or ''). Fill the body with fill-rule evenodd."""
    xl, xr, fold, r = STEM_R - STEM_W, STEM_R, STEM_W, RADIUS
    dy = sqrt(R * R - (xl - CX) ** 2)
    body = (f"M{f(xl)} {f(TOP)}L{f(xr)} {f(TOP + fold)}V{f(BOTTOM - r)}a{f(r)} {f(r)} 0 0 1 {f(-r)} {f(r)}"
            f"H{f(xl)}V{f(CY + dy)}A{R} {R} 0 1 1 {f(xl)} {f(CY - dy)}Z")
    body += keyhole(**(KEYHOLE_SMALL if kind == "small" else KEYHOLE))
    flap = ""
    y0 = TOP + fold
    if kind == "full":
        flap = f"M{f(xl)} {f(TOP)}V{f(y0 - r)}a{f(r)} {f(r)} 0 0 0 {f(r)} {f(r)}H{f(xr)}Z"
    elif kind == "crease":
        t = 1.5
        body += (f"M{f(xl)} {f(TOP)}L{f(xl + t)} {f(TOP + t)}V{f(y0 - t - 2)}a2 2 0 0 0 2 2H{f(xr - t)}"
                 f"L{f(xr)} {f(y0)}H{f(xl + 2.6)}a2.6 2.6 0 0 1-2.6-2.6Z")
    return body, flap


def paint(v, cls=None):
    """fill attribute for a literal colour, inline style for a CSS variable, class for themed pages."""
    if cls:
        return f'class="{cls}"'
    return f'style="fill:{v}"' if v.startswith("var(") else f'fill="{v}"'


def symbol_group(body, flap=None, kind=None, classes=False):
    """SVG fragment on the 64 grid. flap=None picks the one-colour crease drawing."""
    kind = kind or ("full" if flap else "crease")
    b, fl = symbol(kind)
    out = f'<path {paint(body, "mb" if classes else None)} fill-rule="evenodd" d="{b}"/>'
    if fl:
        out += f'<path {paint(flap, "mf" if classes else None)} d="{fl}"/>'
    return out
