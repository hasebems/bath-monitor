use core::fmt::Write as _;

use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::Stack;
use embassy_time::{with_timeout, Duration};
use heapless::String;
use reqwless::client::HttpClient;
use reqwless::request::Method;

use crate::config::{LED_SYNC_INTERVAL_SECS, NUM_PEOPLE, SERVER_REQUEST_TIMEOUT_SECS};
use crate::led_pattern;
use crate::outbox;
use crate::secrets::SERVER_BASE_URL;
use crate::wifi;

/// Periodically fetches `GET /api/led-state` and reconciles all NeoPixels
/// with the server's `pressed_today` state. This is what clears the LEDs
/// after the server's daily reset and restores correct state after a
/// firmware reboot. Does nothing while Wi-Fi is down, and syncs right away
/// when it comes back up.
///
/// A person whose press or cancel the server hasn't acknowledged yet
/// (`outbox::pending_intent`) keeps the LED that action asked for (lit after
/// a press, off after a cancel), since the server's answer can't know about
/// it yet — otherwise a press made offline would be wiped off the NeoPixels
/// by the first sync after reconnecting, and a cancel would light it again.
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
        if wifi::is_connected() {
            // Snapshot both before and after the request: an action whose POST
            // was acknowledged while this GET was in flight is pending in the
            // first snapshot, one that arrived meanwhile in the second (which
            // wins, being the more recent).
            let pending_before = outbox::pending_intents();

            let fetched = with_timeout(
                Duration::from_secs(SERVER_REQUEST_TIMEOUT_SECS),
                fetch_led_state(&tcp_client, &dns_client, &url),
            )
            .await;

            match fetched {
                Ok(Some(mut pressed)) => {
                    let pending_after = outbox::pending_intents();
                    for (i, slot) in pressed.iter_mut().enumerate() {
                        if let Some(intent) = pending_after[i].or(pending_before[i]) {
                            *slot = intent;
                        }
                    }
                    led_pattern::set_all_pressed(pressed);
                }
                Ok(None) => {}
                Err(_) => log::warn!("led-state poll: timed out"),
            }
        }

        // Next poll after the normal interval, or as soon as Wi-Fi (re)connects.
        let _ = with_timeout(
            Duration::from_secs(LED_SYNC_INTERVAL_SECS),
            wifi::WIFI_UP.wait(),
        )
        .await;
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
