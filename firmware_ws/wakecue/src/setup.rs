//! The phone setup: the lamp raises its own Wi-Fi network and hands a phone an address on it.
//!
//! This is a different program from the one that wakes, reads the clock and sleeps. It needs a
//! heap, a scheduler and the radio, none of which the normal path pays for, so nothing here is
//! started until the owner picks "Phone setup" in the menu, or the lamp finds nothing stored at all
//! and so has never been set up. [`run`] never returns: it ends the normal program for good, and
//! Back or a touch resets the chip back into it.
//!
//! Three small servers share the network: DHCP gives the phone an address, DNS answers every name
//! with the lamp's own address, and a web server on port 80 serves the one setup page. The phone
//! notices that every site is the lamp and offers to "sign in", which opens the page by itself.
//! What the page changes is written to flash the same way the menu does it.

use alloc::format;
use defmt::info;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{
    IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr, Runner, Stack, StackResources, StaticConfigV4,
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer, with_timeout};
use esp_hal::gpio::Input;
use esp_hal::i2c::master::I2c;
use esp_hal::rng::Rng;
use esp_radio::wifi::ap::{AccessPointConfig, EventInfo};
use esp_radio::wifi::{Config, Interface, WifiController};
use esp_storage::FlashStorage;
use lamp_core::buttons::{self, Key, KeyReader};
use lamp_core::dhcp::{self, Kind, Server};
use lamp_core::dns;
use lamp_core::http::{self, Parsed};
use lamp_core::portal::{self, BODY_MAX, HEAD_MAX};
use lamp_core::settings::{Settings, Stored};
use static_cell::StaticCell;
use wakecue::display::Face;

use crate::radio::Radio;

/// The lamp's address on its own network. The first of the eight it hands out is `.2`.
const LAMP: [u8; 4] = [192, 168, 4, 1];
const FIRST_HOST: u8 = 2;
const LEASE_S: u32 = 3600;

/// How often the buttons and the pad are looked at while the network is up.
const POLL_MS: u64 = 50;

/// The largest request the page takes: headers and form together. A phone's headers run to a few
/// hundred bytes and the form to a few hundred more.
const REQUEST_MAX: usize = 2048;

/// How long a connection may take to send its request, and to take the reply.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// What the page's task and the leave task both touch: the flash, what is in it, and the settings
/// the page is editing. One lock, so a save and a skip can never write at the same moment.
struct Shared {
    flash: FlashStorage<'static>,
    sectors: [Option<Stored>; 2],
    settings: Settings,
}

type State = Mutex<CriticalSectionRawMutex, Shared>;

