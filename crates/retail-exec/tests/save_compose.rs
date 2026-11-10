//! The save composer `FUN_801E1934` (menu overlay, PROT 0899) against the
//! port's block writers: the staged `0x2000`-byte block retail builds from
//! live RAM, compared whole with a block the port composes from the same
//! live state through `write_retail_block_title`,
//! `write_retail_block_identity` and `write_retail_save_summary`.
//!
//! The VRAM reads (`StoreImage` `FUN_8005842C`, `DrawSync` `FUN_80058104`)
//! are hooked: the store fills its destination from the rect it was asked
//! for, and the port is handed the icon those bytes make. Two inputs are the
//! harness's, not the composer's port: the title template's play-time digits
//! (the save screen patches them before the composer runs; seeded here as
//! `save_title_time_digits` of the clock), and the two live-state words the
//! composer refreshes - the banner location (`0x24` bytes from the pointer at
//! `0x8007B44C`) and the player position halfwords - which the port's save
//! path writes into the live window before composing.

use crate::common::{Rng, disc, scus_cpu};
use legaia_save::card::{
    RETAIL_TITLE_OFFSET, RETAIL_TITLE_TEMPLATE_LEN, RETAIL_TITLE_TIME_DIGIT_OFFSETS,
    save_title_time_digits, write_retail_block_title, write_retail_save_summary,
};
use legaia_save::{
    BLOCK_SIZE, RETAIL_GAME_DATA_OFFSET, RETAIL_ICON_CLUT_BYTES, RETAIL_LIVE_STATE_SIZE,
    RETAIL_PLAY_COUNTER_OFFSET, RetailBlockIcon, write_retail_block_identity,
};
use psx_retail_exec::{A0, A1, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const COMPOSE: u32 = 0x801E_1934;
const STORE_IMAGE: u32 = 0x8005_842C;
const DRAW_SYNC: u32 = 0x8005_8104;
const LIVE: u32 = 0x8008_4140;
const STAGING: u32 = 0x801E_7120;
const SLOT: u32 = 0x801F_0210;
const TEMPLATE: u32 = 0x801E_4FC8;
const LOCATION_PTR: u32 = 0x8007_B44C;
const LOCATION: u32 = 0x8012_1000;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8012_2000;

/// What the hooked `StoreImage` writes for a rect: a function of the rect
/// and the byte index, so each frame and the palette row differ.
fn vram_byte(rect: [u16; 4], i: usize) -> u8 {
    (rect[0] as usize * 7 + rect[1] as usize * 13 + rect[3] as usize * 3 + i * 31) as u8
}

#[test]
fn the_composer_stages_the_block_the_port_composes() {
    differential!(prot(899, COMPOSE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry_footprint(899).expect("PROT 0899");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let template_off = (TEMPLATE - OVERLAY_BASE) as usize;
    let pristine: [u8; RETAIL_TITLE_TEMPLATE_LEN] = overlay
        [template_off..template_off + RETAIL_TITLE_TEMPLATE_LEN]
        .try_into()
        .unwrap();
    cpu.hook(STORE_IMAGE, |r, m| {
        let rect: [u16; 4] = std::array::from_fn(|k| m.rd16(r.r[A0] + 2 * k as u32));
        let n = usize::from(rect[2]) * usize::from(rect[3]) * 2;
        for i in 0..n {
            m.wr8(r.r[A1] + i as u32, vram_byte(rect, i));
        }
    });
    cpu.stub(DRAW_SYNC, 0);
    let mut rng = Rng(0x434F_4D50);
    for case in 0..200 {
        let mut live = vec![0u8; RETAIL_LIVE_STATE_SIZE];
        live.iter_mut().for_each(|b| *b = rng.next() as u8);
        // The header's pad after the title is zero in live RAM.
        live[0x44..0x60].fill(0);
        // A party the game can hold: up to three of the four roster records.
        live[0x454] = [0u8, 1, 2, 3, 3][rng.below(5) as usize];
        for k in 0..4 {
            live[0x458 + k] = rng.below(4) as u8;
        }
        let clock = rng.below(400 * 216_000);
        live[RETAIL_PLAY_COUNTER_OFFSET..RETAIL_PLAY_COUNTER_OFFSET + 4]
            .copy_from_slice(&clock.to_le_bytes());
        let slot = rng.below(15);
        cpu.mem.load(LIVE, &live);
        cpu.mem.wr32(SLOT, slot);
        let mut template = pristine;
        for (off, digit) in RETAIL_TITLE_TIME_DIGIT_OFFSETS
            .iter()
            .zip(save_title_time_digits(clock))
        {
            template[*off] = digit;
        }
        cpu.mem.load(TEMPLATE, &template);
        let location: Vec<u8> = (0..0x24).map(|_| rng.next() as u8).collect();
        cpu.mem.load(LOCATION, &location);
        cpu.mem.wr32(LOCATION_PTR, LOCATION);
        let (px, pz) = (rng.next() as i16, rng.next() as i16);
        cpu.mem.wr32(PLAYER_PTR, PLAYER);
        cpu.mem.wr16(PLAYER + 0x14, px as u16);
        cpu.mem.wr16(PLAYER + 0x18, pz as u16);
        for i in 0..BLOCK_SIZE as u32 / 4 {
            cpu.mem.wr32(STAGING + 4 * i, 0xDEAD_BEEF);
        }
        assert_eq!(cpu.call(COMPOSE, &[]), Stop::Returned, "case {case}");
        let retail = cpu.mem.read(STAGING, BLOCK_SIZE);

        // The port: the same live state in a fresh block, refreshed where
        // the save path refreshes it, then the identity and summary writers.
        let mut port = vec![0u8; BLOCK_SIZE];
        port[..RETAIL_LIVE_STATE_SIZE].copy_from_slice(&live);
        let loc = RETAIL_GAME_DATA_OFFSET;
        port[loc..loc + 0x24].copy_from_slice(&location);
        port[0x428..0x42C].copy_from_slice(&i32::from(px).to_le_bytes());
        port[0x42C..0x430].copy_from_slice(&i32::from(pz).to_le_bytes());
        port[RETAIL_TITLE_OFFSET..RETAIL_TITLE_OFFSET + RETAIL_TITLE_TEMPLATE_LEN].fill(0);
        write_retail_block_title(&mut port, &template).unwrap();
        let frame = [0x3C0 + 4 * slot as u16, 0xE0, 4, 0x10];
        let row = [0, 1, 0x100, 1];
        let icon = RetailBlockIcon {
            clut: std::array::from_fn(|i| {
                vram_byte(row, slot as usize * RETAIL_ICON_CLUT_BYTES + i)
            }),
            pixels: std::array::from_fn(|i| vram_byte(frame, i)),
        };
        write_retail_block_identity(&mut port, slot, Some(&icon)).unwrap();
        write_retail_save_summary(&mut port).unwrap();
        assert!(
            retail == port,
            "case {case} slot {slot} count {}: {:?}",
            live[0x454],
            crate::common::diff_offsets(&retail, &port)
        );
    }
}
