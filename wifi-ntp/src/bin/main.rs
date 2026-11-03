#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_net::{Runner, StackResources};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_println as _;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethodConfig, Config, Interface};
use static_cell::StaticCell;
extern crate alloc;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, Stack};
use embassy_time::with_timeout;
use esp_hal::gpio::{Event, Input, InputConfig, Level, Output, OutputConfig, Pull, WakeupConfig};
use esp_hal::i2c::master::{Config as I2Config, I2c};
use esp_hal::rtc_cntl::sleep::{LowPower, RtcSleepConfig};
use lamp_core::calendar::{civil_from_days, utc_to_egypt};
use lamp_core::ds3231;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
const NTP_UNIX_OFFSET: i64 = 2_208_988_800;

esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 2.0.0
    // generator parameters: -o esp32c3 -o wifi -o alloc -o unstable-hal -o embassy -o defmt -o esp-backtrace -o esp32c3-mini-1 -o agents -o zed

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    // Onboard LED is active-low. On while this boot is awake; driven off just before sleep.
    // GPIO8 is a strap pin and is not held through deep sleep, so do not try to keep it lit.
    let mut led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
    let mut sqw = Input::new(
        peripherals.GPIO3,
        InputConfig::default().with_pull(Pull::Up),
    );
    // The TTP223 drives OUT both ways, so this pin takes no pull-up. Its output
    // idles at 0 V and goes high while touched, so the wake is armed on a high.
    // The weak pull-down only matters if the jumper to the module comes loose:
    // a floating line must not read as a finger.
    let mut pad = Input::new(
        peripherals.GPIO4,
        InputConfig::default().with_pull(Pull::Down),
    );
    // The following pins are used to bootstrap the chip. They are available
    // for use, but check the datasheet of the module for more information on them.
    // - GPIO2
    // - GPIO8
    // - GPIO9
    // These GPIO pins are in use by some feature of the module and should not be used.
    let _gpio11 = peripherals.GPIO11;
    let _gpio12 = peripherals.GPIO12;
    let _gpio13 = peripherals.GPIO13;
    let _gpio14 = peripherals.GPIO14;
    let _gpio15 = peripherals.GPIO15;
    let _gpio16 = peripherals.GPIO16;
    let _gpio17 = peripherals.GPIO17;

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!("Embassy initialized!");

    let mut wifi_controller =
        esp_radio::wifi::WifiController::new(peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");
    let station = Config::Station(
        StationConfig::default()
            .with_ssid(env!("WIFI_SSID").try_into().unwrap())
            .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
                env!("WIFI_PASSWORD").try_into().unwrap(),
            )),
    );
    let wifi_interface = esp_radio::wifi::Interface::station();
    wifi_controller.set_config(&station).unwrap();
    static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let rng = Rng::new();
    let seed = ((rng.random() as u64) << 32) | rng.random() as u64;
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(net_task(runner).unwrap());
    match wifi_controller.connect_async().await {
        Ok(_) => info!("Wi-Fi connected"),
        Err(e) => info!("Wi-Fi failed: {:?}", defmt::Debug2Format(&e)),
    }
    // TODO: Spawn some tasks
    stack.wait_config_up().await;
    let cfg = stack.config_v4().unwrap();
    info!("IP: {}", cfg.address);

    let addrs = stack
        .dns_query("time.google.com", embassy_net::dns::DnsQueryType::A)
        .await
        .expect("DNS lookup failed");
    info!("NTP server: {}", addrs[0]);
    let mut bus = I2c::new(peripherals.I2C0, I2Config::default())
        .unwrap()
        .with_sda(peripherals.GPIO6)
        .with_scl(peripherals.GPIO7);
    match ntp_unix_seconds(stack, addrs[0]).await {
        Some(unix) => {
            info!("Unix time: {}", unix);
            print_utc(&unix);

            // set the time

            let regs = lamp_core::ds3231::time_registers(unix);
            let mut frame = [0u8; 8];
            frame[0] = lamp_core::ds3231::REG_TIME;
            frame[1..].copy_from_slice(&regs);
            bus.write(lamp_core::ds3231::I2C_ADDR, &frame)
                .expect("write time");
            info!("Getting the time");
            let mut t = [0u8; 7];
            bus.write_read(0x68, &[lamp_core::ds3231::REG_TIME], &mut t)
                .unwrap();
            let unix_from_registrs = lamp_core::ds3231::parse_time_registers(t);
            info!("{}, {}", t, unix_from_registrs);
            print_utc(&unix_from_registrs);

            let now_min = ds3231::parse_time_registers(t).div_euclid(60);
            // Two minutes out: keeps a full clock-wake cycle short enough to watch.
            let target = now_min + 2;

            // 2. clear a flag the chip may have been carrying
            let mut st = [0u8; 1];
            bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut st)
                .expect("status");
            bus.write(
                ds3231::I2C_ADDR,
                &[ds3231::REG_STATUS, st[0] & !ds3231::STATUS_A1F],
            )
            .expect("clear a1f");

            // 3. program alarm 1
            let a = ds3231::alarm1_registers(target);
            bus.write(
                ds3231::I2C_ADDR,
                &[ds3231::REG_ALARM1, a[0], a[1], a[2], a[3]],
            )
            .expect("alarm1");

            // 4. arm: alarm drives INT, alarm 1 enabled
            bus.write(
                ds3231::I2C_ADDR,
                &[ds3231::REG_CONTROL, ds3231::CONTROL_ALARM1_ONLY],
            )
            .expect("control");

            // 5. watch bit 0
            info!(
                "armed for {}, now {}; line high = {}",
                target,
                now_min,
                sqw.is_high()
            );
        }
        None => info!("NTP failed"),
    }

    let mut announced = false;

    loop {
        led.set_low();

        let clock_high = sqw.is_high();
        let pad_high = pad.is_high();

        // A pin sitting at its wake level ends the sleep the moment it starts, and
        // the board then looks like it never sleeps. Wait for the level to clear,
        // and show on the LED which pin is holding it.
        if !clock_high || pad_high {
            if !announced {
                info!(
                    "waiting: clock high = {}, pad high = {}",
                    clock_high, pad_high
                );
                announced = true;
            }
            blink_state(&mut led, clock_high, pad_high).await;
            continue;
        }

        sqw.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
            .expect("arm GPIO3 as a wake source");
        sqw.listen(Event::LowLevel);

        // Same setup, opposite level: this module has no A/B pads, so a touch is a high.
        pad.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
            .expect("arm GPIO4 as a wake source");
        pad.listen(Event::HighLevel);

        info!("sleeping");
        led.set_high();

        let mut lp = LowPower::new(peripherals.LPWR);
        lp.sleep_deep(RtcSleepConfig::deep());
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
}

