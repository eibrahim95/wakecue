"""Wakecue lamp base: printable body + lid + bayonet ring + TPU foot.

All sizes in mm. Natural orientation: z=0 is the underside of the base,
z=50 the top the globe sits on, -y is the front with the e-paper.

Two shapes:
  round  tapered drum, Ø160 at the bottom to Ø150 at the top, flat front
         that leans back ~5° so the clock faces you in bed
  rect   180 × 170 box with 12 mm corners, vertical front

Two names are engraved on the curved wall just past each end of the flat front (name_engraving(); the
outlines are in names.json, made by make_names.py, so this file needs no font tool).

Four menu buttons sit in a row under the e-paper window, on the flat front: Up, Down, Ok, Back,
left to right. They grow from the front wall (a bar inside it, one pocket and one cap hole per
button), so the row is on the face you are already looking at when you read the clock.

The front ballast cups sit just behind that bar, so the bar is only as deep as the switch stack.
The e-paper window is set high enough for the row to clear it.

The MP3 board (DFPlayer Mini) sits at the back, flat on the lid on four posts, with its microSD slot
pointing at the wall. A window through the wall lines up with the slot, so the card goes in and comes
out from outside without opening the lamp. See the PLAYER_* numbers: the board's size is the
datasheet's guess until it is measured on the real board.

Print the body upside down (top face on the bed): every internal feature
grows from the top plate or the walls, so it needs no supports. The button
block lies on its side then, which prints cleanly as a plain bridge-free block.
Print the lid flat, the bayonet ring as exported (it is already upside down, lugs flat on the bed, no
supports), the feet in TPU, and four button caps.

Run:  python base.py round|rect [FLANGE_D]

FLANGE_D is the diameter of the notched ring at the very end of the globe's neck, in mm. The default,
97, is the globe in hand: an end ring 97 mm across with three notches, a plain cylinder of 86.6 mm
above it, 81 mm clear inside, 165 g, 20 cm wide and tall.

The globe is held by a bayonet. A separate rigid ring drops into the seat and carries three lugs on
its inside, printed with it as one piece. The notches in the globe's end ring let the lugs through;
the globe is turned about 55 degrees, and the end ring, which thickens from about 3 mm at the notch
edges to about 5 mm, wedges under the lugs, which leave a gap of LUG_GAP above the seat floor. The
ring is a plain insert, so a wrong guess costs one small ring and never the body: print the test
plate (three rings with different gaps) and keep the one that turns smoothly and holds.
"""
import json
import math
import sys
from pathlib import Path

import numpy as np
import trimesh
from manifold3d import CrossSection, FillRule, JoinType, Manifold, set_circular_segments

set_circular_segments(256)

SHAPE = sys.argv[1] if len(sys.argv) > 1 else "round"

# --- the globe's neck, measured 13 Nov (A to D from the measuring page) ---------
FLANGE_D = float(sys.argv[2]) if len(sys.argv) > 2 else 97.0   # A: the notched ring at the very end
FLANGE_H = 5.0      # B: measured 3 to 5 mm, and the lugs ramp about 2 mm between notches; take the most
NECK_D = 86.6       # C: the plain cylinder above the ring
NECK_H = 15.0       # D: 13 to 15 mm of straight cylinder, so the glass starts well above the seat

# --- the bayonet ring, from the notch numbers measured 14 Nov ---------------------
NOTCH_W_OUT = 10.0   # notch width at the outer edge of the end ring
NOTCH_W_IN = 5.0     # and at the bottom of the notch
NOTCH_DEPTH = 5.0    # from the outer edge inwards, which is 3 mm clear of the bore and meets the neck
LUG_T = 3.0          # lug thickness: its top is the ring's top, so the ring prints upside down on it
LUG_GAP = 4.1        # the gap under a lug. The end ring is about 3 mm thick at the notch edges and
                     # about 5 mm at its thickest, and the black fitting's lug is thought to allow 4 mm
LUG_CLR = 0.8        # each side, between a lug and the notch walls
LUG_TIP_CLR = 0.5    # the lug stops this far above the notch floor (and 0.7 mm off the neck)
RING_BORE_CLR = 0.4  # the ring's bore radius is the end ring's radius plus this
RING_WALL = 2.0
KEY_W, KEY_OUT = 3.0, 1.2   # three tabs on the ring's outside that sit in slots in the seat wall
SEAT_CLR = 0.25      # radial play between the ring and the seat
TEST_GAPS = (3.8, 4.1, 4.4)  # the test plate: tight, middle, loose

