//! Disc-gated: the ambient part pool across every scene that installs a tree
//! at entry.
//!
//! The pool cap is retail's own - `FUN_800203EC` seeds the actor free stack
//! with `0x8E` down to `0`, so 143 slots of `0xD8` bytes, handed out by
//! `FUN_80020454` and returned by `FUN_800204A4`. A cap is only meaningful
//! next to the free path, and this is the measurement that says so: with
//! halted parts freed (`FUN_8002519C`'s halt arm) every scene's population is
//! flat, and without it the corpus climbs past any cap you pick, because
//! several trees are emitters that spawn on an infinite loop.
//!
//! That is also why a truncating cap reads as an authored part count if you
//! only look once: a scene sampled at the ceiling reports exactly the ceiling,
//! composed of whatever it last spawned.
//!
//! Skip-pass when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::scene_entry_ambient_installs;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;
use legaia_engine_core::world::ambient::MAX_AMBIENT_PARTS;

/// Game ticks per scene. Every retail tree settles inside the first ~200; the
/// rest is headroom for a slow-cadence emitter to show growth if it has any.
const TICKS: usize = 1200;

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

/// One scene's run: the population peak over each half of the run, and
/// whether the free path ever fired.
struct Run {
    peak: usize,
    first_half_peak: usize,
    second_half_peak: usize,
    retired: bool,
    exhausted: bool,
}

fn run_scene(index: &ProtIndex, name: &str) -> Option<Run> {
    let scene = Scene::load(index, name).ok()?;
    let scripts = scene.find_event_scripts()?;
    let man_bytes = scene.field_man_payload(index).ok()??;
    let man_file = legaia_asset::man_section::parse(&man_bytes).ok()?;
    let installs = scene_entry_ambient_installs(&man_file, &man_bytes);
    if installs.is_empty() {
        return None;
    }
    let mut world = World {
        frame_step: 2,
        ..Default::default()
    };
    world.install_field_stagers(scripts.bytes);
    world.set_vdf_buffer(legaia_engine_core::scene_bundle::find_vdf_buffer(&scene));
    for arg in installs {
        world.spawn_ambient_record(arg as usize + 1, [0, 0, 0]);
    }
    let mut run = Run {
        peak: world.ambient_fx.len(),
        first_half_peak: world.ambient_fx.len(),
        second_half_peak: 0,
        retired: false,
        exhausted: world.ambient_pool_exhausted(),
    };
    let mut prev = world.ambient_fx.len();
    for i in 0..TICKS {
        world.tick_ambient_fx();
        let n = world.ambient_fx.len();
        run.peak = run.peak.max(n);
        if i < TICKS / 2 {
            run.first_half_peak = run.first_half_peak.max(n);
        } else {
            run.second_half_peak = run.second_half_peak.max(n);
        }
        run.retired |= n < prev;
        run.exhausted |= world.ambient_pool_exhausted();
        prev = n;
    }
    Some(run)
}

/// No scene's ambient tree grows without bound, and none reaches the pool
/// ceiling. Both halves matter: the ceiling assertion alone would pass on a
/// tree that grows to exactly the cap and then truncates in silence, which is
/// the state this measurement replaced.
#[test]
fn every_scene_ambient_tree_settles_below_the_pool_ceiling_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");

    let mut scenes = 0usize;
    let mut emitters = 0usize;
    let mut worst: (usize, String) = (0, String::new());
    for name in index.cdname_scene_names() {
        let Some(run) = run_scene(&index, &name) else {
            continue;
        };
        scenes += 1;
        if run.retired {
            emitters += 1;
        }
        if run.peak > worst.0 {
            worst = (run.peak, name.clone());
        }
        assert!(
            !run.exhausted,
            "{name}: ambient pool exhausted ({MAX_AMBIENT_PARTS} parts) - the tree \
             stopped seating what it authored"
        );
        assert!(
            run.second_half_peak <= run.first_half_peak,
            "{name}: population still climbing after {} ticks ({} -> {}) - an \
             emitter whose children are never freed",
            TICKS / 2,
            run.first_half_peak,
            run.second_half_peak
        );
    }

    assert!(
        scenes >= 60,
        "surveyed {scenes} scenes with an entry ambient tree"
    );
    // Non-vacuity: the settling above is the free path doing work, not the
    // trees being static. Several scenes are emitters whose children are
    // spawned and freed continuously.
    assert!(
        emitters >= 5,
        "only {emitters} scene(s) ever freed a part - the bound would then say \
         nothing about the free path"
    );
    assert!(
        worst.0 <= MAX_AMBIENT_PARTS,
        "corpus peak {} parts in {} exceeds the {MAX_AMBIENT_PARTS}-slot pool",
        worst.0,
        worst.1
    );
    eprintln!(
        "ambient pool census: {scenes} scenes, {emitters} emitters, peak {} parts in {}",
        worst.0, worst.1
    );
}

/// The two scenes whose fan-out used to sit exactly at the old 128-part cap.
/// Their true populations are nowhere near it - what the cap was reporting was
/// its own value, filled with whatever the emitter had last spawned.
#[test]
fn the_two_scenes_that_pinned_the_old_cap_settle_far_below_it_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    for name in ["dolk", "uru2"] {
        let run = run_scene(&index, name).unwrap_or_else(|| panic!("{name} installs a tree"));
        assert!(run.retired, "{name} is an emitter - it frees parts");
        assert!(
            !run.exhausted,
            "{name} must not reach the pool ceiling ({} peak)",
            run.peak
        );
        assert!(
            run.second_half_peak <= run.first_half_peak,
            "{name} population still climbing ({} -> {})",
            run.first_half_peak,
            run.second_half_peak
        );
    }
}
