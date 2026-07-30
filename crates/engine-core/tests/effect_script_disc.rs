//! Disc-gated oracle for the battle **effect-script** chain: the entry
//! `+0x14..+0x53` region every decoded action animation carries
//! (`legaia_asset::monster_archive::MonsterAnimation::effect_script`) must
//! read as the record stream `FUN_801DEA50` walks - `[frame_gate, effect,
//! i16 x, y, z]` at 8-byte stride from `+0x14`, ended by a zero frame gate -
//! and driving the engine stepper over a real player walk clip must produce
//! positioned spawns.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing
//! (CI without disc data).
//!
//! What this catches:
//! - the head capture drifting off the entry base (records would read as
//!   noise: streams no longer front-packed, spawn counts collapse);
//! - the region being silently dropped from either parser family
//!   (monster archive / player record[0]);
//! - the stepper regressing against real record data.

use legaia_engine_core::action_effect_script::{
    EffectRecord, MAX_CURSOR, retail_rotation_lut, step_effect_script,
};
use legaia_engine_core::action_effect_script::{EffectScriptActor, FacingBias, rotate_offset};
use std::path::PathBuf;

fn prot_file(name: &str) -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join(name);
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

/// Walk one entry head like the stepper: streams must be front-packed
/// (no record after the zero-gate tail). Returns
/// `(any_record, spawn_records, terminators)`.
fn walk_head(head: &[u8], what: &str) -> (bool, usize, usize) {
    let mut any = false;
    let mut ended = false;
    let (mut spawns, mut terms) = (0usize, 0usize);
    for cursor in 0..MAX_CURSOR {
        let rec = EffectRecord::at(head, cursor).expect("head covers all 8 record slots");
        if rec.frame == 0 {
            ended = true;
            continue;
        }
        assert!(
            !ended,
            "{what}: record after the zero-gate tail at {cursor}"
        );
        any = true;
        if rec.is_terminator() {
            terms += 1;
        } else {
            spawns += 1;
        }
    }
    (any, spawns, terms)
}

#[test]
fn monster_archive_effect_scripts_read_as_record_streams() {
    use legaia_asset::monster_archive;
    let Some(entry) = prot_file("0867_battle_data.BIN") else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    let mut entries = 0usize;
    let mut scripted = 0usize;
    let mut spawn_records = 0usize;
    let mut terminated = 0usize;
    for id in 1..120u16 {
        let Ok(Some(anims)) = monster_archive::animations(&entry, id) else {
            continue;
        };
        for anim in &anims {
            entries += 1;
            assert_eq!(
                anim.effect_script.len(),
                monster_archive::EFFECT_SCRIPT_HEAD_BYTES,
                "monster {id} entry tag {:#04x}: full head expected",
                anim.action_id
            );
            let what = format!("monster {id} entry tag {:#04x}", anim.action_id);
            let (any, s, t) = walk_head(&anim.effect_script, &what);
            scripted += usize::from(any);
            spawn_records += s;
            terminated += t;
        }
    }
    assert!(entries > 500, "expected many entries, got {entries}");
    // The region is real data, not padding: most action entries carry at
    // least one record, and the corpus holds both spawn forms + terminators
    // (the terminator is the move-power install marker, present only on
    // entries that stage one).
    assert!(
        scripted * 2 > entries,
        "expected most entries scripted, got {scripted}/{entries}"
    );
    assert!(spawn_records > 500, "spawn records: {spawn_records}");
    assert!(terminated > 10, "terminator records: {terminated}");
    eprintln!(
        "[effect_script] {entries} entries, {scripted} scripted, \
         {spawn_records} spawn records, {terminated} terminators"
    );
}