# --- shared parameters ------------------------------------------------------
H = 50.0
WALL = 3.0
TOP = 3.0
FRONT_WALL = 2.0     # thin front wall: e-paper window + touch sensing
LID_T = 3.0
CLR = 0.25

LIGHT_R = 33.0       # light hole (ring LEDs sit at r ~ 27-32)
RING_R = 34.0        # 16-LED ring, 68 mm
RING_R_BORE = FLANGE_D / 2 + RING_BORE_CLR      # the bayonet ring's bore
RING_R_OUT = RING_R_BORE + RING_WALL             # and its outside
SEAT_R = RING_R_OUT + SEAT_CLR                   # the body's seat bore, which the ring drops into
SEAT_DEPTH = FLANGE_H + 3.0                      # taller than the ring (LUG_GAP + LUG_T, about 7 mm)
COLLAR_Z0 = H - SEAT_DEPTH - 3.0   # 3 mm floor under the seat

WIN_W, WIN_H, WIN_Z = 50.0, 25.0, 30.5       # e-paper window, high enough to clear the buttons
EPD_W, EPD_H, EPD_T = 72.0, 30.0, 3.4         # WeAct 2.13 module + slack
EPD_Z0 = WIN_Z - EPD_H / 2
TOUCH_X, TOUCH_Z = 31.0, 8.5

# the touch mark: one small lamp, a solid bulb on a foot, engraved into the wall where the pad sits
LAMP_R = 2.2           # bulb radius
LAMP_BASE_W, LAMP_BASE_H = 2.2, 1.1     # foot under the bulb
LAMP_ENGRAVE = 0.4     # how deep the mark is cut into the 2 mm front wall

# the two names, engraved on the curved wall just past each end of the flat front (round only). The
# outlines are in names.json, made by make_names.py. Each side has a stretch of wall as long as the
# longer name, starting NAME_GAP from the flat; a shorter name is centred in its stretch. Both wrap round
# the wall so they read left to right as you look at them
NAMES_FILE = Path(__file__).parent / "names.json"
NAME_Z = 24.0          # height of the middle of the capital letters above the underside
NAME_GAP = 3.0         # plain wall between the flat front and the nearest letter
NAME_DEPTH = 0.5       # how deep the letters are cut into the 3 mm wall

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
WASHER_R = 12.5      # ballast cups, Ø25: lead sinkers or steel nuts
CUP_H = 10.7         # round: three cups this deep hold what four 8 mm ones did (3 x 10.7 = 4 x 8)

# the MP3 board (DFPlayer Mini HW-247A), lying flat, card slot toward the back wall. Measured with
# calipers, 13 Nov: 20 x 20 mm, and the card socket 5.4 mm above the 1.6 mm PCB, which is 7 mm from the
# PCB's underside to the top of the socket with the pin legs out. A card pushed home stands about 3 mm
# proud of the body, so the window is for pinching it, not for reaching in.
PLAYER_W, PLAYER_L = 20.0, 20.0         # across the pin rows, and along the card's way in
PLAYER_CLR = 0.4                        # each side
PLAYER_POST_H = 3.0                     # the board's underside above the lid (chip and solder)
PLAYER_PCB_T = 1.6
PLAYER_SOCKET_H = 5.4                   # microSD socket above the board, measured 13 Nov
CARD_WIN_W, CARD_WIN_H = 14.0, 7.0      # the window in the wall: the card is 11 x 1 mm, the rest is for a fingernail
PLAYER_WIRE_SIDE = 3.0                  # room each side of the board for the wires soldered to its pads
PLAYER_WIRE_BACK = 3.0                  # and behind it
# where it sits: a turn about the body's axis, 0 = straight at the back; the board's slot edge is put at the
# inner wall. Found by player_search.py, which checks the keep-out against everything else in the base.
PLAYER_ANGLE = -30.0     # back-right, the card slot facing the wall straight on

R_BOT, R_TOP = 80.0, 75.0   # round only
if SHAPE == "round":
    FRONT_OUT = -66.0
    BACK_IN = R_BOT - WALL                # inner back wall at the bottom
    BOSSES = [(72 * math.cos(math.radians(a)), 72 * math.sin(math.radians(a)))
              for a in (45, 135, 225, 315)]
    FEET = [(68 * math.cos(math.radians(a)), 68 * math.sin(math.radians(a)))
            for a in (22.5, 112.5, 202.5, 292.5)]
    # the front pair sits behind the button bar, so it is pulled back from -45 to -40. The back
    # right cup is gone: the MP3 board sits there, and the three left are deeper to carry its share
    CUPS = [(-25, 45), (-26, -40), (26, -40)]
