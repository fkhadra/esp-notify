use alloc::{
    fmt,
    string::String,
};
use embassy_net::{
    Stack,
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use esp_hal::rng::Rng;
use reqwless::{
    client::{HttpClient, TlsConfig},
    request::Method,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Status {
    indicator: String,
    description: String,
}

#[derive(Deserialize)]
struct Page {
    name: String,
    updated_at: String,
    time_zone: String,
}

#[derive(Deserialize)]
struct StatusResponse {
    page: Page,
    status: Status,
}

pub struct ServiceStatus {
    pub name: Service,
    pub description: String,
    pub indicator: String,
    pub updated_at: String,
    pub time_zone: String,
}




#[derive(Clone, Copy)]
pub enum Service {
    Github,
    Anthropic,
    Cloudflare,
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let val = match self {
            Service::Anthropic => "Anthropic",
            Service::Github => "Github",
            Service::Cloudflare => "Cloudflare",
        };

        write!(f, "{val}")
    }
}

impl Service {
    pub fn index(&self) -> usize {
        match self {
            Service::Github => 0,
            Service::Anthropic => 1,
            Service::Cloudflare => 2,
        }
    }

    fn endpoint(&self) -> &str {
        match self {
            Service::Github => "https://www.githubstatus.com/api/v2/status.json",
            Service::Anthropic => "https://status.claude.com/api/v2/status.json",
            Service::Cloudflare => "https://www.cloudflarestatus.com/api/v2/status.json",
        }
    }
}

// static RX_BUFFER: StaticCell<[u8; 16640]> = StaticCell::new();
// static TX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();

pub async fn fetch_status(
    stack: Stack<'_>,
    service: &Service,
) -> Result<ServiceStatus, reqwless::Error> {
    let mut rx_buffer = [0; 16640];
    let mut tx_buffer = [0; 4096];
    let dns = DnsSocket::new(stack);
    let tcp_state = TcpClientState::<1, 4096, 4096>::new();
    let tcp = TcpClient::new(stack, &tcp_state);
    let rng = Rng::new();
    let tls_seed = rng.random() as u64 | ((rng.random() as u64) << 32);

    let tls = TlsConfig::new(
        tls_seed,
        &mut rx_buffer,
        &mut tx_buffer,
        reqwless::client::TlsVerify::None,
    );

    let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);
    let mut buffer = [0u8; 4096];
    let mut http_req = client.request(Method::GET, service.endpoint()).await?;
    let response = http_req.send(&mut buffer).await?;
    let res = response.body().read_to_end().await?;
    let s = serde_json::from_slice::<StatusResponse>(res).expect("deserialize failed");

    Ok(ServiceStatus {
        name: *service,
        description: s.status.description,
        indicator: s.status.indicator,
        updated_at: s.page.updated_at,
        time_zone: s.page.time_zone,
    })
}
