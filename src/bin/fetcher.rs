use anyhow::Result;
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
use serde::de::DeserializeOwned;
use static_cell::ConstStaticCell;

const TLS_RX: usize = 16640;
const TLS_TX: usize = 4096;
const BODY: usize = 8192;

type TcpState = TcpClientState<1, 4096, 4096>;

pub struct Fetcher {
    rx: &'static mut [u8; TLS_RX],
    tx: &'static mut [u8; TLS_TX],
    body: &'static mut [u8; BODY],
    tcp_state: &'static TcpState,
    stack: Stack<'static>,
}

impl Fetcher {
    pub fn new(stack: Stack<'static>) -> Self {
        static RX: ConstStaticCell<[u8; TLS_RX]> = ConstStaticCell::new([0; TLS_RX]);
        static TX: ConstStaticCell<[u8; TLS_TX]> = ConstStaticCell::new([0; TLS_TX]);
        static BODY_BUF: ConstStaticCell<[u8; BODY]> = ConstStaticCell::new([0; BODY]);
        static TCP: ConstStaticCell<TcpState> = ConstStaticCell::new(TcpState::new());

        Self {
            rx: RX.take(),
            tx: TX.take(),
            body: BODY_BUF.take(),
            tcp_state: TCP.take(),
            stack,
        }
    }

    pub async fn get<T: DeserializeOwned>(&mut self, url: &str) -> Result<T> {
        let dns = DnsSocket::new(self.stack);
        let tcp = TcpClient::new(self.stack, self.tcp_state);
        let rng = Rng::new();
        let tls_seed = rng.random() as u64 | ((rng.random() as u64) << 32);

        let tls = TlsConfig::new(
            tls_seed,
            self.rx.as_mut_slice(),
            self.tx.as_mut_slice(),
            reqwless::client::TlsVerify::None,
        );

        let mut client = HttpClient::new_with_tls(&tcp, &dns, tls);
        let body = self.body.as_mut_slice();
        let mut req = client.request(Method::GET, &url).await?;
        let response = req.send(&mut *body).await?;
        let response_body = response.body().read_to_end().await?;

        let parsed_body: T = serde_json::from_slice(response_body)?;

        Ok(parsed_body)
    }
}
