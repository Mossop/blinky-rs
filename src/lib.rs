#![no_std]

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_rp::peripherals;

mod leds;
mod network;
mod usb;
mod ws2812;

use log::{info, LevelFilter};
use smart_leds::RGB8;

use crate::{leds::Leds, network::spawn_network, usb::spawn_usb};

assign_resources! {
    usb: UsbPeripherals {
        usb: USB,
    },
    network: NetPeripherals {
        pwr: PIN_23,
        cs: PIN_25,
        pio: PIO0,
        dio: PIN_24,
        clk: PIN_29,
        dma: DMA_CH0,
    }
    leds: LedPeripherals {
        pio: PIO1,
        dma: DMA_CH1,
        pin: PIN_15,
    }
}

pub async fn main(spawner: Spawner) {
    let peripherals = embassy_rp::init(Default::default());

    let resources = split_resources!(peripherals);

    spawn_usb(&spawner, resources.usb, LevelFilter::Trace);
    spawn_network(&spawner, resources.network).await;

    let mut leds = Leds::<50, leds::RGB>::new(resources.leds);

    loop {
        info!("Going up");
        for p in 0..=255 {
            let pixel = RGB8::new(p, 0, 0);
            for i in 0..leds.len() {
                leds.set(i, &pixel);
            }

            leds.write().await;
        }

        info!("Going down");
        for p in (0..=255).rev() {
            let pixel = RGB8::new(p, 0, 0);
            for i in 0..leds.len() {
                leds.set(i, &pixel);
            }

            leds.write().await;
        }
    }
}
