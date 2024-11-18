#![no_std]
#![no_main]

use embassy_executor::Spawner;
use panic_probe as _;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    blinky_rs::main(spawner).await;
}
