//! Disc-gated: a field NPC walks only where retail walks it.
//!
//! Retail has two sources of free-roam NPC movement and the port runs both:
//! the ambient tail-section-1 stream (`FUN_80038158` walk ops, the
//! villagers' wandering) and script-started legs (an interaction prologue, a
//! cutscene poke). A placement's own `0x4C 0x51` ops are neither: each is an
//! instant **seat**, one per story-flag branch, applied once by the
//! scene-entry pre-run. The port once looped those seats as an autonomous
//! patrol, which walked town01's gate guards (placements 32 / 33, beside the
//! exit to the world map) back and forth between their story stations.
//! Retail holds them still: every catalogued town01 field-run state (both
//! emulators, across the whole chapter) has placement 32 on its seat
//! `(12864, 1856)`, and 33 on one seat or another per story state.
//!
//! Assertions are structural (slots, world coordinates) - no Sony bytes.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md
//! convention).

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};

/// Per placement slot: its entry seat and its summed free-roam travel.
type Excursions = BTreeMap<u8, ((i16, i16), i32)>;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Enter `scene` with the play hosts' liveliness on, tick `ticks` frames,
/// and return each placement's entry position and its summed free-roam
/// travel: only frames with no cutscene timeline running count (a
/// timeline's cross-context walks are scripted legs retail runs too).
fn run_scene(
    extracted: &std::path::Path,
    scene: &str,
    ticks: usize,
) -> Option<(SceneHost, Excursions)> {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    let entered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        host.enter_field_scene(scene, 0).is_ok()
    }));
    if !matches!(entered, Ok(true)) {
        return None;
    }
    host.world.npcs.animate = true;
    let mut prev = host.world.npcs.positions.clone();
    let mut out: Excursions = prev.iter().map(|(&k, &p)| (k, (p, 0))).collect();
    for _ in 0..ticks {
        let stepped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.world.tick();
        }));
        if stepped.is_err() {
            break;
        }
        let free_roam = !host.world.cutscene_timeline_active();
        for (k, &(x, z)) in &host.world.npcs.positions {
            if let (true, Some(&(px, pz)), Some((_, d))) = (free_roam, prev.get(k), out.get_mut(k))
            {
                *d += (x as i32 - px as i32)
                    .abs()
                    .max((z as i32 - pz as i32).abs());
            }
        }
        prev = host.world.npcs.positions.clone();
    }
    Some((host, out))
}

#[test]
fn town01_gate_guards_stand_still_and_only_ambient_walkers_wander() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let (host, seen) = run_scene(&extracted, "town01", 600).expect("enter town01");

    // The gate guards hold their seats for ten seconds of free roam.
    for slot in [32u8, 33] {
        let &(seat, disp) = seen.get(&slot).expect("gate guard is installed");
        assert_eq!(
            disp, 0,
            "town01 gate guard P1[{slot}] stays on its seat {seat:?} (retail never walks it)"
        );
    }
    assert_eq!(
        seen[&32].0,
        (12864, 1856),
        "P1[32]'s seat is the one every retail town01 state shows"
    );

    // The only free-roam movers are placements bound to a walking ambient
    // stream - retail's own wander.
    let mut wanderers = 0;
    for (&slot, &(seat, disp)) in &seen {
        let walks = host.world.npcs.ambient.get(&slot).is_some_and(|c| c.walks);
        if walks {
            wanderers += usize::from(disp > 0);
        } else {
            assert_eq!(
                disp, 0,
                "P1[{slot}] has no ambient walk stream, so it holds {seat:?}"
            );
        }
    }
    // Non-vacuous: the ambient wander really runs (P1[12] wanders in every
    // retail state too).
    assert!(
        wanderers >= 3,
        "several town01 villagers wander (got {wanderers})"
    );
    eprintln!("[ran] town01: {wanderers} ambient wanderers moved; gate guards held");
}

/// The same rule across every field scene the disc ships: no placement
/// without a walking ambient stream moves in free roam.
#[test]
fn no_scene_walks_a_placement_without_an_ambient_stream() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let names = {
        let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        let mut n = host.index.cdname_scene_names();
        n.sort();
        n.dedup();
        n
    };
    let mut entered = 0;
    let mut failures = Vec::new();
    for name in &names {
        let Some((host, seen)) = run_scene(&extracted, name, 300) else {
            continue;
        };
        entered += 1;
        for (&slot, &(seat, disp)) in &seen {
            let walks = host.world.npcs.ambient.get(&slot).is_some_and(|c| c.walks);
            if !walks && disp > 0 {
                failures.push(format!("{name} P1[{slot}] left {seat:?} by {disp}"));
            }
        }
    }
    eprintln!(
        "[ran] {entered} field scenes entered, {} stray walker(s)",
        failures.len()
    );
    for f in &failures {
        eprintln!("  STRAY {f}");
    }
    assert!(
        entered >= 50,
        "expected the field-scene corpus, got {entered}"
    );
    assert!(
        failures.is_empty(),
        "{} placement(s) walk without a stream",
        failures.len()
    );
}
