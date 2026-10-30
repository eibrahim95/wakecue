"""Fajr Lamp base: printable body + lid + TPU grip ring + TPU foot.

All sizes in mm. Natural orientation: z=0 is the underside of the base,
z=50 the top the globe sits on, -y is the front with the e-paper.

Two shapes:
  round  tapered drum, Ø160 at the bottom to Ø150 at the top, flat front
         that leans back ~5° so the clock faces you in bed
  rect   180 × 170 box with 12 mm corners, vertical front

Four menu buttons sit in a column on the right-hand side towards the back (30 degrees
round from the right), each under a small printed cap that passes through a hole in a flat
panel cut into the wall. The spot is chosen to clear the DS3231 frame and the ballast cups.

Print the body upside down (top face on the bed): every internal feature
grows from the top plate or the walls, so it needs no supports. The button
block lies on its side then, which prints cleanly as a plain bridge-free block.
Print the lid flat, the grip ring and feet in TPU, and four button caps.

Run:  python base.py round|rect [NECK_D]
"""
import math
import sys
from pathlib import Path

import numpy as np
import trimesh
from manifold3d import CrossSection, JoinType, Manifold, set_circular_segments

set_circular_segments(256)

SHAPE = sys.argv[1] if len(sys.argv) > 1 else "round"
NECK_D = float(sys.argv[2]) if len(sys.argv) > 2 else 80.0  # measure your globe

# --- shared parameters ------------------------------------------------------
H = 50.0
WALL = 3.0
TOP = 3.0
FRONT_WALL = 2.0     # thin front wall: e-paper window + touch sensing
LID_T = 3.0
CLR = 0.25

LIGHT_R = 33.0       # light hole (ring LEDs sit at r ~ 27-32)
RING_R = 34.0        # 16-LED ring, 68 mm
GRIP_T = 1.6         # TPU grip ring wall
SEAT_R = NECK_D / 2 + GRIP_T
SEAT_DEPTH = 8.0
COLLAR_Z0 = H - SEAT_DEPTH - 3.0   # 3 mm floor under the seat

WIN_W, WIN_H, WIN_Z = 50.0, 25.0, 24.0       # e-paper window
EPD_W, EPD_H, EPD_T = 72.0, 30.0, 3.4         # WeAct 2.13 module + slack
EPD_Z0 = WIN_Z - EPD_H / 2
TOUCH_X = 40.0

# four menu buttons (Mini Push Button 4-pin 6x6x5.5 mm: a 3.5 mm body and about 2 mm of stem, to be checked on the real part)
BTN_Z = (12.5, 22.5, 32.5, 42.5)   # centres, bottom (Back) to top, then Down, Ok, Up
BTN_ANGLE = 30.0 if SHAPE == "round" else 0.0   # turned about the centre; the column points straight out of the wall
BTN_BLOCK_X0 = 60.0        # inner face of the solid block the switches sit in
BTN_BORE_D = 8.8           # switch pocket, wide enough for the 6x6 body (8.5 diagonal) and the cap flange
BTN_HOLE_D = 6.2           # through the wall, for the cap shaft
BTN_SHOULDER_X = 69.0      # where the pocket ends and the wall hole starts
BTN_SW_BODY = 3.5
BTN_SW_STEM = 2.0
BTN_TIP_X = BTN_BLOCK_X0 + BTN_SW_BODY + BTN_SW_STEM   # where the stem tip rests
BTN_CAP_D, BTN_FLANGE_D, BTN_FLANGE_T, BTN_PROUD = 5.8, 8.2, 1.6, 0.8
BTN_BLOCK_Y = 9.5          # half width of the block and the flat panel

USB_W, USB_Z_TOP = 9.6, 13.5
USB_PLUG_W, USB_PLUG_Z_TOP = 13.0, 15.5      # room for the plug's moulded grip
WASHER_R = 12.5      # ballast cups, Ø25 × 8: lead sinkers or steel nuts

R_BOT, R_TOP = 80.0, 75.0   # round only
BTN_PANEL_X = 74.5 if SHAPE == "round" else 89.0   # the flat the caps stand proud of
if SHAPE == "round":
    FRONT_OUT = -66.0
    BACK_IN = R_BOT - WALL                # inner back wall at the bottom
    BOSSES = [(72 * math.cos(math.radians(a)), 72 * math.sin(math.radians(a)))
              for a in (45, 135, 225, 315)]
    FEET = [(68 * math.cos(math.radians(a)), 68 * math.sin(math.radians(a)))
            for a in (22.5, 112.5, 202.5, 292.5)]
    CUPS = [(-25, 45), (25, 45), (-20, -45), (20, -45)]
else:
    W, D, CR = 180.0, 170.0, 12.0
    FRONT_OUT = -D / 2
    BACK_IN = D / 2 - WALL
    BOSSES = [(sx * 81, sy * 76) for sx in (-1, 1) for sy in (-1, 1)]
    FEET = [(sx * 75, sy * 62) for sx in (-1, 1) for sy in (-1, 1)]
    CUPS = [(sx * 55, sy * 55) for sx in (-1, 1) for sy in (-1, 1)]

