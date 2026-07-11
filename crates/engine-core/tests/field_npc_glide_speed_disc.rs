//! Disc-gated: field-NPC glide speed is derived from the `0x4C 0x51` leg's
//! byte-+3 operand off the disc, replacing the flat stand-in.
//!
//! Retail glides an NPC at `_DAT_1f800393 × 0x80 / (4 << bits)` per frame
//! (`FUN_8003774C` ops 0x37/0x41/0x47), where `bits` is a base-step selector
//! encoded in the motion op's own operands - NOT the player's `+0x72` speed
//! path (falsified). Reconcile outcome: `4C 51` itself carries NO speed field
//! (its byte +3 is `[bit7 special-model | facing nibble]`), so this derivation
//! reads the facing nibble as a stable per-NPC variation - see the modelling
//! note on `man_field_scripts::placement_glide_speed`. The engine feeds the
//! derived value through `World::field_npc_glide_speeds`, and
//! `World::start_field_npc_motion` writes it into the leg's motion-VM speed
//! instead of the flat stand-in (8).
//!
//! Assertions are structural (the derived speeds land on the retail base-step
//! ladder, and the engine's motion state carries the derived value, not the
//! constant) - no Sony bytes. Skip-passes without `LEGAIA_DISC_BIN` /
//! `extracted/` (CLAUDE.md convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};

/// The stand-in `FIELD_NPC_MOTION_SPEED` (pub(crate) in the engine): base step
/// 8 = `field_npc_glide_speed(2)`. The whole point of the change is that a
/// placement's derived speed need NOT be this.
const STAND_IN_SPEED: u16 = 8;

/// The retail base-step ladder `0x80 >> (2 + bits)` for `bits = 0..=7`,
/// floored at 1: the only speeds a derived glide can take.
const GLIDE_LADDER: [u16; 6] = [32, 16, 8, 4, 2, 1];

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn town01_npc_glide_speeds_derive_from_real_motion_ops() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let scene = Scene::load(&index, "town01").expect("load town01");
    let man_bytes = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("town01 has a MAN payload");
    let man_file = parse_man(&man_bytes).expect("parse MAN");

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_carriers_from_man(&man_file, &man_bytes);

    // Non-vacuous: several Rim Elm villagers carry decodable local motion legs,
    // so their per-placement glide speed is derived off the disc.
    assert!(
        !world.field_npc_glide_speeds.is_empty(),
        "at least one town01 placement derives a glide speed from its motion op"
    );

    // Every derived speed lands on the retail base-step ladder (i.e. it is a
    // real `0x80 >> (2 + bits)` value, not a synthetic constant). Count how
    // many differ from the flat stand-in to show the derivation is data-driven.
    let mut differ_from_stand_in = 0usize;
    for (&slot, &speed) in &world.field_npc_glide_speeds {
        assert!(
            GLIDE_LADDER.contains(&speed),
            "slot {slot}: derived glide speed {speed} is on the retail base-step ladder"
        );
        // Cross-check the stored value against a direct re-derivation from the
        // same placement's motion op (proves the reader, not just the cache).
        let placement = man_file
            .actor_placements(&man_bytes)
            .into_iter()
            .find(|p| u8::try_from(p.index) == Ok(slot))
            .expect("routed slot is a real placement");
        let redecoded = legaia_engine_core::man_field_scripts::placement_glide_speed(
            &man_file, &man_bytes, &placement,
        );
        assert_eq!(
            redecoded,
            Some(speed),
            "slot {slot}: stored glide speed matches a fresh decode of its motion op"
        );
        if speed != STAND_IN_SPEED {
            differ_from_stand_in += 1;
        }
    }

    eprintln!(
        "[town01] {} placements carry a derived glide speed; {} differ from the stand-in ({}): {:?}",
        world.field_npc_glide_speeds.len(),
        differ_from_stand_in,
        STAND_IN_SPEED,
        world.field_npc_glide_speeds,
    );

    // The engine USES the derived speed: starting a leg for a slot with a
    // derived glide speed writes that value into the motion-VM state, not the
    // flat stand-in.
    let (&slot, &derived) = world
        .field_npc_glide_speeds
        .iter()
        .next()
        .expect("non-empty checked above");
    assert!(
        world.start_field_npc_motion(slot, 0, 0),
        "the slot is an installed field NPC"
    );
    let leg = world
        .field_npc_motions
        .get(&slot)
        .expect("the leg was installed");
    assert_eq!(
        leg.state.speed, derived,
        "start_field_npc_motion writes the derived glide speed, not the stand-in"
    );
}
