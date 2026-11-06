# Lamp & Alarm firmware workspace

- `core` (`lamp-core`): `no_std` logic, unit-tested on the PC: the five prayer times computed from
  the stored place (`astro.rs`), the schedule over them and the lamp's own alarms, the stored
  settings block, the menu state machine, Egypt DST, DS3231 registers, MP3 frames, the ramp curve,
  the PCF8574 button decoding and pad tap/hold, and the INA219 battery reading.
- `settings-tool` (`wakecue-settings`): reads, patches and writes the settings block from the PC —
  the way back when the menu is stuck or the panel is dark.
- `wakecue`: the lamp itself, an esp-hal crate that builds for the chip instead of the PC (its own
  toolchain and target, see its `Justfile`). It sits outside this workspace's members so `cargo
  test` here stays a host build.

    cargo test

`core/tests/data/prayers-cairo.bin` is the ten-year Cairo table the firmware used to embed, written
by the `prayer-gen` crate that has since been removed. The lamp computes its times now, so the file
is not in any image; it stays as the oracle that `core/tests/cairo_table.rs` checks the astronomy
against, every one of its 18,260 times.
