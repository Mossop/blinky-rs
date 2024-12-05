use core::fmt::Write;
use core::str;

use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    peripherals::USB,
    usb::{Driver, InterruptHandler},
};
use embassy_usb_logger::{ReceiverHandler, UsbLogger, Writer};
use log::{LevelFilter, Record};

use crate::board::Board;

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
                Board::reboot_to_bootsel();
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

fn writer(record: &Record, writer: &mut Writer<'_, 1024>) {
    let mut target = record.target();
    if target.starts_with("blinky_rs::") {
        target = &target[11..];
    }

    let _ = write!(
        writer,
        "{:>5} {:<15} - {}\r\n",
        record.level(),
        target,
        record.args()
    );
}

#[embassy_executor::task]
async fn usb_task(driver: Driver<'static, USB>, level: LevelFilter) {
    static mut LOGGER: UsbLogger<1024, Handler> = UsbLogger::with_custom_style(writer);
    unsafe {
        #[allow(static_mut_refs)]
        LOGGER.with_handler(Handler::new());
        #[allow(static_mut_refs)]
        let _ = ::log::set_logger_racy(&LOGGER).map(|()| log::set_max_level_racy(level));
        #[allow(static_mut_refs)]
        let _ = LOGGER
            .run(&mut ::embassy_usb_logger::LoggerState::new(), driver)
            .await;
    }
}

pub fn spawn_usb(spawner: &Spawner, usb: USB) {
    let driver = Driver::new(usb, Irqs);
    spawner.spawn(usb_task(driver, LevelFilter::Trace)).unwrap();
}
