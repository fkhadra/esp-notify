use alloc::{string::String, vec::Vec};
use embassy_net::Ipv4Cidr;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal};
use serde::Deserialize;

use crate::web_mon::{Service, ServiceStatus, Severity};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Overview,
    Detail(usize),
    Network,
    Notification,
}

impl Page {
    pub fn next(self) -> Page {
        match self {
            Page::Overview => Page::Detail(0),
            Page::Detail(i) if i + 1 < Service::ALL.len() => Page::Detail(i + 1),
            Page::Detail(_) => Page::Overview,
            Page::Network | Page::Notification => Page::Overview,
        }
    }
}

#[derive(Deserialize)]
pub struct Notification {
    pub title: String,
    pub content: String,
}

pub struct State {
    pub service_statuses: [Option<ServiceStatus>; Service::ALL.len()],
    pub service_errors: [Option<String>; Service::ALL.len()],
    pub page: Page,
    pub ip: Option<Ipv4Cidr>,
    pub notifications: Option<Vec<Notification>>,
}

impl State {
    const fn new() -> Self {
        Self {
            service_statuses: [const { None }; Service::ALL.len()],
            service_errors: [const { None }; Service::ALL.len()],
            page: Page::Network,
            ip: None,
            notifications: None,
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
