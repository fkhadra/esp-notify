use core::fmt::{self, Display};

use alloc::{format, string::String, vec::Vec};
use anyhow::Result;
use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_time::{Duration, Instant, Ticker};
use serde::Deserialize;

use crate::{fetcher::Fetcher, state::set_state};

#[derive(Deserialize)]
struct Status {
    indicator: String,
    description: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Page {
    updated_at: String,
    time_zone: Option<String>,
}

#[derive(Deserialize)]
struct StatusResponse {
    page: Page,
    status: Status,
}

#[derive(Deserialize)]
struct IncidentsResponse {
    incidents: Vec<Incident>,
}

#[derive(Deserialize)]
pub struct IncidentUpdate {
    pub body: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct Incident {
    pub name: String,
    pub status: String,
    pub impact: Severity,
    pub updated_at: String,
    #[serde(default)]
    pub incident_updates: Vec<IncidentUpdate>,
}

#[derive(PartialEq, Eq, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    None,
    Minor,
    Major,
    Critical,
}

impl Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let val = match self {
            Self::Critical => "critical",
            Self::Major => "major",
            Self::Minor => "minor",
            Self::None => "none",
        };

        write!(f, "{val}")
    }
}

impl From<String> for Severity {
    fn from(value: String) -> Self {
        match value.as_str() {
            "none" => Self::None,
            "minor" => Self::Minor,
            "major" => Self::Major,
            "critical" => Self::Critical,
            _ => Self::None,
        }
    }
}

impl From<u8> for Severity {
    fn from(value: u8) -> Self {
        match value {
            3 => Self::Critical,
            2 => Self::Major,
            1 => Self::Minor,
            _ => Self::None,
        }
    }
}

impl Severity {
    pub fn to_number(&self) -> u8 {
        match self {
            Self::Critical => 3,
            Self::Major => 2,
            Self::Minor => 1,
            Self::None => 0,
        }
    }

    pub fn to_rgb(&self) -> (u8, u8, u8) {
        const BRIGHTNESS: u16 = 50;

        let (r, g, b) = match self {
            Self::None => (0, 255, 0),
            Self::Minor => (255, 140, 0),
            Self::Major => (255, 40, 0),
            Self::Critical => (255, 0, 0),
        };
        let scale = |c: u8| (c as u16 * BRIGHTNESS / 100) as u8;

        (scale(r), scale(g), scale(b))
    }
}

pub struct ServiceStatus {
    pub name: Service,
    pub description: String,
    pub indicator: Severity,
    pub updated_at: String,
    pub time_zone: Option<String>,
    pub incidents: Vec<Incident>,
    pub fetched_at: Instant,
}

impl ServiceStatus {
    pub fn is_healthy(&self) -> bool {
        self.indicator == Severity::None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Github,
    Cloudflare,
    Tailscale,
    Anthropic,
    OpenAI,
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let val = match self {
            Self::Anthropic => "Anthropic",
            Self::Github => "Github",
            Self::Cloudflare => "Cloudflare",
            Self::OpenAI => "OpenAI",
            Self::Tailscale => "Tailscale",
        };

        write!(f, "{val}")
    }
}

impl Service {
    pub const ALL: [Service; 5] = [
        Service::Github,
        Service::Cloudflare,
        Service::Tailscale,
        Service::Anthropic,
        Service::OpenAI,
    ];

    pub fn index(&self) -> usize {
        Self::ALL.iter().position(|s| s == self).unwrap()
    }

    fn host(&self) -> &'static str {
        match self {
            Self::Github => "www.githubstatus.com",
            Self::Anthropic => "status.claude.com",
            Self::Cloudflare => "www.cloudflarestatus.com",
            Self::OpenAI => "status.openai.com",
            Self::Tailscale => "status.tailscale.com",
        }
    }

    fn status_url(&self) -> String {
        self.url("status.json")
    }

    fn incident_url(&self) -> Option<String> {
        match self {
            Self::OpenAI | Self::Tailscale => None,
            _ => Some(self.url("incidents/unresolved.json")),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("https://{}/api/v2/{}", self.host(), path)
    }
}

async fn fetch_status(fetcher: &mut Fetcher, service: &Service) -> Result<ServiceStatus> {
    let res = fetcher.get::<StatusResponse>(&service.status_url()).await?;
    let incidents = match service
        .incident_url()
        .filter(|_| res.status.indicator != "none")
    {
        None => Vec::new(),
        Some(url) => fetcher.get::<IncidentsResponse>(&url).await?.incidents,
    };

    Ok(ServiceStatus {
        name: *service,
        description: res.status.description,
        indicator: res.status.indicator.into(),
        updated_at: res.page.updated_at,
        time_zone: res.page.time_zone,
        fetched_at: Instant::now(),
        incidents,
    })
}

pub fn start_monitoring(spawner: Spawner, net_stack: Stack<'static>) {
    spawner.spawn(monitor_status(net_stack).expect("should monitor status"));
}

#[embassy_executor::task]
async fn monitor_status(net_stack: Stack<'static>) -> ! {
    let mut fetcher = Fetcher::new(net_stack);
    let mut ticker = Ticker::every(Duration::from_secs(30));

    loop {
        for service in &Service::ALL {
            match fetch_status(&mut fetcher, service).await {
                Ok(res) => {
                    set_state(|s| {
                        s.service_statuses[service.index()] = Some(res);
                        s.service_errors[service.index()] = None;
                    })
                    .await;
                }
                Err(err) => {
                    set_state(|s| {
                        s.service_errors[service.index()] = Some(format!("{service}: {err}"));
                    })
                    .await;
                }
            }
        }

        ticker.next().await;
    }
}
