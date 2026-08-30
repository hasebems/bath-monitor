use core::fmt::Write as _;

use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::Stack;
use heapless::String;
use reqwless::client::HttpClient;
use reqwless::headers::ContentType;
use reqwless::request::{Method, RequestBuilder};

use crate::config::PEOPLE;
use crate::events::{AppEvent, EVENT_CHANNEL};
use crate::secrets::SERVER_BASE_URL;

/// Owns the only network client in the firmware; drains `EVENT_CHANNEL` and
/// POSTs each event to the server, serially. This is also what naturally
/// rate-limits outgoing requests, since button/occupancy tasks never block
/// on network I/O themselves.
#[embassy_executor::task]
pub async fn sender_task(stack: Stack<'static>) {
    static CLIENT_STATE: static_cell::StaticCell<TcpClientState<1, 1024, 1024>> =
        static_cell::StaticCell::new();
    let client_state = CLIENT_STATE.init(TcpClientState::new());
    let tcp_client = TcpClient::new(stack, client_state);
    let dns_client = DnsSocket::new(stack);

    loop {
        let event = EVENT_CHANNEL.receive().await;

        let mut url: String<128> = String::new();
        let mut body: String<64> = String::new();
        match event {
            AppEvent::ButtonPressed { person_idx } => {
                let _ = write!(url, "{}/api/press", SERVER_BASE_URL);
                let _ = write!(body, "{{\"person\":\"{}\"}}", PEOPLE[person_idx]);
            }
            AppEvent::OccupancyChanged { occupied } => {
                let _ = write!(url, "{}/api/occupancy", SERVER_BASE_URL);
                let _ = write!(body, "{{\"occupied\":{}}}", occupied);
            }
        }

        post(&tcp_client, &dns_client, &url, &body).await;
    }
}

async fn post<'a>(
    tcp_client: &'a TcpClient<'a, 1, 1024, 1024>,
    dns_client: &'a DnsSocket<'a>,
    url: &str,
    body: &str,
) {
    let mut http_client = HttpClient::new(tcp_client, dns_client);
    let mut rx_buffer = [0u8; 512];

    let request = match http_client.request(Method::POST, url).await {
        Ok(req) => req,
        Err(e) => {
            log::warn!("failed to connect for {}: {:?}", url, e);
            return;
        }
    };

    let mut request = request
        .body(body.as_bytes())
        .content_type(ContentType::ApplicationJson);

    match request.send(&mut rx_buffer).await {
        Ok(response) => log::info!("POST {} -> {}", url, response.status.0),
        Err(e) => log::warn!("failed to send {}: {:?}", url, e),
    }
}
