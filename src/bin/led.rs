use esp_hal::Async;
use esp_hal::gpio::Level;
use esp_hal::peripherals::{GPIO8, RMT};
use esp_hal::rmt::{Channel, PulseCode, Rmt, Tx, TxChannelConfig, TxChannelCreator};
use esp_hal::time::Rate;

const RMT_FREQ_MHZ: u32 = 80;

// WS2812 bit timings, in 12.5 ns ticks.
const T0H: u16 = 28; // 350 ns
const T0L: u16 = 72; // 900 ns
const T1H: u16 = 56; // 700 ns
const T1L: u16 = 48; // 600 ns

/// 24 color bits plus a terminating end marker.
const FRAME: usize = 25;

pub struct Led {
    channel: Channel<'static, Async, Tx>,
}

impl Led {
    pub fn new(rmt: RMT<'static>, pin: GPIO8<'static>) -> Self {
        let rmt = Rmt::new(rmt, Rate::from_mhz(RMT_FREQ_MHZ))
            .expect("rmt clock")
            .into_async();

        let channel = rmt
            .channel0
            .configure_tx(&TxChannelConfig::default().with_clk_divider(1))
            .expect("rmt channel")
            .with_pin(pin);

        Self { channel }
    }

    pub async fn set(&mut self, (r, g, b): (u8, u8, u8)) {
        let rgb = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;

        let mut codes = [PulseCode::end_marker(); FRAME];
        for (i, code) in codes.iter_mut().take(24).enumerate() {
            *code = if (rgb >> (23 - i)) & 1 == 1 {
                PulseCode::new(Level::High, T1H, Level::Low, T1L)
            } else {
                PulseCode::new(Level::High, T0H, Level::Low, T0L)
            };
        }

        let _ = self.channel.transmit(&codes).await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Green,
    Yellow,
    Orange,
    Red,
    Magenta,
}

impl From<Color> for (u8, u8, u8) {
    fn from(color: Color) -> Self {
        match color {
            Color::Green => (0, 255, 0),
            Color::Yellow => (255, 255, 0),
            Color::Orange => (255, 165, 0),
            Color::Red => (255, 0, 0),
            Color::Magenta => (255, 0, 255),
        }
    }
}
