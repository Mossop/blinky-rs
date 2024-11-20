use core::marker::PhantomData;

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel};
use embassy_time::{Duration, Ticker};
use smart_leds::RGB8;

use crate::ws2812::PioWs2812;
use crate::LedPeripherals;

pub static LED_CHANNEL: channel::Channel<CriticalSectionRawMutex, LedProgram, 1> =
    channel::Channel::new();

#[derive(Clone, Copy)]
pub enum LedProgram {
    Off,
    Solid { red: u8, green: u8, blue: u8 },
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

const GAMMA8: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 5, 5, 5,
    5, 6, 6, 6, 6, 7, 7, 7, 7, 8, 8, 8, 9, 9, 9, 10, 10, 10, 11, 11, 11, 12, 12, 13, 13, 13, 14,
    14, 15, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21, 21, 22, 22, 23, 24, 24, 25, 25, 26, 27,
    27, 28, 29, 29, 30, 31, 32, 32, 33, 34, 35, 35, 36, 37, 38, 39, 39, 40, 41, 42, 43, 44, 45, 46,
    47, 48, 49, 50, 50, 51, 52, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 66, 67, 68, 69, 70, 72,
    73, 74, 75, 77, 78, 79, 81, 82, 83, 85, 86, 87, 89, 90, 92, 93, 95, 96, 98, 99, 101, 102, 104,
    105, 107, 109, 110, 112, 114, 115, 117, 119, 120, 122, 124, 126, 127, 129, 131, 133, 135, 137,
    138, 140, 142, 144, 146, 148, 150, 152, 154, 156, 158, 160, 162, 164, 167, 169, 171, 173, 175,
    177, 180, 182, 184, 186, 189, 191, 193, 196, 198, 200, 203, 205, 208, 210, 213, 215, 218, 220,
    223, 225, 228, 231, 233, 236, 239, 241, 244, 247, 249, 252, 255,
];

pub trait Order {
    fn ordered(pixel: &RGB8) -> (u8, u8, u8);
    fn to_word(pixel: &RGB8) -> u32 {
        let (a, b, c) = Self::ordered(pixel);
        (u32::from(GAMMA8[a as usize]) << 24)
            | (u32::from(GAMMA8[b as usize]) << 16)
            | (u32::from(GAMMA8[c as usize]) << 8)
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct GRB;
impl Order for GRB {
    fn ordered(pixel: &RGB8) -> (u8, u8, u8) {
        (pixel.g, pixel.r, pixel.b)
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct RGB;
impl Order for RGB {
    fn ordered(pixel: &RGB8) -> (u8, u8, u8) {
        (pixel.r, pixel.g, pixel.b)
    }
}

pub struct Leds<const N: usize, O: Order> {
    pixels: [RGB8; N],
    data: [u32; N],
    ws2812: PioWs2812,
    _order: PhantomData<O>,
}

impl<const N: usize, O: Order> Leds<N, O> {
    pub fn new(peripherals: LedPeripherals) -> Self {
        let ws2812 = PioWs2812::new(peripherals);

        Leds {
            pixels: [RGB8::default(); N],
            data: [0; N],
            ws2812,
            _order: PhantomData,
        }
    }

    pub fn set_all(&mut self, pixel: &RGB8) {
        let word = O::to_word(pixel);
        for index in 0..N {
            self.pixels[index] = *pixel;
            self.data[index] = word;
        }
    }

    pub async fn write(&mut self) {
        self.ws2812.write(self.data).await;
    }
}

impl LedProgram {
    async fn run<const N: usize, O: Order>(&self, leds: &mut Leds<N, O>) {
        match self {
            Self::Off => {
                leds.set_all(&RGB8::new(0, 0, 0));
                leds.write().await;
            }
            Self::Solid { red, green, blue } => {
                leds.set_all(&RGB8::new(*red, *green, *blue));
                leds.write().await;
            }
        }
    }
}

#[embassy_executor::task]
async fn led_task(peripherals: LedPeripherals) {
    let mut leds = Leds::<50, RGB>::new(peripherals);

    loop {
        let program = LED_CHANNEL.receive().await;
        program.run(&mut leds).await;
    }
}

pub fn spawn_leds(spawner: &Spawner, peripherals: LedPeripherals) {
    spawner.spawn(led_task(peripherals)).unwrap();
}
