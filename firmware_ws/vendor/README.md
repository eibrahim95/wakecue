# Vendored crates

`weact-studio-epd/` is the published `weact-studio-epd` 0.1.3, unmodified except for
the two patches below. `firmware_ws/wakecue/Cargo.toml` points at it with
`[patch.crates-io]`.

## Why

The lamp redraws its clock face once a minute. A full e-paper refresh is a two-second
black-and-white strobe, so every redraw after the first has to use the fast (partial)
waveform — and `fast_refresh` refuses to run until it has done one full refresh *per
driver instance*. A deep-sleep wake builds a fresh instance every time, so without a way
to say "the panel already holds an image" the strobe would come back once a minute.

## The patches

### 1. `assume_image_on_panel()`

`src/driver.rs`: one added method on `DisplayDriver`, `assume_image_on_panel()`, which
marks the driver as having a valid image on the glass. The struct already had the field
(`initial_full_refresh_done`); this only exposes setting it. Nothing else changed.

### 2. the busy line has a timeout

`src/driver.rs`: `wait_until_idle()` used to spin on the panel's busy line with no limit,
so a panel that stopped answering — a wedged refresh, a bad connection, a brown-out on
the panel's own supply — froze the whole lamp inside a driver call: no keys, no sleep, no
log, until the power was pulled. It now gives up after `BUSY_TIMEOUT_MS` (five seconds,
against a two-second full refresh) and returns `DisplayError::BusWriteError`, which the
lamp logs as `panel: refresh failed` and carries on from. Every call site propagates it.

`weact-studio-epd` is MIT OR Apache-2.0, © Alexander van Saase. If upstream grows an
equivalent switch and a timeout, drop this copy and the `[patch.crates-io]` block.
