//! Disc-gated: the move-VM op-`0x30` restore of a recoloured CLUT strip,
//! observed in a live cast.
//!
//! Five summon modules (PROT 0916, 0921, 0928, 0929, 0931) seat a mode-3
//! part that captures a CLUT strip with op `0x2C`, recolours it through
//! `FUN_80019D50` each frame, and then runs op `0x30`, whose
//! `FUN_800583C8(actor + 0xA0, buf)` (`jal` at `0x80023B04` in
//! `FUN_80023070`) uploads the capture back. Without that upload the strip
//! keeps the cast's last recolour for the rest of the fight.
//!
//! The drive is the ordinary engine path, as `cast_stager_arms_ladder`'s is:
//! a real encounter, the cast handed to the action SM through
//! [`InflightCastSeed`], and every frame a headless driver's frame (the
//! scene tick, the world's frame tail, then `apply_battle_vram_moves` on a
//! battle VRAM, the call both hosts make). The battle VRAM's CLUT band is
//! filled with a non-zero ramp so a recolour is visible in it.
//!
//! Observed per cast: whether a part's op `0x30` handed a capture back while
//! the cast was alive, and whether the strip it names reads its pre-cast
//! texels at the end. Skips (and passes) without `LEGAIA_DISC_BIN` /
//! `extracted/`.

use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_battle::spells::{
    SpellCatalog, SpellDef, SpellEffect, SpellElement, SpellTarget,
};
use legaia_engine_battle_loop::battle_state::INFLIGHT_GROUND_SLOTS;
use legaia_engine_battle_loop::battle_state::InflightCastSeed;
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::{InputState, PadButton};
use legaia_overlay_images::cast_effect_pool::{
    CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The modules whose parts capture, recolour and restore a CLUT strip.
const RESTORE_ENTRIES: [u32; 5] = [916, 921, 928, 929, 931];

/// Frames one cast is given past the frame it was armed.
const CAST_FRAMES: usize = 0x1800;
/// Frames a folded cast's still-seated parts are given.
const TAIL_FRAMES: usize = 0x400;

/// Retail's enemy-group target code on the action record's `+0x1DD`.
const TARGET_ENEMY_GROUP: u8 = 9;

/// The CLUT band the summon strips sit in (rows `476..=511`).
const BAND_ROWS: std::ops::Range<u16> = 476..512;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

fn prot_entry(dir: &Path, idx: usize) -> Vec<u8> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries[idx].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .unwrap_or_else(|e| panic!("read PROT {idx}: {e:#}"));
    bytes
}

/// The pool the scene host builds, straight off `PROT.DAT`.
fn build_pool(dir: &Path) -> CastEffectPool {
    let mut pool = CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let bytes = prot_entry(dir, idx as usize);
        assert!(pool.insert(idx, &bytes), "PROT {idx} is in the band");
    }
    pool
}

/// A spell the catalog can dispatch for an id the pinned Seru catalog does
/// not carry. The module, not this record, decides what the cast does.
fn placeholder(id: u8) -> SpellDef {
    SpellDef {
        id,
        name: format!("band {id:#04X}"),
        mp_cost: 0,
        target: SpellTarget::AllEnemies,
        element: SpellElement::Neutral,
        effect: SpellEffect::Damage {
            base_power: 1,
            element: SpellElement::Neutral,
        },
        ..Default::default()
    }
}

struct Disc {
    scus: Vec<u8>,
    overlay_898: Arc<[u8]>,
    pool: Arc<CastEffectPool>,
}

fn build_world(disc: &Disc, ids: &[u8]) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.battle.set_battle_attack(i as u8, 60);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.install_menu_text(&disc.scus);
    let mut spells: SpellCatalog = legaia_engine_battle::retail_magic::retail_seru_magic_catalog();
    for &id in ids {
        if spells.get(id).is_none() {
            spells.insert(placeholder(id));
        }
    }
    w.tables.spell_catalog = spells;
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.locomotion.camera_azimuth = 0;

    use legaia_engine_battle::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("cast_cell_restore_disc");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w.battle.no_escape = true;
    w
}

