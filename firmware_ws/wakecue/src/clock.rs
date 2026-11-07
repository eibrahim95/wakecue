use esp_hal::i2c::master::I2c;
use lamp_core::ds3231;

/// Clear the flag this boot owns and arm Alarm 1 for the next minute, which it returns.
///
/// The lamp redraws its clock every minute, so the next minute is always the next thing to do;
/// an event that is due is found on the boot that lands on it. Clearing `A1F` first matters: while
/// it is set the line stays low, and a wake pin sitting at its wake level ends a sleep as it starts.
pub fn arm_next_minute(bus: &mut I2c<'static, esp_hal::Blocking>, now_min: i64) -> i64 {
    let mut st = [0u8; 1];
    bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut st)
        .expect("status");
    bus.write(
        ds3231::I2C_ADDR,
        &[ds3231::REG_STATUS, st[0] & !ds3231::STATUS_A1F],
    )
    .expect("clear a1f");

    let a = ds3231::alarm1_registers(now_min + 1);
    bus.write(
        ds3231::I2C_ADDR,
        &[ds3231::REG_ALARM1, a[0], a[1], a[2], a[3]],
    )
    .expect("alarm1");
    bus.write(
        ds3231::I2C_ADDR,
        &[ds3231::REG_CONTROL, ds3231::CONTROL_ALARM1_ONLY],
    )
    .expect("control");
    now_min + 1
}

/// The clock's minute count, read fresh. Every boot and every awake window asks it at least once.
pub fn now_minutes(i2c: &mut I2c<'static, esp_hal::Blocking>) -> i64 {
    let mut t = [0u8; 7];
    i2c.write_read(ds3231::I2C_ADDR, &[ds3231::REG_TIME], &mut t)
        .expect("read time");
    ds3231::parse_time_registers(t).div_euclid(60)
}
