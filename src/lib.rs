#![no_std]

use embassy_executor::Spawner;

#[cfg_attr(feature = "rp2040", path = "board/rp2040.rs")]
#[cfg_attr(feature = "rp2350", path = "board/rp2350.rs")]
mod board;
mod leds;
#[cfg(feature = "log")]
mod usb;

#[cfg(feature = "defmt")]
use defmt_rtt as _;
use log::warn;
use mcutie::{
    homeassistant::{
        binary_sensor::BinarySensorState,
        light::{Color, Light, LightState, SupportedColorMode},
        AvailabilityState, AvailabilityTopics, Device, Entity, Origin,
    },
    McutieBuilder, McutieTask, MqttMessage, PublishBytes, Publishable, Topic,
};

use crate::{
    board::Board,
    leds::{spawn_leds, LedProgram, LED_CHANNEL},
};

const DEVICE_AVAILABILITY_TOPIC: Topic<&'static str> = Topic::Device("status");
const LED_STATE_TOPIC: Topic<&'static str> = Topic::Device("leds/state");
const LED_COMMAND_TOPIC: Topic<&'static str> = Topic::Device("leds/set");

const DEVICE: Device<'static> = Device::new();
const ORIGIN: Origin<'static> = Origin::new();

const LED_ENTITY: Entity<'static, 1, Light<'static, 1, 0>> = Entity {
    device: DEVICE,
    origin: ORIGIN,
    object_id: "leds",
    unique_id: Some("leds"),
    name: "Leds",
    availability: AvailabilityTopics::All([DEVICE_AVAILABILITY_TOPIC]),
    state_topic: LED_STATE_TOPIC,
    component: Light {
        command_topic: LED_COMMAND_TOPIC,
        supported_color_modes: [SupportedColorMode::Rgb],
        effects: [],
    },
};

#[embassy_executor::task]
async fn mqtt_task(
    runner: McutieTask<
        'static,
        &'static str,
        PublishBytes<'static, &'static str, AvailabilityState>,
        1,
    >,
) {
    runner.run().await;
}

pub async fn main(spawner: Spawner) {
    let (board, ws2812) = Board::init(&spawner, env!("BLINKY_SSID"), env!("BLINKY_PASSWORD")).await;

    let (receiver, mqtt_runner) =
        McutieBuilder::new(board.network, "blinky", env!("BLINKY_BROKER"))
            .with_device_id(board.board_id)
            .with_last_will(DEVICE_AVAILABILITY_TOPIC.with_bytes(AvailabilityState::Offline))
            .with_subscriptions([LED_COMMAND_TOPIC])
            .build();

    spawner.spawn(mqtt_task(mqtt_runner)).unwrap();

    spawn_leds(&spawner, ws2812);

    let mut last_program: LedProgram = LedProgram::Solid {
        red: 255,
        green: 255,
        blue: 255,
    };
    LED_CHANNEL.send(LedProgram::Off).await;

    loop {
        let message = receiver.receive().await;

        match message {
            MqttMessage::Connected | MqttMessage::HomeAssistantOnline => {
                board.led.set(true).await;

                let _ = DEVICE_AVAILABILITY_TOPIC
                    .with_bytes(AvailabilityState::Online)
                    .publish()
                    .await;

                let _ = LED_ENTITY.publish_discovery().await;
            }
            MqttMessage::Disconnected => {
                board.led.set(false).await;
            }
            MqttMessage::Publish(topic, buffer) => {
                if topic == LED_COMMAND_TOPIC {
                    let new_program = match LightState::from_payload(&buffer) {
                        Ok(light_state) => {
                            if light_state.state == BinarySensorState::Off {
                                LedProgram::Off
                            } else if let Some(effect) = light_state.effect {
                                match effect {
                                    "Flames" => LedProgram::Flames,
                                    e => {
                                        warn!("Unexpected effect {e}");
                                        continue;
                                    }
                                }
                            } else {
                                match light_state.color {
                                    Color::None => last_program,
                                    Color::Rgb { red, green, blue } => {
                                        LedProgram::Solid { red, green, blue }
                                    }
                                    _ => {
                                        warn!("Unexpected color state received.");
                                        continue;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            warn!("Failed to decode state");
                            continue;
                        }
                    };

                    LED_CHANNEL.send(new_program).await;
                    if !matches!(new_program, LedProgram::Off) {
                        last_program = new_program;
                    }
                }
            }
        }
    }
}
