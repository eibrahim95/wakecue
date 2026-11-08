# Wakecue status

The one place that says where the build stands and what is in hand.
`docs/` is reference and does not repeat any of this. Update this file in the
same commit as the build-log row that changes it.

Last updated: 9 Nov 2026

## Current move

**Roadmap move 2 · It wakes you** is in progress; move 1 is built and seen (the last gate checks are
in "Written but not yet seen"). The sound half of move 2 is done on the bench and in the app: the
MP3 board plays by file number, the menu chooses and previews sounds, and a due prayer or alarm
plays its own. Not started: the LED ring in the app, the rail switch, and the low-voltage test.
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
- **Wi-Fi:** joins the house router as a station (scratch project), DHCP,
  DNS, NTP. The lamp's own network, setup page and clock sync are in the entries above.

## Written but not yet seen

- **The face, rearranged:** the date sits under the time, a crescent moon in the top-right corner
  replaces "z z z" as the sleep mark (the corner is kept for the battery later), "Next:" moved down
  5 px, and thin vertical lines separate the five prayers. Builds; not flashed, so the spacing is
  unseen on the panel.
- **The prayer strip:** each name and its bell (solid when the prayer rings, outlined and slashed when not) are centred as one piece over its time, the five columns
  are centred on the rule, and the prayer named on the "Next:" line is drawn white on black (not
  when the next thing is an alarm). Builds; not flashed.
- **One rule for every settings screen:** Up and Down only move between rows. OK on a value row
  activates it (the row turns black, the value sits between `<` and `>`), Up and Down then change
  it, and OK or Back deactivates it where it stands. OK on the Rings row flips it; OK on the boxed
  Save or Delete does it. Back with nothing active discards the screen. A footer line says what the
  buttons do right now. Covers the alarm editor, a prayer's screen, Sound and Time zone. 252 host
  tests pass and it builds; not flashed, so the drawing is unseen on the panel.
- **Sounds and volumes, chosen and played:** the top menu's **Sound** row sets the general volume,
  the default adhan and the default alarm sound (each 1 to 100); each prayer has a screen with Rings,
  Sound and Volume, and each alarm has Sound and Volume rows, every one with Default. Picking a sound
  or a level plays it as you step (command 0x12 by file number, 120 ms after each command). A due
  prayer plays its adhan once and an alarm repeats its sound, both climbing from volume 1 over the fade.
  Heard on the board (8 Nov): stepping the default adhan plays each sound, once the gap between
  commands was added. Not yet reported: the prayer screens, the alarm editor's rows, and a due prayer
  or alarm ringing by itself. The player stays powered from 5V all the time, so nothing yet measures
  the idle draw or the rail switch.
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
soldering iron, Stannol solder, a screwdriver.

**Wired on the breadboard:** SuperMini, DS3231 (no coin cell), TTP223,
PCF8574 + four buttons, e-paper, the MP3 board and its speaker (5V and GND, GPIO20/21 through 1 kΩ).

**In hand but not fitted:** LED ring, AO3400 rail switch, INA219, NCR18650B + holder + TP4056, CR2032, slide switch,
perfboard, the fallback parts (MT3608, AO3401, 1N5819WF).

**Not done to the boards yet:** the SuperMini's power LED and the DS3231's
power LED are still on; the DS3231 charge resistor (most likely `R4`, decided
5 Nov) is not lifted, so no coin cell goes in.

## Still to buy

- The globe: Y-LIGHTS opal globe (Amazon.eg), or a plain white opal ceiling
  globe with a 70–90 mm opening. Buy it first; the print is sized from its neck.
- Lead shot for ballast (Amazon.eg).
- The print from Hawk: body, lid, TPU grip ring and four feet, four button
  caps. Repeat the quote; the body has changed since 25 Oct.

## Open questions

Numbers or sightings only the board can give.

- **Low cell voltage:** the ring is rated from 3.5 V and the MP3 board wants
  4.5–5 V, but the cell runs 3.2–4.2 V. Tested in move 2; the fallback is
  the MT3608 boost.
- **Switched ground:** never built. Does anything leak through the ring or
  player pins while their ground is off? (move 2)
- **Sleep current:** an estimate. The battery calculator assumes 1 mA. (move 3)
- **Panel's hidden edge:** which six rows or columns the e-paper hides is
  unmeasured; every mark is kept six pixels in until then. (move 3)
- **The MP3 board's place in the base:** the model now has the board at the back right with a 14 × 7 mm
  window through the wall for the microSD card, and three deeper ballast cups instead of four.
  Checked only against the model (no overlaps, watertight), never printed. Measure the real board and
  its card socket with calipers first: the model assumes 21 × 21 mm, a socket about 5 mm above the lid,
  and a socket the card can be reached on and removed from through that window. The tipping figures
  on the hardware page are for four cups and have not been redone.
- **The base:** never test-printed; the globe seat assumes an 8 cm neck. The
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
   and Volume rows, and a due prayer or alarm ringing by itself (set an alarm a minute ahead with
   the card in). Then step through the alarm sounds 1 to 88 and say which ringtones are wrong, quiet
   or unwanted; swap them with `tools/ringtone_order.txt` and `tools/audio_card.py`.
2. **Low cell voltage:** run the player and speaker from the LM2596 at 4.2, 3.5 and 3.2 V and see
   whether a track plays at a usable volume and without resets (the open question under "Open
   questions").
3. **The rail switch:** fit the AO3400, switch the player's ground with GPIO5, and measure the idle
   draw with the rail off. Today the player is powered from the 5V pin all the time (about 20 mA
   idle), which will not do on a battery.
4. **The LED ring** in the app (the bench program `bin/ring.rs` works): the ramp before an alarm,
   from the same call that starts the sound.
5. **The CPU clock:** the app uses `CpuClock::max()` (160 MHz). esp-radio needs at least 80 MHz for
   Wi-Fi. Measure how long a normal minute-tick boot is awake, then try 80 MHz and re-check the Wi-Fi
   sync, the setup mode and the panel. Not started.
6. **The base:** measure the real MP3 board and its card socket (width, length, socket height above
   the board), set `PLAYER_W`, `PLAYER_L`, `PLAYER_POST_H`, `PLAYER_SOCKET_H` in `cad/base.py`,
   rerun `python base.py round`, recompute the tipping figures for three cups, and fix the SuperMini
   pin rows in `docs/hardware.html` (see below). Nothing has been printed.
7. **Buy:** the globe first (the print is sized from its neck), lead shot, then the repeat quote from
   Hawk (see "Still to buy").

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
  `firmware_ws` runs 248 core tests and the settings tool's 8. The block grew to 208 payload bytes; an
  old 200-byte block still loads as all Default.
- **The CAD** needs `trimesh`, `numpy` and `manifold3d`. They were in `/tmp/cadvenv`, which may be
  gone; this machine's shell cannot reach the package file host, so recreate it with `uv` or from the
  desktop. `cad/player_search.py` checks where the board can sit.
- **Network from the shell** reaches only some hosts: Wikimedia (throttles hard: wait minutes after a
  429), Openverse and Freesound's CDN, a few free-sound sites. GitHub, Hugging Face and the Python
  package files are blocked. The Chrome DevTools tools exist for the rest.
- **Commits** carry no co-author trailers (the user's rule). The notes in `docs/` and this file are
  updated in the same commit as the build-log row that changes them.
