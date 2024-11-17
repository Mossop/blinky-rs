#![no_std]
#![no_main]

use blinky_rs::{spawn_network, spawn_usb};
use cyw43_pio::PioSpi;
use embassy_executor::Spawner;
use embassy_rp::peripherals::PIO0;
use embassy_rp::pio::{self, Pio};
use embassy_rp::{bind_interrupts, usb::Driver};
use embassy_rp::{
    gpio::{Level, Output},
    peripherals::USB,
    usb,
};
use embassy_time::{Duration, Timer};
use log::{info, LevelFilter};
use panic_probe as _;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let peripherals = embassy_rp::init(Default::default());

    let driver = Driver::new(peripherals.USB, Irqs);
    spawn_usb(&spawner, driver, LevelFilter::Trace);

    let pwr = Output::new(peripherals.PIN_23, Level::Low);
    let cs = Output::new(peripherals.PIN_25, Level::High);
    let mut pio = Pio::new(peripherals.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        pio.irq0,
        cs,
        peripherals.PIN_24,
        peripherals.PIN_29,
        peripherals.DMA_CH0,
    );

    let mut control = spawn_network(&spawner, pwr, spi).await;

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
