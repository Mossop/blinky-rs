use core::str;

use crate::{
    log::{error, trace, warn},
    mqtt::packets::{packet_size, state, subscribe, LedPayload},
    LedState,
};
use cyw43::{Control, JoinOptions};
use cyw43_pio::PioSpi;
use embassy_executor::Spawner;
use embassy_futures::select::select3;
use embassy_net::{
    dns::DnsQueryType,
    tcp::{TcpReader, TcpSocket, TcpWriter},
    Config, Stack, StackResources,
};
use embassy_rp::{
    bind_interrupts,
    clocks::RoscRng,
    gpio::{Level, Output},
    peripherals::{DMA_CH0, PIO0},
    pio::{InterruptHandler, Pio},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel};
use embassy_time::Timer;
use embedded_io_async::Write;
use mqttrust::{
    encoding::v4::{decode_slice, ConnectReturnCode, Error},
    Packet,
};
use rand::RngCore;
use static_cell::StaticCell;

mod packets;

use crate::{
    mqtt::packets::{connect, discovery, ping},
    Command, NetPeripherals, COMMAND_CHANNEL,
};

const WIFI_SSID: &str = env!("BLINKY_SSID");
const WIFI_PASSWORD: &str = env!("BLINKY_PASSWORD");
const BROKER: &str = env!("BLINKY_BROKER");

pub enum Topic {
    HaState,
    LedCommand,
}

impl Topic {
    fn from_topic(client_id: &str, topic: &str) -> Option<Topic> {
        if topic == "homeassistant/status" {
            return Some(Topic::HaState);
        }

        if topic.starts_with("blinky/")
            && &topic[7..client_id.len() + 7] == client_id
            && &topic[client_id.len() + 7..client_id.len() + 8] == "/"
        {
            match &topic[client_id.len() + 8..] {
                "leds/set" => Some(Topic::LedCommand),
                _ => None,
            }
        } else {
            None
        }
    }
}

pub enum DeviceState {
    Online,
    Led(LedState),
}

pub(crate) enum MqttMessage {
    Connect,
    Ping,
    SendDiscovery,
    SendState(DeviceState),
    Subscribe(Topic),
}

pub(crate) static MQTT_CHANNEL: channel::Channel<CriticalSectionRawMutex, MqttMessage, 10> =
    channel::Channel::new();

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

impl MqttMessage {
    fn build_packet<'a>(
        &self,
        client_id: &str,
        mac_addr: &str,
        buffer: &'a mut [u8],
    ) -> Result<&'a [u8], Error> {
        match self {
            Self::Connect => connect(client_id, buffer),
            Self::Ping => ping(buffer),
            Self::SendDiscovery => discovery(client_id, mac_addr, buffer),
            Self::SendState(device_state) => state(client_id, device_state, buffer),
            Self::Subscribe(topic) => subscribe(client_id, topic, buffer),
        }
    }
}

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

async fn send_loop(
    client_id: &str,
    mac_addr: &str,
    mut writer: TcpWriter<'_>,
    write_buf: &mut [u8],
) {
    loop {
        let message = MQTT_CHANNEL.receive().await;

        match message.build_packet(client_id, mac_addr, write_buf) {
            Ok(packet) => {
                if writer.write_all(packet).await.is_err() {
                    error!("Failed to send packet");
                    break;
                }
            }
            Err(_) => {
                error!("Failed to encode packet");
                break;
            }
        }
    }
}

