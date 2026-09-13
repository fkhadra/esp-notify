use alloc::string::String;
use embassy_net::Ipv4Cidr;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal};

use crate::web_mon::{Service, ServiceStatus, Severity};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Detail(usize),
    Network,
}

impl Page {
    pub fn next(self) -> Page {
        match self {
            Page::Overview => Page::Detail(0),
            Page::Detail(i) if i + 1 < Service::ALL.len() => Page::Detail(i + 1),
            Page::Detail(_) => Page::Overview,
            Page::Network => Page::Overview,
        }
    }
}

pub struct State {
    pub service_statuses: [Option<ServiceStatus>; Service::ALL.len()],
    pub service_errors: [Option<String>; Service::ALL.len()],
    pub page: Page,
    pub ip: Option<Ipv4Cidr>,
    pub has_notification: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            service_statuses: [const { None }; Service::ALL.len()],
            service_errors: [const { None }; Service::ALL.len()],
            page: Page::Network,
            ip: None,
            has_notification: false,
        }
    }

    pub fn worst_severity(&self) -> Severity {
        self.service_statuses
            .iter()
            .filter_map(|v| v.as_ref())
            .map(|status| status.indicator.to_number())
            .max()
            .unwrap_or(0)
            .into()
    }
}

pub static STATE: Mutex<CriticalSectionRawMutex, State> = Mutex::new(State::new());
pub static RERENDER: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub async fn set_state<F: FnOnce(&mut State)>(f: F) {
    f(&mut *STATE.lock().await);
    RERENDER.signal(());
}
