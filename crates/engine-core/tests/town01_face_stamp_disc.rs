//! Disc-gated: town01's opening-record **Noa face-frame stamps** - the two
//! field-VM `4C 60` literal-operand VRAM `MoveImage` ops in the Rim Elm
//! opening timeline record (`docs/formats/character-mesh.md`, "Runtime
//! scroll-cell residue") - decode to the pinned rects, and the engine's
//! ported path (`op4c_n6_sub0_emitter6` -> `World::queue_script_vram_move`
//! -> `World::apply_script_vram_moves`) lands the stamp in software VRAM.
//!
//! What this catches:
//! - The `field_disasm` `4C 60` decode regressing (operand width drifting
//!   off the six misaligned LE16 words desyncs the walk; the two ops stop
//!   resolving or their rects change).
//! - The MAN bytes moving out from under the pinned offsets (a container /
//!   decompression regression).
//! - The world-side queue/apply dropping or distorting the rect copy.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    partition_record_span, partition2_record_gates, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{ScriptVramMove, World};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker, MenuCtrlKind};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<ProtIndex> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"))
}

/// The two pinned stamps: `(man_offset, [src_x, src_y, w, h, dst_x, dst_y])`.
/// Both sit in town01 MAN P2[3] (the C1-gated opening record) and copy
/// authored alternate Noa face frames from below the live atlas rows onto
/// the live cells.
const PINNED: [(usize, [i16; 6]); 2] = [
    (0x735A, [852, 336, 6, 16, 852, 268]),
    (0x7368, [852, 368, 4, 8, 853, 284]),
];

#[test]
fn town01_opening_record_face_stamps_pinned_and_stamp_lands_in_vram() {
    let Some(index) = open_index() else { return };

    let town01 = Scene::load(&index, "town01").expect("load town01");
    let carriers = scene_man_carriers(&index, &town01);
    let carrier = carriers.first().expect("town01 MAN carrier");
    assert_eq!(carrier.entry_idx, 4, "town01 bundle MAN lives in PROT[4]");
    let man = &carrier.payload;
    let man_file = legaia_asset::man_section::parse(man).expect("parse town01 MAN");

    // The opening record P2[3] is C1-gated on story flag 0x225.
    let (c1, _c2) = partition2_record_gates(&man_file, man, 3).expect("P2[3] gates");
    assert!(
        c1.contains(&0x225),
        "P2[3] (the opening record) is C1-gated on flag 0x225, got {c1:X?}"
    );

    // Raw-byte pin: both `4C 60` ops sit at the pinned MAN offsets inside
    // the P2[3] span, with the pinned rect operands (six LE16s).
    let (start, pc0, len) = partition_record_span(&man_file, man, 2, 3).expect("P2[3] span");
    for (off, words) in PINNED {
        assert_eq!(&man[off..off + 2], &[0x4C, 0x60], "op bytes at {off:#X}");
        for (i, w) in words.iter().enumerate() {
            let lo = man[off + 2 + i * 2];
            let hi = man[off + 3 + i * 2];
            assert_eq!(
                i16::from_le_bytes([lo, hi]),
                *w,
                "operand word {i} at MAN {off:#X}"
            );
        }
        assert!(
            (start..start + len).contains(&off),
            "op at {off:#X} lies in the P2[3] span {start:#X}+{len:#X}"
        );
    }

    // Decoder pin: the linear walk of P2[3] resolves the pinned Noa pair as
    // consecutive `4C 60` ops (the six-misaligned-LE16 operand width keeps
    // the walk in sync across them). The record carries more stamps - the
    // Vahn blink/mouth pairs recur through the timeline - and every one of
    // them targets the player texture atlas band.
    let body = &man[start..start + len];
    let mut seen: Vec<[i16; 6]> = Vec::new();
    for insn in LinearWalker::new(body, pc0).flatten() {
        if let InsnInfo::MenuCtrl {
            kind: MenuCtrlKind::Nibble6Emitter6 { words },
            ..
        } = insn.info
        {
            seen.push(words);
        }
    }
    assert!(
        seen.windows(2).any(|w| w == [PINNED[0].1, PINNED[1].1]),
        "P2[3] walk yields the pinned Noa MoveImage pair back-to-back, got {seen:?}"
    );
    for words in &seen {
        let [_, _, w, h, dx, dy] = *words;
        assert!(
            (832..1024).contains(&dx) && (256..512).contains(&dy) && w > 0 && h > 0,
            "every P2[3] stamp targets the player atlas band, got {words:?}"
        );
    }

    // Engine path: queue both stamps through the world (the
    // `op4c_n6_sub0_emitter6` host hook's target) and apply them against a
    // scratch software VRAM seeded with distinct source-rect content.
    let mut world = World::new();
    let mut vram = legaia_tim::Vram::new();
    for (k, (_, words)) in PINNED.iter().enumerate() {
        let [sx, sy, w, h, ..] = *words;
        let mut bytes = Vec::new();
        for i in 0..(w as u16 * h as u16) {
            bytes.extend_from_slice(&(0x2000 + (k as u16) * 0x800 + i).to_le_bytes());
        }
        vram.write_block(sx as u16, sy as u16, w as u16, h as u16, &bytes);
        world.queue_script_vram_move(*words);
    }
    assert_eq!(
        world.script_vram_moves,
        PINNED.map(|(_, w)| ScriptVramMove::from_words(w)).to_vec(),
        "both stamps queued in script order"
    );
    assert!(
        world.apply_script_vram_moves(&mut vram),
        "stamps wrote VRAM"
    );
    for (k, (_, words)) in PINNED.iter().enumerate() {
        let [_, _, w, h, dx, dy] = *words;
        for row in 0..h as usize {
            for col in 0..w as usize {
                assert_eq!(
                    vram.pixel(dx as usize + col, dy as usize + row),
                    0x2000 + (k as u16) * 0x800 + (row as u16 * w as u16 + col as u16),
                    "stamp {k} dst ({col},{row})"
                );
            }
        }
    }
}
