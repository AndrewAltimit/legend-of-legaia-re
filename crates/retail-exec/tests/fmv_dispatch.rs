//! The FMV master dispatch `FUN_801CEA3C` (PROT 0970) for every movie id,
//! its play loop `FUN_801CF098` hooked and every other call stubbed, against
//! the port's three readings of it: `fmv_bitstream` (the Iki word
//! `DAT_801E09FC`), `fmv_is_skippable` (the pad-abort arm `DAT_801E09F4`)
//! and `fmv_post_play_handoff` (the second `switch`: the scene label
//! `0x80084548`, the door word `0x80084540`, the mode word `0x8007B83C`, the
//! card selector `_DAT_8007BB00` and the field-entry word `0x8007B8B8`) - and
//! the dispatch slot the play loop is handed.

use crate::common::{disc, scus_cpu};
use legaia_engine_system::movie::cutscene::{
    FmvBitstream, FmvHandoff, fmv_bitstream, fmv_is_skippable, fmv_post_play_handoff,
};
use psx_retail_exec::{A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const DISPATCH: u32 = 0x801C_EA3C;
const PLAY: u32 = 0x801C_F098;
const BASE: u32 = 0x801C_E818;
const SLOTS: u32 = 0x801D_0A6C;
const FMV_ID: u32 = 0x8007_BA78;
const IKI: u32 = 0x801E_09FC;
const SKIPPABLE: u32 = 0x801E_09F4;
const NAME: u32 = 0x8008_4548;
const DOOR: u32 = 0x8008_4540;
const MODE: u32 = 0x8007_B83C;
const CARD_ARG: u32 = 0x8007_BB00;
const FIELD_ENTRY: u32 = 0x8007_B8B8;

#[test]
fn the_dispatch_selects_and_hands_off_as_the_port() {
    differential!(prot(970, DISPATCH));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(BASE, &d.read_entry(970).expect("PROT 0970"));
    cpu.stub_outside = Some((DISPATCH, PLAY));
    let played = Rc::new(RefCell::new(Vec::<u32>::new()));
    let p = played.clone();
    cpu.hook(PLAY, move |r, m| {
        // The skip arm is read while the movie plays.
        p.borrow_mut().push(r.r[A1]);
        let _ = m;
    });
    let (mut fields, mut others) = (0, 0);
    for id in (-2i16..=24).chain([0x7FFF, -0x8000]) {
        cpu.mem.wr16(FMV_ID, id as u16);
        cpu.mem.load(NAME, b"SENTINEL_SCENE!\0");
        cpu.mem.wr32(DOOR, 0xDEAD);
        cpu.mem.wr16(MODE, 0x77);
        cpu.mem.wr32(CARD_ARG, 0x55);
        cpu.mem.wr32(FIELD_ENTRY, 0x99);
        cpu.mem.wr32(IKI, 0x1234);
        cpu.mem.wr32(SKIPPABLE, 0x1234);
        played.borrow_mut().clear();
        assert_eq!(cpu.call(DISPATCH, &[]), Stop::Returned, "fmv {id}");
        let label = format!("fmv {id}");
        assert_eq!(
            *played.borrow(),
            vec![SLOTS.wrapping_add((id as i32 as u32) << 5)],
            "{label}: the slot"
        );
        let iki = cpu.mem.rd32(IKI);
        assert_eq!(
            iki != 0,
            fmv_bitstream(id) == FmvBitstream::Iki,
            "{label}: Iki word {iki:#x}"
        );
        assert_eq!(
            cpu.mem.rd32(SKIPPABLE) != 0,
            fmv_is_skippable(id),
            "{label}: skip arm"
        );
        let name: Vec<u8> = (0..16)
            .map(|i| cpu.mem.rd8(NAME + i))
            .take_while(|b| *b != 0)
            .collect();
        let (door, mode) = (cpu.mem.rd32(DOOR), cpu.mem.rd16(MODE));
        let (card, entry) = (cpu.mem.rd32(CARD_ARG), cpu.mem.rd32(FIELD_ENTRY));
        match fmv_post_play_handoff(id) {
            FmvHandoff::Field { scene, door: want } => {
                assert_eq!(name, scene.as_bytes(), "{label}");
                assert_eq!(door, u32::from(want), "{label}");
                assert_eq!(mode, 2, "{label}");
                assert_eq!(card, 0x55, "{label}");
                // The arms' shared tail clears the field-entry word
                // (`0x801CF020`; `jou`'s own copy at `0x801CEF68`), so the
                // field enters through MAIN INIT's plain arm.
                assert_eq!(entry, 0, "{label}");
                fields += 1;
            }
            FmvHandoff::ResumeField => {
                assert_eq!(name, b"SENTINEL_SCENE!", "{label}");
                assert_eq!((door, mode, entry), (0xDEAD, 2, 2), "{label}");
                others += 1;
            }
            FmvHandoff::CardInit { card_arg } => {
                assert_eq!((mode, card), (0x16, u32::from(card_arg)), "{label}");
                assert_eq!((door, entry), (0xDEAD, 0x99), "{label}");
                others += 1;
            }
            FmvHandoff::ModeZero => {
                assert_eq!((mode, door, card), (0, 0xDEAD, 0x55), "{label}");
                others += 1;
            }
            FmvHandoff::None => {
                assert_eq!(name, b"SENTINEL_SCENE!", "{label}");
                assert_eq!(
                    (door, mode, card, entry),
                    (0xDEAD, 0x77, 0x55, 0x99),
                    "{label}"
                );
            }
        }
    }
    assert_eq!((fields, others), (7, 4));
}
