"""Find where the MP3 board can sit: turn its keep-out about the body's axis and report, for each turn,
what else in the base it touches (the lid's features, and the body's features inside the cavity; the
wall's own curve is not a collision, so the keep-out is cut to the cavity first).

    python player_search.py [from to step]"""
import importlib.util, sys
args = sys.argv[1:]
lo, hi, st = (int(a) for a in args[:3]) if len(args) >= 3 else (-180, 180, 5)
sys.argv = ["base.py", "round"]
spec = importlib.util.spec_from_file_location("base", __file__.replace("player_search.py", "base.py"))
base = importlib.util.module_from_spec(spec); spec.loader.exec_module(base)
from manifold3d import Manifold
base.player_cradle = lambda: Manifold()      # test against the rest of the lid, not against the cradle itself
base.card_window = lambda: Manifold()        # and the body without the window
body, lid = base.body(), base.lid()
# things that sit in the lid's frames and cups: they fill them, so the frame rims alone understate them
holder = base.box(-62.5, -40.5, -39.0, 39.0, base.LID_T, base.LID_T + 22)        # 18650 holder
ds3231 = base.box(40.5, 63.5, -19.5, 19.5, base.LID_T, base.LID_T + 12)         # clock board
tp4056 = base.box(-9, 9, base.BACK_IN - 31, base.BACK_IN - 2, base.LID_T, base.LID_T + 12)  # charger
ballast = Manifold.compose([base.cyl(base.WASHER_R, base.LID_T, base.LID_T + 8, x, y) for x, y in base.CUPS])
lid = lid + holder + ds3231 + tp4056 + ballast
cav_scale = 1 - (1 - base.TOP_SCALE) * (base.H - base.TOP) / base.H
cav = base.inner_section().extrude(base.H - base.TOP, scale_top=(cav_scale, cav_scale))
ok = []
for a in range(lo, hi, st):
    ko = base.player_place(base.player_keepout(), a) ^ cav
    hit_lid, hit_body = ko ^ lid, ko ^ body
    v_lid, v_body = hit_lid.volume(), hit_body.volume()
    free = v_lid < 1e-3 and v_body < 1e-3
    if free:
        ok.append(a)
    where = ""
    if not free:
        for name, h in (("lid", hit_lid), ("body", hit_body)):
            if h.volume() > 1e-3:
                bb = h.bounding_box()
                where += f" | {name} x {bb[0]:5.1f}..{bb[3]:5.1f} y {bb[1]:5.1f}..{bb[4]:5.1f} z {bb[2]:4.1f}..{bb[5]:4.1f}"
    print(f"{a:5d}  lid {v_lid:8.1f}  body {v_body:8.1f}  {'FREE' if free else '    '}{where}")
print("free turns:", ok)
