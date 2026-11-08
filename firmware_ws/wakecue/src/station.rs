//! Joining the owner's Wi-Fi network to ask for the time.
//!
//! The finished lamp does this rarely: only on a boot where the clock says it lost power, because
//! otherwise its registers can be trusted and the radio never starts. The whole exchange has a
//! deadline and every failure inside it means "no time from the network", so a wrong password or a
//! router that is off can never keep the lamp awake or stop the boot from finishing.

use alloc::string::String;
use defmt::info;
use embassy_futures::block_on;
use embassy_futures::select::{Either, select};
use embassy_net::dns::DnsQueryType;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, Stack, StackResources};
use embassy_time::{Duration, with_timeout};
use esp_hal::rng::Rng;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethodConfig, Config, Interface, WifiController};
use lamp_core::ntp;
use static_cell::StaticCell;

use crate::radio::Radio;

/// The whole join, lookup and question must be done inside this.
const DEADLINE: Duration = Duration::from_secs(30);

/// How long to wait for the server's answer once the question is out.
const ANSWER: Duration = Duration::from_secs(5);

/// Join `ssid` and return the current Unix time in seconds, or `None` for any failure.
///
/// Call it once per boot: it takes the station interface, and the network stack it builds lives in
/// a static. An empty `psk` means an open network.
pub fn sync(radio: &mut Radio, ssid: &str, psk: &str) -> Option<i64> {
    let authentication = if psk.is_empty() {
        AuthenticationMethodConfig::Open
    } else {
        AuthenticationMethodConfig::Wpa2Personal(psk.try_into().ok()?)
    };
    let station = Config::Station(
        StationConfig::default()
            .with_ssid(String::from(ssid).try_into().ok()?)
            .with_authentication(authentication),
    );
    radio.controller.set_config(&station).ok()?;

    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let rng = Rng::new();
    let seed = ((rng.random() as u64) << 32) | rng.random() as u64;
    let (stack, mut runner) = embassy_net::new(
        Interface::station(),
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );

    // The network stack has to be polled while the exchange waits on it, and nothing else runs in
    // this boot that could poll it, so the two futures are polled together right here. The runner
    // never finishes; the exchange or the deadline does.
    let outcome = block_on(with_timeout(
        DEADLINE,
        select(runner.run(), exchange(stack, &mut radio.controller)),
    ));
    match outcome {
        Ok(Either::Second(unix)) => unix,
        _ => {
            info!("station: no time within {} s", DEADLINE.as_secs());
            None
        }
    }
}

async fn exchange(stack: Stack<'_>, controller: &mut WifiController<'static>) -> Option<i64> {
    if let Err(e) = controller.connect_async().await {
        info!("station: could not join: {:?}", defmt::Debug2Format(&e));
        return None;
    }
    info!("station: joined");
    stack.wait_config_up().await;
    let addresses = stack.dns_query("time.google.com", DnsQueryType::A).await.ok()?;
    let server = *addresses.first()?;
    info!("station: asking {}", server);
    ask(stack, server).await
}

async fn ask(stack: Stack<'_>, server: IpAddress) -> Option<i64> {
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx_buf = [0u8; 128];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_buf = [0u8; 128];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    socket.bind(0).ok()?;
    socket.send_to(&ntp::request(), (server, ntp::PORT)).await.ok()?;
    let mut reply = [0u8; 48];
    let (len, _) = with_timeout(ANSWER, socket.recv_from(&mut reply)).await.ok()?.ok()?;
    ntp::parse_reply(&reply[..len])
}
