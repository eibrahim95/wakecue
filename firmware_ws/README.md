# Lamp & Alarm firmware workspace

- `core` (`lamp-core`): `no_std` logic, unit-tested on the PC: schedule (`next_event`) over the five prayers and own alarms,
  Egypt DST, DS3231 registers, MP3 frames, ramp curve, the stored settings block, the menu state machine, the PCF8574 button
  decoding and pad tap/hold, and the INA219 battery reading.
- `prayer-gen`: writes the prayer table (per day five u16 LE, minutes after 00:00 UTC: Fajr, Dhuhr, Asr, Maghrib, Isha).

    cargo test
    cargo run --release -p prayer-gen -- 31.04 31.38 2026 20 prayers_mansoura.bin

Defaults are the Egyptian General Authority of Survey method (Fajr 19.5°, Isha 17.5°) with the standard Asr (shadow = 1);
pass `fajr_angle isha_angle asr_shadow` after the output name to change them. Every time is rounded up to the next whole minute.

The `firmware` crate (esp-hal) is not written yet; it needs the ESP toolchain and is part of milestone 1.
