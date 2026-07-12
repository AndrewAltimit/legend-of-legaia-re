//! Disc-gated: field-NPC glide speed is decoded from each placement's real
//! walk-kernel operands off the disc, replacing both the flat stand-in and
//! the facing-nibble heuristic.
//!
//! Retail encodes the base step in the walk ops' own operands, on the shared
//! ladder `numerator >> (2 + bits)` per frame:
//!
//! - MAN tail-section-1 motion streams (`FUN_80038158` - the ambient
//!   town-NPC wander): directional steps `0x03`/`0x19`/`0x20` carry `bits`
//!   in operand byte 1's low nibble; the pad-echo step `0x06` and the AABB
//!   wander `0x18` scatter it over the four operand bytes' high bits. All
//!   step `0x80 >> (2 + bits)`.
//! - Field-VM yield ops interpreted in place by `FUN_8003774C` (scripted
//!   glide legs): `0x37`/`0x41` (`bits = (op0>>5 & 4)|(op1>>6)`; numerator
//!   `0x80` for 0x37, `0x40` for 0x41) and `0x47` (`bits = b2 & 7`).
//!
//! The facing-nibble heuristic (the `4C 51` byte-+3 low nibble - NOT a
//! retail speed field) survives only as the last-resort arm for placements
//! with no walk-kernel op in either carrier. The engine feeds the decoded
//! value through `World::field_npc_glide_speeds`, and
//! `World::start_field_npc_motion` writes it into the leg's motion-VM speed
//! instead of the flat stand-in (8).
//!
//! Assertions pin a handful of town01 placements to their decoded steps
//! (structural: op class + selector + ladder value - no Sony bytes).
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md
//! convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::man_field_scripts::{
    placement_glide_speed, placement_wander_step, placement_yield_step,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};

/// The stand-in `FIELD_NPC_MOTION_SPEED` (pub(crate) in the engine): base step
/// 8. The whole point of the change is that a placement's decoded speed need
/// NOT be this.
const STAND_IN_SPEED: u16 = 8;

/// The retail base-step ladder `numerator >> (2 + bits)`, floored at 1: the
/// only speeds a decoded glide can take (`numerator` 0x80, or 0x40 for the
/// half-speed op 0x41).
const GLIDE_LADDER: [u16; 6] = [32, 16, 8, 4, 2, 1];

/// town01 placements whose ambient wander pace is a tail-section-1 AABB
/// wander op `0x18`: `(placement_slot, bits, speed)`. The binding id space
/// is `N0 + slot` (`FUN_8003A1E4` writes actor `+0x50 = N0 + index`; town01
/// `N0 = 36`), so e.g. binding `0x30` = slot 12.
const TOWN01_WANDER_PINS: [(u8, u8, u16); 6] = [
    (12, 3, 4),
    (13, 2, 8),
    (14, 2, 8),
    (17, 4, 2),
    (37, 4, 2),
    (43, 3, 4),
];

