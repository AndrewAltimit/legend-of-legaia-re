//! The play page's mid-battle VRAM re-upload channel
//! (`crates/web-viewer/src/play_battle_vram.rs`): the three per-tick
//! re-stamps the native window runs against its battle VRAM (facial
//! animation, the Stone CLUT recolour, the effect CLUT stage) now run on the
//! page's battle VRAM copy, and `play_battle_vram_take_dirty` is the page's
//! cue to re-upload.
//!
//! Three rungs, each driven on the real play page (a forced encounter on
//! `map01` after the new-game seed), each asserting on the bytes
//! `play_battle_vram_bytes` hands the page:
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | face stamps | the animator registered members; 120 idle ticks (a clip with no active record on disc) raise no dirty edge; a clip whose tracks carry active records, staged through the world's own `+0x1DA` commit, raises one and the face-stamp region (the rects `FaceFrameTables` names for the members) differs from the entry image |
//! | 2 | status CLUT | a Stone affliction armed through the world's tracker restages party CLUT row `481 + slot` grey - every entry equal to the shared kernel's `bgr555_to_grey` of the pristine entry |
//! | 3 | effect CLUT | a queued stage byte copies sixteen entries of row 476 from the source column onto column 224 |
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::battle_effect_clut::{
    EFFECT_CLUT_DEST_X, EFFECT_CLUT_ENTRIES, EFFECT_CLUT_ROW,
};
use legaia_engine_core::battle_status_clut::{PARTY_CLUT_ENTRIES, PARTY_CLUT_ROW_BASE};
use legaia_engine_vm::scus_battle_helpers::bgr555_to_grey;
use legaia_web_viewer::runtime::LegaiaRuntime;

const VRAM_WIDTH: usize = 1024;

fn disc_bytes() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(path).ok()
}

/// VRAM halfword at `(x, y)` out of a `play_battle_vram_bytes` image.
fn px(vram: &[u8], x: usize, y: usize) -> u16 {
    let i = (y * VRAM_WIDTH + x) * 2;
    u16::from_le_bytes([vram[i], vram[i + 1]])
}

/// One party CLUT row (`481 + slot`), the 240 entries the recolour covers.
fn party_row(vram: &[u8], slot: usize) -> Vec<u16> {
    let row = PARTY_CLUT_ROW_BASE as usize + slot;
    (0..PARTY_CLUT_ENTRIES).map(|x| px(vram, x, row)).collect()
}

/// The halfwords inside every `[x, y, w, h]` quad of `regions`.
fn region_words(vram: &[u8], regions: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    for q in regions.as_chunks::<4>().0 {
        let (x, y, w, h) = (q[0] as usize, q[1] as usize, q[2] as usize, q[3] as usize);
        for row in y..y + h {
            for col in x..x + w {
                out.push(px(vram, col, row));
            }
        }
    }
    out
}

/// Seed the party, walk to map01, force a fight and tick into it.
fn enter_forced_battle(rt: &mut LegaiaRuntime) -> Result<(), String> {
    rt.debug_enter_town01_opening()
        .map_err(|e| format!("enter town01 opening: {e}"))?;
    for _ in 0..8 {
        let _ = rt.tick_frame();
    }
    rt.enter_field("map01")
        .map_err(|_| "enter_field(map01) failed".to_string())?;
    for _ in 0..5 {
        let _ = rt.tick_frame();
    }
    if rt.party_display_name(0).is_empty() {
        return Err("the new-game seed left party slot 0 without a record".into());
    }
    if !rt.debug_force_battle(-1) {
        return Err("debug_force_battle(-1) resolved no formation on map01".into());
    }
    for _ in 0..400 {
        let _ = rt.tick_frame();
        if rt.play_battle_active() {
            return Ok(());
        }
    }
    Err("forced encounter never reached SceneMode::Battle".into())
}

fn rung1_face_stamps(rt: &mut LegaiaRuntime) -> Result<(), String> {
    let faces = rt.play_battle_face_count();
    if faces == 0 {
        return Err("no party member registered with the facial animator".into());
    }
    let regions = rt.debug_battle_face_regions();
    if regions.is_empty() {
        return Err("SCUS face tables resolved no stamp rects for the registered members".into());
    }
    // The entry tick's stamps are folded into the generation upload; the
    // channel must not ask for a second identical upload for them.
    let _ = rt.play_battle_vram_take_dirty();
    let entry = rt.play_battle_vram_bytes();
    if entry.is_empty() {
        return Err("play_battle_vram_bytes is empty in battle".into());
    }
    let entry_face = region_words(&entry, &regions);

    // Phase A - idle. The idle clip (slot 0) carries no active face record
    // on disc, so the animator re-issues the neutral stamp set every frame
    // and the channel must stay quiet: a dirty edge here would be a
    // re-upload of an identical image every tick.
    let mut idle_dirty = 0u32;
    for _ in 0..120 {
        if !rt.play_battle_active() {
            return Err("battle ended during the idle window".into());
        }
        let _ = rt.tick_frame();
        if rt.play_battle_vram_take_dirty() {
            idle_dirty += 1;
        }
    }
    if idle_dirty != 0 {
        return Err(format!(
            "{idle_dirty} dirty edges over 120 idle ticks - identical stamp sets must not re-upload"
        ));
    }
    if region_words(&rt.play_battle_vram_bytes(), &regions) != entry_face {
        return Err("the face region moved during idle without a dirty edge".into());
    }

    // Phase B - a clip whose tracks carry active records, staged through
    // the world's own `+0x1DA` byte + `FUN_8004AD80` commit. The id comes
    // off the member's disc tracks, not a constant.
    let tracked = rt.debug_battle_face_tracked_ids(0);
    let Some(&id) = tracked.first() else {
        return Err("no action id of member 0 carries an active face record".into());
    };
    eprintln!("[face] tracked ids for member 0: {tracked:x?}; staging {id:#04x}");
    if !rt.debug_stage_battle_anim(0, id as u8) {
        return Err(format!(
            "debug_stage_battle_anim(0, {id:#04x}) did not install the clip"
        ));
    }
    let mut dirty_frames = 0u32;
    let mut region_changed = false;
    for _ in 0..240 {
        if !rt.play_battle_active() {
            break;
        }
        let _ = rt.tick_frame();
        if rt.play_battle_vram_take_dirty() {
            dirty_frames += 1;
            if region_words(&rt.play_battle_vram_bytes(), &regions) != entry_face {
                region_changed = true;
                break;
            }
        }
    }
    if dirty_frames == 0 {
        return Err(format!(
            "clip {id:#04x} played but play_battle_vram_take_dirty never reported a change: {}",
            rt.debug_battle_face_state()
        ));
    }
    if !region_changed {
        return Err(format!(
            "{dirty_frames} dirty frames but the face-stamp region never differed from entry"
        ));
    }
    Ok(())
}

