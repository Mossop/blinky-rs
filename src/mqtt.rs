use core::str;

use cyw43::{Control, JoinOptions};
use cyw43_pio::PioSpi;
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_net::{
    dns::DnsQueryType,
    tcp::{TcpReader, TcpSocket},
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
use log::{error, trace};
use mqttrust::{
    encoding::v4::{decode_slice, encode_slice, Connect, ConnectReturnCode, Protocol},
    Packet,
};
use rand::RngCore;
use static_cell::StaticCell;

use crate::{Command, NetPeripherals, COMMAND_CHANNEL};

const WIFI_SSID: &str = env!("BLINKY_SSID");
const WIFI_PASSWORD: &str = env!("BLINKY_PASSWORD");
const BROKER: &str = env!("BLINKY_BROKER");

pub(crate) enum MqttMessage {}

pub(crate) static MQTT_CHANNEL: channel::Channel<CriticalSectionRawMutex, MqttMessage, 10> =
    channel::Channel::new();

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

impl MqttMessage {
    fn packet(&self) -> Packet<'_> {
        todo!();
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

enum MqttError {
    Network,
    Mqtt,
}

impl From<embassy_net::tcp::Error> for MqttError {
    fn from(_: embassy_net::tcp::Error) -> Self {
        Self::Network
    }
}

impl From<mqttrust::encoding::v4::Error> for MqttError {
    fn from(_: mqttrust::encoding::v4::Error) -> Self {
        Self::Mqtt
    }
}

async fn read_packet<'a, 'b>(
    reader: &mut TcpReader<'b>,
    buffer: &'a mut [u8; 4096],
) -> Result<Packet<'a>, MqttError> {
    let mut pos: usize = 0;
    loop {
        let len = reader.read(&mut buffer[pos..]).await?;
        pos += len;

        if decode_slice(&buffer[0..pos])?.is_some() {
            break;
        }
    }

    Ok(decode_slice(&buffer[0..pos])?.unwrap())
}

#[embassy_executor::task]
async fn mqtt_task(mut control: Control<'static>, stack: Stack<'static>) {
    control.gpio_set(0, false).await;

    loop {
        match control
            .join(WIFI_SSID, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
            .await
        {
            Ok(_) => break,
            Err(err) => {
                error!("MQTT: Failed to join network: {}", err.status);
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
    let client_id = str::from_utf8(&hex_slice).unwrap();

    trace!("MQTT: Connected to network");

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
                error!("MQTT: Failed to lookup '{}' for broker", BROKER);
                continue;
            }
        };

        let ip = match ip_addrs.first() {
            Some(i) => *i,
            None => {
                error!("MQTT: No IP address found for broker '{}'", BROKER);
                continue;
            }
        };

        trace!("MQTT: Connecting to {ip}:1883");

        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        if socket.connect((ip, 1883)).await.is_err() {
            error!("MQTT: Failed to connect to {ip}:1883");
            continue;
        }

        let (mut reader, mut writer) = socket.split();

        let packet = Connect {
            protocol: Protocol::MQTT311,
            keep_alive: 60,
            client_id,
            clean_session: true,
            last_will: None,
            username: None,
            password: None,
        }
        .into();

        let len = match encode_slice(&packet, &mut write_buf) {
            Ok(l) => l,
            Err(_) => {
                error!("MQTT: Failed to encode connect packet");
                continue;
            }
        };

        if writer.write_all(&write_buf[0..len]).await.is_err() {
            error!("MQTT: Failed to send connect message");
            continue;
        }

        let packet = match read_packet(&mut reader, &mut read_buf).await {
            Ok(p) => p,
            Err(_) => {
                error!("MQTT: Failed to receive packet");
                continue;
            }
        };

        match packet {
            Packet::Connack(ack) => {
                if ack.code != ConnectReturnCode::Accepted {
                    error!("Unexpected connection return code {:?}", ack.code);
                    continue;
                }
            }
            p => {
                error!("Unexpected packet {:?}", p.get_type());
                continue;
            }
        }

        control.gpio_set(0, true).await;
        COMMAND_CHANNEL.send(Command::MqttConnected).await;

        let send_loop = async {
            loop {
                let message = MQTT_CHANNEL.receive().await;

                let len = match encode_slice(&message.packet(), &mut write_buf) {
                    Ok(l) => l,
                    Err(_) => {
                        error!("MQTT: Failed to encode connect packet");
                        break;
                    }
                };

                if writer.write_all(&write_buf[0..len]).await.is_err() {
                    error!("MQTT: Failed to send packet");
                    break;
                }
            }
        };

        let recv_loop = async {
            loop {
                let packet = match read_packet(&mut reader, &mut read_buf).await {
                    Ok(p) => p,
                    Err(_) => {
                        error!("MQTT: Failed to receive packet");
                        break;
                    }
                };
            }
        };

        select(send_loop, recv_loop).await;
    }
}

pub async fn spawn_mqtt(spawner: &Spawner, peripherals: NetPeripherals) {
    let fw = include_bytes!("../cyw43/43439A0.bin");
    let clm = include_bytes!("../cyw43/43439A0_clm.bin");

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

    spawner.spawn(mqtt_task(control, stack)).unwrap();
}