/// town01 placements with no bound motion stream whose scripted glide is an
/// own-context field-VM `0x41` nudge (`bits = 0`, numerator `0x40` -> 16).
const TOWN01_YIELD_PINS: [(u8, u8, u16); 2] = [(34, 0, 16), (36, 0, 16)];

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
fn town01_npc_glide_speeds_decode_from_real_walk_kernel_ops() {
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

    // Non-vacuous: several Rim Elm villagers carry decodable motion legs.
    assert!(
        !world.field_npc_glide_speeds.is_empty(),
        "at least one town01 placement derives a glide speed from its motion op"
    );

    let placements = man_file.actor_placements(&man_bytes);
    let placement = |slot: u8| {
        placements
            .iter()
            .find(|p| u8::try_from(p.index) == Ok(slot))
            .expect("pinned slot is a real placement")
    };

    // Pinned wander placements: the bound tail-section-1 stream's AABB
    // wander op 0x18 carries the base-step selector; the decoded speed is on
    // the ladder and lands in the world map.
    for (slot, bits, speed) in TOWN01_WANDER_PINS {
        let step = placement_wander_step(&man_file, &man_bytes, placement(slot))
            .unwrap_or_else(|| panic!("slot {slot}: bound wander stream decodes"));
        assert_eq!(step.op, 0x18, "slot {slot}: the walk op is the AABB wander");
        assert_eq!(step.bits, bits, "slot {slot}: base-step selector");
        assert_eq!(step.speed, speed, "slot {slot}: decoded per-frame step");
        assert_eq!(
            world.field_npc_glide_speeds.get(&slot),
            Some(&speed),
            "slot {slot}: the engine installs the wander-decoded speed"
        );
    }

    // Pinned yield placements: no bound motion stream; the scripted
    // own-context field-VM 0x41 glide carries the selector (numerator 0x40).
    for (slot, bits, speed) in TOWN01_YIELD_PINS {
        let p = placement(slot);
        assert!(
            placement_wander_step(&man_file, &man_bytes, p).is_none(),
            "slot {slot}: no tail-section-1 walk op (yield arm exercised)"
        );
        let step = placement_yield_step(&man_file, &man_bytes, p)
            .unwrap_or_else(|| panic!("slot {slot}: field-VM yield op decodes"));
        assert_eq!(step.op, 0x41, "slot {slot}: the walk op is the 0x41 glide");
        assert_eq!(step.bits, bits, "slot {slot}: base-step selector");
        assert_eq!(step.speed, speed, "slot {slot}: decoded per-frame step");
        assert_eq!(
            world.field_npc_glide_speeds.get(&slot),
            Some(&speed),
            "slot {slot}: the engine installs the yield-decoded speed"
        );
    }

    // Every installed speed is on the retail ladder, and re-decoding through
    // the public chain reproduces the stored value (proves the reader, not
    // just the cache). Count divergence from the flat stand-in to show the
    // decode is data-driven.
    let mut differ_from_stand_in = 0usize;
    for (&slot, &speed) in &world.field_npc_glide_speeds {
        assert!(
            GLIDE_LADDER.contains(&speed),
            "slot {slot}: decoded glide speed {speed} is on the retail base-step ladder"
        );
        let redecoded = placement_glide_speed(&man_file, &man_bytes, placement(slot));
        assert_eq!(
            redecoded,
            Some(speed),
            "slot {slot}: stored glide speed matches a fresh decode"
        );
        if speed != STAND_IN_SPEED {
            differ_from_stand_in += 1;
        }
    }
    assert!(
        differ_from_stand_in > 0,
        "the decode is data-driven, not a re-spelled constant"
    );

    eprintln!(
        "[town01] {} placements carry a decoded glide speed; {} differ from the stand-in ({}): {:?}",
        world.field_npc_glide_speeds.len(),
        differ_from_stand_in,
        STAND_IN_SPEED,
        world.field_npc_glide_speeds,
    );

    // The engine USES the decoded speed: starting a leg for a pinned wander
    // slot writes that value into the motion-VM state, not the flat stand-in.
    let (slot, _, derived) = TOWN01_WANDER_PINS[0];
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
        "start_field_npc_motion writes the decoded glide speed, not the stand-in"
    );
}

#[test]
fn town01_motion_op17_default_move_pairs_harvest() {
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

    // Every wandering villager's stream opens with an op-0x17 default-move
    // write; the harvest keys it by placement slot (binding id - N0). The
    // pair is fed to the motion-pause kick (`FUN_8003C9AC` port) as its
    // per-actor table entry.
    assert!(
        !world.field_npc_default_moves.is_empty(),
        "town01 motion streams carry op-0x17 default-move writes"
    );
    // Structural cross-check: each pinned wander slot also carries a pair
    // (the authored streams open `0x17` before the wander op), and no pair
    // byte is the 0x8C "unset" sentinel.
    for (slot, _, _) in [(12u8, 3u8, 4u16), (17, 4, 2), (43, 3, 4)] {
        let pair = world
            .field_npc_default_moves
            .get(&slot)
            .unwrap_or_else(|| panic!("slot {slot}: op-0x17 pair harvested"));
        assert!(
            pair.iter().all(|&b| b != 0x8C),
            "slot {slot}: pair {pair:?} is a real move/anim id write"
        );
    }
    eprintln!(
        "[town01] default-move pairs: {:?}",
        world.field_npc_default_moves
    );
}
