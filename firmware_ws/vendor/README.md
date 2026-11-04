# Vendored crates

`weact-studio-epd/` is the published `weact-studio-epd` 0.1.3, unmodified except for
the patch below. `firmware_ws/wakecue/Cargo.toml` points at it with `[patch.crates-io]`.

## Why

The lamp redraws its clock face once a minute. A full e-paper refresh is a two-second
black-and-white strobe, so every redraw after the first has to use the fast (partial)
waveform — and `fast_refresh` refuses to run until it has done one full refresh *per
driver instance*. A deep-sleep wake builds a fresh instance every time, so without a way
to say "the panel already holds an image" the strobe would come back once a minute.

## The patch

`src/driver.rs`: one added method on `DisplayDriver`, `assume_image_on_panel()`, which
marks the driver as having a valid image on the glass. The struct already had the field
(`initial_full_refresh_done`); this only exposes setting it. Nothing else in the crate
changed.

`weact-studio-epd` is MIT OR Apache-2.0, © Alexander van Saase. If upstream grows an
equivalent switch, drop this copy and the `[patch.crates-io]` block.
