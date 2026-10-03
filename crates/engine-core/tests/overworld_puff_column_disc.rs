//! Disc-gated: `map01`'s puff column - the overworld's one ambient tree of
//! draw-kind-4 sprite-arm nodes - holds the population and the motion the
//! retail capture shows.
//!
//! `keikoku_chest_preload` (retail, `map01`) holds seven live kind-4 nodes on
//! list `_DAT_8007C350`, all at `x = 9152`: four at the spawn point
//! `z = 10432` and three strung out toward `z = 9216` (`9216`, `9624`,
//! `10068`). They are the two children of one spawner record: it re-seats
//! itself at `(9152, -320 + rand % 160, 10432)` and alternately spawns a
//! puff that stays and a puff whose op `0x00` sets a `-4 << 3` Z velocity.
//!
//! Two port defects hid this. The scene-entry install and first run were
//! right, but a probe that never drove the ambient tick saw only the first
//! run's two nodes; and once ticked, the travelling puffs still stood still,
//! because the ambient tick ran the move VM without the part tick's motion
//! block (`legaia_engine_core::part_motion`), which is what integrates a
//! velocity the VM only sets.
//!
//! Skip-pass when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::scene_entry_ambient_installs;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{FrameClock, World};

const COLUMN_X: i16 = 9152;
const SPAWN_Z: i16 = 10432;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

#[test]
fn map01_puff_column_matches_the_retail_population_and_drift() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    let scene = Scene::load(&index, "map01").expect("map01");
    let stager = scene
        .find_event_scripts()
        .expect("map01 carries a prescript bundle")
        .bytes
        .to_vec();
    let man = scene
        .field_man_payload(&index)
        .ok()
        .flatten()
        .expect("map01 MAN");
    let man_file = legaia_asset::man_section::parse(&man).expect("parse MAN");
    let mut world = World {
        clock: FrameClock {
            frame_step: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    world.install_field_stagers(&stager);
    for arg in scene_entry_ambient_installs(&man_file, &man) {
        world.spawn_ambient_record(arg as usize + 1, [0, 0, 0]);
    }

    let (mut min_n, mut max_n) = (usize::MAX, 0usize);
    let mut still_seen = false;
    let mut drift_seen = false;
    for t in 0..1200 {
        world.tick_ambient_fx();
        if t < 300 {
            continue;
        }
        let nodes: Vec<_> = world
            .ambient
            .fx
            .iter()
            .filter(|p| !p.finished && p.state.move_substate == 4 && p.state.field_9e & 0x4000 != 0)
            .map(|p| (p.state.world_x, p.state.world_y, p.state.world_z))
            .collect();
        min_n = min_n.min(nodes.len());
        max_n = max_n.max(nodes.len());
        for &(x, y, z) in &nodes {
            assert_eq!(x, COLUMN_X, "every puff sits on the column");
            assert!(
                (-320..-160).contains(&y),
                "y {y} outside the spawner's roll"
            );
            assert!(z <= SPAWN_Z && z > 8700, "z {z} off the column's run");
            still_seen |= z == SPAWN_Z;
            drift_seen |= z < SPAWN_Z - 400;
        }
        // Everything the tree draws is on the draw-kind-4 list.
        let draws = world.ambient_sprite_arm_draws();
        assert_eq!(draws.len(), nodes.len());
        // At retail's size: the record's `1024 x 256` sheet (`+0xB4 / +0xB6`)
        // at the seater's render scale `+0x72 = 0x1000`. A part seated with a
        // zero scale collapses every corner onto the node and draws nothing.
        for d in &draws {
            let extent = |axis: usize| {
                d.mesh
                    .positions
                    .iter()
                    .map(|p| p[axis].abs())
                    .fold(0.0f32, f32::max)
            };
            assert_eq!((extent(0), extent(1)), (512.0, 128.0), "puff sheet size");
        }
    }
    eprintln!(
        "[ok] map01 puff column: {min_n}..={max_n} live sprite-arm puffs (retail capture: 7)"
    );
    assert!(
        (6..=8).contains(&min_n) && (6..=8).contains(&max_n),
        "population {min_n}..={max_n}, retail holds 7"
    );
    assert!(still_seen, "no puff stays at the spawn point");
    assert!(drift_seen, "no puff drifts down the column");
}
