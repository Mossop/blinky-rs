use core::marker::PhantomData;

use embassy_rp::{
    bind_interrupts,
    peripherals::PIO1,
    pio::{InterruptHandler, Pio},
};
use smart_leds::RGB8;

use crate::ws2812::PioWs2812;
use crate::LedPeripherals;

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => InterruptHandler<PIO1>;
});

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

pub fn gamma_corrected(pixel: &RGB8) -> RGB8 {
    RGB8::new(
        GAMMA8[pixel.r as usize],
        GAMMA8[pixel.g as usize],
        GAMMA8[pixel.b as usize],
    )
}

pub trait Order {
    fn ordered(pixel: &RGB8) -> (&u8, &u8, &u8);
    fn to_word(pixel: &RGB8) -> u32 {
        let (a, b, c) = Self::ordered(pixel);
        (u32::from(*a) << 24) | (u32::from(*b) << 16) | (u32::from(*c) << 8)
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct GRB;
impl Order for GRB {
    fn ordered(pixel: &RGB8) -> (&u8, &u8, &u8) {
        (&pixel.g, &pixel.r, &pixel.b)
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct RGB;
impl Order for RGB {
    fn ordered(pixel: &RGB8) -> (&u8, &u8, &u8) {
        (&pixel.r, &pixel.g, &pixel.b)
    }
}

pub struct Leds<const N: usize, O: Order> {
    pixels: [RGB8; N],
    data: [u32; N],
    ws2812: PioWs2812<'static, PIO1, 0, N>,
    _order: PhantomData<O>,
}

impl<const N: usize, O: Order> Leds<N, O> {
    pub fn new(peripherals: LedPeripherals) -> Self {
        let Pio {
            mut common, sm0, ..
        } = Pio::new(peripherals.pio, Irqs);

        let ws2812 = PioWs2812::new(&mut common, sm0, peripherals.dma, peripherals.pin);

        Leds {
            pixels: [RGB8::default(); N],
            data: [0; N],
            ws2812,
            _order: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        N
    }

    pub fn get(&self, index: usize) -> &RGB8 {
        &self.pixels[index]
    }

    pub fn set(&mut self, index: usize, pixel: &RGB8) {
        self.pixels[index] = *pixel;

        self.data[index] = O::to_word(&gamma_corrected(pixel));
    }

    pub async fn write(&mut self) {
        self.ws2812.write(self.data).await;
    }
}
