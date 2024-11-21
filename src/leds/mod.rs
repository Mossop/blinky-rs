use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel};
use embassy_time::{Duration, Ticker};

mod animations;
mod color;
mod ws2812;

use crate::{
    leds::color::{Order, OrderRGB, Pixel, RGB},
    LedPeripherals,
};
use ws2812::PioWs2812;

pub static LED_CHANNEL: channel::Channel<CriticalSectionRawMutex, LedProgram, 1> =
    channel::Channel::new();

#[derive(Clone, Copy)]
pub enum LedProgram {
    Off,
    Solid { red: u8, green: u8, blue: u8 },
    Flames,
}

struct AbortableTicker {
    ticker: Ticker,
}

impl AbortableTicker {
    fn every(duration: Duration) -> Self {
        Self {
            ticker: Ticker::every(duration),
        }
    }

    async fn next(&mut self) -> bool {
        let result = select(self.ticker.next(), LED_CHANNEL.ready_to_receive()).await;

        matches!(result, Either::Second(_))
    }
}

impl LedProgram {
    async fn run<const N: usize, O: Order>(&self, ws2812: &mut PioWs2812) {
        let ticker = AbortableTicker::every(Duration::from_millis(5));

        match self {
            Self::Off => {
                ws2812.write(&[0_u32; N]).await;
            }
            Self::Solid { red, green, blue } => {
                let word = RGB::from_rgb((*red, *green, *blue)).to_word::<O>();
                ws2812.write(&[word; N]).await;
            }
            Self::Flames => {
                animations::flames::<N, O>(ticker, ws2812).await;
            }
        }
    }
}

#[embassy_executor::task]
async fn led_task(mut ws2812: PioWs2812) {
    loop {
        let program = LED_CHANNEL.receive().await;
        program.run::<50, OrderRGB>(&mut ws2812).await;
    }
}

pub fn spawn_leds(spawner: &Spawner, peripherals: LedPeripherals) {
    let ws2812 = PioWs2812::new(peripherals);

    spawner.spawn(led_task(ws2812)).unwrap();
}
