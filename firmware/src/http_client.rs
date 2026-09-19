use core::fmt::Write as _;

use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::Stack;
use embassy_time::{with_timeout, Duration, Timer};
use heapless::String;
use reqwless::client::HttpClient;
use reqwless::headers::ContentType;
use reqwless::request::{Method, RequestBuilder};

use crate::config::{PEOPLE, SEND_RETRY_INTERVAL_SECS, SERVER_REQUEST_TIMEOUT_SECS};
use crate::outbox::{self, Outgoing};
use crate::secrets::SERVER_BASE_URL;
use crate::wifi;

/// What happened to one delivery attempt.
enum Outcome {
    /// The server accepted it (2xx).
    Delivered,
    /// The server refused it (4xx, e.g. an unknown person id): retrying the
    /// same request can't succeed, so it's dropped rather than retried forever.
    Rejected,
    /// No usable answer (couldn't connect, timed out, 5xx, ...): try again.
    Retry,
}

/// Owns the only client that POSTs to the server. Delivers whatever
/// `outbox` says is still owed, one request at a time, but only while Wi-Fi
/// is connected; on failure it keeps the item pending and retries every
/// `SEND_RETRY_INTERVAL_SECS`, so a press made offline (or while the server
/// was down) still gets through eventually. Button/occupancy tasks only
/// record state in `outbox`, so they never wait on any of this.
#[embassy_executor::task]
pub async fn sender_task(stack: Stack<'static>) {
    static CLIENT_STATE: static_cell::StaticCell<TcpClientState<1, 1024, 1024>> =
        static_cell::StaticCell::new();
    let client_state = CLIENT_STATE.init(TcpClientState::new());
    let tcp_client = TcpClient::new(stack, client_state);
    let dns_client = DnsSocket::new(stack);

    loop {
        outbox::WORK.wait().await;

        while wifi::is_connected() {
            let Some(item) = outbox::next() else { break };

            match send(&tcp_client, &dns_client, &item).await {
                Outcome::Delivered => outbox::complete(&item),
                Outcome::Rejected => {
                    log::error!("server rejected a request; dropping it");
                    outbox::complete(&item);
                }
                Outcome::Retry => {
                    Timer::after(Duration::from_secs(SEND_RETRY_INTERVAL_SECS)).await;
                }
            }
        }
    }
}

async fn send<'a>(
    tcp_client: &'a TcpClient<'a, 1, 1024, 1024>,
    dns_client: &'a DnsSocket<'a>,
    item: &Outgoing,
) -> Outcome {
    let mut url: String<128> = String::new();
    let mut body: String<64> = String::new();
    match *item {
        Outgoing::Press { person_idx, .. } => {
            let _ = write!(url, "{}/api/press", SERVER_BASE_URL);
            let _ = write!(body, "{{\"person\":\"{}\"}}", PEOPLE[person_idx]);
        }
        Outgoing::Occupancy { occupied } => {
            let _ = write!(url, "{}/api/occupancy", SERVER_BASE_URL);
            let _ = write!(body, "{{\"occupied\":{}}}", occupied);
        }
    }

    match with_timeout(
        Duration::from_secs(SERVER_REQUEST_TIMEOUT_SECS),
        post(tcp_client, dns_client, &url, &body),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => {
            log::warn!("POST {} timed out", url);
            Outcome::Retry
        }
    }
}

async fn post<'a>(
    tcp_client: &'a TcpClient<'a, 1, 1024, 1024>,
    dns_client: &'a DnsSocket<'a>,
    url: &str,
    body: &str,
) -> Outcome {
    let mut http_client = HttpClient::new(tcp_client, dns_client);
    let mut rx_buffer = [0u8; 512];

    let request = match http_client.request(Method::POST, url).await {
        Ok(req) => req,
        Err(e) => {
            log::warn!("failed to connect for {}: {:?}", url, e);
            return Outcome::Retry;
        }
    };

    let mut request = request
        .body(body.as_bytes())
        .content_type(ContentType::ApplicationJson);

    match request.send(&mut rx_buffer).await {
        Ok(response) => {
            let status = response.status.0;
            log::info!("POST {} -> {}", url, status);
            match status {
                200..=299 => Outcome::Delivered,
                400..=499 => Outcome::Rejected,
                _ => Outcome::Retry,
            }
        }
        Err(e) => {
            log::warn!("failed to send {}: {:?}", url, e);
            Outcome::Retry
        }
    }
}
