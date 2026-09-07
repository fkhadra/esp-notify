#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use esp_hal::clock::CpuClock;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;

use esp_println::println;

use embedded_graphics::mono_font::ascii::FONT_10X20;
use esp_alloc::HEAP;
use mousefood::{EmbeddedBackend, EmbeddedBackendConfig};
use ratatui::widgets::{Block, Paragraph};
use ratatui::{Frame, Terminal};
use reqwless::client::{HttpClient, TlsConfig};

use crate::screen::init_screen;
use crate::server::Srv;
use crate::wifi::init_wifi;

mod screen;
mod server;
mod wifi;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {}", info);
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
    esp_alloc::heap_allocator!(size: 36 * 1024);

    // Required for wifi
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    let mut screen = init_screen(
        peripherals.LEDC,
        peripherals.SPI2,
        peripherals.GPIO7,
        peripherals.GPIO6,
        peripherals.GPIO14,
        peripherals.GPIO15,
        peripherals.GPIO21,
        peripherals.GPIO22,
    );
    screen.set_brightness(10);

    let backend_config = EmbeddedBackendConfig {
        font_regular: FONT_10X20,
        ..Default::default()
    };
    let be = EmbeddedBackend::new(&mut screen.display, backend_config);
    let mut terminal = Terminal::new(be).expect("Hopefully it works");

    terminal.draw(draw).expect("msg");

    let net_stack = init_wifi(spawner, peripherals.WIFI).await;

    access_website(net_stack).await;

    loop {}

    // let server = Srv::new();
    // server.start(net_stack).await.into_never()

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples
}

fn draw(frame: &mut Frame) {
    let block = Block::bordered().title("Mousefood");
    let paragraph = Paragraph::new("Initializing network").block(block);
    frame.render_widget(paragraph, frame.area());
}

async fn access_website(stack: Stack<'_>) {
    let mut rx_buffer = [0; 4096];
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
    let mut http_req = client
        .request(
            reqwless::request::Method::GET,
            "https://jsonplaceholder.typicode.com/posts/1",
        )
        .await
        .unwrap();
    let response = http_req.send(&mut buffer).await.unwrap();

    println!("Got response");
    let res = response.body().read_to_end().await.unwrap();

    let content = core::str::from_utf8(res).unwrap();
    println!("{}", content);
}
