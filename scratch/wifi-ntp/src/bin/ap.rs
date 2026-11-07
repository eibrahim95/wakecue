//! The lamp's own network: raise `Wakecue-XXXX`, take 192.168.4.1, and hand a phone an address.
//!
//! This is roadmap move 1's trip-wire, tried on its own before anything is built on it. The gate
//! it answers: a real phone joins the network and gets an address from our own DHCP server. Join
//! it, and the log shows the station joining and then the DHCP exchange (DISCOVER -> offer,
//! REQUEST -> ack); the phone's Wi-Fi screen shows 192.168.4.x. Nothing is served on top yet: no
//! page, no DNS, no captive-portal prompt.
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

use alloc::format;
use defmt::info;
use embassy_executor::Spawner;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{
    IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr, Runner, Stack, StackResources, StaticConfigV4,
};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_println as _;
use esp_radio::wifi::ap::{AccessPointConfig, EventInfo};
use esp_radio::wifi::{Config, Interface, WifiController};
use lamp_core::dhcp::{self, Kind, Server};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

/// The lamp's address on its own network, and the first of the eight it hands out is `.2`.
const LAMP: [u8; 4] = [192, 168, 4, 1];
const FIRST_HOST: u8 = 2;
const LEASE_S: u32 = 3600;

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    let mut controller =
        WifiController::new(peripherals.WIFI, Default::default()).expect("Wi-Fi controller");

    // The name carries the last two bytes of the access point's own MAC, so two lamps in one
    // house raise two different networks and the owner can tell theirs by the sticker.
    let interface = Interface::access_point();
    let mac = interface.mac_address();
    let ssid = format!("Wakecue-{:02X}{:02X}", mac[4], mac[5]);
    info!("raising {}", ssid.as_str());

    // Open on purpose: nothing sensitive crosses it, and there is no password to print on a lamp
    // the owner has not set up yet. Four stations at most; a phone and a laptop is the use.
    let ap = AccessPointConfig::default()
        .with_ssid(ssid.as_str().try_into().expect("SSID fits in 32 bytes"))
        .with_max_connections(4);
    controller.set_config(&Config::AccessPoint(ap)).expect("start the access point");
    info!("access point up on channel 1");

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
    spawner.spawn(net_task(runner).unwrap());
    spawner.spawn(dhcp_task(stack).unwrap());

    // The access point's own events: who joined and who left. A phone that joins and then asks
    // for no address is a DHCP fault, and this is the line that tells it from a radio fault.
    loop {
        match controller.wait_for_access_point_connected_event_async().await {
            Ok(EventInfo::Connected(c)) => {
                info!("station joined: mac {=[u8]:x}, aid {}", &c.mac[..], c.aid)
            }
            Ok(EventInfo::Disconnected(d)) => {
                info!("station left: mac {=[u8]:x}, aid {}", &d.mac[..], d.aid)
            }
            Err(e) => info!("access point event: {:?}", defmt::Debug2Format(&e)),
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
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
    info!("dhcp: listening on port {}", dhcp::SERVER_PORT);

    let mut server = Server::new(LAMP, FIRST_HOST, LEASE_S);
    let mut request = [0u8; 1024];
    let mut reply = [0u8; dhcp::REPLY_MAX];
    let to = IpEndpoint::new(IpAddress::v4(255, 255, 255, 255), dhcp::CLIENT_PORT);
    loop {
        let n = match socket.recv_from(&mut request).await {
            Ok((n, _)) => n,
            Err(e) => {
                info!("dhcp: receive failed: {:?}", defmt::Debug2Format(&e));
                continue;
            }
        };
        let Some(r) = server.handle(&request[..n], &mut reply) else {
            info!("dhcp: {} bytes in, no answer", n);
            continue;
        };
        let kind = match r.kind {
            Kind::Offer => "offer",
            Kind::Ack => "ack",
            Kind::Nak => "nak",
        };
        info!("dhcp: {} to mac {=[u8]:x}, ip {}", kind, &r.mac[..], r.ip);
        if let Err(e) = socket.send_to(&reply[..r.len], to).await {
            info!("dhcp: send failed: {:?}", defmt::Debug2Format(&e));
        }
    }
}
