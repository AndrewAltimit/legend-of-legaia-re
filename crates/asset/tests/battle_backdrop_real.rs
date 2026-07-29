//! Disc-gated sweep pinning [`legaia_asset::battle_backdrop`] against the
//! retail corpus.
//!
//! Four properties carry the weight here, and all are corpus-level - no
//! single entry proves any of them.
//!
//! 1. **The id space.** The mirror table holds runtime stage ids; the
//!    viewers hold PROT extraction indices. The mapping is a constant
//!    offset, and the evidence for it is that *every* distinct id in the
//!    table lands on a `scene_tmd_stream` entry under it. A wrong offset
//!    leaves misses.
//! 2. **The table respects one geometric constraint.** A shell whose open
//!    side faces `-Z` is symmetric about `X = 0`, so mirroring in X maps it
//!    onto itself and fills nothing; only a half turn closes it. Retail
//!    never puts such a shell on the mirror list.
//! 3. **...but it is not otherwise derivable from the mesh.** Many backdrop
//!    meshes are shared verbatim between scenes, and the table splits a
//!    quarter of those groups across the two transforms. So a viewer must
//!    read the table, and two entries that are the same place can be placed
//!    differently without either being wrong.
//! 4. **The ground tile is per-entry and all-or-nothing.** The grid emitter
//!    hardcodes one page and one CLUT address for the whole game; nearly
//!    every backdrop entry carries a TIM at exactly those addresses, and
//!    none carries that page under a different palette.
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
    raw: Vec<u8>,
}

impl Backdrop {
    /// The scene name the extractor's `NNNN_<name>.BIN` label carries.
    fn scene(&self) -> &str {
        self.name
            .split_once('_')
            .map_or("", |(_, rest)| rest.trim_end_matches(".BIN"))
    }

