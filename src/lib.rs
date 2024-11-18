#![no_std]

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_rp::peripherals;

mod network;
mod usb;

use embassy_time::{Duration, Timer};
use log::{info, LevelFilter};

use crate::{network::spawn_network, usb::spawn_usb};

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
        pin: PIN_20,
    }
}

pub async fn main(spawner: Spawner) {
    let peripherals = embassy_rp::init(Default::default());

    let resources = split_resources!(peripherals);

    spawn_usb(&spawner, resources.usb, LevelFilter::Trace);
    let mut control = spawn_network(&spawner, resources.network).await;

    let delay = Duration::from_secs(1);
    loop {
        info!("led on!");
        control.gpio_set(0, true).await;
        Timer::after(delay).await;

        info!("led off!");
        control.gpio_set(0, false).await;
        Timer::after(delay).await;
    }
}