else:
    W, D, CR = 180.0, 170.0, 12.0
    FRONT_OUT = -D / 2
    BACK_IN = D / 2 - WALL
    BOSSES = [(sx * 81, sy * 76) for sx in (-1, 1) for sy in (-1, 1)]
    FEET = [(sx * 75, sy * 62) for sx in (-1, 1) for sy in (-1, 1)]
    CUPS = [(sx * 55, sy * 55) for sx in (-1, 1) for sy in (-1, 1)]
    CUP_H = 8.0

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


# --- the MP3 board ------------------------------------------------------------
def inner_radius_at(z):
    """The cavity's wall radius at height z (round body)."""
    cav_scale = 1 - (1 - TOP_SCALE) * (H - TOP) / H
    return (R_BOT - WALL) * (1 - (1 - cav_scale) * z / (H - TOP))


def player_place(m, angle=None):
    """Move a shape drawn with the board centred on the origin and its card slot toward +y to where
    the board sits: turned about the axis by `angle`, slot edge a hair inside the wall."""
    a = PLAYER_ANGLE if angle is None else angle
    z_mid = LID_T + PLAYER_POST_H + PLAYER_PCB_T + PLAYER_SOCKET_H / 2
    centre = inner_radius_at(z_mid) - PLAYER_L / 2 - 0.3
    return m.translate((0, centre, 0)).rotate((0, 0, a))


def player_keepout():
    """Everything the board and its wires need: no other part may be in here."""
    w = PLAYER_W + 2 * PLAYER_CLR + 2 * PLAYER_WIRE_SIDE
    return box(-w / 2, w / 2, -PLAYER_L / 2 - PLAYER_CLR - PLAYER_WIRE_BACK, PLAYER_L / 2,
               LID_T + 0.01, LID_T + PLAYER_POST_H + PLAYER_PCB_T + PLAYER_SOCKET_H + 4.0)


def card_window():
    """The opening through the wall, level with the card slot."""
    z_mid = LID_T + PLAYER_POST_H + PLAYER_PCB_T + PLAYER_SOCKET_H / 2
    return box(-CARD_WIN_W / 2, CARD_WIN_W / 2, PLAYER_L / 2 - 4.0, PLAYER_L / 2 + WALL + 8.0,
               z_mid - CARD_WIN_H / 2, z_mid + CARD_WIN_H / 2)


def player_cradle():
    """On the lid: four posts the board rests on, and low walls that stop it sliding sideways and
    back. It is open on the card side and left open at the sides in the middle, where the wires
    come off the pads."""
    half_w = PLAYER_W / 2 + PLAYER_CLR
    half_l = PLAYER_L / 2 + PLAYER_CLR
    post_r = 2.0
    m = Manifold()
    for sx in (-1, 1):
        for sy in (-1, 1):
            # a half millimetre clear of the walls: a post touching a wall along a line is not
            # printable geometry (the lid stops being watertight)
            m += cyl(post_r, LID_T, LID_T + PLAYER_POST_H,
                     sx * (half_w - post_r - 0.5), sy * (half_l - post_r - 0.5))
    wall_h = PLAYER_POST_H + PLAYER_PCB_T + 0.6
    t = 1.2
    # behind the board, the notch side: it takes the push of the card going in
    m += box(-6.0, 6.0, -half_l - t, -half_l, LID_T, LID_T + wall_h)
    for sx in (-1, 1):
        # at each back corner, a short wall along the side
        x0, x1 = (half_w, half_w + t) if sx > 0 else (-half_w - t, -half_w)
        m += box(x0, x1, -half_l - t, -half_l + 7.0, LID_T, LID_T + wall_h)
        # and a small tab at the front, by the wall, so the board cannot walk out sideways
        m += box(x0, x1, half_l - 5.0, half_l, LID_T, LID_T + wall_h)
    return m


