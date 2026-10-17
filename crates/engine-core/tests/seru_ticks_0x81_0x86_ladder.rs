//! Disc-gated **player-Seru tick ladder**: seat spell ids `0x81..=0x86`
//! (PROT 0903..0908) and walk each module's phase chain from `0` to its
//! terminal through `World::run_cast_module_code`.
//!
//! Why it exists, separately from `cast_band_ladder`. That ladder steps
//! two *arms* per band entry and asks whether a ported body was reached at
//! all. These six bodies are `beq` **chains** fifteen arms deep whose
//! simulation writes live in the late arms - PROT 0903's hit is arm 11, PROT
//! 0906's sweep is arm 13, PROT 0907's kill / confuse fork is arm 13 - so a
//! two-arm probe enters the prologue and nothing else. This one walks every
//! phase until the tick reports done, and asserts each body actually wrote
//! battle state on the way through.
//!
//! What it does **not** assert: damage. The seam feeds no roll (the engine
//! folds a cast's HP outcome once, at `cast_spell_on_slots_prepaid`), so the
//! observable change is the choreography - staged clips, render flags,
//! retargets and animation rates - which is exactly the half these bodies
//! own.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_overlay_images::cast_effect_pool::{CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST};
use std::path::PathBuf;

/// The six ids this ladder walks, and the PROT entry each resolves to.
const IDS: [(u8, u32); 6] = [
    (0x81, 903),
    (0x82, 904),
    (0x83, 905),
    (0x84, 906),
    (0x85, 907),
    (0x86, 908),
];

/// Hard stop on the walk. A body the engine runs ungated steps one arm a
/// frame; one whose arms are paced on its own countdown
/// (`legaia_engine_cast::cast_module_camera`) holds each counted arm
/// for as long as retail does - about 500 frames for Gimard (PROT 0903) and
/// over 1000 for Gizam (PROT 0906) - so anything past this is a body that
/// never reported done.
const MAX_FRAMES: usize = 4096;

/// The hosts seat the summon creature at `8 + party_count`.
const LONE_SUMMON_SLOT: u8 = 9;

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

/// A minimal live battle laid out the way the engine seats a lone member:
/// the party seat, four monster seats compacted right after it (`1..=4`) and
/// the summon seat the hosts use (`8 + party_count`). All six bodies read the
/// caster, the victim and retail's `actor_table[7]`, and three of them sweep
/// retail's seats `3..=6` - which the cast seam's retail-shaped row maps onto
/// engine slots `1..=4` - so the row has to be wide enough or the sweep arm
/// writes nothing and the ladder goes vacuous.
fn battle_world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    while world.actors.len() < 12 {
        world.actors.push(Actor::default());
    }
    world.mode = SceneMode::Battle;
    for slot in [0usize, 1, 2, 3, 4, LONE_SUMMON_SLOT as usize] {
        world.actors[slot].active = true;
        world.actors[slot].battle.max_hp = 400;
        world.actors[slot].battle.hp = 400;
        world.actors[slot].battle.mp = 99;
        world.actors[slot].battle.liveness = 1;
    }
    for slot in 1..=4 {
        world.actors[slot].battle_monster_id = Some(1);
    }
    world.actors[0].battle.active_target = 1;
    world.battle_ctx.active_actor = 0;
    world.casting.summon_actor_slot = Some(LONE_SUMMON_SLOT);
    world
}

/// Every resident cast-band module image, read off `PROT.DAT`.
fn cast_pool(
    dir: &std::path::Path,
) -> std::sync::Arc<legaia_overlay_images::cast_effect_pool::CastEffectPool> {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = legaia_overlay_images::cast_effect_pool::CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let Some(entry) = archive.entries.get(idx as usize).cloned() else {
            continue;
        };
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        pool.insert(idx, &bytes);
    }
    std::sync::Arc::new(pool)
}

/// The fields these bodies write that survive `write_cast_actor_state`, for
/// the seats they write them on.
fn choreography_probe(w: &World) -> Vec<(u8, u8, u8, u8)> {
    (0..=LONE_SUMMON_SLOT)
        .map(|s| {
            let a = &w.actors[s as usize];
            (
                a.battle.queued_anim,
                a.battle.render_flag,
                a.battle.active_target,
                a.battle.anim_rate.get(),
            )
        })
        .collect()
}

