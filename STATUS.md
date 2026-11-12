# Wakecue status

The one place that says where the build stands and what is in hand.
`docs/` is reference and does not repeat any of this. Update this file in the
same commit as the build-log row that changes it.

Last updated: 13 Nov 2026

## Current move

**Roadmap move 2 · It wakes you** is in progress; move 1 is built and seen (the last gate checks are
in "Written but not yet seen"). The sound half of move 2 is done on the bench and in the app: the
MP3 board plays by file number, the menu chooses and previews sounds, and a due prayer or alarm
plays its own. The LED ring is in the app, the rail switch (AO3400 on GPIO5) is fitted, and the rail
now comes on only for a wake that needs it: flashed on 13 Nov and reported working on every point
asked of it, including no click on a plain minute tick and nothing drawn with the rail off.
Not yet done: the ring at 3.2-3.5 V, and what the ring does after an adhan ends.
The plan and its gates are in `docs/index.html#roadmap`. Move 3 (it ships) follows.

## Seen on the board

Each line has a row in `docs/build-log.html`.

- **The face:** twelve-hour time, date, the next prayer or alarm with a
  countdown, and the five prayers with their times and a bell each.
- **The menu:** four buttons (PCF8574 at `0x20`) walk Prayers, My alarms and
  the alarm editor; a change is written to the settings block in flash and
  survives an unplug.
- **The alarm:** named on the face before it comes, and it takes the whole
  panel at its minute until a key or a touch answers it. It is silent: the
  ring and the MP3 card are not wired yet (move 2).
- **Sleep:** the chip deep-sleeps, wakes itself every minute (DS3231 Alarm 1
  on GPIO3), and a touch (GPIO4, active high) or any button (expander INT on
  GPIO3) wakes it. The display keeps its image through the sleep.
- **The clock:** DS3231 keeps UTC; set from NTP on the bench; OSF read at boot.
- **The zone:** the face, the alarms and the prayer strip follow the offset and summer-time
  rule stored in the block, not Egypt's. The Time zone screen (top menu, third row) sets the
  hour, the minute (00, 15, 30, 45) and the rule, and writes them on Save.
- **The phone setup:** top menu, fourth row. The lamp raises an open network `Wakecue-XXXX` at
  192.168.4.1 and answers DHCP from `core::dhcp`; a phone joined it and was given `.2`. Back or
  a touch resets into the normal program. The Saved page now says what happens next.
- **The setup page:** on that network DNS answers every name with 192.168.4.1, a web server on
  port 80 serves one form for the Wi-Fi name and password, the time zone and the place, and a
  valid form is written to flash like a menu change. An Android phone offered to sign in by
  itself and the form worked; the saved zone was still there afterwards. Saving the Wi-Fi name does not make the lamp join it yet: that is the station half of
  move 1.
- **The clock from the network:** (`wakecue/src/station.rs`, `core::ntp`) on a boot where the
  DS3231 says it lost power and a Wi-Fi name is stored, the lamp joins that network, asks
  `time.google.com` for the time, writes the clock, clears the power-loss flag, records
  `setup.wifi` and `setup.clock`, and resets into a clean boot. It synced. Any failure inside
  30 s falls back to waking once a minute.
- **The place from a list:** the setup page searches 440 built-in places (`core::cities`) and fills
  in the name, coordinates, UTC offset and summer-time rule, so nobody types a latitude. The raw
  fields stay in a collapsed section for a village that is not listed.
- **The calculation method:** the page has a Calculation method box with 19 authorities
  (`core::methods`), filled in with a suggestion for the place picked and changeable; the chosen id
  is kept in the settings block. Makkah's Fajr now reads 5:09 as on Google, where the lamp used the
  Egyptian convention and gave 5:04.
- **The minute:** times are rounded to the nearest minute, as the Egyptian Survey Authority's
  published table is (94% of its 760 Mansoura times exactly, never more than a minute off).
- **The sync message:** while the clock is fetched the panel says "Syncing time..." with the
  network's name under it.
- **The first boot:** a lamp with nothing stored raises the setup network by itself
  (`Wakecue-XXXX`), without going through the menu.