# --- body -------------------------------------------------------------------
def name_engraving():
    """The two names as solids to cut from the wall, or None (no names.json, or not the round shape).

    Each name is drawn flat (u along the line, v up, w out of the wall), then bent round the tapered wall:
    u becomes an angle, v the height, w the depth below the surface. The wall is a cone, so the radius
    at each height is worked out, and the letters follow it. Every vertex is moved, so the flat solid is
    first split into small pieces."""
    if SHAPE != "round" or not NAMES_FILE.exists():
        return None
    data = json.loads(NAMES_FILE.read_text())
    cap_h = data["cap_h"]
    r_ref = R_BOT + (R_TOP - R_BOT) * NAME_Z / H
    edge = math.acos(-FRONT_OUT / R_BOT)             # angle from the front at which the flat meets the circle
    start_right = edge + NAME_GAP / r_ref
    slot = max(data["left"]["width"], data["right"]["width"])   # the stretch of wall each name has
    out = Manifold()
    for side in ("left", "right"):
        d = data[side]
        polys = [np.array(p) - np.array([d["u0"], 0.0]) for p in d["polys"]]
        flat = Manifold.extrude(CrossSection(polys, FillRule.EvenOdd), NAME_DEPTH + 1.0).refine(5)
        centred = (slot - d["width"]) / 2 / r_ref                  # a shorter name sits in the middle of its stretch
        phi0 = start_right + centred if side == "right" else -(start_right + slot / r_ref) + centred

        def bend(v, phi0=phi0):
            u, vv, w = v[:, 0], v[:, 1], v[:, 2]
            z = NAME_Z - cap_h / 2 + vv
            r = R_BOT + (R_TOP - R_BOT) * z / H - NAME_DEPTH + w
            phi = phi0 + u / r_ref
            return np.stack([r * np.sin(phi), -r * np.cos(phi), z], axis=1)

        out += flat.warp_batch(bend)
    return out


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
    for ang in KEY_ANGLES:                      # the ring's three tabs sit in these, so it cannot turn
        b -= box(SEAT_R - 0.5, SEAT_R + KEY_OUT + 0.4, -(KEY_W + 0.4) / 2, (KEY_W + 0.4) / 2,
                 H - SEAT_DEPTH, H + 1).rotate((0, 0, ang))
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
    # touch pad mark: one lamp engraved into the 2 mm wall where the pad sits; the cut also thins the wall under the finger, which is what the sensor wants. The pad
    # has to clear BTN_BLOCK_X (inside it the wall is 9.5 mm thick) and the mark stays well inboard
    # of the tapered edge, where the face is 45 mm either side of the middle at this height
    cut0, cut1 = FRONT_OUT - 1.0, FRONT_OUT + LAMP_ENGRAVE
    bulb_z = TOUCH_Z + (LAMP_BASE_H - 0.35) / 2   # centre the whole mark on the pad
    def lamp(cx):
        bulb = ycyl(LAMP_R, cut0, cut1, cx, bulb_z)
        # a wide little foot under the bulb, so the mark reads as a lamp on a stand rather than a dot
        base = box(cx - LAMP_BASE_W / 2, cx + LAMP_BASE_W / 2, cut0, cut1,
                   bulb_z - LAMP_R + 0.35 - LAMP_BASE_H, bulb_z - LAMP_R + 0.35)
        return bulb + base
    b -= tilt(lamp(TOUCH_X))

    names = name_engraving()
    if names is not None:
        b -= names

    # TP4056: USB-C notch open to the underside, guides that stop the board
    b -= box(-USB_W / 2, USB_W / 2, BACK_IN - 2, BACK_IN + WALL + 10, -1, USB_Z_TOP)
    b -= box(-USB_PLUG_W / 2, USB_PLUG_W / 2, BACK_IN + 0.5, BACK_IN + WALL + 10,
             -1, USB_PLUG_Z_TOP)
    for s in (-1, 1):
        g = box(8.0, 10.0, BACK_IN - 31, BACK_IN + 1, 10.0, H)
        b += (g if s > 0 else g.mirror((1, 0, 0))) ^ clip
    # the card window: through the back wall, level with the MP3 board's card slot
    if SHAPE == "round":
        b -= player_place(card_window())
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
    if SHAPE == "round":
        m += player_place(player_cradle())                        # MP3 board, card slot to the wall

    def frame(x0, x1, y0, y1, h=2.0, t=1.2):
        return box(x0 - t, x1 + t, y0 - t, y1 + t, LID_T, LID_T + h) - box(x0, x1, y0, y1, 0, 99)
    m += frame(-62.5, -40.5, -39.0, 39.0)                         # 18650 holder
    m += frame(40.5, 63.5, -19.5, 19.5)                           # DS3231
    for x, y in CUPS:                                             # ballast: 3 M12 washers each (4 in the deep cups)
        m += cyl(WASHER_R + 1.2, LID_T, LID_T + CUP_H, x, y) - cyl(WASHER_R, LID_T, 99, x, y)
    for x, y in BOSSES:                                           # countersunk M3
        m -= cyl(1.7, -1, LID_T + 1, x, y)
        m -= Manifold.cylinder(1.8, 3.2, 1.7).translate((x, y, -0.01))
    for x, y in FEET:
        m -= cyl(6.2, -1, 1.0, x, y)
    return m


