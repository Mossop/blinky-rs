use core::str;

use embassy_rp::block::ImageDef;
use embassy_rp::{otp::get_chipid, rom_data::reboot};
use static_cell::StaticCell;

#[link_section = ".start_block"]
#[used]
pub static IMAGE_DEF: ImageDef = ImageDef::secure_exe();

static BOARD_ID: StaticCell<[u8; 16]> = StaticCell::new();

pub fn reboot_to_bootsel() {
    reboot(2, 0, 0, 0);
}

pub fn board_id() -> &'static str {
    let board_id = BOARD_ID.init_with(|| {
        let chip_id = match get_chipid() {
            Ok(u) => u.to_ne_bytes(),
            Err(_) => [0; 8],
        };

        let mut hex_slice = [0; 16];
        hex::encode_to_slice(chip_id, &mut hex_slice).unwrap();
        hex_slice
    });

    str::from_utf8(board_id).unwrap()
}
