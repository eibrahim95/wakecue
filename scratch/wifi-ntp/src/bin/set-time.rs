//! Set the DS3231 from NTP, then stay awake.
//!
//! No alarm, no sleep. The board keeps running so the USB port never disappears,
//! which matters while the clock has no coin cell: unplugging it is what costs the
//! time being set. Flash this, watch the time correct itself, then flash whatever
//! you want next — without cutting the power in between.

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
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, Runner, Stack, StackResources};
use embassy_time::{Duration, Timer, with_timeout};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::i2c::master::{Config as I2Config, I2c};
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_println as _;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethodConfig, Config, Interface};
use lamp_core::calendar::{civil_from_days, utc_to_egypt};
use lamp_core::ds3231;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const NTP_UNIX_OFFSET: i64 = 2_208_988_800;

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    info!("Embassy initialized!");

    let mut bus = I2c::new(peripherals.I2C0, I2Config::default())
        .unwrap()
        .with_sda(peripherals.GPIO6)
        .with_scl(peripherals.GPIO7);

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
    let wifi_interface = Interface::station();
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

    // Everything network-shaped is optional and bounded: if the router or the server
    // says nothing, the clock is left alone rather than being fed a wrong time.
    let unix = with_timeout(Duration::from_secs(60), async {
        wifi_controller.connect_async().await.ok()?;
        info!("Wi-Fi connected");
        stack.wait_config_up().await;
        info!("IP: {}", stack.config_v4()?.address);
        let addrs = stack
            .dns_query("time.google.com", embassy_net::dns::DnsQueryType::A)
            .await
            .ok()?;
        let server = *addrs.first()?;
        info!("NTP server: {}", server);
        ntp_unix_seconds(stack, server).await
    })
    .await
    .ok()
    .flatten();

    match unix {
        Some(unix) => {
            info!("Unix time: {}", unix);
            let regs = ds3231::time_registers(unix);
            let mut frame = [0u8; 8];
            frame[0] = ds3231::REG_TIME;
            frame[1..].copy_from_slice(&regs);
            bus.write(ds3231::I2C_ADDR, &frame).expect("write time");

            // A fresh time is the sign-off: only now may OSF be cleared. A1F goes too,
            // so a stale alarm cannot claim to be due.
            let mut st = [0u8; 1];
            bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_STATUS], &mut st)
                .expect("status");
            bus.write(
                ds3231::I2C_ADDR,
                &[
                    ds3231::REG_STATUS,
                    st[0] & !(ds3231::STATUS_A1F | ds3231::STATUS_OSF),
                ],
            )
            .expect("clear flags");
            info!("time written, osf and a1f cleared");
        }
        None => info!("no time from the network; the clock was left alone"),
    }

    // Stay awake on purpose. The USB port stays up, the clock keeps its time as long as
    // the board has power, and nothing resets on the way to the next flash.
    loop {
        let mut t = [0u8; 7];
        bus.write_read(ds3231::I2C_ADDR, &[ds3231::REG_TIME], &mut t)
            .expect("read time");
        let now = ds3231::parse_time_registers(t);
        info!("clock: {}, {}", t, now);
        print_utc(&now);
        Timer::after(Duration::from_secs(1)).await;
    }
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
    socket.bind(0).ok()?;

    let mut request = [0u8; 48];
    request[0] = 0x1B;
    socket.send_to(&request, (server, 123)).await.ok()?;

    let mut reply = [0u8; 48];
    let (len, _) = with_timeout(Duration::from_secs(5), socket.recv_from(&mut reply))
        .await
        .ok()?
        .ok()?;
    if len < 48 || reply[1] == 0 {
        return None;
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
