"""Turn the names engraved on the base into outlines, so base.py needs no font tool.

    python make_names.py            writes names.json next to this file

The outlines are DejaVu Sans Bold, scaled so a capital letter is CAP_H mm tall, with the baseline at
v = 0 and the text starting at u = 0. Each name is a list of closed contours; base.py fills them with
the even-odd rule, so the holes in an "a" or a "b" come out right. Needs matplotlib, only to run this.
Edit NAMES to change what is engraved, run it, then run base.py.
"""
import json
from pathlib import Path

from matplotlib.font_manager import FontProperties
from matplotlib.textpath import TextPath

CAP_H = 7.0
NAMES = {"left": "Ibrahim", "right": "Aya"}

prop = FontProperties(family="DejaVu Sans", weight="bold")
capital = TextPath((0, 0), "I", size=100, prop=prop).get_extents()
scale = CAP_H / capital.height * 100 / 100      # glyph units per size unit, so that "I" is CAP_H tall
out = {"cap_h": CAP_H}
for side, text in NAMES.items():
    tp = TextPath((0, 0), text, size=100, prop=prop)
    k = CAP_H / capital.height
    polys = [[[round(float(x) * k, 3), round(float(y) * k, 3)] for x, y in poly] for poly in tp.to_polygons(closed_only=True)]
    xs = [p[0] for poly in polys for p in poly]
    out[side] = {"text": text, "polys": polys, "width": round(max(xs) - min(xs), 2), "u0": round(min(xs), 3)}
    print(f"{side:5s} {text!r}: {len(polys)} contours, {out[side]['width']} mm wide, capital {CAP_H} mm")
(Path(__file__).parent / "names.json").write_text(json.dumps(out))