#[test]
fn every_player_seru_tick_walks_its_phase_chain_and_writes_state() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let pool = cast_pool(&dir);

    let mut walked = 0usize;
    let mut total_frames = 0usize;
    let mut bodies_that_wrote = 0usize;

    for (id, expect_entry) in IDS {
        let mut w = battle_world();
        w.install_menu_text(&scus);
        w.casting.effect_pool = Some(pool.clone());

        assert_eq!(
            w.cast_module_for(id),
            Some(expect_entry),
            "id {id:#04x} did not resolve to its PROT entry"
        );

        let before = choreography_probe(&w);
        let mut frames = 0usize;
        let mut phases: Vec<u8> = Vec::new();
        let mut entered = false;
        let mut finished = false;

        while frames < MAX_FRAMES {
            let Some(run) = w.run_cast_module_code(id, 0) else {
                panic!("id {id:#04x}: no band entry resident - the pool did not install");
            };
            assert_eq!(
                run.prot_entry, expect_entry,
                "id {id:#04x} ran a different entry than the resolver named"
            );
            // The battle animation pass: it commits the clips the arms stage
            // (PROT 0904's arm 7 waits on its creature's) and fades the dead
            // (the settle polls wait on that).
            w.tick_battle_animations();
            entered |= run.tick_ported;
            phases.push(run.phase);
            frames += 1;
            if !run.busy {
                finished = true;
                break;
            }
        }

        assert!(entered, "id {id:#04x}: no ported tick body was entered");
        assert!(
            finished,
            "id {id:#04x}: the chain never reported done in {MAX_FRAMES} frames \
             (phases {phases:?})"
        );
        // Non-vacuity 1: the phase walked past the prologue, so the late arms
        // that hold the simulation writes were actually reached.
        let deepest = phases
            .iter()
            .copied()
            .filter(|&p| p != 0xFF)
            .max()
            .unwrap_or(0);
        assert!(
            deepest >= 8,
            "id {id:#04x}: the walk only reached phase {deepest} (phases {phases:?})"
        );
        // Non-vacuity 2: something the body writes is visible on the world.
        let after = choreography_probe(&w);
        if after != before {
            bodies_that_wrote += 1;
        }

        walked += 1;
        total_frames += frames;
    }

    assert_eq!(walked, IDS.len(), "not every id was walked");
    assert_eq!(
        bodies_that_wrote,
        IDS.len(),
        "{bodies_that_wrote} of {} bodies left the battle state untouched",
        IDS.len()
    );
    eprintln!(
        "[ok] player-Seru tick ladder: {walked} of {} ids walked to their terminal \
         in {total_frames} frames, all {bodies_that_wrote} wrote battle state",
        IDS.len()
    );
}

/// Gizam (`0x84`, PROT 0906) cast into a row that already holds a dead foe.
///
/// Arm 7 relights the row (`0x801F6F64..0x801F6F94`): the render-flag clear
/// `sb zero,0x21c(v1)` at `0x801F6F7C` sits **past** the `beq` on the seat's
/// HP (its delay slot is a `nop`), so only a living seat is reset. A dead
/// seat keeps its defeat-fade state `2` and its spent colour word
/// `+0x04 = 0`, which is what arm 14's row poll (`0x801F7644..0x801F76DC`)
/// waits on for a dead seat. Clearing the dead seat's state too put it back on the tint
/// pass's decay arm, which walks the colour word up to neutral and never
/// back to zero, so the poll held forever: the two-foe softlock where the
/// first foe died earlier in the round and Gizam fell on the survivor.
#[test]
fn gizam_settles_over_a_row_with_a_dead_foe() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let mut w = battle_world();
    w.install_menu_text(&scus);
    w.casting.effect_pool = Some(cast_pool(&dir));
    // Engine slot 1 (retail seat 3): a foe killed earlier in the round,
    // its defeat fade already run out. The survivors stay at full HP.
    let dead = 1usize;
    w.actors[dead].battle.hp = 0;
    w.actors[dead].battle.render_flag = legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE;
    w.actors[dead].battle.render_color = 0;
    w.actors[0].battle.active_target = 2;

    let mut phases = Vec::new();
    let mut finished = false;
    for _ in 0..MAX_FRAMES {
        let run = w.run_cast_module_code(0x84, 0).expect("PROT 0906 resident");
        assert_eq!(run.prot_entry, 906);
        w.tick_battle_animations();
        if phases.last() != Some(&run.phase) {
            phases.push(run.phase);
        }
        if !run.busy {
            finished = true;
            break;
        }
    }
    assert!(
        phases.contains(&14),
        "the walk never reached arm 14 (phases {phases:?})"
    );
    assert!(
        finished,
        "Gizam never left its row poll with a dead foe in the row (phases {phases:?}; \
         dead seat state {:#x} colour {:#x})",
        w.actors[dead].battle.render_flag, w.actors[dead].battle.render_color,
    );
    assert_eq!(
        w.actors[dead].battle.render_flag,
        legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE,
        "arm 7 reset the dead seat's defeat-fade state"
    );
    assert_eq!(w.actors[dead].battle.render_color, 0);
    eprintln!("[ok] Gizam over a dead foe: phases {phases:?}");
}

