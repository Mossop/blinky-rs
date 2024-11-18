use core::str;

use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    peripherals::USB,
    rom_data::reset_to_usb_boot,
    usb::{Driver, InterruptHandler},
};
use embassy_usb_logger::ReceiverHandler;
use log::LevelFilter;

use crate::UsbPeripherals;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

struct Handler;

impl ReceiverHandler for Handler {
    async fn handle_data(&self, data: &[u8]) {
        if let Ok(data) = str::from_utf8(data) {
            let data = data.trim();

            // If you are using elf2uf2-term with the '-t' flag, then when closing the serial monitor,
            // this will automatically put the pico into boot mode
            if data == "q" || data == "elf2uf2-term" {
                reset_to_usb_boot(0, 0); // Restart the chip
            } else if data.eq_ignore_ascii_case("hello") {
                log::info!("World!");
            } else {
                log::info!("Recieved: {:?}", data);
            }
        }
    }

    fn new() -> Self {
        Self
    }
}

#[embassy_executor::task]
async fn usb_task(driver: Driver<'static, USB>, level: LevelFilter) {
    embassy_usb_logger::run!(1024, level, driver, Handler)
}

pub fn spawn_usb(spawner: &Spawner, peripherals: UsbPeripherals, level: LevelFilter) {
    let driver = Driver::new(peripherals.usb, Irqs);
    spawner.spawn(usb_task(driver, level)).unwrap();
}
