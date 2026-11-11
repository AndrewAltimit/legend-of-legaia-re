//! The menu overlay's character-parameter editor `FUN_801D6E18` (PROT
//! 0899) against `legaia_engine_menus::debug_char_editor`: its input and
//! clamp halves, run whole with every call outside the routine stubbed (the
//! window walk, the stat aggregator `FUN_80042558`, the SFX cue and the
//! debug renderer's drawers). Random rows, characters, pads, modifiers and
//! records; the four character records and both cursors are compared byte
//! for byte after the tick with `DebugEditor::tick`, the confirm row's
//! `apply_confirm_clear` and `clamp_record_stats` over all four.
//!
//! The renderer half (`0x801D7524` on) is not ported and not compared.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_menus::debug_char_editor::{
    CONFIRM_ROW, DebugEditor, apply_confirm_clear, clamp_record_stats,
};
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const EDITOR: u32 = 0x801D_6E18;
const EDITOR_END: u32 = 0x801D_7C00;
const PHASE: u32 = 0x801E_46AC;
const CHARACTER: u32 = 0x801E_46C4;
const CANCEL_MASK: u32 = 0x801E_46D4;
const ROW: u32 = 0x8007_BB88;
const PAD_HELD: u32 = 0x8007_BB84;
const MODIFIERS: u32 = 0x8007_B850;
const PAD_PRESS: u32 = 0x8007_B874;
const CONFIRM_MASK: u32 = 0x8008_4140 + 0x590;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;

#[test]
fn the_character_editor_ticks_as_retail_does() {
    differential!(prot(899, EDITOR));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((EDITOR, EDITOR_END));
    let mut rng = Rng(0x4544_4954);
    let mut confirmed = 0;
    for case in 0..5000 {
        let row = rng.below(13);
        let character = rng.below(4);
        let pad = [0x1000u32, 0x4000, 0x2000, 0x8000, 0xA000, 0x5000, 0][rng.below(7) as usize]
            | (rng.next() & 0x0FFF);
        let modifiers = rng.below(16);
        let press = if rng.below(4) == 0 { 0x20 } else { 0 };
        let mut records: Vec<Vec<u8>> = (0..4)
            .map(|_| {
                (0..STRIDE)
                    .map(|k| match k {
                        // Stats and level near their caps and their resets.
                        0x11C..=0x131 => [0u8, 1, 0x3E, 0xE7, 0xFF, 0x4E][rng.below(6) as usize],
                        _ => rng.next() as u8,
                    })
                    .collect()
            })
            .collect();
        for (n, r) in records.iter().enumerate() {
            cpu.mem.load(RECORDS + STRIDE * n as u32, r);
        }
        cpu.mem.wr32(PHASE, 1);
        cpu.mem.wr32(CHARACTER, character);
        cpu.mem.wr32(ROW, row);
        cpu.mem.wr32(PAD_HELD, pad);
        cpu.mem.wr32(MODIFIERS, modifiers);
        cpu.mem.wr32(PAD_PRESS, press);
        cpu.mem.wr32(CANCEL_MASK, 0x40);
        cpu.mem.wr32(CONFIRM_MASK, 0x20);
        assert_eq!(cpu.call(EDITOR, &[]), Stop::Returned, "case {case}");

        let mut ed = DebugEditor { row, character };
        {
            let mut refs: Vec<&mut [u8]> = records.iter_mut().map(|r| r.as_mut_slice()).collect();
            ed.tick(pad, modifiers, &mut refs);
        }
        if press != 0 && ed.row == CONFIRM_ROW {
            apply_confirm_clear(&mut records[ed.character as usize]);
            confirmed += 1;
        }
        for r in &mut records {
            clamp_record_stats(r);
        }
        assert_eq!(cpu.mem.rd32(ROW), ed.row, "case {case}: the row");
        assert_eq!(
            cpu.mem.rd32(CHARACTER),
            ed.character,
            "case {case}: the character"
        );
        for (n, r) in records.iter().enumerate() {
            let got = cpu.mem.read(RECORDS + STRIDE * n as u32, STRIDE as usize);
            assert!(
                &got == r,
                "case {case} record {n} (row {row} pad {pad:#x} mods {modifiers:#x}): {:?}",
                diff_offsets(&got, r)
            );
        }
    }
    assert!(confirmed > 50, "the confirm row ran {confirmed} times");
}
