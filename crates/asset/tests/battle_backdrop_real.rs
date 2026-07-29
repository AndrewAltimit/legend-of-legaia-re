//! Disc-gated sweep pinning [`legaia_asset::battle_backdrop`] against the
//! retail corpus.
//!
//! Two properties carry the weight here, and both are corpus-level - no
//! single entry proves either.
//!
//! 1. **The id space.** The mirror table holds runtime stage ids; the
//!    viewers hold PROT extraction indices. The mapping is a constant
//!    offset, and the evidence for it is that *every* distinct id in the
//!    table lands on a `scene_tmd_stream` entry under it. A wrong offset
//!    leaves misses.
//! 2. **The table is a geometric decision, not a list.** A shell whose
//!    open side faces `-Z` is symmetric about `X = 0`, so mirroring in X
//!    maps it onto itself and fills nothing; only a half turn closes it.
//!    Retail never puts such a shell on the mirror list. That the two
//!    partitions are exactly disjoint is what rules out coincidence.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or the extracted files
//! aren't on disk.

use legaia_asset::battle_backdrop::{MirrorXTable, SecondCopy};
use legaia_asset::categorize::{Class, classify};
use legaia_asset::scene_tmd_stream::{self, OpenSide};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// `scene_tmd_stream` entries on the retail disc - a disc invariant.
const BACKDROP_ENTRIES: usize = 182;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    [PathBuf::from("extracted"), PathBuf::from("../../extracted")]
        .into_iter()
        .find(|p| p.join("PROT").is_dir() && p.join("SCUS_942.54").is_file())
}

struct Backdrop {
    prot_index: u32,
    name: String,
    objects: usize,
    open: OpenSide,
    tmd: legaia_tmd::Tmd,
    tmd_bytes: Vec<u8>,
}

fn corpus(root: &std::path::Path) -> Vec<Backdrop> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join("PROT"))
        .expect("read extracted/PROT")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let raw = std::fs::read(&path).expect("read PROT entry");
        if classify(&raw).class != Class::SceneTmdStream {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let prot_index: u32 = name[..4].parse().expect("NNNN_ prefix");
        let stream = scene_tmd_stream::detect(&raw).expect("detect");
        let tmd = legaia_tmd::parse(&raw[stream.tmd_range()]).expect("parse TMD");
        let shape = scene_tmd_stream::shell_shape(&tmd).expect("shell shape");
        out.push(Backdrop {
            prot_index,
            name,
            objects: tmd.objects.len(),
            open: shape.open,
            tmd_bytes: raw[stream.tmd_range()].to_vec(),
            tmd,
        });
    }
    out
}