#[test]
fn player_walk_clip_effect_script_steps_to_positioned_spawns() {
    use legaia_asset::battle_char_assembly;
    // Every player file's walk entry (slot 1) carries footstep effect
    // records: step the real stream through the engine stepper with the
    // retail rotation LUT and assert positioned spawns come out.
    for (file, who) in [
        ("0863_edstati3.BIN", "Vahn"),
        ("0864_edstati3.BIN", "Noa"),
        ("0865_battle_data.BIN", "Gala"),
        ("0866_battle_data.BIN", "Terra"),
    ] {
        let Some(bytes) = prot_file(file) else {
            eprintln!("[skip] extracted/PROT/{file} or LEGAIA_DISC_BIN missing");
            return;
        };
        let anims = battle_char_assembly::battle_animations(&bytes).expect("record[0] decodes");
        let walk = anims
            .iter()
            .find(|a| a.action_id == 1)
            .unwrap_or_else(|| panic!("{who}: walk entry (slot 1) present"));
        let (any, spawns, _) = walk_head(&walk.effect_script, &format!("{who} walk"));
        assert!(any && spawns >= 1, "{who}: walk carries footstep records");

        // Drive the stepper across the whole clip's frame range.
        let mut cursor = 0u8;
        let mut emitted = Vec::new();
        for frame in 0..walk.frame_count.clamp(64, 250) as u8 {
            let actor = EffectScriptActor {
                cursor,
                facing: 0,
                world: (0, 0, -800),
                scale: 1 << 12,
                scope: 9,
                action: 0,
                suppressed: false,
            };
            let step = step_effect_script(
                retail_rotation_lut(),
                &walk.effect_script,
                actor,
                frame,
                &[],
            );
            cursor = step.cursor;
            emitted.extend(step.spawns);
        }
        assert_eq!(
            emitted.len(),
            spawns,
            "{who}: every walk record spawns exactly once per cycle"
        );
        for s in &emitted {
            // A footstep lands near the actor, not at garbage coordinates:
            // the local offsets are small (|off| < 0x800 across the retail
            // corpus) and rotation preserves magnitude.
            let (dx, dz) = (s.at.0, s.at.2 + 800);
            assert!(
                dx.abs() < 0x800 && dz.abs() < 0x800,
                "{who}: spawn at {:?} implausibly far",
                s.at
            );
        }
    }
}

#[test]
fn world_tick_emits_spawns_from_a_real_committed_walk_clip() {
    use legaia_asset::battle_char_assembly;
    use legaia_engine_core::world::World;
    // End-to-end producer chain on real data: install Vahn's disc action
    // clips on a battle actor, stage the walk (anim id 1), and drive the
    // world's battle animation tick - the effect-script walk must surface
    // positioned BattleEffectSpawn requests through the drain.
    let Some(bytes) = prot_file("0863_edstati3.BIN") else {
        eprintln!("[skip] extracted/PROT/0863_edstati3.BIN or LEGAIA_DISC_BIN missing");
        return;
    };
    let anims = battle_char_assembly::battle_animations(&bytes).expect("record[0] decodes");
    let max_slot = anims.iter().map(|a| a.action_id as usize).max().unwrap();
    let mut clips = vec![None; max_slot + 1];
    for a in anims {
        let slot = a.action_id as usize;
        clips[slot] = Some(a);
    }

    let mut world = World::new();
    world.enter_battle(1, 1);
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 1; // walk / approach
    world.commit_staged_battle_anim(0);
    assert!(
        world.actors[0].battle_effect_script.is_some(),
        "walk clip carries an effect script"
    );

    let mut spawns = Vec::new();
    for _ in 0..600 {
        world.tick_battle_animations();
        spawns.extend(world.drain_battle_effect_spawns());
    }
    assert!(
        spawns.len() >= 2,
        "walk footfalls should fire (looping clip re-arms per cycle), got {}",
        spawns.len()
    );
    for s in &spawns {
        assert_eq!(s.actor_slot, 0);
        // Positioned near the actor's seat (0, 0, -800).
        assert!(
            s.at.0.abs() < 0x800 && (s.at.2 + 800).abs() < 0x800,
            "spawn at {:?} implausibly far",
            s.at
        );
    }
    eprintln!("[world_walk] {} spawns over 600 frames", spawns.len());
}

#[test]
fn retail_lut_rotation_matches_the_synthetic_expectation_on_real_angles() {
    // Sanity for the LUT identity independent of disc data content: a full
    // revolution of quarter turns returns a unit offset near its origin.
    // NOT exactly - the stepper's complement read is `0xFFF - a` (one angle
    // unit short of a true negation) and each product truncates, so retail
    // itself drifts a couple of units per revolution; the tolerance below is
    // that faithful drift, not slack.
    let lut = retail_rotation_lut();
    let mut p = (1000i32, 0i32);
    for _ in 0..4 {
        p = rotate_offset(lut, 1024, FacingBias::None, p.0, p.1);
    }
    assert!(
        (p.0 - 1000).abs() <= 4 && p.1.abs() <= 4,
        "four quarter turns should land near the origin: {p:?}"
    );
}
