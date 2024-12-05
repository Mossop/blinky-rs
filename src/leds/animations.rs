use embassy_rp::clocks::RoscRng;
use rand::{distributions::Uniform, prelude::Distribution, Rng};

use crate::{
    board::Ws2812,
    leds::{
        color::{Float, Order, Pixel, HSV},
        AbortableTicker,
    },
};

pub async fn flames<const N: usize, O: Order>(mut ticker: AbortableTicker, ws2812: &mut Ws2812) {
    let mut pixels = [0_u32; N];
    let min_hue: Float = 0.0;
    let max_hue: Float = 50.0 / 360.0;
    let uniform = Uniform::new_inclusive(min_hue, max_hue);
    let mut rng = RoscRng;

    loop {
        for px in pixels.iter_mut() {
            let pixel = HSV {
                h: uniform.sample(&mut rng),
                s: 1.0,
                v: rng.gen(),
            };

            *px = pixel.to_word::<O>();
        }

        ws2812.write(&pixels).await;

        if ticker.next().await {
            break;
        }
    }
}
