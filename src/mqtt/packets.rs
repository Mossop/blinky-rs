use core::fmt::Write;

use crate::{
    log::{debug, warn},
    mqtt::{DeviceState, Topic},
    LedState,
};
use mqttrust::{
    encoding::v4::{encode_slice, Connect, Error, LastWill, Protocol},
    Packet, Publish, QoS, Subscribe, SubscribeTopic,
};
use serde::{Deserialize, Serialize};

use crate::buffer::ByteBuffer;

const DEVICE: &str = "Blinky";

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum State {
    On,
    #[default]
    Off,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    Rgb,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LedColor {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct LedPayload {
    state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brightness: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color_mode: Option<ColorMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<LedColor>,
}

impl LedPayload {
    fn from_state(state: &LedState) -> Self {
        match state {
            LedState::Off => LedPayload::default(),
            LedState::On { red, green, blue } => LedPayload {
                state: State::On,
                brightness: None,
                color_mode: Some(ColorMode::Rgb),
                color: Some(LedColor {
                    r: *red,
                    g: *green,
                    b: *blue,
                }),
            },
        }
    }

    pub fn to_state(&self) -> LedState {
        match (&self.state, &self.brightness, &self.color) {
            (State::On, _, Some(LedColor { r, g, b })) => LedState::On {
                red: *r,
                green: *g,
                blue: *b,
            },
            (State::On, Some(br), None) => LedState::On {
                red: *br,
                green: *br,
                blue: *br,
            },
            (State::Off, _, _) => LedState::Off,
            _ => {
                warn!("Unknown led state");
                LedState::Off
            }
        }
    }
}

pub(super) fn packet_size(buffer: &[u8]) -> Option<usize> {
    let mut pos = 1;
    let mut multiplier = 1;
    let mut value = 0;

    while pos < buffer.len() {
        value += (buffer[pos] & 127) as usize * multiplier;
        multiplier *= 128;

        if (buffer[pos] & 128) == 0 {
            return Some(value + pos + 1);
        }

        pos += 1;
        if pos == 5 {
            return Some(0);
        }
    }

    None
}

fn encode<'a, 'b, P: Into<Packet<'b>>>(pkt: P, buffer: &'a mut [u8]) -> Result<&'a [u8], Error> {
    let packet: Packet<'_> = pkt.into();
    debug!("Sending {:?} packet", packet.get_type());

    let len = encode_slice(&packet, buffer)?;
    Ok(&buffer[0..len])
}

pub(super) fn ping(buffer: &mut [u8]) -> Result<&[u8], Error> {
    encode(Packet::Pingreq, buffer)
}

pub(super) fn connect<'a>(client_id: &str, buffer: &'a mut [u8]) -> Result<&'a [u8], Error> {
    let mut lw_topic = ByteBuffer::<50>::new();
    write!(&mut lw_topic, "blinky/{client_id}/status").unwrap();
    let lw = LastWill {
        topic: lw_topic.as_str(),
        message: "offline".as_bytes(),
        qos: QoS::AtMostOnce,
        retain: false,
    };

    encode(
        Connect {
            protocol: Protocol::MQTT311,
            keep_alive: 60,
            client_id,
            clean_session: true,
            last_will: Some(lw),
            username: None,
            password: None,
        },
        buffer,
    )
}

pub(super) fn discovery<'a>(
    client_id: &str,
    mac_addr: &str,
    buffer: &'a mut [u8],
) -> Result<&'a [u8], Error> {
    let mut topic = ByteBuffer::<50>::new();
    write!(&mut topic, "homeassistant/device/{client_id}/config").unwrap();

    let mut payload = ByteBuffer::<1024>::new();

    write!(
        &mut payload,
        r#"{{
  "device": {{
    "identifiers": [
      "{client_id}",
      "{mac_addr}"
    ],
    "connections": [["mac", "{}:{}:{}:{}:{}:{}"]],
    "name": "{DEVICE} {client_id}"
  }},
  "origin": {{
    "name": "{DEVICE}"
  }},
  "components": {{
    "leds": {{
      "unique_id": "bl_{client_id}_leds",
      "name": "leds",
      "platform": "light",
      "schema": "json",
      "availability_topic": "blinky/{client_id}/status",
      "state_topic": "blinky/{client_id}/leds/state",
      "command_topic": "blinky/{client_id}/leds/set",
      "supported_color_modes": ["rgb"],
      "effect_list": ["rainbow"]
    }}
  }}
}}"#,
        &mac_addr[0..2],
        &mac_addr[2..4],
        &mac_addr[4..6],
        &mac_addr[6..8],
        &mac_addr[8..10],
        &mac_addr[10..],
    )
    .unwrap();

    encode(
        Publish {
            dup: false,
            qos: QoS::AtMostOnce,
            pid: None,
            retain: false,
            topic_name: topic.as_str(),
            payload: &payload,
        },
        buffer,
    )
}

pub(super) fn state<'a>(
    client_id: &str,
    state: &DeviceState,
    buffer: &'a mut [u8],
) -> Result<&'a [u8], Error> {
    let mut topic = ByteBuffer::<50>::new();
    let mut payload = ByteBuffer::<512>::new();

    match state {
        DeviceState::Online => {
            write!(&mut topic, "blinky/{client_id}/status").unwrap();
            write!(&mut payload, "online").unwrap();
        }
        DeviceState::Led(state) => {
            write!(&mut topic, "blinky/{client_id}/leds/state").unwrap();

            payload.serialize(&LedPayload::from_state(state)).unwrap();
        }
    }

    encode(
        Publish {
            dup: false,
            qos: QoS::AtMostOnce,
            pid: None,
            retain: false,
            topic_name: topic.as_str(),
            payload: &payload,
        },
        buffer,
    )
}

pub(super) fn subscribe<'a>(
    client_id: &str,
    topic: &Topic,
    buffer: &'a mut [u8],
) -> Result<&'a [u8], Error> {
    let mut topic_buf = ByteBuffer::<50>::new();
    match topic {
        Topic::HaState => write!(&mut topic_buf, "homeassistant/status").unwrap(),
        Topic::LedCommand => write!(&mut topic_buf, "blinky/{client_id}/leds/set").unwrap(),
    }

    let subscribe_topic = SubscribeTopic {
        topic_path: topic_buf.as_str(),
        qos: QoS::AtMostOnce,
    };

    encode(Subscribe::new(&[subscribe_topic]), buffer)
}