FRONT_IN = FRONT_OUT + FRONT_WALL
TOP_SCALE = (R_TOP / R_BOT) if SHAPE == "round" else 1.0
# the front leans back by the same amount the taper pulls it in
TILT = math.degrees(math.atan(-FRONT_IN * (1 - TOP_SCALE) / H)) if SHAPE == "round" else 0.0


# --- helpers ----------------------------------------------------------------
def box(x0, x1, y0, y1, z0, z1):
    return Manifold.cube((x1 - x0, y1 - y0, z1 - z0)).translate((x0, y0, z0))


def cyl(r, z0, z1, x=0.0, y=0.0):
    return Manifold.cylinder(z1 - z0, r).translate((x, y, z0))


def flat_circle(r, front_y):
    big = 1000.0
    return CrossSection.circle(r) ^ CrossSection.square((big, big)).translate((-big / 2, front_y))


def rounded_rect(w, d, r):
    core = CrossSection.square((w - 2 * r, d - 2 * r), center=True)
    return core.offset(r, JoinType.Round)


def xcyl(r, x0, x1, y=0.0, z=0.0):
    """Cylinder along +x from x0 to x1."""
    return Manifold.cylinder(x1 - x0, r).rotate((0, 90, 0)).translate((x0, y, z))


def tilt(m):
    """Lean a front feature back with the front wall (pivot at its foot)."""
    if TILT == 0:
        return m
    return m.translate((0, -FRONT_IN, 0)).rotate((-TILT, 0, 0)).translate((0, FRONT_IN, 0))


def outer_section():
    if SHAPE == "round":
        return flat_circle(R_BOT, FRONT_OUT)
    return rounded_rect(W, D, CR)


def inner_section(shrink=0.0):
    if SHAPE == "round":
        return flat_circle(R_BOT - WALL - shrink, FRONT_IN + shrink)
    return rounded_rect(W - 2 * WALL - 2 * shrink, D - 2 * WALL - 2 * shrink, CR - WALL)


# --- body -------------------------------------------------------------------
def body():
    shell = outer_section().extrude(H, scale_top=(TOP_SCALE, TOP_SCALE))
    # internal features are clipped 0.8 mm inside the skin, so no face of theirs
    # coincides with the outer surface (that leaves slivers in the STL)
    clip = outer_section().offset(-0.8, JoinType.Round).extrude(
        H - 0.5, scale_top=(TOP_SCALE, TOP_SCALE))
    cav_scale = 1 - (1 - TOP_SCALE) * (H - TOP) / H
    cavity = inner_section().extrude(H - TOP, scale_top=(cav_scale, cav_scale))
    b = shell - cavity

    # globe seat: collar under the top, 8 mm seat, light hole, ring locating skirt
    b += cyl(SEAT_R + 5, COLLAR_Z0, H - TOP)
    b += cyl(RING_R + 2.5, COLLAR_Z0 - 2.5, COLLAR_Z0) - cyl(RING_R + 0.5, 0, H)
    b -= cyl(SEAT_R, H - SEAT_DEPTH, H + 1)
    b -= cyl(LIGHT_R, 0, H + 1)

    # lid screw bosses, tied to the wall
    for x, y in BOSSES:
        ang = math.degrees(math.atan2(y, x))
        rib = box(-4, 4, 0, 14, LID_T, H - TOP).rotate((0, 0, ang - 90)).translate((x, y, 0))
        b += (cyl(4.0, LID_T, H - TOP, x, y) + rib) ^ clip
        b -= cyl(1.25, LID_T - 1, LID_T + 12, x, y)          # M3 self-tapper pilot

    # menu buttons: a solid block on the right side, a flat panel cut into the wall, then per button a
    # pocket for the switch, a wider bore for the cap flange and a hole for the cap shaft
    z_lo, z_hi = BTN_Z[0] - BTN_BLOCK_Y + 0.5, BTN_Z[-1] + BTN_BLOCK_Y - 0.5
    def turn(m):
        return m.rotate((0, 0, BTN_ANGLE))
    block = box(BTN_BLOCK_X0, 100, -BTN_BLOCK_Y, BTN_BLOCK_Y, z_lo - 0.2, z_hi + 0.2)
    b += turn(block) ^ clip
    b -= turn(box(BTN_PANEL_X, 110, -BTN_BLOCK_Y, BTN_BLOCK_Y, z_lo, z_hi))
    for z in BTN_Z:
        b -= turn(xcyl(BTN_BORE_D / 2, BTN_BLOCK_X0 - 1, BTN_SHOULDER_X, 0, z))
        b -= turn(xcyl(BTN_HOLE_D / 2, BTN_SHOULDER_X - 0.5, 110, 0, z))

    # e-paper window, rails with back lips, top stops, touch ring (lean with the wall)
    front = Manifold()
    half = EPD_W / 2 + CLR
    for s in (-1, 1):
        rail = box(half, half + 1.6, FRONT_IN, FRONT_IN + EPD_T + 1.6, EPD_Z0, H)
        lip = box(half - 2.5, half + 1.6, FRONT_IN + EPD_T, FRONT_IN + EPD_T + 1.6, EPD_Z0, H)
        stop = box(half - 6, half, FRONT_IN, FRONT_IN + EPD_T, EPD_Z0 + EPD_H + CLR, H)
        part = rail + lip + stop
        front += part if s > 0 else part.mirror((1, 0, 0))
    b += tilt(front) ^ clip
    b -= tilt(box(-WIN_W / 2, WIN_W / 2, FRONT_OUT - 3, FRONT_IN + 0.01,
                  WIN_Z - WIN_H / 2, WIN_Z + WIN_H / 2))
    ring = (cyl(3.0, 0, 1) - cyl(2.2, -1, 2)).rotate((90, 0, 0))
    b -= tilt(ring.translate((TOUCH_X, FRONT_OUT + 0.6, WIN_Z)))

    # TP4056: USB-C notch open to the underside, guides that stop the board
    b -= box(-USB_W / 2, USB_W / 2, BACK_IN - 2, BACK_IN + WALL + 10, -1, USB_Z_TOP)
    b -= box(-USB_PLUG_W / 2, USB_PLUG_W / 2, BACK_IN + 0.5, BACK_IN + WALL + 10,
             -1, USB_PLUG_Z_TOP)
    for s in (-1, 1):
        g = box(8.0, 10.0, BACK_IN - 31, BACK_IN + 1, 10.0, H)
        b += (g if s > 0 else g.mirror((1, 0, 0))) ^ clip
    return b


