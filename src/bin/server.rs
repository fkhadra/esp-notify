use embassy_net::Stack;
use esp_alloc::HEAP;
use esp_println::println;
use picoserve::extract::Json;
use picoserve::response;
use picoserve::routing::{NotFound, PathRouter, get, post};
use picoserve::{NoGracefulShutdown, Router, Server};
use static_cell::StaticCell;

pub struct Srv<R: PathRouter> {
    router: Router<R>,
}

impl Srv<NotFound> {
    pub fn new() -> Srv<impl PathRouter> {
        Srv {
            router: Router::new()
                .route("/", get(async || "hello from esp-noti\n"))
                .route("/health", get(health))
                .route("/notify", post(notify)),
        }
    }
}

impl<R: PathRouter> Srv<R> {
    pub async fn start(&self, net_stack: Stack<'_>) -> NoGracefulShutdown {
        static HTTP_BUF: StaticCell<[u8; 2048]> = StaticCell::new();
        static TCP_RX: StaticCell<[u8; 1024]> = StaticCell::new();
        static TCP_TX: StaticCell<[u8; 1024]> = StaticCell::new();

        let config = picoserve::Config::default();

        Server::new(&self.router, &config, HTTP_BUF.init([0; 2048]))
            .listen_and_serve(
                "web",
                net_stack,
                80,
                TCP_RX.init([0; 1024]),
                TCP_TX.init([0; 1024]),
            )
            .await
    }
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

#[derive(serde::Deserialize)]
struct Notify {
    title: alloc::string::String,
    body: alloc::string::String,
}

async fn notify(Json(n): Json<Notify>) -> &'static str {
    println!("notify: {} / {}", n.title, n.body);
    "ok\n"
}
