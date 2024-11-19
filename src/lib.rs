#![no_std]

use core::str;

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_rp::{
    flash::{Async, Flash},
    peripherals,
};

mod buffer;
mod leds;
mod log;
mod mqtt;
#[cfg(feature = "log")]
mod usb;
mod ws2812;

#[cfg(feature = "defmt")]
use defmt_rtt as _;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel};
use static_cell::StaticCell;

use crate::{
    leds::{Leds, RGB},
    mqtt::{spawn_mqtt, DeviceState, MqttMessage, MQTT_CHANNEL},
};

const FLASH_SIZE: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy)]
enum LedState {
    On { red: u8, green: u8, blue: u8 },
    Off,
}

enum Command {
    MqttConnected,
    SetLedState(LedState),
}

static COMMAND_CHANNEL: channel::Channel<CriticalSectionRawMutex, Command, 10> =
    channel::Channel::new();

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
    flash: FlashPeripherals {
        flash: FLASH,
        dma: DMA_CH2,
    }
}

pub async fn main(spawner: Spawner) {
    let peripherals = embassy_rp::init(Default::default());

    let resources = split_resources!(peripherals);

    #[cfg(feature = "log")]
    usb::spawn_usb(&spawner, resources.usb);

    let mut flash = Flash::<_, Async, FLASH_SIZE>::new(resources.flash.flash, resources.flash.dma);

    static BOARD_ID: StaticCell<[u8; 16]> = StaticCell::new();
    let board_id = BOARD_ID.init_with(|| {
        let mut uid = [0; 8];
        flash.blocking_unique_id(&mut uid).unwrap();
        let mut hex_slice = [0; 16];
        hex::encode_to_slice(uid, &mut hex_slice).unwrap();
        hex_slice
    });

    spawn_mqtt(
        &spawner,
        str::from_utf8(board_id).unwrap(),
        resources.network,
    )
    .await;

    let receiver = COMMAND_CHANNEL.receiver();
    let mut led_state = LedState::Off;
    let mut leds = Leds::<50, RGB>::new(resources.leds);
    leds.set_state(led_state).await;

    loop {
        let command = receiver.receive().await;

        match command {
            Command::MqttConnected => {
                MQTT_CHANNEL
                    .send(MqttMessage::SendState(DeviceState::Online))
                    .await;
                MQTT_CHANNEL
                    .send(MqttMessage::SendState(DeviceState::Led(led_state)))
                    .await;
            }
            Command::SetLedState(state) => {
                led_state = state;
                leds.set_state(state).await;
                MQTT_CHANNEL
                    .send(MqttMessage::SendState(DeviceState::Led(led_state)))
                    .await;
            }
        }
    }
}
