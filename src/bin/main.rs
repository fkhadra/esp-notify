#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use alloc::vec::Vec;
use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Ticker};
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;

use alloc::string::ToString;
use esp_println::println;

use mousefood::fonts::mono_10x20_atlas;
use mousefood::{EmbeddedBackend, EmbeddedBackendConfig};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use static_cell::StaticCell;

use crate::screen::{Screen, init_screen};
use crate::web_status::{Service, ServiceStatus, fetch_status};
use crate::wifi::init_wifi;

mod screen;
mod server;
mod web_status;
mod wifi;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {}", info);
    loop {}
}

extern crate alloc;

use alloc::string::String;

/// What the display should be showing.
enum UiSig {
    Connecting,
    StatusUpdate(ServiceStatus),
    Post(String),
    Connected(String),
    Error,
}

/// Latest UI state. A `Signal` rather than a `Channel` so a redraw that is
/// still in flight is superseded instead of queued.
static UI: Signal<CriticalSectionRawMutex, UiSig> = Signal::new();

static ENDPOINT: [Service; 3] = [Service::Github, Service::Anthropic, Service::Cloudflare];

struct UiState {
    statuses: [Option<ServiceStatus>; ENDPOINT.len()],
}

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) {
    // generator version: 1.3.0
    // generator parameters: --chip esp32c6 -o esp32c6-mini-1 -o unstable-hal -o embassy -o alloc -o wifi

    esp_println::logger::init_logger_from_env();

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

    // The render task borrows the display for `'static`, so the screen has to
    // outlive this stack frame.
    static SCREEN: StaticCell<Screen> = StaticCell::new();
    let screen = SCREEN.init(init_screen(
        peripherals.LEDC,
        peripherals.SPI2,
        peripherals.GPIO7,
        peripherals.GPIO6,
        peripherals.GPIO14,
        peripherals.GPIO15,
        peripherals.GPIO21,
        peripherals.GPIO22,
    ));
    screen.set_brightness(5);

    // let mut C: Channel<NoopRawMutex, UiSig, 2> = Channel::new();

    spawner.spawn(render(screen).unwrap());
    UI.signal(UiSig::Connecting);

    let net_stack = init_wifi(spawner, peripherals.WIFI).await;
    let addr = net_stack.config_v4().expect("msg").address;

    UI.signal(UiSig::Connected(addr.address().to_string()));

    spawner.spawn(monitor_status(net_stack).unwrap());

    // let server = Srv::new();
    // server.start(net_stack).await.into_never()

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples
}

#[embassy_executor::task]
async fn monitor_status(net_stack: Stack<'static>) -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(20));
    let endpoints = [Service::Github, Service::Anthropic, Service::Cloudflare];

    loop {
        for endpoint in &endpoints {
            match fetch_status(net_stack, endpoint).await {
                Ok(res) => {
                    println!("All good {}: {}", endpoint, res.description);
                    UI.signal(UiSig::StatusUpdate(res));
                }
                Err(err) => {
                    println!("Failed to fetch {endpoint} {err}")
                }
            }
        }

        ticker.next().await;
    }
}

#[embassy_executor::task]
async fn render(screen: &'static mut Screen) -> ! {
    let backend_config = EmbeddedBackendConfig {
        font_regular: mono_10x20_atlas(),
        ..Default::default()
    };
    let be = EmbeddedBackend::new(&mut screen.display, backend_config);
    let mut terminal = Terminal::new(be).expect("Hopefully it works");
    let mut state = UiState {
        statuses: [None, None, None],
    };

    loop {
        let sig = UI.wait().await;

        terminal
            .draw(|frame| draw(frame, &mut state, sig))
            .expect("msg");
    }
}

fn draw(frame: &mut Frame, state: &mut UiState, sig: UiSig) {
    // let _text = match sig {
    //     UiSig::Connecting => "Initializing network",
    //     UiSig::Post(body) => body.as_str(),
    //     UiSig::Error => "Request failed",
    //     UiSig::Connected(ip) => ip,
    //     UiSig::StatusUpdate(res) => {
    //         match res.name {
    //         Service::Github => {}
    //         Service::Anthropic => {}
    //         Service::Cloudflare => {}
    //     }
    //     },
    // };
    if let UiSig::StatusUpdate(res) = sig {
        let idx = res.name.index();
        state.statuses[idx] = Some(res);
    }

    let mut bdy: Vec<Line<'_>> = Vec::new();

    for st in &state.statuses {
        if let Some(s) = st {
            bdy.push(Line::from(alloc::vec![
                Span::styled(
                    "\u{25cf} ",
                    Style::default().fg(indicator_color(&s.indicator))
                ),
                Span::from(s.name.to_string()),
            ]));
        }
    }

    let body = Text::from(bdy);

    let paragraph = Paragraph::new(body)
        .block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::LightMagenta))
                .title("Status"),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, frame.area());
}

fn indicator_color(indicator: &str) -> Color {
    match indicator {
        "none" => Color::Green,
        "minor" => Color::Yellow,
        "major" => Color::LightRed,
        "critical" => Color::Red,
        _ => Color::DarkGray,
    }
}
