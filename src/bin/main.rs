#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use embassy_executor::Spawner;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;

use esp_println::println;
use picoserve::extract::Json;
use picoserve::routing::{get, post};
use static_cell::StaticCell;

use esp_alloc::HEAP;

use crate::screen::init_screen;
use crate::wifi::init_wifi;

mod screen;
mod wifi;

async fn notify(Json(n): Json<Notify>) -> &'static str {
    println!("notify: {} / {}", n.title, n.body);
    "ok\n"
}

#[derive(serde::Deserialize)]
struct Notify {
    title: alloc::string::String,
    body: alloc::string::String,
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // generator version: 1.3.0
    // generator parameters: --chip esp32c6 -o esp32c6-mini-1 -o unstable-hal -o embassy -o alloc -o wifi

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // These GPIO pins are in use by some feature of the module and should not be used.
    let _ = peripherals.GPIO24;
    let _ = peripherals.GPIO25;
    let _ = peripherals.GPIO26;
    let _ = peripherals.GPIO27;
    let _ = peripherals.GPIO28;
    let _ = peripherals.GPIO29;
    let _ = peripherals.GPIO30;

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    // Required for wifi
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    static HTTP_BUF: StaticCell<[u8; 2048]> = StaticCell::new();
    static TCP_RX: StaticCell<[u8; 1024]> = StaticCell::new();
    static TCP_TX: StaticCell<[u8; 1024]> = StaticCell::new();

    let mut screen = init_screen(
        peripherals.SPI2,
        peripherals.GPIO7,
        peripherals.GPIO6,
        peripherals.GPIO14,
        peripherals.GPIO15,
        peripherals.GPIO21,
        peripherals.GPIO22,
    );
    let net_stack = init_wifi(spawner, peripherals.WIFI).await;

    let app = picoserve::Router::new()
        .route("/", get(async || "hello from esp-noti\n"))
        .route("/notify", post(notify));
    let config = picoserve::Config::default();

    println!("Used: {} ", HEAP.stats());

    picoserve::Server::new(&app, &config, HTTP_BUF.init([0; 2048]))
        .listen_and_serve(
            "web",
            net_stack,
            80,
            TCP_RX.init([0; 1024]),
            TCP_TX.init([0; 1024]),
        )
        .await
        .into_never()

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples
}
