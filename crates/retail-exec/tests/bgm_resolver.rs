//! The BGM swap poller `FUN_800243F0`'s track resolution: from a track word
//! `_DAT_8007BAC8` that differs from the loaded index, the poller's first three
//! calls (stages `0` and `1` with the CD idle, then stage `2`) reach the payload read
//! `FUN_8001FC00`, whose first argument is the raw TOC index it loads. Held
//! against the port's resolution, `music_labels::prot_entry_for_bgm_id` for a
//! global id and the scene-local fallback `SCENE_LOCAL_BGM_FALLBACK_ID` for a
//! local one, through the `+2` raw-to-extraction skew.
//!
//! Past the bank the poller reads on into the `vab_01` block, which carries no
//! score; the port resolves nothing there, and no shipped script asks.
//!
//! The bank base `_DAT_8007BC64` is seated at the raw `music_01` define the
//! running game holds (`990`); the scene block base `0x80084540` at a
//! generated block.

use crate::common::{Rng, scus_cpu};
use legaia_engine_scene_host::scene_host::SCENE_LOCAL_BGM_FALLBACK_ID;
use legaia_engine_system::music_labels::{MUSIC_BANK_SLOTS, prot_entry_for_bgm_id};
use legaia_overlay_images::boot_overlay::raw_to_extraction;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const POLLER: u32 = 0x8002_43F0;
const PAYLOAD_READ: u32 = 0x8001_FC00;
const CD_SYNC: u32 = 0x8003_DE7C;
const TRACK: u32 = 0x8007_BAC8;
const REQUESTED: u32 = 0x8007_BAB8;
const LOADED: u32 = 0x8007_BA9C;
const BANK_BASE: u32 = 0x8007_BC64;
const SCENE_BASE: u32 = 0x8008_4540;
const TWO_PART_LATCH: u32 = 0x8007_B9B8;
/// `gp + 0x744`, the stage word.
const STAGE: u32 = 0x8007_BA5C;
/// The `music_01` define on a running image.
const MUSIC_01_RAW: u32 = 990;

#[test]
fn the_poller_loads_the_entry_the_port_resolves() {
    differential!(scus(POLLER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for (va, ret) in [
        (0x8001_9890, 0),
        (CD_SYNC, 0),
        (0x8002_66E0, 0),
        (0x8002_6520, 0),
        (0x8005_FB84, 0),
        (0x8001_A068, 0),
    ] {
        cpu.stub(va, ret);
    }
    let reads = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let r2 = reads.clone();
    cpu.hook(PAYLOAD_READ, move |r, _| {
        r2.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    let mut rng = Rng(0x4247_4D52);
    let mut ids: Vec<u16> = (2000..2000 + MUSIC_BANK_SLOTS as u16).collect();
    ids.extend((0..64).map(|_| rng.below(40) as u16));
    let mut mismatched = Vec::new();
    for id in ids {
        // The ending theme stages its two streams through the field init with
        // the poller latched off (`_DAT_8007B9B8`); the poller never loads it.
        if id == legaia_engine_system::mode_entry_init::FIELD_BGM_TWO_PART_ID as u16 {
            continue;
        }
        cpu.mem.wr32(TWO_PART_LATCH, 0);
        // The dev arm's suppress (`_DAT_8007B868`), the sound flags and the
        // battle-music hold (`_DAT_8007B8B8`), all clear in play.
        for va in [0x8007_B868, 0x8007_B750, 0x8007_B8B8, 0x8007_BA88] {
            cpu.mem.wr32(va, 0);
        }
        cpu.mem.wr32(0x1F80_0394, 0);
        cpu.mem.wr32(BANK_BASE, MUSIC_01_RAW);
        cpu.mem.wr32(SCENE_BASE, 0x200 + rng.below(0x200));
        cpu.mem.wr32(TRACK, u32::from(id));
        cpu.mem.wr32(LOADED, 0xFFFF);
        cpu.mem.wr32(STAGE, 0);
        reads.borrow_mut().clear();
        // Stages `0` and `1` wait on the drive; stage `2` kicks the read.
        for call in 0..3 {
            assert_eq!(cpu.call(POLLER, &[]), Stop::Returned, "id {id} call {call}");
        }
        let reads = reads.borrow();
        assert_eq!(
            reads.len(),
            1,
            "id {id}: one payload read, calls {:x?}",
            cpu.calls
        );
        let (raw, slot) = reads[0];
        assert_eq!(slot, 1, "id {id}: the BGM sound slot");
        let port_id = if id < 2000 {
            SCENE_LOCAL_BGM_FALLBACK_ID
        } else {
            id
        };
        let retail = raw_to_extraction(raw);
        let port = prot_entry_for_bgm_id(port_id);
        if retail != port {
            mismatched.push((id, retail, port));
        }
        // The change test's requested index: the bank slot for a global id,
        // the scene block's `+6` slot for a local one.
        let want_requested = if id < 2000 {
            cpu.mem.rd32(SCENE_BASE) + 6 + u32::from(id)
        } else {
            MUSIC_01_RAW + u32::from(id) - 2000
        };
        assert_eq!(cpu.mem.rd32(REQUESTED), want_requested, "id {id}");
    }
    assert!(
        mismatched.is_empty(),
        "id, retail extraction entry, port extraction entry: {mismatched:?}"
    );
}