- **The MP3 board:** serial link both ways on GPIO20/21, plays a track and the speaker works at volume
  20 straight from the SuperMini's 5V pin; the card's file count is command 0x48 (21 adhan plus a
  test beep reads 22); `/mp3/0101.mp3` plays by name across a gap, so adhan are 0001-0099 and alarm
  sounds start at 0101. The datasheet's table has 0x47 and 0x48 the wrong way round for this board.
  Test programs `just sound` and `just numbering`. Powered from 5V here, not through the rail switch.
  Also played from the LM2596 module (screw-terminal buck converter, no USB) at 4.2, 3.5 and 3.2 V on
  10 Nov.
- **Wi-Fi:** joins the house router as a station (scratch project), DHCP,
  DNS, NTP. The lamp's own network, setup page and clock sync are in the entries above.
- **The ring in the app, without the rail switch:** each wake asks `schedule::active_ramp` how far
  into a light ramp it is and sets all 16 pixels to that step (`ramp_pixel`, capped at 64 of 255); an
  alarm lights the ring at the end-of-ramp warm white while it rings and turns it off after; any other
  wake sends black. The pixels hold their colour through the sleep, so the ramp is one step a minute.
  The ring is powered all the time (no rail switch). Reported working on the board on 11 Nov; what
  was not reported: the detail of the ramp steps, whether a floating GPIO10 flickers the pixels in
  deep sleep (a 10 kΩ from the ring's data pin to ground would stop it), and the ring at 3.2-3.5 V.
  Not decided: what the ring does after a prayer's adhan ends (it goes dark now).
  Since 12 Nov the ring sits behind the rail switch (next entry).
- **The rail switch:** the AO3400 is wired and driven by GPIO5 (1 kΩ to the gate, 100 kΩ from the gate to
  ground; source to the lamp's ground, drain to the joined grounds of the ring and the MP3 board). The
  ring and the MP3 board are powered from the LM2596 at 3.2 V, itself fed by a 5 V 1 A charger; the
  ESP32 is on USB. Reported on 12 Nov: it works. Reported with it: a small click from the speaker on
  every one-minute wake, because that firmware switched the rail on at the start of every wake and the
  MP3 board pops when its ground is joined. Not reported then: the current with the rail off, the ring's
  colours at 3.2 V, and whether the held rail keeps the glow through the sleep. The first and the third
  were answered on 13 Nov (next entry); the ring at 3.2 V is still open.
- **The rail only when needed (13 Nov):** `wakecue/src/rail.rs`. A plain minute tick leaves GPIO5 alone,
  so the ring and the MP3 board stay unpowered and the click is gone. A light-ramp wake switches the rail
  on, sets the ring, and holds GPIO5 high through the sleep (`set_pad_hold`), so the glow is still there
  at the next wake and the player is not power-cycled each minute; any other sleep drops the rail. An
  alarm, an adhan and a menu preview switch it on and wait 1.5 s for the MP3 board before the first
  command: a due prayer or alarm and a preview both started their sound. The 3.2 V line reads 0 on the
  meter with the gate low, so nothing leaks through the ring's data pin or the player's serial pins.

## Written but not yet seen

- **The face, rearranged:** the date sits under the time, a crescent moon in the top-right corner
  replaces "z z z" as the sleep mark (the corner is kept for the battery later), "Next:" moved down
  5 px, and thin vertical lines separate the five prayers. On the glass since 13 Nov; nobody has read
  the spacing back off it.
- **The prayer strip:** each name and its bell (solid when the prayer rings, outlined and slashed when not) are centred as one piece over its time, the five columns
  are centred on the rule, and the prayer named on the "Next:" line is drawn white on black (not
  when the next thing is an alarm). On the glass since 13 Nov; not read back.
- **One rule for every settings screen:** Up and Down only move between rows. OK on a value row
  activates it (the row turns black, the value sits between `<` and `>`), Up and Down then change
  it, and OK or Back deactivates it where it stands. OK on the Rings row flips it; OK on the boxed
  Save or Delete does it. Back with nothing active discards the screen. A footer line says what the
  buttons do right now. Covers the alarm editor, a prayer's screen, Sound and Time zone. 256 host
  tests pass and it builds. On the glass since 13 Nov; the drawing has not been read back.
- **Sounds and volumes, chosen and played:** the top menu's **Sound** row sets the general volume,
  the default adhan and the default alarm sound (each 1 to 100); each prayer has a screen with Rings,
  Sound and Volume, and each alarm has Sound and Volume rows, every one with Default. Picking a sound
  or a level plays it as you step (command 0x12 by file number, 120 ms after each command). A due
  prayer plays its adhan once and an alarm repeats its sound, both climbing from volume 1 over the fade.
  Heard on the board (8 Nov): stepping the default adhan plays each sound, once the gap between
  commands was added. A due prayer or alarm and a menu preview start their sound through the rail
  switch, so the 1.5 s wait for the player's ground is enough (13 Nov). Not yet reported: the prayer
  screens and the alarm editor's rows on the panel.
- **The sound library on the card:** `tools/audio_card.py` builds it, in the order of
  `tools/ringtone_order.txt`. Adhan 0001-0021 are the IslamCan files; alarm sounds start at 0101 with
  Egypt and the Arab world first (a traditional mizmar from Cairo, an Egyptian wedding, an orange
  street vendor, oud, ney, flute, darbuka, a Lebanese zaffa), then Turkey, Armenia, Iran and Israel,
  then about 30 funny ones (kazoo fanfare, goats, clown horn, camel, slide whistle, rubber chicken,
  donkeys, a laughing child, yodel, vuvuzela, Hee Haw), then the rest of the world: 87 in all,
  0101-0187, with credits in `docs/audio-credits.md`. A very short clip is repeated with gaps so it
  does not hammer. The test beep is gone. Every other slot up to 0100 and 0200 holds a spoken
  placeholder. Found on Wikimedia Commons and, through Openverse, on Freesound and Jamendo; chosen by
  title and licence, so nobody has listened to them. The card itself still has the earlier set until it
  is copied again.
- Skipping the first-boot setup with Back or a touch, which writes the settings as they stand with
  the place marked as decided so the lamp does not ask again: written, nobody has said it was
  tried.
- The five prayer times on the face, computed on the boot from the stored place
  (`core::astro`): checked on the PC against the Egyptian Survey Authority's published table
  (760 times) and against AlAdhan for every method. On the glass, Fajr in Makkah has been compared
  with Google and agrees; the other four prayers and the prayer strip's dots have not been said
  to match yet.
- The prayer strip on the face: drawn on every face, but nobody has checked
  that its times and dots match the switches.
- The 30 s deadline on the network phase: written, never seen firing.

## In hand

Everything in the parts list (`docs/hardware.html#bom`) except the Amazon.eg
and Hawk groups: all of the Makers order (parts, tools, LM2596 test supply),
the ESP32-C3 SuperMini from Most, and the WeAct 2.13″ e-paper. Also already
owned, not bought: a USB SD reader, a 16 GB microSD (FAT32, `mp3` folder), a
soldering iron, solder in 0.4, 0.6, 0.8 and 1 mm (plus the old Stannol HS10
1.8 mm), a screwdriver.

**Wired on the breadboard:** SuperMini, DS3231 (no coin cell), TTP223,
PCF8574 + four buttons, e-paper, the MP3 board and its speaker (GPIO20/21 through 1 kΩ), the LED ring
(GPIO10 through 330 Ω), and the AO3400 rail switch (GPIO5). The ring and the MP3 board run from the
LM2596 at 3.2 V (5 V 1 A charger in) and take their ground through the AO3400.

**The globe (11 Nov):** white opal, 20 cm wide and tall, 165 g; neck Ø97 mm outside, Ø81 mm inside, 15 mm
straight. It came with a black ceiling fitting (a ring with an E27 socket) that it twists onto through
three notches in its neck rim; neither is used, the printed seat holds the neck by friction.

**The lead shot (12 Nov):** bought and arrived; not yet weighed, and not poured into the base (no base printed).

**In hand but not fitted:** INA219, NCR18650B + holder + TP4056, CR2032, slide switch,
perfboard, the fallback parts (MT3608, AO3401, 1N5819WF).

**Not done to the boards yet:** the SuperMini's power LED and the DS3231's
power LED are still on; the DS3231 charge resistor (most likely `R4`, decided
5 Nov) is not lifted, so no coin cell goes in.

## Still to buy

- The print from Hawk: body, lid, TPU grip ring and four feet, four button
  caps. Repeat the quote; the body has changed since 25 Oct.

## Open questions

Numbers or sightings only the board can give.

- **Low cell voltage:** the MP3 board and speaker play at 4.2, 3.5 and 3.2 V from the LM2596 (10 Nov,
  by ear), so the player needs no boost. Still open: the LED ring, rated from 3.5 V, now on the 3.2 V
  supply but its colours at that voltage not reported; the file count and resets were not checked at low voltage. The fallback is the MT3608
  boost.
- **Sleep current:** an estimate. The battery calculator assumes 1 mA. (move 3)
- **Panel's hidden edge:** which six rows or columns the e-paper hides is
  unmeasured; every mark is kept six pixels in until then. (move 3)
- **The MP3 board's place in the base:** the model now has the board at the back right with a 14 × 7 mm
  window through the wall for the microSD card, and three deeper ballast cups instead of four.
  Checked only against the model (no overlaps, watertight), never printed. Measure the real board and
  its card socket with calipers first: the model assumes 21 × 21 mm, a socket about 5 mm above the lid,
  and a socket the card can be reached on and removed from through that window. The tipping figures
  on the hardware page are for four cups and have not been redone.
- **The base:** never test-printed. The seat is now sized for the real globe (`python base.py round`, neck
  97 mm, generated and watertight), held by the TPU grip ring alone: whether that grips a 165 g globe
  firmly enough is unknown until the ring is printed. The tipping estimate for a 165 g, 20 cm globe is
  about 52° and 170 g of push with the lead (48° and 140 g without); it is a rough statics estimate, not a
  measurement. The
  button stem height (drawn as a 3.5 mm body with about 2 mm of stem) needs
  checking on the real switch before printing.
- **Prayer times:** the lamp rounds to the nearest minute, as the Egyptian General
  Authority of Survey does, and against the Authority's published Mansoura
  timetable (760 times, five months of 2026) it gives the published minute 94% of
  the time and is never more than a minute away; the Authority's table shows no
  systematic margin on any prayer. A mosque may still ring a minute or two off the Authority's table: set
  that prayer's offset (the settings block keeps one per prayer, but no screen
  sets it yet). Not done: Isha 120 minutes in Ramadan for the Umm al-Qura method
  (Saudi Arabia), which the lamp has no Hijri calendar to follow.

## Next steps, in the order I would take them

1. **Check what is written but not seen** (list above): the prayer screens, the alarm editor's Sound
   and Volume rows, the rearranged face and the prayer strip on the glass. Then step through the alarm
   sounds 1 to 88 and say which ringtones are wrong, quiet or unwanted; swap them with
   `tools/ringtone_order.txt` and `tools/audio_card.py`.
2. **The ring at 3.2-3.5 V:** the rail switch now lets the ring be darkened and measured, so this is one
   look at the ring on the 3.2 V supply, then the decision on what it does after an adhan ends (it goes
   dark now).
3. **The CPU clock:** the app uses `CpuClock::max()` (160 MHz). esp-radio needs at least 80 MHz for
   Wi-Fi. Measure how long a normal minute-tick boot is awake, then try 80 MHz and re-check the Wi-Fi
   sync, the setup mode and the panel. Not started.
4. **The base:** measure the real MP3 board and its card socket (width, length, socket height above
   the board), set `PLAYER_W`, `PLAYER_L`, `PLAYER_POST_H`, `PLAYER_SOCKET_H` in `cad/base.py`,
   rerun `python base.py round`, redo the tipping estimate once the real printed weights are known, and
   fix the SuperMini pin rows in `docs/hardware.html` (see below). Nothing has been printed. Print the TPU
   grip ring first and see whether it holds the globe.
5. **Buy:** the repeat quote from Hawk (see "Still to buy"). The lead shot has arrived (12 Nov); weigh it,
   since the tipping estimate assumes about 100 g in the three cups.
6. **The cell and the charger (move 3), none of it done:** the NCR18650B, its holder and the TP4056 are in
   hand and unfitted. In order: switch off the SuperMini's and the DS3231's power LEDs; check the cell's
   voltage and the TP4056's output and polarity with the meter before anything is connected; run the board
   from the cell with the charger unplugged; charge it through the TP4056 while the board runs and watch
   termination with the lamp asleep; note the cell voltage at which the chip resets; fit the INA219 on the
   same I²C wires (0x40) and read a plausible cell voltage with the ring and player off; then measure the
   whole-board sleep current (under 0.5 mA is the gate) and replace the calculator's 1 mA estimate.
   Check on the parts themselves whether the cell and the TP4056 board have protection; this file does not say.
7. **The battery gauge on the face, in scope for move 3 (decided 13 Nov):** `core::ina219` has the
   bus-voltage decode, the power-down config word and a generic 18650 curve in 100 mV steps, tested on
   the PC; no firmware reads it and the face has no cell mark, the top-right corner being the crescent
   sleep mark. Move 3's gate now asks for the charge on the face, so to do: wire the INA219 on the shared
   I²C (`0x40`), read it once per boot and leave it powered down (about 6 µA, where a continuous read
   would add around 1 mA), draw it beside the crescent — decided 13 Nov: crescent where it is, a 14 × 9
   cell glyph with a 2 × 3 nub across x 206–222, three 4-px segments inside — and replace the generic
   curve with the one measured on the real cell. Written when move 3 starts, not before.

## Ideas for later

- **Say what a track is in its file name.** Add a suffix to the MP3 files: `_alarm`, `_alarm_placeholder`,
  `_adhan`, `_adhan_placeholder`, so the card tells you which sounds are real and which are stand-ins
  without opening the index. (Today the split is by number: adhan 0001-0099, alarms from 0101; the
  player plays by number, so the suffix would be for people and for `tools/audio_card.py`, which
  would need to read it.)

## Working notes for whoever picks this up

- **The SuperMini's pin rows.** With the USB-C at the top and the component side up, 5V, GND, 3V3,
  GPIO4, 3, 2, 1, 0 are on the **right** row and GPIO5, 6, 7, 8, 9, 10, 20, 21 on the **left**. The
  wiring diagram in `docs/hardware.html` lists them the other way round: it is wrong, not yet fixed.
  The player test wires: 5V and GND to the player's VCC and GND, GPIO21 through 1 kΩ to its RX,
  GPIO20 through 1 kΩ from its TX, the speaker across its two outer pads (SPK1 +, SPK2 −).
- **The MP3 board** (DFPlayer Mini clone, HW-247A V0.5.1) has no letters printed on it. Hold it with
  the microSD slot up and "DFPlayer Mini" the right way up: VCC is the top-left pad, and the pins
  follow DFRobot's datasheet picture. It was first mounted the wrong way round, which killed the
  ESP32 on power-up without harm. What this board really does: the card's file count is command 0x48
  (the datasheet table has 0x47 and 0x48 swapped), 0x12 plays `/mp3/NNNN.mp3` by the number in the
  name across gaps, 0x3D announces a track's end with its position, 0x4B always answers 0 (useless),
  and it needs about 120 ms between commands (`Player::send`), or a play right after a volume is lost.
- **The card** is numbered adhan `0001`-`0100` and alarm sounds `0101`-`0200`; the lamp never counts
  files, and every unused slot holds a spoken placeholder. `audio/` is git-ignored; `tools/audio_card.py`
  rebuilds `audio/sd/mp3` from `audio/ringtones-source` (ringtones, placeholders, index), the credits are
  `docs/audio-credits.md`. The card shows up as `/dev/sda1` or `/dev/sdb1` under `/run/media/ibrahim/`;
  copy only the files that changed and compare every file afterwards.
- **Building:** `just build`, `just lamp`, `just sound`, `just numbering` from `firmware_ws/wakecue`
  (`just` loads `DEV_MODE` from `.env`; plain `cargo build` fails without it). `cargo test` from
  `firmware_ws` runs 256 core tests and the settings tool's 8. The block grew to 208 payload bytes; an
  old 200-byte block still loads as all Default.
- **The CAD** needs `trimesh`, `numpy` and `manifold3d`. They were in `/tmp/cadvenv`, which may be
  gone; this machine's shell cannot reach the package file host, so recreate it with `uv` or from the
  desktop. `cad/player_search.py` checks where the board can sit.
- **Network from the shell** reaches only some hosts: Wikimedia (throttles hard: wait minutes after a
  429), Openverse and Freesound's CDN, a few free-sound sites. GitHub, Hugging Face and the Python
  package files are blocked. The Chrome DevTools tools exist for the rest.
- **Commits** carry no co-author trailers (the user's rule). The notes in `docs/` and this file are
  updated in the same commit as the build-log row that changes them.
