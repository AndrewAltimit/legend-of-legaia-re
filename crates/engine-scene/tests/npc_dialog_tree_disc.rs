//! Disc-gated: every talkable placement's **conversation tree**
//! (`man_field_scripts::placement_dialog_tree`) is bounded, cycle-safe and
//! well-formed on every field scene, and branching NPCs reach more than one
//! page.
//!
//! Asserts structure only - no decoded game text is quoted. Skips (passes)
//! when `LEGAIA_DISC_BIN` is unset.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_field::man_field_scripts::{
    DialogNode, DialogTree, EndReason, MAX_NODES, classify_placements, placement_dialog_tree,
};
use legaia_engine_scene::scene::{ProtIndex, Scene};

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Structural checks on one tree; returns (pages, choices, branches, undecoded ends).
fn check_tree(tag: &str, man_len: usize, t: &DialogTree) -> (usize, usize, usize, usize) {
    let mut ids = HashSet::new();
    let (mut pages, mut choices, mut branches, mut undecoded) = (0, 0, 0, 0);
    t.visit(|n| {
        assert!(ids.insert(n.id()), "{tag}: node id {} repeats", n.id());
        match n {
            DialogNode::Page { lines, pc, .. } => {
                pages += 1;
                assert!(!lines.is_empty(), "{tag}: empty page at 0x{pc:X}");
                for l in lines {
                    assert!(l.lead < man_len && l.bytes.len() < 0x400);
                }
            }
            DialogNode::Choice { options, .. } => {
                choices += 1;
                assert!((2..=4).contains(&options.len()), "{tag}: picker arity");
            }
            DialogNode::Branch { .. } => branches += 1,
            DialogNode::End {
                reason: r @ EndReason::Undecoded { .. },
                pc,
                ..
            } => {
                eprintln!("{tag}: undecoded end at 0x{pc:X}: {r:?}");
                undecoded += 1;
            }
            _ => {}
        }
    });
    assert_eq!(
        ids.len(),
        t.node_count,
        "{tag}: node_count matches the walk"
    );
    assert!(t.node_count <= MAX_NODES + 1, "{tag}: node budget");
    // Every goto points at a node of this tree.
    t.visit(|n| {
        if let DialogNode::Goto { to: Some(to), .. } = n {
            assert!(ids.contains(to), "{tag}: goto to a missing node {to}");
        }
    });
    (pages, choices, branches, undecoded)
}

#[test]
fn npc_dialog_trees_are_bounded_and_reach_their_branches() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));

    let (mut talkers, mut with_tree, mut multi_page, mut branching) = (0, 0, 0, 0);
    let (mut undecoded_trees, mut scenes, mut silent_talkers) = (0, 0, 0);
    let mut town01_multi = 0;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let Ok(Some(man_bytes)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(man) = legaia_scene_formats::man_section::parse(&man_bytes) else {
            continue;
        };
        scenes += 1;
        for (p, kind) in classify_placements(&man, &man_bytes) {
            let talks = matches!(
                kind,
                legaia_engine_field::man_field_scripts::PlacementKind::Npc {
                    dialog_inline: Some(_),
                    ..
                }
            );
            let Some(tree) = placement_dialog_tree(&man, &man_bytes, &p) else {
                assert!(!talks, "{name} P1[{}]: talk NPC with no tree", p.index);
                continue;
            };
            let tag = format!("{name} P1[{}]", p.index);
            let (pages, _choices, branches, undecoded) = check_tree(&tag, man_bytes.len(), &tree);
            if talks {
                talkers += 1;
                // A record can carry text its interaction never reaches (the
                // `town01` debug flag-setter parks on its own `0x21`).
                if pages == 0 {
                    silent_talkers += 1;
                }
            }
            with_tree += 1;
            if pages > 1 {
                multi_page += 1;
                if name == "town01" {
                    town01_multi += 1;
                }
            }
            if branches > 0 {
                branching += 1;
            }
            if undecoded > 0 {
                undecoded_trees += 1;
            }
        }
    }
    eprintln!(
        "scenes {scenes}, talk NPCs {talkers}, trees {with_tree}, multi-page {multi_page}, \
         branching {branching}, with an undecoded end {undecoded_trees}, \
         talkers reaching no text {silent_talkers}"
    );
    assert!(
        silent_talkers * 10 < talkers,
        "nearly every talker's tree reaches its text"
    );
    assert!(
        talkers > 1000,
        "the field corpus has well over a thousand talkers"
    );
    // The point of the tree: most conversations are more than one page.
    assert!(multi_page * 2 > with_tree, "most trees reach several pages");
    assert!(branching * 4 > with_tree, "story-gated branches are common");
    assert!(
        town01_multi > 10,
        "Rim Elm's villagers say more than one thing"
    );
    assert!(
        undecoded_trees * 20 < with_tree,
        "undecodable ops end under 5% of trees"
    );
}
