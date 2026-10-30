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
use lamp_core::calendar::{civil_from_days, utc_to_egypt};
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
        .dns_query("pool.ntp.org", embassy_net::dns::DnsQueryType::A)
        .await
        .expect("DNS lookup failed");
    info!("NTP server: {}", addrs[0]);
    match ntp_unix_seconds(stack, addrs[0]).await {
        Some(unix) => {
            info!("Unix time: {}", unix);
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
        None => info!("NTP failed"),
    }

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(10)).await;
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.2.2/examples
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
