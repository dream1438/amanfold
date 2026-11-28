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

if __name__ == '__main__':
    print(json.dumps(outline(sys.argv[1], sys.argv[2], float(sys.argv[3]), float(sys.argv[4]), float(sys.argv[5]))))
