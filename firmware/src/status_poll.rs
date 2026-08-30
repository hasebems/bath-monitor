use core::fmt::Write as _;

use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use heapless::String;
use reqwless::client::HttpClient;
use reqwless::request::Method;

use crate::config::{LED_SYNC_INTERVAL_SECS, NUM_PEOPLE};
use crate::events::{LedEvent, LED_CHANNEL};
use crate::secrets::SERVER_BASE_URL;

/// Periodically fetches `GET /api/led-state` and reconciles all NeoPixels
/// with the server's `pressed_today` state. This is what clears the LEDs
/// after the server's daily reset and restores correct state after a
/// firmware reboot.
#[embassy_executor::task]
pub async fn status_poll_task(stack: Stack<'static>) {
    static CLIENT_STATE: static_cell::StaticCell<TcpClientState<1, 512, 512>> =
        static_cell::StaticCell::new();
    let client_state = CLIENT_STATE.init(TcpClientState::new());
    let tcp_client = TcpClient::new(stack, client_state);
    let dns_client = DnsSocket::new(stack);

    let mut url: String<128> = String::new();
    let _ = write!(url, "{}/api/led-state", SERVER_BASE_URL);

    loop {
        if let Some(pressed) = fetch_led_state(&tcp_client, &dns_client, &url).await {
            LED_CHANNEL.send(LedEvent::Sync { pressed }).await;
        }
        Timer::after(Duration::from_secs(LED_SYNC_INTERVAL_SECS)).await;
    }
}

async fn fetch_led_state<'a>(
    tcp_client: &'a TcpClient<'a, 1, 512, 512>,
    dns_client: &'a DnsSocket<'a>,
    url: &str,
) -> Option<[bool; NUM_PEOPLE]> {
    let mut http_client = HttpClient::new(tcp_client, dns_client);
    let mut rx_buffer = [0u8; 512];

    let mut request = match http_client.request(Method::GET, url).await {
        Ok(req) => req,
        Err(e) => {
            log::warn!("led-state poll: failed to connect: {:?}", e);
            return None;
        }
    };

    let response = match request.send(&mut rx_buffer).await {
        Ok(resp) => resp,
        Err(e) => {
            log::warn!("led-state poll: request failed: {:?}", e);
            return None;
        }
    };

    let body = match response.body().read_to_end().await {
        Ok(b) => b,
        Err(e) => {
            log::warn!("led-state poll: failed to read body: {:?}", e);
            return None;
        }
    };

    if body.len() < NUM_PEOPLE {
        log::warn!("led-state poll: short response ({} bytes)", body.len());
        return None;
    }

    let mut pressed = [false; NUM_PEOPLE];
    for (i, slot) in pressed.iter_mut().enumerate() {
        *slot = body[i] == b'1';
    }
    Some(pressed)
}
