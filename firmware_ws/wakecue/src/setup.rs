//! The phone setup: the lamp raises its own Wi-Fi network and hands a phone an address on it.
//!
//! This is a different program from the one that wakes, reads the clock and sleeps. It needs a
//! heap, a scheduler and the radio, none of which the normal path pays for, so nothing here is
//! started until the owner picks "Phone setup" in the menu. [`run`] never returns: it ends the
//! normal program for good, and Back or a touch resets the chip back into it.
//!
//! So far it answers the question the roadmap's trip-wire asks: can a real phone join and get an
//! address from our own DHCP server. Nothing is served on top yet: no page and no DNS.

use alloc::format;
use defmt::info;
use embassy_executor::Spawner;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{
    IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr, Runner, Stack, StackResources, StaticConfigV4,
};
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Input;
use esp_hal::i2c::master::I2c;
use esp_hal::peripherals::{FROM_CPU_INTR0, TIMG0, WIFI};
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_radio::wifi::ap::{AccessPointConfig, EventInfo};
use esp_radio::wifi::{Config, Interface, WifiController};
use lamp_core::buttons::{self, Key, KeyReader};
use lamp_core::dhcp::{self, Kind, Server};
use static_cell::StaticCell;

use wakecue::display::Face;

/// The lamp's address on its own network. The first of the eight it hands out is `.2`.
const LAMP: [u8; 4] = [192, 168, 4, 1];
const FIRST_HOST: u8 = 2;
const LEASE_S: u32 = 3600;

/// How often the buttons and the pad are looked at while the network is up.
const POLL_MS: u64 = 50;

/// Raise `Wakecue-XXXX` and serve DHCP until Back or a touch resets the chip.
///
/// The panel is drawn once with the network's name and put to sleep, so its picture stays up for
/// as long as the radio runs.
pub fn run(
    mut face: Face,
    wifi: WIFI<'static>,
    timer: TIMG0<'static>,
    interrupt: FROM_CPU_INTR0<'static>,
    bus: I2c<'static, esp_hal::Blocking>,
    pad: Input<'static>,
) -> ! {
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);
    let timg0 = TimerGroup::new(timer);
    esp_rtos::start(timg0.timer0, interrupt);

    let mut controller =
        WifiController::new(wifi, Default::default()).expect("Wi-Fi controller");

    // The name carries the last two bytes of the access point's own MAC, so two lamps in one
    // house raise two different networks and the owner can tell theirs by the sticker.
    let interface = Interface::access_point();
    let mac = interface.mac_address();
    let ssid = format!("Wakecue-{:02X}{:02X}", mac[4], mac[5]);
    info!("setup: raising {}", ssid.as_str());

    // Open on purpose: nothing sensitive crosses it, and there is no password to print on a lamp
    // the owner has not set up yet. Four stations at most; a phone and a laptop is the use.
    let ap = AccessPointConfig::default()
        .with_ssid(ssid.as_str().try_into().expect("SSID fits in 32 bytes"))
        .with_max_connections(4);
    controller.set_config(&Config::AccessPoint(ap)).expect("start the access point");
    info!("setup: access point up on channel 1");

    // The panel last: the radio is already up, so the name on the glass is one a phone can see.
    face.show_setup(ssid.as_str());
    face.finish();

    let address = Ipv4Address::new(LAMP[0], LAMP[1], LAMP[2], LAMP[3]);
    let net_config = embassy_net::Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(address, 24),
        gateway: Some(address),
        dns_servers: Default::default(),
    });
    static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let rng = Rng::new();
    let seed = ((rng.random() as u64) << 32) | rng.random() as u64;
    let (stack, runner) =
        embassy_net::new(interface, net_config, RESOURCES.init(StackResources::new()), seed);

    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    EXECUTOR.init(esp_rtos::embassy::Executor::new()).run(move |spawner: Spawner| {
        spawner.spawn(net_task(runner).unwrap());
        spawner.spawn(dhcp_task(stack).unwrap());
        spawner.spawn(events_task(controller).unwrap());
        spawner.spawn(leave_task(bus, pad).unwrap());
    })
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}

/// Who joined and who left. A phone that joins and then asks for no address is a DHCP fault, and
/// these lines are what tell it from a radio fault.
#[embassy_executor::task]
async fn events_task(controller: WifiController<'static>) -> ! {
    loop {
        match controller.wait_for_access_point_connected_event_async().await {
            Ok(EventInfo::Connected(c)) => {
                info!("setup: station joined, mac {=[u8]:x}, aid {}", &c.mac[..], c.aid)
            }
            Ok(EventInfo::Disconnected(d)) => {
                info!("setup: station left, mac {=[u8]:x}, aid {}", &d.mac[..], d.aid)
            }
            Err(e) => info!("setup: access point event: {:?}", defmt::Debug2Format(&e)),
        }
    }
}

/// Answer every datagram on port 67 from `lamp_core::dhcp`, and send each reply to the broadcast
/// address: the client has no address yet, so it cannot be answered any other way.
#[embassy_executor::task]
async fn dhcp_task(stack: Stack<'static>) -> ! {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 1024];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 1024];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(dhcp::SERVER_PORT).expect("bind the DHCP port");
    info!("setup: dhcp listening on port {}", dhcp::SERVER_PORT);

    let mut server = Server::new(LAMP, FIRST_HOST, LEASE_S);
    let mut request = [0u8; 1024];
    let mut reply = [0u8; dhcp::REPLY_MAX];
    let to = IpEndpoint::new(IpAddress::v4(255, 255, 255, 255), dhcp::CLIENT_PORT);
    loop {
        let n = match socket.recv_from(&mut request).await {
            Ok((n, _)) => n,
            Err(e) => {
                info!("setup: dhcp receive failed: {:?}", defmt::Debug2Format(&e));
                continue;
            }
        };
        let Some(r) = server.handle(&request[..n], &mut reply) else {
            info!("setup: dhcp {} bytes in, no answer", n);
            continue;
        };
        let kind = match r.kind {
            Kind::Offer => "offer",
            Kind::Ack => "ack",
            Kind::Nak => "nak",
        };
        info!("setup: dhcp {} to mac {=[u8]:x}, ip {}", kind, &r.mac[..], r.ip);
        if let Err(e) = socket.send_to(&reply[..r.len], to).await {
            info!("setup: dhcp send failed: {:?}", defmt::Debug2Format(&e));
        }
    }
}

/// Back or a touch ends the setup: a reset puts the lamp back into its normal program, which
/// starts from the clock and the stored settings as it always does.
#[embassy_executor::task]
async fn leave_task(mut bus: I2c<'static, esp_hal::Blocking>, pad: Input<'static>) -> ! {
    let mut reader = KeyReader::default();
    let mut port = [0u8; 1];
    loop {
        Timer::after(Duration::from_millis(POLL_MS)).await;
        if bus.read(buttons::PCF8574_ADDR, &mut port).is_ok() {
            if reader.update(port[0]) == Some(Key::Back) {
                info!("setup: Back, resetting");
                esp_hal::system::software_reset();
            }
        }
        if pad.is_high() {
            info!("setup: touch, resetting");
            esp_hal::system::software_reset();
        }
    }
}
