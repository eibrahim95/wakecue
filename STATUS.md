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

- **Sounds and volumes, chosen and played:** the top menu's **Sound** row sets the general volume,
  the default adhan (1 to 21) and the default alarm sound; each prayer has a screen with Rings,
  Sound and Volume, and each alarm has Sound and Volume rows, every one with Default. Picking a sound
  or a level plays it as you step (command 0x12 by file number), and a due prayer plays its adhan once
  and an alarm repeats its sound, both climbing from volume 1 over the fade. Written and tested on the
  PC (248 tests, the reply parser against the frames the board really sent) and the firmware builds,
  but not flashed: nobody has seen the screens or heard the lamp play a sound by itself. The player
  stays powered from 5V all the time here, so nothing yet measures the idle draw or the rail switch.
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