/// Raise `Wakecue-XXXX` and serve the setup page until Back or a touch resets the chip.
///
/// `first_boot` says the lamp had nothing stored: leaving then writes the settings as they stand,
/// with the place marked as decided, so a lamp whose owner skipped the setup does not ask again at
/// the next power-on. From the menu, leaving writes nothing: the page has already stored what it
/// was given.
///
/// The panel is drawn once with the network's name and put to sleep, so its picture stays up for
/// as long as the radio runs.
#[allow(clippy::too_many_arguments)]
pub fn run(
    mut face: Face,
    radio: Radio,
    bus: I2c<'static, esp_hal::Blocking>,
    pad: Input<'static>,
    flash: FlashStorage<'static>,
    sectors: [Option<Stored>; 2],
    settings: Settings,
    first_boot: bool,
) -> ! {
    let mut controller = radio.controller;

    // The name carries the last two bytes of the access point's own MAC, so two lamps in one
    // house raise two different networks and the owner can tell theirs by the sticker.
    let interface = Interface::access_point();
    let mac = interface.mac_address();
    let ssid = format!("Wakecue-{:02X}{:02X}", mac[4], mac[5]);
    info!("setup: raising {}", ssid.as_str());

    // Open on purpose: nothing sensitive is kept on it, and there is no password to print on a
    // lamp the owner has not set up yet. Four stations at most; a phone and a laptop is the use.
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

    static STATE: StaticCell<State> = StaticCell::new();
    let state: &'static State = STATE.init(Mutex::new(Shared { flash, sectors, settings }));

    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    EXECUTOR.init(esp_rtos::embassy::Executor::new()).run(move |spawner: Spawner| {
        spawner.spawn(net_task(runner).unwrap());
        spawner.spawn(dhcp_task(stack).unwrap());
        spawner.spawn(dns_task(stack).unwrap());
        spawner.spawn(http_task(stack, state).unwrap());
        spawner.spawn(events_task(controller).unwrap());
        spawner.spawn(leave_task(bus, pad, state, first_boot).unwrap());
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

/// Answer every name with the lamp's own address.
#[embassy_executor::task]
async fn dns_task(stack: Stack<'static>) -> ! {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 1024];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 1024];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(dns::PORT).expect("bind the DNS port");
    info!("setup: dns listening on port {}", dns::PORT);

    let mut query = [0u8; 512];
    let mut reply = [0u8; dns::REPLY_MAX];
    loop {
        let (n, from) = match socket.recv_from(&mut query).await {
            Ok(got) => got,
            Err(e) => {
                info!("setup: dns receive failed: {:?}", defmt::Debug2Format(&e));
                continue;
            }
        };
        if let Some(len) = dns::answer(&query[..n], LAMP, &mut reply)
            && let Err(e) = socket.send_to(&reply[..len], from).await
        {
            info!("setup: dns send failed: {:?}", defmt::Debug2Format(&e));
        }
    }
}

/// Write all of `data`, a piece at a time: the socket's buffer is smaller than the page.
async fn send_all(socket: &mut TcpSocket<'_>, mut data: &[u8]) -> bool {
    while !data.is_empty() {
        match with_timeout(IO_TIMEOUT, socket.write(data)).await {
            Ok(Ok(n)) if n > 0 => data = &data[n..],
            _ => return false,
        }
    }
    true
}

/// The setup page, one connection at a time. A phone that opens a second connection while this one
/// is busy is refused and tries again, which the portal checks are happy to do.
///
/// The page edits the shared settings, and only a form that passed every check is written, the
/// same way the menu stores a change.
#[embassy_executor::task]
async fn http_task(stack: Stack<'static>, state: &'static State) -> ! {
    let mut rx_buf = [0u8; 1024];
    let mut tx_buf = [0u8; 2048];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    let mut request = [0u8; REQUEST_MAX];
    let mut head = [0u8; HEAD_MAX];
    let mut body = [0u8; BODY_MAX];
    info!("setup: http listening on port 80");
    loop {
        if let Err(e) = socket.accept(80).await {
            info!("setup: http accept failed: {:?}", defmt::Debug2Format(&e));
            continue;
        }

        // Read until the request is whole, or has failed, or has outgrown its buffer.
        let mut have = 0;
        let parsed = loop {
            match http::parse(&request[..have]) {
                Parsed::Incomplete => {}
                done => break Some(done),
            }
            if have == request.len() {
                break None;
            }
            match with_timeout(IO_TIMEOUT, socket.read(&mut request[have..])).await {
                Ok(Ok(n)) if n > 0 => have += n,
                _ => break Some(Parsed::Bad),
            }
        };

        let (head_len, body_len) = match parsed {
            Some(Parsed::Complete(req)) => {
                let mut shared = state.lock().await;
                let Shared { flash, sectors, settings } = &mut *shared;
                let reply = portal::respond(&req, settings, &mut head, &mut body);
                info!(
                    "setup: http {} {}, {} bytes back, saved {}",
                    match req.method {
                        http::Method::Get => "GET",
                        http::Method::Post => "POST",
                        http::Method::Other => "other",
                    },
                    req.path,
                    reply.body,
                    reply.saved
                );
                if reply.saved {
                    // `respond` has changed the shared copy; the flash write is what makes it
                    // outlast the reset that ends the setup.
                    if !crate::store(flash, sectors, *settings) {
                        info!("setup: the change could not be written to flash");
                    }
                }
                (reply.head, reply.body)
            }
            Some(_) => (
                http::head(&mut head, http::Status::BadRequest, "text/plain", 0, None).unwrap_or(0),
                0,
            ),
            None => (
                http::head(&mut head, http::Status::PayloadTooLarge, "text/plain", 0, None)
                    .unwrap_or(0),
                0,
            ),
        };
        let _ = send_all(&mut socket, &head[..head_len]).await
            && send_all(&mut socket, &body[..body_len]).await;
        let _ = with_timeout(IO_TIMEOUT, socket.flush()).await;
        socket.close();
        let _ = with_timeout(IO_TIMEOUT, socket.flush()).await;
        socket.abort();
    }
}

/// Back or a touch ends the setup: a reset puts the lamp back into its normal program, which
/// starts from the clock and the stored settings as it always does.
///
/// On a first boot that is also the skip. The lamp would otherwise find nothing stored at the next
/// reset and start the setup again, so the settings as they stand (the defaults, or what the page
/// stored) are written once more with the place marked as decided.
#[embassy_executor::task]
async fn leave_task(
    mut bus: I2c<'static, esp_hal::Blocking>,
    pad: Input<'static>,
    state: &'static State,
    first_boot: bool,
) -> ! {
    let mut reader = KeyReader::default();
    let mut port = [0u8; 1];
    loop {
        Timer::after(Duration::from_millis(POLL_MS)).await;
        let back = bus.read(buttons::PCF8574_ADDR, &mut port).is_ok()
            && reader.update(port[0]) == Some(Key::Back);
        let touch = pad.is_high();
        if !(back || touch) {
            continue;
        }
        info!("setup: {}, resetting", if back { "Back" } else { "touch" });
        if first_boot {
            let mut shared = state.lock().await;
            let Shared { flash, sectors, settings } = &mut *shared;
            settings.setup.place = true;
            if !crate::store(flash, sectors, *settings) {
                info!("setup: the skip could not be written to flash");
            }
        }
        esp_hal::system::software_reset();
    }
}
