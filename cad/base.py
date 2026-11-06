"""Wakecue lamp base: printable body + lid + TPU grip ring + TPU foot.

All sizes in mm. Natural orientation: z=0 is the underside of the base,
z=50 the top the globe sits on, -y is the front with the e-paper.

Two shapes:
  round  tapered drum, Ø160 at the bottom to Ø150 at the top, flat front
         that leans back ~5° so the clock faces you in bed
  rect   180 × 170 box with 12 mm corners, vertical front

Four menu buttons sit in a row under the e-paper window, on the flat front: Up, Down, Ok, Back,
left to right. They grow from the front wall (a bar inside it, one pocket and one cap hole per
button), so the row is on the face you are already looking at when you read the clock.

The front ballast cups sit just behind that bar, so the bar is only as deep as the switch stack.
The e-paper window is set high enough for the row to clear it.

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

WIN_W, WIN_H, WIN_Z = 50.0, 25.0, 30.5       # e-paper window, high enough to clear the buttons
EPD_W, EPD_H, EPD_T = 72.0, 30.0, 3.4         # WeAct 2.13 module + slack
EPD_Z0 = WIN_Z - EPD_H / 2
TOUCH_X, TOUCH_Z = 31.0, 8.5

# the touch mark: two tiny lamps, one lit and one dark, engraved into the wall where the pad sits.
# They sit 0.8 mm apart on purpose, so the pair reads as ONE mark for the pad and not as two
# indicator dots
LAMP_DX = 2.7          # half the distance between the two lamp centres
LAMP_R = 2.2           # bulb radius
LAMP_BASE_W, LAMP_BASE_H = 2.2, 1.1     # foot under the bulb
LAMP_RING = 0.9        # the dark bulb is a ring this wide: a 0.5 mm groove gets closed by gap fill
LAMP_ENGRAVE = 0.4     # how deep the mark is cut into the 2 mm front wall

# four menu buttons in a row under the window: Up, Down, Ok, Back, left to right
# (Mini Push Button 4-pin 6x6x5.5 mm: a 3.5 mm body and about 2 mm of stem)
BTN_ROW_X = (-18.0, -6.0, 6.0, 18.0)   # centres, mm from the middle
BTN_ROW_Z = 8.5                         # row centre above the underside
BTN_BLOCK_X = 23.5                      # half width of the solid bar behind the face
BTN_BLOCK_Z0, BTN_BLOCK_Z1 = 4.2, 14.5  # above the lid's plate (the bar leans with the wall), below the window
BTN_BLOCK_DEPTH = 9.5                   # face to the bar's back: the whole switch stack
BTN_BORE_D = 8.8           # switch pocket, wide enough for the 6x6 body (8.5 diagonal)
BTN_HOLE_D = 6.2           # through the wall, for the cap shaft
BTN_PROUD = 0.8            # how far the cap stands out of the face
BTN_CAP_D, BTN_FLANGE_D, BTN_FLANGE_T = 5.8, 8.2, 1.6
BTN_CAP_LEN = BTN_FLANGE_T + FRONT_WALL + BTN_PROUD   # flange base to the cap tip

USB_W, USB_Z_TOP = 9.6, 13.5
USB_PLUG_W, USB_PLUG_Z_TOP = 13.0, 15.5      # room for the plug's moulded grip
WASHER_R = 12.5      # ballast cups, Ø25 × 8: lead sinkers or steel nuts

R_BOT, R_TOP = 80.0, 75.0   # round only
if SHAPE == "round":
    FRONT_OUT = -66.0
    BACK_IN = R_BOT - WALL                # inner back wall at the bottom
    BOSSES = [(72 * math.cos(math.radians(a)), 72 * math.sin(math.radians(a)))
              for a in (45, 135, 225, 315)]
    FEET = [(68 * math.cos(math.radians(a)), 68 * math.sin(math.radians(a)))
            for a in (22.5, 112.5, 202.5, 292.5)]
    # the front pair sits behind the button bar, so it is pulled back from -45 to -40
    CUPS = [(-25, 45), (25, 45), (-26, -40), (26, -40)]
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


def ycyl(r, y0, y1, x=0.0, z=0.0):
    """Cylinder along +y from y0 to y1."""
    return Manifold.cylinder(y1 - y0, r).rotate((-90, 0, 0)).translate((x, y0, z))


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

    # menu buttons: a bar inside the flat front, under the window. Each button gets a pocket for
    # the switch body and a hole through the wall for its cap. The bar is shallow on purpose: the
    # front ballast cups sit about a millimetre behind it.
    bar = box(-BTN_BLOCK_X, BTN_BLOCK_X, FRONT_OUT, FRONT_OUT + BTN_BLOCK_DEPTH,
              BTN_BLOCK_Z0, BTN_BLOCK_Z1)
    b += tilt(bar) ^ clip
    for x in BTN_ROW_X:
        b -= tilt(ycyl(BTN_BORE_D / 2, FRONT_IN, FRONT_OUT + BTN_BLOCK_DEPTH, x, BTN_ROW_Z))
        b -= tilt(ycyl(BTN_HOLE_D / 2, FRONT_OUT - 1, FRONT_IN, x, BTN_ROW_Z))

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
    # touch pad mark: two lamps, one lit and one dark, engraved into the 2 mm wall where the pad
    # sits; the cut also thins the wall under the finger, which is what the sensor wants. The pad
    # has to clear BTN_BLOCK_X (inside it the wall is 9.5 mm thick) and the mark stays well inboard
    # of the tapered edge, where the face is 45 mm either side of the middle at this height
    cut0, cut1 = FRONT_OUT - 1.0, FRONT_OUT + LAMP_ENGRAVE
    bulb_z = TOUCH_Z + (LAMP_BASE_H - 0.35) / 2   # centre the whole mark on the pad
    def lamp(cx, lit):
        bulb = ycyl(LAMP_R, cut0, cut1, cx, bulb_z)
        if not lit:                            # dark: the bulb is left in outline
            bulb -= ycyl(LAMP_R - LAMP_RING, cut0 - 1, cut1 + 1, cx, bulb_z)
        # a wide little foot under the bulb, so the mark reads as a lamp on a stand rather than a dot
        base = box(cx - LAMP_BASE_W / 2, cx + LAMP_BASE_W / 2, cut0, cut1,
                   bulb_z - LAMP_R + 0.35 - LAMP_BASE_H, bulb_z - LAMP_R + 0.35)
        return bulb + base
    b -= tilt(lamp(TOUCH_X - LAMP_DX, True) + lamp(TOUCH_X + LAMP_DX, False))

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
    """Shaft through the wall, flange inside; printed flange down. The switch's stem pushes the
    flange, and the wall keeps the cap from falling out."""
    return cyl(BTN_CAP_D / 2, 0, BTN_CAP_LEN) + cyl(BTN_FLANGE_D / 2, 0, BTN_FLANGE_T)


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
        t = save(part, out / f"wakecue_{SHAPE}_{name}.stl")
        bb = t.bounds
        print(f"{SHAPE:5s} {name:9s} watertight={t.is_watertight} "
              f"vol={t.volume / 1000:6.1f} cm3 size={np.round(bb[1] - bb[0], 1)}")
    print(f"front tilt {TILT:.1f} deg")
