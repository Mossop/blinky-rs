use core::str;

use cyw43::{Control, JoinOptions};
use cyw43_pio::PioSpi;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_net::{Config, Stack, StackResources};
use embassy_rp::{
    bind_interrupts,
    clocks::RoscRng,
    flash::{Async, Flash},
    gpio::{Level, Output},
    peripherals::{DMA_CH0, PIO0},
    pio::{InterruptHandler, Pio},
    rom_data::reset_to_usb_boot,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;
use log::{error, info, warn};
use rand::RngCore;
use static_cell::StaticCell;

mod ws2812;

pub use ws2812::Ws2812;

static LED_STATE: Signal<CriticalSectionRawMutex, bool> = Signal::new();

const FLASH_SIZE: usize = 2 * 1024 * 1024;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn embassy_net_task(
    mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn wifi_task(
    mut control: Control<'static>,
    network: Stack<'static>,
    ssid: &'static str,
    password: &'static str,
) -> ! {
    loop {
        loop {
            match control
                .join(ssid, JoinOptions::new(password.as_bytes()))
                .await
            {
                Ok(_) => {
                    info!("Connected to wifi");
                    break;
                }
                Err(err) => {
                    error!("Failed to join network: {}", err.status);
                    Timer::after_secs(1).await;
                }
            }
        }

        network.wait_link_up().await;

        loop {
            match select(network.wait_link_down(), LED_STATE.wait()).await {
                Either::First(_) => {
                    break;
                }
                Either::Second(state) => {
                    control.gpio_set(0, state).await;
                }
            }
        }

        control.gpio_set(0, false).await;
        warn!("Lost wifi connection");

        control.leave().await;
    }
}

#[derive(Clone, Copy)]
pub struct Led;

impl Led {
    pub async fn set(&self, state: bool) {
        LED_STATE.signal(state);
    }
}

#[derive(Clone, Copy)]
pub struct Board {
    pub board_id: &'static str,
    pub network: Stack<'static>,
    pub led: Led,
}

impl Board {
    pub async fn init(
        spawner: &Spawner,
        ssid: &'static str,
        password: &'static str,
    ) -> (Self, Ws2812) {
        let peripherals = embassy_rp::init(Default::default());

        #[cfg(feature = "log")]
        crate::usb::spawn_usb(spawner, peripherals.USB);

        let fw = include_bytes!("../../cyw43/43439A0.bin");
        let clm = include_bytes!("../../cyw43/43439A0_clm.bin");

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

        static STATE: StaticCell<cyw43::State> = StaticCell::new();
        let state = STATE.init(cyw43::State::new());
        let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;

        spawner.spawn(cyw43_task(runner)).unwrap();

        control.init(clm).await;
        control
            .set_power_management(cyw43::PowerManagementMode::PowerSave)
            .await;

        let mut rng = RoscRng;
        let seed = rng.next_u64();

        let config = Config::dhcpv4(Default::default());

        static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
        let (network, runner) = embassy_net::new(
            net_device,
            config,
            RESOURCES.init(StackResources::new()),
            seed,
        );

        spawner.spawn(embassy_net_task(runner)).unwrap();

        spawner
            .spawn(wifi_task(control, network, ssid, password))
            .unwrap();

        static BOARD_ID: StaticCell<[u8; 16]> = StaticCell::new();
        let mut flash = Flash::<_, Async, FLASH_SIZE>::new(peripherals.FLASH, peripherals.DMA_CH2);
        let mut uid = [0; 8];
        flash.blocking_unique_id(&mut uid).unwrap();

        let hex_slice = BOARD_ID.init_with(|| {
            let mut hex_slice = [0; 16];
            hex::encode_to_slice(uid, &mut hex_slice).unwrap();
            hex_slice
        });

        let ws2812 = Ws2812::new(peripherals.PIO1, peripherals.DMA_CH1, peripherals.PIN_15);

        (
            Board {
                board_id: str::from_utf8(hex_slice).unwrap(),
                network,
                led: Led,
            },
            ws2812,
        )
    }

    pub fn reboot_to_bootsel() {
        reset_to_usb_boot(0, 0);
    }
}