#[test]
fn every_mirror_table_id_names_a_backdrop_entry_at_the_same_offset() {
    let Some(root) = extracted_dir() else {
        eprintln!("[skip] extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(root.join("SCUS_942.54")).expect("read SCUS");
    let table = MirrorXTable::from_scus(&scus).expect("mirror table parses out of retail SCUS");

    let corpus = corpus(&root);
    assert_eq!(corpus.len(), BACKDROP_ENTRIES);
    let have: BTreeSet<u32> = corpus.iter().map(|b| b.prot_index).collect();

    let distinct: BTreeSet<u16> = table.ids().iter().copied().collect();
    let missing: Vec<u16> = distinct
        .iter()
        .copied()
        .filter(|id| {
            !have.contains(&legaia_asset::battle_backdrop::prot_index_for_runtime_id(
                *id,
            ))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "these mirror-table stage ids do not land on a scene_tmd_stream entry \
         under the id->PROT offset, so the offset is wrong: {missing:?}"
    );
    eprintln!(
        "mirror table: {} slots, {} distinct ids, all {} resolve to backdrop entries",
        table.ids().len(),
        distinct.len(),
        distinct.len()
    );
}

#[test]
fn no_z_open_shell_takes_the_mirror_transform() {
    let Some(root) = extracted_dir() else {
        eprintln!("[skip] extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(root.join("SCUS_942.54")).expect("read SCUS");
    let table = MirrorXTable::from_scus(&scus).expect("mirror table");

    let mut z_open = 0usize;
    let mut mirrored = 0usize;
    let mut wrong = Vec::new();
    for b in corpus(&root) {
        let copy = table.second_copy_for_prot_index(b.prot_index);
        if copy == SecondCopy::MirrorX {
            mirrored += 1;
        }
        if !matches!(b.open, OpenSide::NegZ | OpenSide::PosZ) {
            continue;
        }
        z_open += 1;
        // A Z-open shell is symmetric in X. Reflecting it in the YZ plane
        // reproduces it in place and leaves the hole open, so retail must
        // not have chosen that transform for it.
        if copy == SecondCopy::MirrorX {
            wrong.push(b.name);
        }
    }
    assert!(
        z_open > 0 && mirrored > 0,
        "sweep is vacuous: {z_open} Z-open, {mirrored} mirrored"
    );
    assert!(
        wrong.is_empty(),
        "a Z-open shell is invariant under x -> -x, so mirroring it cannot \
         complete it - these are on the mirror list anyway: {wrong:?}"
    );
    eprintln!(
        "{mirrored} of {BACKDROP_ENTRIES} backdrops take the mirrored second copy; \
         none of the {z_open} Z-open shells is among them"
    );
}

#[test]
fn the_dropped_object_is_never_the_only_one() {
    let Some(root) = extracted_dir() else {
        eprintln!("[skip] extracted/ incomplete");
        return;
    };
    // Retail decrements the object count unconditionally on the default
    // path, so a single-object backdrop would end up drawing nothing at
    // all. No such entry exists.
    let mut counts = std::collections::BTreeMap::<usize, usize>::new();
    let mut multi = Vec::new();
    for b in corpus(&root) {
        assert!(
            b.objects >= 2,
            "{} has {} object(s): dropping object 1 would leave it empty",
            b.name,
            b.objects
        );
        let drawn = legaia_asset::battle_backdrop::drawn_object_indices(b.objects);
        assert_eq!(drawn.len(), b.objects - 1);
        assert!(!drawn.contains(&1));
        // The other arm of the `DAT_8007B64B` gate keeps everything.
        let kept = legaia_asset::battle_backdrop::drawn_object_indices_gated(b.objects, true);
        assert_eq!(kept.len(), b.objects);
        if b.objects > 2 {
            multi.push(b.prot_index);
        }
        *counts.entry(b.objects).or_default() += 1;
    }
    // The only backdrops with more than two objects are the nine kingdom
    // overworld shells' populated variants - the ones whose draw list is
    // `0, 2, 3` rather than `0` alone.
    assert_eq!(multi, vec![88, 89, 90, 247, 248, 249, 394]);
    eprintln!("backdrop object-count histogram: {counts:?}; >2 objects: {multi:?}");
}

#[test]
fn the_second_copy_closes_every_shell() {
    let Some(root) = extracted_dir() else {
        eprintln!("[skip] extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(root.join("SCUS_942.54")).expect("read SCUS");
    let table = MirrorXTable::from_scus(&scus).expect("mirror table");

    // The point of the whole mechanism: whatever the table chose, the two
    // copies together must span the axis the authored half leaves open.
    // "Closed" here is the mirror image of `ShellShape`'s own test - the
    // authored half puts under 10% of its extent past the plane, so the
    // placed pair must put at least 40% there.
    let mut worst: Option<(f32, String)> = None;
    for b in corpus(&root) {
        let drawn = legaia_asset::battle_backdrop::drawn_objects_tmd(&b.tmd);
        let one = legaia_tmd::mesh::tmd_to_mesh(&drawn, &b.tmd_bytes);
        let mut both = one.clone();
        both.append_scaled(&one, table.second_copy_for_prot_index(b.prot_index).scale());
        let (lo, hi) = both.aabb();
        let axis = match b.open {
            OpenSide::NegX | OpenSide::PosX => 0,
            OpenSide::NegZ | OpenSide::PosZ => 2,
        };
        let (l, h) = (lo[axis], hi[axis]);
        let span = h - l;
        assert!(span > 0.0, "{}: degenerate span on axis {axis}", b.name);
        let open_share = l.abs().min(h.abs()) / span;
        assert!(
            open_share >= 0.40,
            "{}: the placed pair still only puts {:.3} of its axis-{axis} extent \
             past the plane, so the shell is not closed",
            b.name,
            open_share
        );
        if worst.as_ref().is_none_or(|(f, _)| open_share < *f) {
            worst = Some((open_share, b.name.clone()));
        }
    }
    let (f, name) = worst.expect("at least one backdrop");
    eprintln!("least-symmetric placed backdrop: {name} at {f:.4} (0.5 = perfect)");
}