/// Keep both sides standing for the whole cast: a module that kills its
/// row, or a capture spell that wipes the party, ends the fight before the
/// late arms make their calls.
///
/// Every HP write goes through `set_hp_synced` (a bare `hp` write leaves the
/// action SM's `0x51` bar-drain gate parked).
fn refill(w: &mut World) {
    let pc = w.party.party_count as usize;
    for i in 0..pc {
        w.actors[i].battle.max_hp = 9000;
        w.actors[i].battle.set_hp_synced(9000);
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.mp = 250;
    }
    w.actors[pc].battle.max_hp = 9000;
    w.actors[pc].battle.set_hp_synced(9000);
}

/// One frame as a headless driver runs it, with the battle VRAM pass.
fn frame(w: &mut World, vram: &mut psx_tim::Vram) {
    let _ = w.take_pending_summon_spawn();
    let _ = w.tick();
    let _ = w.step_world_frame_tail(None, None, |_| None);
    if w.mode == SceneMode::Battle {
        let _ = w.apply_battle_vram_moves(vram);
    }
}

fn enter_battle(w: &mut World, disc: &Disc, vram: &mut psx_tim::Vram) -> bool {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        frame(w, vram);
        if w.mode == SceneMode::Battle {
            w.set_pad(0);
            w.install_cast_effect_pool(disc.pool.clone());
            w.tables.move_power_overlay = Some(disc.overlay_898.clone());
            return true;
        }
    }
    false
}

/// A non-zero BGR555 ramp over the CLUT band: a texel the HSV kernel moves.
fn seed_band(vram: &mut psx_tim::Vram) {
    for y in BAND_ROWS {
        let row: Vec<u8> = (0..1024u16)
            .flat_map(|x| {
                let r = 4 + (x % 24);
                let g = 6 + ((x / 3 + y) % 20);
                let b = 8 + ((x / 7) % 16);
                (r | (g << 5) | (b << 10)).to_le_bytes()
            })
            .collect();
        vram.write_block(0, y, 1024, 1, &row);
    }
}

fn read_rect(vram: &psx_tim::Vram, (x, y, w, h): (u16, u16, u16, u16)) -> Vec<u16> {
    let mut out = Vec::new();
    for row in 0..h {
        for col in 0..w {
            out.push(vram.pixel(usize::from(x + col) & 0x3FF, usize::from(y + row) & 0x1FF));
        }
    }
    out
}

#[derive(Debug, Default)]
struct CastRun {
    armed: bool,
    /// Rects some frame's apply recoloured (they differed from the seed).
    recoloured: Vec<(u16, u16, u16, u16)>,
    /// Rects a live part's op `0x30` handed back, in order.
    restored: Vec<(u16, u16, u16, u16)>,
    /// Rects that read their pre-cast texels when the cast had folded.
    clean_at_end: Vec<(u16, u16, u16, u16)>,
    frames: usize,
    ended_battle: bool,
}

