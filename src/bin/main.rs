#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use embassy_executor::Spawner;

use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Input, InputConfig, Pull};
use esp_hal::timer::timg::TimerGroup;

use esp_println::println;

use crate::led::{Color, Led};
use crate::screen::{Backlight, init_display};
use crate::server::start_server;
use crate::state::{STATE, set_state};
use crate::ui::{cycle_pages, render_ui};
use crate::web_mon::start_monitoring;
use crate::wifi::init_network;

mod fetcher;
mod led;
mod screen;
mod server;
mod ui;
mod web_mon;
mod wifi;

mod state;

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

    let display = init_display(
        peripherals.SPI2,
        peripherals.GPIO7,
        peripherals.GPIO6,
        peripherals.GPIO14,
        peripherals.GPIO15,
        peripherals.GPIO21,
    );
    let backlight = Backlight::new(peripherals.LEDC, peripherals.GPIO22);
    let led = Led::new(peripherals.RMT, peripherals.GPIO8);

    render_ui(spawner, display);

    let net_stack = init_network(spawner, peripherals.WIFI).await;
    let ip = net_stack
        .config_v4()
        .expect("should acquire address")
        .address;

    println!("Got the IP: {ip}");

    set_state(|s| {
        s.ip = Some(ip);
    })
    .await;

    spawner.spawn(cycle_pages().unwrap());
    start_monitoring(spawner, net_stack);
    spawner.spawn(ambient(backlight, led).unwrap());

    let button = Input::new(
        peripherals.GPIO9,
        InputConfig::default().with_pull(Pull::Up),
    );

    spawner.spawn(button_task(button).unwrap());

    start_server(net_stack).await;
}

#[embassy_executor::task]
async fn button_task(mut button: Input<'static>) {
    loop {
        button.wait_for_falling_edge().await;

        set_state(|s| {
            if let Some(notifications) = &mut s.notifications {
                notifications.pop();

                if notifications.is_empty() {
                    s.notifications = None;
                    s.page = s.page.next();
                }
            } else {
                s.page = s.page.next();
            }
        })
        .await;

        // Simple software debouncing: ignore noise/bounce signals for 50ms
        Timer::after(Duration::from_millis(50)).await;

        button.wait_for_rising_edge().await;
    }
}

#[embassy_executor::task]
async fn ambient(backlight: Backlight, mut led: Led) -> ! {
    let mut led_color: Color = Color::Green;

    loop {
        let (severity, has_notification) = {
            let guard = STATE.lock().await;

            (guard.worst_severity(), guard.notifications.is_some())
        };

        if has_notification {
            led_color = Color::Magenta;
        } else if led_color != Color::from(severity) {
            let brightness = if severity.to_number() > 0 { 10 } else { 5 };
            backlight.set_brightness(brightness);
            led_color = Color::from(severity);
        }

        led.set(led_color.into()).await;

        Timer::after(Duration::from_secs(1)).await;
    }
}