# --- lid --------------------------------------------------------------------
def lid():
    m = inner_section(shrink=CLR + 0.3).extrude(LID_T)
    holes = []
    for j in range(-7, 8):
        for i in range(-7, 8):
            x, y = i * 5.0 + (2.5 if j % 2 else 0), j * 5.0 * 0.866
            if math.hypot(x, y) <= 28:
                holes.append(cyl(1.5, -1, LID_T + 1, x, y))
    m -= Manifold.compose(holes)                                  # speaker grille
    m += cyl(34.8, LID_T, LID_T + 4) - cyl(33.3, 0, LID_T + 5)    # speaker locator
    for x in (-32.5, 32.5):                                       # 5x7 cm perfboard, M3 self-tapper pilot
        for y in (-22.5, 22.5):
            m += cyl(3.0, LID_T, 25.0, x, y) - cyl(1.25, 10, 26, x, y)
    for x in (-33.0, 33.0):                                       # hold the e-paper up
        m += tilt(box(x - 2, x + 2, FRONT_IN + CLR + 0.3, FRONT_IN + EPD_T - 0.3,
                      LID_T - 0.5, EPD_Z0))
    m += box(-6, 6, BACK_IN - 27, BACK_IN - 5, LID_T, 7.8)        # TP4056 platform

    def frame(x0, x1, y0, y1, h=2.0, t=1.2):
        return box(x0 - t, x1 + t, y0 - t, y1 + t, LID_T, LID_T + h) - box(x0, x1, y0, y1, 0, 99)
    m += frame(-62.5, -40.5, -39.0, 39.0)                         # 18650 holder
    m += frame(40.5, 63.5, -19.5, 19.5)                           # DS3231
    for x, y in CUPS:                                             # ballast: 3 M12 washers each
        m += cyl(WASHER_R + 1.2, LID_T, LID_T + 8, x, y) - cyl(WASHER_R, LID_T, 99, x, y)
    for x, y in BOSSES:                                           # countersunk M3
        m -= cyl(1.7, -1, LID_T + 1, x, y)
        m -= Manifold.cylinder(1.8, 3.2, 1.7).translate((x, y, -0.01))
    for x, y in FEET:
        m -= cyl(6.2, -1, 1.0, x, y)
    return m


def grip_ring():
    """TPU ring that lines the seat and grips the globe neck."""
    return cyl(SEAT_R - 0.1, 0, SEAT_DEPTH - 0.5) - cyl(NECK_D / 2 - 0.3, -1, SEAT_DEPTH)


def button_cap():
    """Shaft through the wall, flange inside; printed flange down. Its inner end rests on the switch stem."""
    length = BTN_PANEL_X + BTN_PROUD - BTN_TIP_X
    return cyl(BTN_FLANGE_D / 2, 0, BTN_FLANGE_T) + cyl(BTN_CAP_D / 2, 0, length)


def foot():
    return Manifold.cylinder(7.0, 6.0, 5.0)


def save(m, path):
    mesh = m.to_mesh()
    t = trimesh.Trimesh(vertices=mesh.vert_properties[:, :3], faces=mesh.tri_verts)
    t.export(path)
    return t


if __name__ == "__main__":
    out = Path(__file__).parent
    parts = (("body", body()), ("lid", lid()), ("grip_tpu", grip_ring()), ("foot_tpu", foot()),
             ("button_cap", button_cap()))
    for name, part in parts:
        t = save(part, out / f"fajr_{SHAPE}_{name}.stl")
        bb = t.bounds
        print(f"{SHAPE:5s} {name:9s} watertight={t.is_watertight} "
              f"vol={t.volume / 1000:6.1f} cm3 size={np.round(bb[1] - bb[0], 1)}")
    print(f"front tilt {TILT:.1f} deg")
