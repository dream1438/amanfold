"""Shape text with HarfBuzz and emit it as one SVG path (font-independent wordmark)."""
import sys, json, uharfbuzz as hb
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.boundsPen import BoundsPen
import io

def outline(fontpath, text, wght=None, size=100, tracking=0, rtl=False):
    tt = TTFont(fontpath)
    if wght and 'fvar' in tt:
        tt = instantiateVariableFont(tt, {'wght': wght}, inplace=False)
    buf = io.BytesIO(); tt.save(buf); data = buf.getvalue()
    tt = TTFont(io.BytesIO(data))
    face = hb.Face(data); font = hb.Font(face)
    upem = face.upem
    b = hb.Buffer(); b.add_str(text); b.guess_segment_properties()
    hb.shape(font, b, {"kern": True, "liga": True})
    gs = tt.getGlyphSet(); order = tt.getGlyphOrder()
    s = size / upem
    pen = SVGPathPen(gs, ntos=lambda v: ('%.2f' % v).rstrip('0').rstrip('.'))
    bp = BoundsPen(gs)
    x = 0
    infos, poss = b.glyph_infos, b.glyph_positions
    for i, (inf, pos) in enumerate(zip(infos, poss)):
        name = order[inf.codepoint]
        tp = TransformPen(pen, (s, 0, 0, -s, (x + pos.x_offset) * s, -pos.y_offset * s))
        gs[name].draw(tp)
        gs[name].draw(TransformPen(bp, (s, 0, 0, -s, (x + pos.x_offset) * s, -pos.y_offset * s)))
        x += pos.x_advance + (tracking * upem / 1000 if i < len(infos) - 1 else 0)
    asc = tt['hhea'].ascent * s; desc = tt['hhea'].descent * s
    xh = getattr(tt['OS/2'], 'sxHeight', 0) * s; cap = getattr(tt['OS/2'], 'sCapHeight', 0) * s
    x0, y0, x1, y1 = bp.bounds  # y is already flipped: y0 = top (negative), y1 = bottom
    return {"d": pen.getCommands(), "bbox": (x0, y0, x1, y1), "width": x * s, "ascent": asc, "descent": desc, "xheight": xh, "cap": cap}


def word(fontpath, text, wght=600, tracking=0, single_storey_a=False, kern=None):
    """Outline a word in font units (y down, baseline at 0).

    single_storey_a redraws every Latin "a" as the font's "d" cropped at the x-height, which gives a round
    one-storey "a" with exactly the font's own weight and curves, matching the symbol.
    tracking and kern (a dict of letter pairs) are in font units, 1000 to the em.
    """
    from fontTools.pens.recordingPen import RecordingPen
    tt = TTFont(fontpath)
    if 'fvar' in tt:
        axes = {a.axisTag: a.defaultValue for a in tt['fvar'].axes}
        axes['wght'] = wght
        tt = instantiateVariableFont(tt, axes, inplace=False)
    buf = io.BytesIO(); tt.save(buf); data = buf.getvalue()
    tt = TTFont(io.BytesIO(data))
    top = max(y for _, y in tt['glyf']['d'].coordinates)
    xh = tt['OS/2'].sxHeight
    custom = [single_storey_a and ch == 'a' for ch in text]
    shaped = ''.join('d' if c else ch for ch, c in zip(text, custom))
    font = hb.Font(hb.Face(data)); b = hb.Buffer(); b.add_str(shaped); b.guess_segment_properties()
    hb.shape(font, b, {"kern": True, "liga": True})
    gs = tt.getGlyphSet(); order = tt.getGlyphOrder()
    pen = SVGPathPen(gs, ntos=lambda v: ('%.1f' % v).rstrip('0').rstrip('.')); bp = BoundsPen(gs)
    x = 0; kern = kern or {}; infos = b.glyph_infos
    for i, (inf, pos) in enumerate(zip(infos, b.glyph_positions)):
        rec = RecordingPen(); gs[order[inf.codepoint]].draw(rec)
        if single_storey_a and inf.cluster < len(custom) and custom[inf.cluster]:
            rec.value = [(op, tuple((px, xh) if py == top else (px, py) for px, py in args)) for op, args in rec.value]
        for p in (pen, bp):
            rec.replay(TransformPen(p, (1, 0, 0, -1, x + pos.x_offset, -pos.y_offset)))
        x += pos.x_advance
        if i < len(infos) - 1:
            x += tracking + kern.get(shaped[i:i + 2], 0)
    return {"d": pen.getCommands(), "bbox": bp.bounds, "ascender": top, "xheight": xh}