async fn recv_loop(client_id: &str, mut reader: TcpReader<'_>, buffer: &mut [u8]) {
    let mut cursor: usize = 0;

    loop {
        match reader.read(&mut buffer[cursor..]).await {
            Ok(0) => {
                error!("Receive socket closed");
                break;
            }
            Ok(len) => {
                cursor += len;
            }
            Err(_) => {
                error!("I/O failure reading packet");
                break;
            }
        }

        let packet_length = match packet_size(&buffer[0..cursor]) {
            Some(0) => {
                error!("Invalid MQTT packet");
                break;
            }
            Some(len) => len,
            None => {
                // None is returned when there is not yet enough data to decode a packet.
                continue;
            }
        };

        let packet = match decode_slice(&buffer[0..packet_length]) {
            Ok(Some(p)) => p,
            Ok(None) => {
                error!("Packet length calculation failed.");
                break;
            }
            Err(_) => {
                error!("Invalid MQTT packet");
                break;
            }
        };

        trace!("Received {:?} packet", packet.get_type());

        match packet {
            Packet::Connack(ack) => {
                if ack.code != ConnectReturnCode::Accepted {
                    error!("Unexpected connection return code {:?}", ack.code);
                    break;
                }

                MQTT_CHANNEL.send(MqttMessage::SendDiscovery).await;
                MQTT_CHANNEL
                    .send(MqttMessage::Subscribe(Topic::HaState))
                    .await;
                MQTT_CHANNEL
                    .send(MqttMessage::Subscribe(Topic::LedCommand))
                    .await;

                COMMAND_CHANNEL.send(Command::MqttConnected).await;
            }
            Packet::Pingresp => {}
            // Used for QoS 1, ignored for now.
            Packet::Puback(_) => {}
            // Used for QoS 2, ignored for now.
            Packet::Pubrec(_) => {}
            Packet::Pubcomp(_) => {}
            // We just assume it all worked.
            Packet::Suback(_) => {}
            Packet::Publish(publish) => {
                if let Some(topic) = Topic::from_topic(client_id, publish.topic_name) {
                    match topic {
                        Topic::HaState => {
                            MQTT_CHANNEL.send(MqttMessage::SendDiscovery).await;
                            COMMAND_CHANNEL.send(Command::MqttConnected).await;
                        }
                        Topic::LedCommand => {
                            match serde_json_core::from_slice::<LedPayload>(publish.payload) {
                                Ok((payload, _)) => {
                                    COMMAND_CHANNEL
                                        .send(Command::SetLedState(payload.to_state()))
                                        .await;
                                }
                                Err(_) => {
                                    trace!(
                                        "Failed to decode led state payload: {}",
                                        str::from_utf8(publish.payload).unwrap()
                                    );
                                }
                            }
                        }
                    }
                }
            }
            p => {
                warn!("Unexpected packet: {:?}", p.get_type());
            }
        }

        // Adjust the buffer to reclaim any unused data
        if packet_length == cursor {
            cursor = 0;
        } else {
            buffer.copy_within(packet_length..cursor, 0);
            cursor -= packet_length;
        }
    }
}

#[embassy_executor::task]
async fn mqtt_task(mut control: Control<'static>, stack: Stack<'static>, client_id: &'static str) {
    control.gpio_set(0, false).await;

    loop {
        match control
            .join(WIFI_SSID, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
            .await
        {
            Ok(_) => break,
            Err(err) => {
                error!("Failed to join network: {}", err.status);
                Timer::after_secs(1).await;
            }
        }
    }

    // Wait for DHCP, not necessary when using static IP
    while !stack.is_config_up() {
        Timer::after_millis(100).await;
    }

    let mut hex_slice = [0; 12];
    hex::encode_to_slice(control.address().await, &mut hex_slice).unwrap();
    let mac_addr = str::from_utf8(&hex_slice).unwrap();

    trace!("Connected to network");

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut read_buf = [0; 4096];
    let mut write_buf = [0; 4096];

    let mut is_first = true;

    loop {
        control.gpio_set(0, false).await;

        if !is_first {
            Timer::after_secs(5).await;
        }
        is_first = false;

        let ip_addrs = match stack.dns_query(BROKER, DnsQueryType::A).await {
            Ok(v) => v,
            Err(_) => {
                error!("Failed to lookup '{}' for broker", BROKER);
                continue;
            }
        };

        let ip = match ip_addrs.first() {
            Some(i) => *i,
            None => {
                error!("No IP address found for broker '{}'", BROKER);
                continue;
            }
        };

        trace!("Connecting to {ip}:1883");

        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        if socket.connect((ip, 1883)).await.is_err() {
            error!("Failed to connect to {ip}:1883");
            continue;
        }

        let (reader, writer) = socket.split();

        control.gpio_set(0, true).await;
        MQTT_CHANNEL.clear();
        MQTT_CHANNEL.send(MqttMessage::Connect).await;

        let recv_loop = recv_loop(client_id, reader, &mut read_buf);
        let send_loop = send_loop(client_id, mac_addr, writer, &mut write_buf);

        let ping_loop = async {
            loop {
                Timer::after_secs(45).await;
                MQTT_CHANNEL.send(MqttMessage::Ping).await;
                COMMAND_CHANNEL.send(Command::MqttConnected).await;
            }
        };

        select3(send_loop, ping_loop, recv_loop).await;

        socket.close();
    }
}

pub async fn spawn_mqtt(spawner: &Spawner, client_id: &'static str, peripherals: NetPeripherals) {
    let fw = include_bytes!("../../cyw43/43439A0.bin");
    let clm = include_bytes!("../../cyw43/43439A0_clm.bin");

    let pwr = Output::new(peripherals.pwr, Level::Low);
    let cs = Output::new(peripherals.cs, Level::High);
    let mut pio = Pio::new(peripherals.pio, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        pio.irq0,
        cs,
        peripherals.dio,
        peripherals.clk,
        peripherals.dma,
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
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(StackResources::new()),
        seed,
    );

    spawner.spawn(embassy_net_task(runner)).unwrap();

    spawner.spawn(mqtt_task(control, stack, client_id)).unwrap();
}
