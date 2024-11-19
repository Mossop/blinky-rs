#![no_std]

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_rp::peripherals;

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

use crate::mqtt::{spawn_mqtt, Device, MqttMessage, State, MQTT_CHANNEL};

enum Command {
    MqttConnected,
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
}

pub async fn main(spawner: Spawner) {
    let peripherals = embassy_rp::init(Default::default());

    let resources = split_resources!(peripherals);

    #[cfg(feature = "log")]
    usb::spawn_usb(&spawner, resources.usb);

    spawn_mqtt(&spawner, resources.network).await;

    let receiver = COMMAND_CHANNEL.receiver();

    loop {
        let command = receiver.receive().await;

        match command {
            Command::MqttConnected => {
                MQTT_CHANNEL
                    .send(MqttMessage::SendState(Device::Power, State::On))
                    .await;
            }
        }
    }
}
