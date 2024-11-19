use core::fmt::{self, Write};

use crate::{
    log::debug,
    mqtt::{Device, State},
};
use mqttrust::{
    encoding::v4::{encode_slice, Connect, Error, LastWill, Protocol},
    Packet, Publish, QoS,
};

use crate::buffer::ByteBuffer;

const DEVICE: &str = "Blinky";

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
    write!(&mut lw_topic, "blinky/{client_id}/power").unwrap();
    let lw = LastWill {
        topic: lw_topic.as_str(),
        message: "OFF".as_bytes(),
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

pub(super) fn discovery<'a>(client_id: &str, buffer: &'a mut [u8]) -> Result<&'a [u8], Error> {
    let mut topic = ByteBuffer::<50>::new();
    write!(&mut topic, "homeassistant/device/{client_id}/config").unwrap();

    let mut payload = ByteBuffer::<1024>::new();

    write!(
        &mut payload,
        r#"{{
  "device": {{
    "identifiers": [
      "{client_id}"
    ]
  }},
  "o": {{
    "name": "{DEVICE}"
  }},
  "cmps": {{
    "power": {{
      "p": "binary_sensor",
      "state_topic": "blinky/{client_id}/power",
      "unique_id": "bl_power"
    }}
  }}
}}"#,
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

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "power")
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            State::On => write!(f, "ON"),
            State::Off => write!(f, "OFF"),
        }
    }
}

pub(super) fn state<'a>(
    client_id: &str,
    device: &Device,
    state: &State,
    buffer: &'a mut [u8],
) -> Result<&'a [u8], Error> {
    let mut topic = ByteBuffer::<50>::new();
    write!(&mut topic, "blinky/{client_id}/{device}").unwrap();

    let mut payload = ByteBuffer::<16>::new();
    write!(&mut payload, "{state}").unwrap();

    encode(
        Publish {
            dup: false,
            qos: QoS::AtMostOnce,
            pid: None,
            retain: true,
            topic_name: topic.as_str(),
            payload: &payload,
        },
        buffer,
    )
}