fn run_cast(w: &mut World, disc: &Disc, vram: &mut psx_tim::Vram, id: u8) -> CastRun {
    let mut run = CastRun::default();
    if w.mode == SceneMode::Battle && w.battle.end.is_some() {
        for _ in 0..0x1000 {
            if w.mode != SceneMode::Battle {
                break;
            }
            w.set_pad(0);
            frame(w, vram);
        }
    }
    if w.mode != SceneMode::Battle && !enter_battle(w, disc, vram) {
        return run;
    }
    for _ in 0..0x400 {
        if w.battle.command.is_some() {
            break;
        }
        refill(w);
        w.set_pad(0);
        frame(w, vram);
    }
    refill(w);
    seed_band(vram);
    let seeded = vram.clone();
    w.battle.inflight_seed = Some(InflightCastSeed {
        caster: 0,
        spell_id: id,
        target: TARGET_ENEMY_GROUP,
        ground: [None; INFLIGHT_GROUND_SLOTS],
        idle: [None; INFLIGHT_GROUND_SLOTS],
    });
    let mut tail = TAIL_FRAMES;
    for _ in 0..CAST_FRAMES {
        if w.mode != SceneMode::Battle {
            run.ended_battle = true;
            break;
        }
        if w.casting.pending_cast.is_some() {
            run.armed = true;
        } else if run.armed {
            // The band has folded; a part still seated keeps running its
            // program, and op `0x30` can come after the fold.
            let live = w
                .casting
                .active_summon
                .as_ref()
                .is_some_and(|sc| sc.parts.iter().any(|p| !p.finished));
            if !live || tail == 0 {
                break;
            }
            tail -= 1;
        }
        w.set_pad(0);
        refill(w);
        // What this frame's apply will see: the recolours the part pass
        // queues and the restores op `0x30` hands back.
        let _ = w.take_pending_summon_spawn();
        let _ = w.tick();
        let _ = w.step_world_frame_tail(None, None, |_| None);
        for f in &w.battle.cell_fx {
            if !run.recoloured.contains(&f.rect) {
                run.recoloured.push(f.rect);
            }
        }
        for (rect, _) in &w.battle.cell_restores {
            run.restored.push(*rect);
        }
        if w.mode == SceneMode::Battle {
            let _ = w.apply_battle_vram_moves(vram);
        }
        run.frames += 1;
    }
    for &rect in &run.restored {
        if read_rect(vram, rect) == read_rect(&seeded, rect) && !run.clean_at_end.contains(&rect) {
            run.clean_at_end.push(rect);
        }
    }
    w.set_pad(0);
    w.battle.inflight_seed = None;
    w.casting.active_summon = None;
    run
}

#[test]
fn a_summon_strip_reads_its_capture_after_op_0x30() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] cast cell restore: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let overlay_898: Arc<[u8]> = Arc::from(
        prot_entry(
            &dir,
            legaia_game_tables::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
        )
        .as_slice(),
    );
    let disc = Disc {
        scus,
        overlay_898,
        pool: Arc::new(build_pool(&dir)),
    };
    let probe = build_world(&disc, &[]);
    let mut ids_for: std::collections::BTreeMap<u32, u8> = Default::default();
    for id in 0u8..=0xFF {
        if let Some(e) = probe.cast_module_for(id)
            && RESTORE_ENTRIES.contains(&e)
        {
            ids_for.entry(e).or_insert(id);
        }
    }
    let mut restoring = Vec::new();
    for (&entry, &id) in &ids_for {
        // A fresh fight per cast: one cast's kills cannot end the next one's.
        let mut w = build_world(&disc, &[id]);
        let mut vram = psx_tim::Vram::new();
        assert!(
            enter_battle(&mut w, &disc, &mut vram),
            "no encounter in 6000 field ticks"
        );
        let r = run_cast(&mut w, &disc, &mut vram, id);
        eprintln!(
            "[cast] PROT {entry} id {id:#04x}: armed={} ended={} frames={} recoloured={:?} \
             restored={:?} clean_at_end={:?}",
            r.armed, r.ended_battle, r.frames, r.recoloured, r.restored, r.clean_at_end
        );
        if !r.restored.is_empty() {
            restoring.push(entry);
        }
        // Every strip op `0x30` handed back reads as it was captured.
        for rect in &r.restored {
            assert!(
                r.clean_at_end.contains(rect),
                "PROT {entry}: strip {rect:?} kept the recolour after op 0x30"
            );
        }
    }
    assert!(
        !restoring.is_empty(),
        "no cast ran op 0x30 on a recoloured strip - the observation is vacuous"
    );
}