KEY_ANGLES = (60.0, 180.0, 300.0)   # between the lugs, which are at 0, 120 and 240


def bayonet_ring(gap=LUG_GAP, flip=False):
    """Rigid ring with three lugs, printed upside down: its top face, which is also the lugs' top face,
    goes on the bed, so the lugs are flat on it and need no supports. `flip` returns it that way up, ready
    to print; without it the ring is as it sits in the seat, lugs at the top.

    The lug is as wide as the notch (10 mm at the outer edge of the end ring, 5 mm at the bottom) less
    LUG_CLR each side, and reaches in to LUG_TIP_CLR above the notch floor, so it passes the notch and
    overlaps the solid ring by about 4.5 mm once the globe is turned."""
    top = gap + LUG_T
    ring = cyl(RING_R_OUT, 0, top) - cyl(RING_R_BORE, -1, top + 1)
    r_f = FLANGE_D / 2
    r_floor = r_f - NOTCH_DEPTH
    r_tip = r_floor + LUG_TIP_CLR

    def half(r):
        w = NOTCH_W_IN + (NOTCH_W_OUT - NOTCH_W_IN) * (min(r, r_f) - r_floor) / NOTCH_DEPTH
        return (w - 2 * LUG_CLR) / 2

    r_root = RING_R_BORE + RING_WALL / 2
    pts = [(r_tip, -half(r_tip)), (r_f, -half(r_f)), (r_root, -half(r_f)),
           (r_root, half(r_f)), (r_f, half(r_f)), (r_tip, half(r_tip))]
    lug = Manifold.extrude(CrossSection([pts]), LUG_T).translate((0, 0, gap))
    for ang in (0.0, 120.0, 240.0):
        ring += lug.rotate((0, 0, ang))
    for ang in KEY_ANGLES:
        ring += box(RING_R_OUT - 0.5, RING_R_OUT + KEY_OUT, -KEY_W / 2, KEY_W / 2, 0, top).rotate((0, 0, ang))
    if flip:
        ring = ring.rotate((180.0, 0.0, 0.0)).translate((0, 0, top))
    return ring


def test_plate():
    """Three rings with different lug gaps, in one print: two in a row and one below, about 215 mm square."""
    pitch = 2 * (RING_R_OUT + KEY_OUT) + 3.0
    spots = ((-pitch / 2, pitch / 2), (pitch / 2, pitch / 2), (0.0, -pitch / 2 + 6.0))
    plate = Manifold()
    for g, (x, y) in zip(TEST_GAPS, spots):
        plate += bayonet_ring(g, flip=True).translate((x, y, 0))
    return plate


def rings_and_caps():
    """The test plate with the four button caps in its free corners: one PLA file, so Hawk's minimum price
    per copy is paid once and not five times. The caps stand flange down, as they print."""
    plate = test_plate()
    for x, y in ((-78.0, -70.0), (-78.0, -88.0), (78.0, -70.0), (78.0, -88.0)):
        plate += button_cap().translate((x, y, 0))
    return plate


def feet_set():
    """The four feet side by side in one TPU file, for the same reason."""
    out = Manifold()
    for x, y in ((-10.0, -10.0), (10.0, -10.0), (-10.0, 10.0), (10.0, 10.0)):
        out += foot().translate((x, y, 0))
    return out


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
    parts = (("body", body()), ("lid", lid()), ("bayonet_ring", bayonet_ring(flip=True)), ("bayonet_test_plate", test_plate()), ("foot_tpu", foot()), ("rings_and_caps", rings_and_caps()), ("feet_set_tpu", feet_set()),
             ("button_cap", button_cap()))
    for name, part in parts:
        t = save(part, out / f"wakecue_{SHAPE}_{name}.stl")
        bb = t.bounds
        print(f"{SHAPE:5s} {name:9s} watertight={t.is_watertight} "
              f"vol={t.volume / 1000:6.1f} cm3 size={np.round(bb[1] - bb[0], 1)}")
    print(f"front tilt {TILT:.1f} deg")

    # the four files that go to Hawk, copied into to_print/ so that is the one folder to upload from; the
    # rest in this folder are the single parts and the old test plate, which would only be duplicates
    import shutil
    to_print = out / "to_print"
    to_print.mkdir(exist_ok=True)
    for name in ("body", "lid", "rings_and_caps", "feet_set_tpu"):
        shutil.copy(out / f"wakecue_{SHAPE}_{name}.stl", to_print / f"wakecue_{SHAPE}_{name}.stl")
    print(f"copied the order files to {to_print.name}/")
