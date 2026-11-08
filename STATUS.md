# Wakecue status

The one place that says where the build stands and what is in hand.
`docs/` is reference and does not repeat any of this. Update this file in the
same commit as the build-log row that changes it.

Last updated: 8 Nov 2026

## Current move

**Roadmap move 1 · It keeps its own time** — the move in progress.
The plan and its gate are in `docs/index.html#roadmap`. Moves 2 (it wakes you)
and 3 (it ships) follow in that order.

## Seen on the board

Each line has a row in `docs/build-log.html`.

- **The face:** twelve-hour time, date, the next prayer or alarm with a
  countdown, and the five prayers with their times and a dot each.
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
  a touch resets into the normal program. Nothing is served on top yet: no page, no DNS.
- **Wi-Fi:** joins the house router as a station (scratch project), DHCP,
  DNS, NTP. The lamp's setup page is move 1, not built.

## Written but not yet seen

- The five prayer times computed on the boot from the stored place
  (`core::astro`): checked on the PC against all 18,260 times of the old Cairo
  table; not yet compared on the glass.
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
PCF8574 + four buttons, e-paper.

**In hand but not fitted:** LED ring, AO3400 rail switch, MP3 board and
speaker, INA219, NCR18650B + holder + TP4056, CR2032, slide switch,
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
- **The base:** never test-printed; the globe seat assumes an 8 cm neck. The
  button stem height (drawn as a 3.5 mm body with about 2 mm of stem) needs
  checking on the real switch before printing.
- **Prayer times:** all five come from a calculator method (Egyptian General
  Authority of Survey), and official timetables add safety margins. Compare
  each of the five with the local mosque's timetable and set that prayer's
  offset (the settings block keeps one per prayer) to any constant difference.