fn rung2_status_clut(rt: &mut LegaiaRuntime) -> Result<(), String> {
    if !rt.play_battle_active() {
        return Err("battle over before the status rung".into());
    }
    let _ = rt.play_battle_vram_take_dirty();
    let before = rt.play_battle_vram_bytes();
    let pristine = party_row(&before, 0);
    if pristine.iter().all(|&w| w == 0) {
        return Err("party CLUT row 481 is blank - no palette to recolour".into());
    }
    if !rt.debug_apply_battle_status(0, "Stone") {
        return Err("debug_apply_battle_status(0, Stone) refused".into());
    }
    let mut dirty = false;
    for _ in 0..4 {
        let _ = rt.tick_frame();
        dirty |= rt.play_battle_vram_take_dirty();
    }
    if !dirty {
        return Err("Stone armed but the channel never reported dirty".into());
    }
    let after = party_row(&rt.play_battle_vram_bytes(), 0);
    let expected: Vec<u16> = pristine.iter().map(|&c| bgr555_to_grey(c)).collect();
    if after != expected {
        let first = after
            .iter()
            .zip(&expected)
            .position(|(a, e)| a != e)
            .unwrap_or(0);
        return Err(format!(
            "row 481 after Stone != bgr555_to_grey(pristine); first mismatch at entry {first}: \
             got {:#06x} expected {:#06x} (pristine {:#06x})",
            after[first], expected[first], pristine[first]
        ));
    }
    if after == pristine {
        return Err("the grey restage left row 481 identical to the pristine palette".into());
    }
    // Retail spends the latch (`sb zero,0x220`): a steady Stone must not
    // restage every frame.
    for _ in 0..3 {
        let _ = rt.tick_frame();
    }
    if party_row(&rt.play_battle_vram_bytes(), 0) != expected {
        return Err("row 481 moved again after the latch was spent".into());
    }
    Ok(())
}

fn rung3_effect_clut(rt: &mut LegaiaRuntime) -> Result<(), String> {
    if !rt.play_battle_active() {
        return Err("battle over before the effect rung".into());
    }
    let _ = rt.play_battle_vram_take_dirty();
    let before = rt.play_battle_vram_bytes();
    // `0xB0` is one of the three live values of the retail map (`{0xB0,
    // 0xC0, 0xD0}`), so this is a stage a real cast issues.
    let src_x = 0xB0u8;
    if !rt.debug_stage_battle_effect_clut(src_x) {
        return Err("debug_stage_battle_effect_clut refused".into());
    }
    let _ = rt.tick_frame();
    if !rt.play_battle_vram_take_dirty() {
        return Err("effect CLUT stage queued but the channel never reported dirty".into());
    }
    let after = rt.play_battle_vram_bytes();
    let row = EFFECT_CLUT_ROW as usize;
    let src: Vec<u16> = (0..EFFECT_CLUT_ENTRIES)
        .map(|i| px(&before, src_x as usize + i, row))
        .collect();
    let dst: Vec<u16> = (0..EFFECT_CLUT_ENTRIES)
        .map(|i| px(&after, EFFECT_CLUT_DEST_X as usize + i, row))
        .collect();
    if src != dst {
        return Err(format!(
            "row {row} columns {}..+16 != source columns {src_x:#x}..+16 after the stage",
            EFFECT_CLUT_DEST_X
        ));
    }
    Ok(())
}

#[test]
fn battle_vram_channel_ladder() {
    let Some(disc) = disc_bytes() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set (disc-gated)");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc, String::new()).expect("load_disc");
    enter_forced_battle(&mut rt).expect("enter forced battle");

    let mut fail: Option<(&str, String)> = None;
    type Rung = fn(&mut LegaiaRuntime) -> Result<(), String>;
    let rungs: [(&str, Rung); 3] = [
        ("face-stamps", rung1_face_stamps),
        ("status-clut", rung2_status_clut),
        ("effect-clut", rung3_effect_clut),
    ];
    for (name, rung) in rungs {
        match rung(&mut rt) {
            Ok(()) => eprintln!("[ok] {name}"),
            Err(e) => {
                eprintln!("[FAIL] {name}: {e}");
                fail.get_or_insert((name, e));
            }
        }
    }
    if let Some((name, e)) = fail {
        panic!("battle VRAM channel rung '{name}' failed: {e}");
    }
}