    /// The entry's own VRAM, built the way a viewer builds it: every TIM the
    /// entry carries, uploaded to the framebuffer slot its header names.
    fn vram(&self) -> legaia_tim::Vram {
        let mut vram = legaia_tim::Vram::new();
        let scan = legaia_asset::tim_scan::scan_entry(&self.raw);
        for (source, hit) in &scan.hits {
            let buf: Option<&[u8]> = match source {
                legaia_asset::tim_scan::Source::Raw => self.raw.get(hit.offset..),
                legaia_asset::tim_scan::Source::Lzs(i) => {
                    scan.lzs_sections.get(*i).and_then(|s| s.get(hit.offset..))
                }
            };
            if let Some(b) = buf
                && let Ok(tim) = legaia_tim::parse(b)
            {
                vram.upload_tim(&tim);
            }
        }
        vram
    }
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
            raw,
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

/// The ground tile is per-entry data, and it is either wholly there or
/// wholly absent.
///
/// The grid emitter hardcodes one page address and one CLUT address for
/// every stage in the game, which reads like a guess until you check what
/// the entries carry: a large majority put a TIM at exactly that page with
/// its palette at exactly that row, and **no** entry puts one at that page
/// under a different palette. That disjointness is the evidence - a wrong
/// page constant would land on entries that palette it elsewhere.
///
/// It is also what lets a viewer decide honestly whether to draw a floor:
/// an untextured grid is a flat slab across the whole stage, so an entry
/// without the tile has to draw none.
#[test]
fn the_ground_tile_is_addressed_by_the_emitters_own_constants() {
    let Some(root) = extracted_dir() else {
        eprintln!("[skip] extracted/ incomplete");
        return;
    };
    use legaia_asset::battle_backdrop as bd;
    let (page_x, page_y) = bd::ground_page_xy();
    let (clut_x, clut_y) = bd::ground_clut_xy();

    let (win_x, win_y, win_w, win_h) = bd::ground_page_rect();
    let mut drawable = 0usize;
    let mut page_without_clut = Vec::new();
    let mut lost_to_prim_heuristic = Vec::new();
    for b in corpus(&root) {
        let vram = b.vram();
        let has_page = vram.region_has_data(
            win_x as usize,
            win_y as usize,
            win_w as usize,
            win_h as usize,
        );
        let has_clut = vram.region_has_data(clut_x as usize, clut_y as usize, 16, 1);
        assert_eq!(
            bd::ground_grid_drawable(&vram),
            has_page && has_clut,
            "{}: the drawability predicate disagrees with the two regions it is made of",
            b.name
        );
        if has_page && !has_clut {
            page_without_clut.push(b.name.clone());
        }
        if has_page && has_clut {
            drawable += 1;
        }
        // The predicate carries a second rule the two region checks do not:
        // it rejects a 4bpp prim whose CLUT scanline is populated past 256
        // entries, as a row that wide is more likely a texture misread as a
        // palette. Row 479 is the shared NPC CLUT row, so that could in
        // principle fire here. On the retail corpus it never does - and the
        // day it starts to, this is the arm that says so rather than a floor
        // quietly disappearing from one scene.
        if has_page && has_clut && !bd::ground_grid_drawable(&vram) {
            lost_to_prim_heuristic.push(b.name.clone());
        }
    }
    assert!(
        page_without_clut.is_empty(),
        "these entries fill the ground page but palette it somewhere other than \
         row {clut_y}, which the page constant should have made impossible: {page_without_clut:?}"
    );
    // Non-vacuity: both sides of the split have to be populated, or the
    // predicate is untested in one direction.
    assert!(
        drawable > BACKDROP_ENTRIES / 2 && drawable < BACKDROP_ENTRIES,
        "{drawable} of {BACKDROP_ENTRIES} backdrops carry the ground tile - \
         a split this lopsided means the constants or the scan changed"
    );
    assert!(
        lost_to_prim_heuristic.is_empty(),
        "these entries carry the tile and its palette but the wide-CLUT-row \
         heuristic suppresses the floor anyway - decide deliberately whether \
         row 479 is a palette row here before letting it drop: \
         {lost_to_prim_heuristic:?}"
    );
    eprintln!(
        "{drawable} of {BACKDROP_ENTRIES} backdrops carry the ground tile at \
         page ({page_x},{page_y}) / CLUT ({clut_x},{clut_y}); \
         {} do not and so draw no floor",
        BACKDROP_ENTRIES - drawable
    );
}

/// The mirror table is authorial per-stage data, **not** a property of the
/// mesh - so a viewer cannot derive it, and two entries that look like the
/// same place can legitimately be placed differently.
///
/// This is the shape of a recurring bug report: a stage renders with its
/// far half rotated where a byte-identical stage elsewhere renders it
/// reflected, and the difference reads as a port defect. It is not. Many
/// backdrop meshes are shared verbatim between scenes, and retail's table
/// splits several of those groups across the two transforms - including
/// one pair of byte-identical entries **inside a single scene**, which no
/// geometric rule could ever separate.
///
/// The reason it costs retail nothing is in the second half of the check:
/// where the split is within one scene, the shell's cut section is exactly
/// symmetric in `z`, and a `z`-symmetric half is mapped to the same set by
/// both transforms. The choice is only visible where the halves differ.
#[test]
fn the_second_copy_transform_is_not_a_function_of_the_mesh() {
    let Some(root) = extracted_dir() else {
        eprintln!("[skip] extracted/ incomplete");
        return;
    };
    let scus = std::fs::read(root.join("SCUS_942.54")).expect("read SCUS");
    let table = MirrorXTable::from_scus(&scus).expect("mirror table");

    // Group the corpus by exact mesh bytes.
    let mut by_mesh: std::collections::BTreeMap<Vec<u8>, Vec<Backdrop>> = Default::default();
    for b in corpus(&root) {
        by_mesh.entry(b.tmd_bytes.clone()).or_default().push(b);
    }
    let shared: Vec<&Vec<Backdrop>> = by_mesh.values().filter(|g| g.len() > 1).collect();
    let mut split = Vec::new();
    let mut split_within_one_scene = Vec::new();
    for g in &shared {
        let verdicts: Vec<SecondCopy> = g
            .iter()
            .map(|b| table.second_copy_for_prot_index(b.prot_index))
            .collect();
        if verdicts.windows(2).all(|w| w[0] == w[1]) {
            continue;
        }
        split.push(
            g.iter()
                .map(|b| {
                    format!(
                        "{}={:?}",
                        b.name,
                        table.second_copy_for_prot_index(b.prot_index)
                    )
                })
                .collect::<Vec<_>>()
                .join(" "),
        );
        // A group spanning several scenes can still be split *inside* one of
        // them, and that is the case worth having: it is the one no per-scene
        // rule could explain. Test each scene's own members, not the group's.
        let mut per_scene: std::collections::BTreeMap<&str, Vec<SecondCopy>> = Default::default();
        for b in g.iter() {
            per_scene
                .entry(b.scene())
                .or_default()
                .push(table.second_copy_for_prot_index(b.prot_index));
        }
        if per_scene
            .values()
            .any(|v| !v.windows(2).all(|w| w[0] == w[1]))
        {
            split_within_one_scene.push(g);
        }
    }
    assert!(
        !shared.is_empty() && !split.is_empty(),
        "sweep is vacuous: {} shared-mesh groups, {} split",
        shared.len(),
        split.len()
    );
    assert!(
        !split_within_one_scene.is_empty(),
        "no byte-identical pair inside one scene is split across the two \
         transforms - without one, 'the table is not a mesh property' rests \
         only on cross-scene pairs and a per-scene rule could still explain it"
    );
    for g in &split_within_one_scene {
        // Both transforms agree on a z-symmetric half, which is why retail
        // can differ here without it showing.
        let cut: BTreeSet<(i16, i16)> = g[0]
            .tmd
            .objects
            .first()
            .expect("object 0")
            .vertices
            .iter()
            .filter(|v| v.x.unsigned_abs() <= 250)
            .map(|v| (v.y, v.z))
            .collect();
        assert!(!cut.is_empty(), "{}: no cut-plane vertices", g[0].name);
        let flipped: BTreeSet<(i16, i16)> = cut.iter().map(|&(y, z)| (y, -z)).collect();
        assert_eq!(
            cut, flipped,
            "{} is split within one scene on a half that is NOT z-symmetric, so \
             the two transforms give visibly different rings - that would make \
             retail's own data self-contradictory rather than merely redundant",
            g[0].name
        );
    }
    eprintln!(
        "{} backdrop meshes are shared by more than one entry; retail's table \
         splits {} of those groups across the two transforms ({} of them inside \
         a single scene): {:?}",
        shared.len(),
        split.len(),
        split_within_one_scene.len(),
        split
    );
}
