#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::peripherals::{self, Peripherals};
use esp_hal::rng::Rng;
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;

use esp_println::println;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{Config, ControllerConfig, WifiController};
use mipidsi::interface::SpiInterface;
use mipidsi::models::ST7789;
use mipidsi::options::ColorInversion;
use picoserve::extract::Json;
use picoserve::routing::{get, post};
use static_cell::StaticCell;

use esp_alloc::HEAP;

use crate::wifi::init_wifi;

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

    let mut backlight = Output::new(peripherals.GPIO22, Level::High, OutputConfig::default());

    // https://docs.waveshare.com/ESP32-C6-LCD-1.47?variant=ESP32-C6-LCD-1.47#interface-introduction
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(40))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO7)
    .with_mosi(peripherals.GPIO6);

    let cs = Output::new(peripherals.GPIO14, Level::High, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO15, Level::Low, OutputConfig::default());
    let rst = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());

    // let rgb_led = Output::new(peripherals.GPIO8, Level::High, config)

    let mut delay = Delay::default();
    let spi_device = ExclusiveDevice::new(spi, cs, delay).unwrap();

    let mut buffer = [0u8; 512];
    let di = SpiInterface::new(spi_device, dc, &mut buffer);

    let mut display = mipidsi::Builder::new(ST7789, di)
        .reset_pin(rst)
        .display_size(DISPLAY_SIZE.0, DISPLAY_SIZE.1)
        .display_offset(DISPLAY_OFFSET.0, DISPLAY_OFFSET.1)
        // .orientation(Orientation::new().rotate(ROTATION))
        .invert_colors(ColorInversion::Inverted)
        .init(&mut delay)
        .unwrap();

    display.clear(Rgb565::BLACK).unwrap();

    // The following pins are used to bootstrap the chip. They are available
    // for use, but check the datasheet of the module for more information on them.
    // - GPIO4
    // - GPIO5
    // - GPIO8
    // - GPIO9
    // - GPIO15
    // These GPIO pins are in use by some feature of the module and should not be used.
    let _ = peripherals.GPIO24;
    let _ = peripherals.GPIO25;
    let _ = peripherals.GPIO26;
    let _ = peripherals.GPIO27;
    let _ = peripherals.GPIO28;
    let _ = peripherals.GPIO29;
    let _ = peripherals.GPIO30;

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    static HTTP_BUF: StaticCell<[u8; 2048]> = StaticCell::new();
    static TCP_RX: StaticCell<[u8; 1024]> = StaticCell::new();
    static TCP_TX: StaticCell<[u8; 1024]> = StaticCell::new();

    println!("Used: {} ", HEAP.stats());

    backlight.toggle();
    let app = picoserve::Router::new()
        .route("/", get(async || "hello from esp-noti\n"))
        .route("/notify", post(notify));
    let config = picoserve::Config::default();

    let net_stack = init_wifi(spawner, peripherals.WIFI).await;

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

const DISPLAY_SIZE: (u16, u16) = (172, 320);
const DISPLAY_OFFSET: (u16, u16) = (34, 0);

#[embassy_executor::task]
async fn net_task(
    mut runner: embassy_net::Runner<'static, esp_radio::wifi::Interface<'static>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) -> ! {
    loop {
        match controller.connect_async().await {
            Ok(info) => {
                println!("Wifi connected to {:?}", info);

                let info = controller.wait_for_disconnect_async().await.ok();
                println!("Disconnected: {:?}", info);
            }
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
            }
        }

        Timer::after(Duration::from_secs(5)).await;
    }
}
