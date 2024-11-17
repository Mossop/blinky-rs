use core::{fmt::Write as _, str};

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::{peripherals::USB, rom_data::reset_to_usb_boot, usb::Driver as RpDriver};
use embassy_sync::pipe::Pipe;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender, State};
use embassy_usb::driver::Driver;
use embassy_usb::{Builder, Config};
use log::{set_logger_racy, set_max_level_racy, LevelFilter, Metadata, Record};

type CS = embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

/// The logger state containing buffers that must live as long as the USB peripheral.
struct LoggerState<'d> {
    state: State<'d>,
    config_descriptor: [u8; 128],
    bos_descriptor: [u8; 16],
    msos_descriptor: [u8; 256],
    control_buf: [u8; 64],
}

impl<'d> LoggerState<'d> {
    /// Create a new instance of the logger state.
    pub fn new() -> Self {
        Self {
            state: State::new(),
            config_descriptor: [0; 128],
            bos_descriptor: [0; 16],
            msos_descriptor: [0; 256],
            control_buf: [0; 64],
        }
    }
}

/// The packet size used in the usb logger, to be used with `create_future_from_class`
const MAX_PACKET_SIZE: u8 = 64;

/// The logger handle, which contains a pipe with configurable size for buffering log messages.
struct UsbLogger<const N: usize> {
    buffer: Pipe<CS, N>,
}

impl<const N: usize> UsbLogger<N> {
    /// Create a new logger instance.
    const fn new() -> Self {
        Self {
            buffer: Pipe::new(),
        }
    }

    /// Run the USB logger using the state and USB driver. Never returns.
    async fn run<'d, D>(&'d self, state: &'d mut LoggerState<'d>, driver: D) -> !
    where
        D: Driver<'d>,
        Self: 'd,
    {
        let mut config = Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("Blinky");
        config.product = Some("USB-serial logger");
        config.serial_number = None;
        config.max_power = 100;
        config.max_packet_size_0 = MAX_PACKET_SIZE;

        // Required for windows compatiblity.
        // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
        config.device_class = 0xEF;
        config.device_sub_class = 0x02;
        config.device_protocol = 0x01;
        config.composite_with_iads = true;

        let mut builder = Builder::new(
            driver,
            config,
            &mut state.config_descriptor,
            &mut state.bos_descriptor,
            &mut state.msos_descriptor,
            &mut state.control_buf,
        );

        // Create classes on the builder.
        let class = CdcAcmClass::new(&mut builder, &mut state.state, MAX_PACKET_SIZE as u16);
        let (mut sender, mut receiver) = class.split();

        // Build the builder.
        let mut device = builder.build();
        loop {
            let run_fut = device.run();
            let class_fut = self.run_logger_class(&mut sender, &mut receiver);
            join(run_fut, class_fut).await;
        }
    }

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

    async fn run_logger_class<'d, D>(
        &self,
        sender: &mut Sender<'d, D>,
        receiver: &mut Receiver<'d, D>,
    ) where
        D: Driver<'d>,
    {
        let log_fut = async {
            let mut rx: [u8; MAX_PACKET_SIZE as usize] = [0; MAX_PACKET_SIZE as usize];
            sender.wait_connection().await;
            loop {
                let len = self.buffer.read(&mut rx[..]).await;
                let _ = sender.write_packet(&rx[..len]).await;
                if len as u8 == MAX_PACKET_SIZE {
                    let _ = sender.write_packet(&[]).await;
                }
            }
        };

        let reciever_fut = async {
            let mut reciever_buf: [u8; MAX_PACKET_SIZE as usize] = [0; MAX_PACKET_SIZE as usize];
            receiver.wait_connection().await;
            loop {
                let n = receiver.read_packet(&mut reciever_buf).await.unwrap();
                let data = &reciever_buf[..n];
                self.handle_data(data).await;
            }
        };

        join(log_fut, reciever_fut).await;
    }
}

impl<const N: usize> log::Log for UsbLogger<N> {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let _ = write!(Writer(&self.buffer), "{}\r\n", record.args());
        }
    }

    fn flush(&self) {}
}

/// A writer that writes to the USB logger buffer.
pub struct Writer<'d, const N: usize>(&'d Pipe<CS, N>);

impl<'d, const N: usize> core::fmt::Write for Writer<'d, N> {
    fn write_str(&mut self, s: &str) -> Result<(), core::fmt::Error> {
        // The Pipe is implemented in such way that we cannot
        // write across the wraparound discontinuity.
        let b = s.as_bytes();
        if let Ok(n) = self.0.try_write(b) {
            if n < b.len() {
                // We wrote some data but not all, attempt again
                // as the reason might be a wraparound in the
                // ring buffer, which resolves on second attempt.
                let _ = self.0.try_write(&b[n..]);
            }
        }
        Ok(())
    }
}

#[embassy_executor::task]
async fn usb_task(driver: RpDriver<'static, USB>, level: LevelFilter) {
    unsafe {
        static mut LOGGER: UsbLogger<1024> = UsbLogger::new();
        #[allow(static_mut_refs)]
        let _ = set_logger_racy(&LOGGER).map(|()| set_max_level_racy(level));
        let _ = LOGGER.run(&mut LoggerState::new(), driver).await;
    }
}

pub fn spawn_usb(spawner: &Spawner, driver: RpDriver<'static, USB>, level: LevelFilter) {
    spawner.spawn(usb_task(driver, level)).unwrap();
}
