use alloc::vec::{self, Vec};
use embassy_net::Stack;
use esp_alloc::HEAP;

use picoserve::extract::Json;
use picoserve::response::{self, StatusCode};
use picoserve::routing::{get, post};
use picoserve::{NoGracefulShutdown, Router, Server};
use static_cell::StaticCell;

use crate::state::{Notification, Page, set_state};

pub async fn start_server(net_stack: Stack<'_>) -> NoGracefulShutdown {
    let router = Router::new()
        .route("/health", get(health))
        .route("/notify", post(notify));

    static HTTP_BUF: StaticCell<[u8; 2048]> = StaticCell::new();
    static TCP_RX: StaticCell<[u8; 1024]> = StaticCell::new();
    static TCP_TX: StaticCell<[u8; 1024]> = StaticCell::new();

    let config = picoserve::Config::default();

    Server::new(&router, &config, HTTP_BUF.init([0; 2048]))
        .listen_and_serve(
            "web",
            net_stack,
            80,
            TCP_RX.init([0; 1024]),
            TCP_TX.init([0; 1024]),
        )
        .await
}

#[derive(serde::Serialize)]
struct Health {
    status: &'static str,
    heap_free: usize,
    heap_used: usize,
}

async fn health() -> response::Json<Health> {
    response::Json(Health {
        status: "ok",
        heap_free: HEAP.free(),
        heap_used: HEAP.used(),
    })
}

async fn notify(Json(notification): Json<Notification>) -> StatusCode {
    set_state(|s| {
        s.notifications.get_or_insert(Vec::new()).push(notification);
        s.page = Page::Notification;
    })
    .await;

    StatusCode::CREATED
}