/// Readout on the one LED, so a wake level that is holding the board awake can be
/// named without a serial log. Count the blinks in each burst, then a one-second
/// pause:
///
/// - **1 blink** — both levels clear, so the sleep below would start (good).
/// - **2 blinks** — pad line high while untouched: jumper on the wrong row, or the
///   module's OUT is latched high.
/// - **3 blinks** — clock line low: Alarm 1 is due, so the SQW pin pulls low.
/// - **4 blinks** — both of the above.
async fn blink_state(led: &mut Output<'static>, clock_high: bool, pad_high: bool) {
    let blinks = if clock_high && !pad_high {
        1
    } else if clock_high {
        2
    } else if pad_high {
        4
    } else {
        3
    };

    for _ in 0..blinks {
        led.set_low(); // active low: on
        Timer::after(Duration::from_millis(200)).await;
        led.set_high(); // off
        Timer::after(Duration::from_millis(200)).await;
    }
    Timer::after(Duration::from_millis(1000)).await;
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}
async fn ntp_unix_seconds(stack: Stack<'_>, server: IpAddress) -> Option<i64> {
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx_buf = [0u8; 128];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_buf = [0u8; 128];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(0).ok()?; // 0 = any free local port

    let mut request = [0u8; 48];
    request[0] = 0x1B; // LI 0, version 3, mode 3 (client)
    socket.send_to(&request, (server, 123)).await.ok()?;

    let mut reply = [0u8; 48];
    let (len, _) = with_timeout(Duration::from_secs(5), socket.recv_from(&mut reply))
        .await
        .ok()? // timed out
        .ok()?; // receive error
    if len < 48 || reply[1] == 0 {
        return None; // short packet, or stratum 0
    }
    let secs = u32::from_be_bytes([reply[40], reply[41], reply[42], reply[43]]);
    Some(secs as i64 - NTP_UNIX_OFFSET)
}
fn print_utc(unix: &i64) {
    let utc_min = unix.div_euclid(60);
    let local_min = utc_to_egypt(utc_min);
    let (y, mo, d) = civil_from_days(local_min.div_euclid(1440) as i32);
    let minute_of_day = local_min.rem_euclid(1440);
    info!(
        "{}-{}-{} {}:{}:{} (UTC+{})",
        y,
        mo,
        d,
        minute_of_day / 60,
        minute_of_day % 60,
        unix.rem_euclid(60),
        (local_min - utc_min) / 60
    );
}
