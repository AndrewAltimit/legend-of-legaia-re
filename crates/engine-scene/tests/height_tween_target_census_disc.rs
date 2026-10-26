//! Disc-gated census: whose `+0x8E` every **tweened** `4C 42` moves.
//!
//! Op `0x4C` nibble-4 sub-2 with a non-zero tick count schedules a
//! `FUN_8003C5F0` ramp on the resolved actor's `&+0x8E` (`0x801E1214..
//! 0x801E1230`, PROT 0897) - whichever actor that is: a placed object's, a
//! placement's, the player's. The actor tick's `0x20000000` height law turns
//! the slot into Y. This census classifies each tweened site's actor so the
//! port's arm can be checked against every kind the disc issues:
//!
//! - unprefixed: the executing record's own actor (partition 0 = object,
//!   partition 1 = placement; a partition-2 timeline has no own actor);
//! - prefixed `CC <id> 42 ..`: `id < N0` an object, `N0 <= id < N0 + N1` a
//!   placement (the spawner writes `+0x50 = N0 + index`), `0xF8` the player.
//!
//! `rayman`'s bridge collapse (`P2[7]`) is the placement case: four planks
//! (`0x54..=0x57`) tween to `-768`.
//!
//! Every carrier is walked (bundle + streaming variant MANs), every record of
//! every partition from its own first-opcode offset; only clean decodes count
//! (`legaia_scene_formats::field_disasm::clean_hit_offsets`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_scene::man_census::scene_man_carriers;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::{OpKey, clean_hit_offsets, man_script_spans};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Actor {
    Object,
    Placement,
    Player,
    Other,
}

#[test]
fn tweened_height_writes_reach_placements_or_skip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(ex) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&ex).expect("open ProtIndex");
    let key = OpKey {
        opcode: 0x4C,
        sub: Some(0x42),
    };
    let mut tally: BTreeMap<Actor, usize> = BTreeMap::new();
    let mut placement_sites: Vec<String> = Vec::new();
    let mut rayman_planks = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for c in scene_man_carriers(&index, &scene) {
            let Ok(mf) = legaia_scene_formats::man_section::parse(&c.payload) else {
                continue;
            };
            let n0 = mf.header.partition_counts[0].max(0) as usize;
            let n1 = mf.header.partition_counts[1].max(0) as usize;
            for (p, r, start, pc0, len) in man_script_spans(&mf, &c.payload) {
                let body = &c.payload[start..start + len];
                for pc in clean_hit_offsets(body, pc0, key) {
                    // `[4C 42 v v t t]` or `[CC id 42 v v t t]`.
                    let (target, op) = if body[pc] & 0x80 != 0 {
                        (body.get(pc + 1).copied(), body.get(pc + 2..pc + 7))
                    } else {
                        (None, body.get(pc + 1..pc + 6))
                    };
                    let Some(op) = op else { continue };
                    let ticks = u16::from_le_bytes([op[3], op[4]]);
                    if ticks == 0 {
                        continue;
                    }
                    let actor = match target {
                        None if p == 0 => Actor::Object,
                        None if p == 1 => Actor::Placement,
                        None => Actor::Other,
                        Some(0xF8) => Actor::Player,
                        Some(id) if usize::from(id) < n0 => Actor::Object,
                        Some(id) if usize::from(id) < n0 + n1 => Actor::Placement,
                        Some(_) => Actor::Other,
                    };
                    *tally.entry(actor).or_default() += 1;
                    if actor == Actor::Placement {
                        let end = i16::from_le_bytes([op[1], op[2]]);
                        placement_sites.push(format!(
                            "{name} P{p}[{r}] +{pc:#x} target {target:?} -> {end} / {ticks}"
                        ));
                        if name == "rayman" && p == 2 && r == 7 && end == -768 {
                            rayman_planks.push(target);
                        }
                    }
                }
            }
        }
    }
    eprintln!("[ok] tweened 4C 42 by actor: {tally:?}");
    for s in &placement_sites {
        eprintln!("[ok]   placement: {s}");
    }
    rayman_planks.sort();
    rayman_planks.dedup();
    assert_eq!(
        rayman_planks,
        vec![Some(0x54), Some(0x55), Some(0x56), Some(0x57)],
        "rayman's collapse beat tweens the four planks 0x54..=0x57 to -768"
    );
    assert!(
        tally.get(&Actor::Object).copied().unwrap_or(0) > 0,
        "objects tween too (chitei2's boulder)"
    );
}
