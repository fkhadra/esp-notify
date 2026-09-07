use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::peripherals::{GPIO6, GPIO7, GPIO14, GPIO15, GPIO21, GPIO22, SPI2};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use mipidsi::interface::SpiInterface;
use mipidsi::models::ST7789;
use mipidsi::options::{ColorInversion, Orientation, Rotation};
use static_cell::StaticCell;

// https://docs.waveshare.com/ESP32-C6-LCD-1.47?variant=ESP32-C6-LCD-1.47#interface-introduction
const DISPLAY_SIZE: (u16, u16) = (172, 320);
const DISPLAY_OFFSET: (u16, u16) = (34, 0);
const ROTATION: Rotation = Rotation::Deg270;
const SPI_BUFFER_SIZE: usize = 512;

type DisplaySpi = ExclusiveDevice<Spi<'static, Blocking>, Output<'static>, Delay>;
type DisplayInterface = SpiInterface<'static, DisplaySpi, Output<'static>>;

pub type Display = mipidsi::Display<DisplayInterface, ST7789, Output<'static>>;

pub struct Screen {
    pub display: Display,
    pub backlight: Output<'static>,
}

pub fn init_screen(
    spi: SPI2<'static>,
    sck: GPIO7<'static>,
    mosi: GPIO6<'static>,
    cs: GPIO14<'static>,
    dc: GPIO15<'static>,
    rst: GPIO21<'static>,
    backlight: GPIO22<'static>,
) -> Screen {
    let backlight = Output::new(backlight, Level::High, OutputConfig::default());

    let spi = Spi::new(
        spi,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(40))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(sck)
    .with_mosi(mosi);

    let cs = Output::new(cs, Level::High, OutputConfig::default());
    let dc = Output::new(dc, Level::Low, OutputConfig::default());
    let rst = Output::new(rst, Level::High, OutputConfig::default());

    let mut delay = Delay::default();
    let spi_device = ExclusiveDevice::new(spi, cs, delay).unwrap();

    static BUFFER: StaticCell<[u8; SPI_BUFFER_SIZE]> = StaticCell::new();
    let di = SpiInterface::new(spi_device, dc, BUFFER.init([0; SPI_BUFFER_SIZE]));

    let mut display = mipidsi::Builder::new(ST7789, di)
        .reset_pin(rst)
        .display_size(DISPLAY_SIZE.0, DISPLAY_SIZE.1)
        .display_offset(DISPLAY_OFFSET.0, DISPLAY_OFFSET.1)
        .orientation(Orientation::new().rotate(ROTATION))
        .invert_colors(ColorInversion::Inverted)
        .init(&mut delay)
        .unwrap();

    display.clear(Rgb565::GREEN).unwrap();

    Screen { display, backlight }
}