/// Theeder (`0x82`, PROT 0904) with its real creature clips: arm 7 holds on
/// the creature's loop counter `+0x21B` (`lbu v1,0x21b(s4)` / `beq` on `0xFF`
/// at `0x801F71C4`) for as long as clip 1 - `summon.dat`'s Theeder record,
/// entry window `(0xFF, 17, 17)` at rate `1` - takes to reach its park
/// frame. At the creature's post-commit animation rate `4` that is 17
/// keyframes at an eighth of a keyframe a tick: 136 ticks, against the ~141
/// vsyncs the cast-sequence oracle measures on retail. Before the counter
/// was kept the arm passed on the commit, one tick.
#[test]
fn theeder_arm_seven_holds_on_the_creature_loop_counter() {
    use legaia_engine_cast::seru_magic_ticks::THEEDER_ATTACK_ARM;
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("read SCUS_942.54");
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(usize::from(
            legaia_actor_formats::summon_readef::SUMMON_PROT_INDEX,
        ))
        .cloned()
        .expect("summon.dat entry");
    let mut summon_dat = Vec::new();
    archive
        .read_entry(&entry, &mut summon_dat)
        .expect("read summon.dat");
    let cast = legaia_actor_formats::summon_readef::parse_cast(&summon_dat, 0x82)
        .expect("Theeder's summon record");
    assert_eq!(
        cast.clips.get(1).and_then(|c| c.entry_loop_window()),
        Some((0xFF, 17, 17)),
        "Theeder's attack clip parks on frame 17 with the count 0xFF"
    );

    let mut w = battle_world();
    w.install_menu_text(&scus);
    w.casting.effect_pool = Some(cast_pool(&dir));
    let seat = usize::from(LONE_SUMMON_SLOT);
    let idle = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(&cast.clips[0])
        .expect("idle clip");
    w.set_actor_battle_animation(seat, idle);
    w.set_actor_battle_action_clips(
        seat,
        std::sync::Arc::new(cast.clips.iter().cloned().map(Some).collect()),
    );
    // The seat installer leaves the clip unseeded and the creature at rate 2.
    w.actors[seat].battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(2);

    let mut held = 0usize;
    let mut counters = Vec::new();
    for _ in 0..MAX_FRAMES {
        let run = w.run_cast_module_code(0x82, 0).expect("PROT 0904 runs");
        w.tick_battle_animations();
        if run.phase == THEEDER_ATTACK_ARM {
            held += 1;
            counters.push(w.actors[seat].battle.hit_count_bound);
        }
        if !run.busy {
            break;
        }
    }
    assert!(
        counters.first() == Some(&0xFF),
        "the attack clip's commit seeds the counter ({:?})",
        &counters[..counters.len().min(4)]
    );
    assert!(
        (130..=150).contains(&held),
        "arm 7 held {held} ticks; retail holds ~141"
    );
    eprintln!("[ok] Theeder arm 7 held {held} ticks on the creature's loop counter");
}
