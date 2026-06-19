//! Disc-gated: pin the Rim Elm sparring partner - the town01 actor placement
//! whose talk-menu installs the opening lone-Tetsu training fight - and verify
//! that field-scene NPC dialogue is recovered as renderable text.
//!
//! Two findings are locked here:
//!
//! 1. **Carrier identity.** A single partition-1 placement at
//!    [`RIM_ELM_SPARRING_CARRIER_TILE`] / [`RIM_ELM_SPARRING_CARRIER_MODEL`]
//!    carries the multi-page sparring dialog. It is the only on-map placement
//!    whose inline dialog block is that long, which is what distinguishes the
//!    sparring partner from the village's many one-line NPCs.
//! 2. **Structural dialog recovery.** Field interaction records desync under
//!    linear disassembly (embedded message bytes alias field-VM opcodes), so
//!    the dialog text is found structurally (the `0x1F`-lead segment block) and
//!    renders through [`OwnedDialogPanel::from_inline_dialog`]. Before this, the
//!    opcode-`len` capture returned empty for every town01 NPC.
//!
//! No Sony text is asserted - only structural shape (placement coordinates,
//! model byte, that the dialog renders a substantial run of printable glyphs).
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::dialog::OwnedDialogPanel;
use legaia_engine_core::encounter_record::{
    RIM_ELM_SPARRING_CARRIER_MODEL, RIM_ELM_SPARRING_CARRIER_TILE,
};
use legaia_engine_core::man_field_scripts::{PlacementKind, classify_placement};
use legaia_engine_core::scene::{ProtIndex, Scene};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Tick a panel to its first page break / end and count printable glyphs.
fn printable_glyphs(inline: &[u8]) -> usize {
    let Some(mut panel) = OwnedDialogPanel::from_inline_dialog(inline) else {
        return 0;
    };
    for _ in 0..4000 {
        panel.tick();
        if panel.is_done() || panel.is_waiting_for_input() {
            break;
        }
    }
    panel
        .page_bytes()
        .iter()
        .filter(|&&b| (0x20..0x7F).contains(&b))
        .count()
}

#[test]
fn town01_sparring_carrier_is_pinned_and_renders_dialogue() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, "town01").expect("load town01");
    let bundle =
        legaia_engine_core::scene_bundle::find_bundle(&scene).expect("town01 scene bundle");
    let entry_bytes = index
        .entry_bytes_extended(bundle.entry_idx())
        .expect("entry bytes");
    let man = legaia_engine_core::scene_bundle::extract_man_payload(&bundle, &entry_bytes)
        .expect("man extract")
        .expect("town01 MAN payload");
    let man_file = legaia_asset::man_section::parse(&man).expect("man parse");
    let placements = man_file.actor_placements(&man);

    // The sparring partner: a single placement at the pinned tile + model.
    let carrier = placements
        .iter()
        .find(|p| {
            (p.tile_x, p.tile_z) == RIM_ELM_SPARRING_CARRIER_TILE
                && p.model_index == RIM_ELM_SPARRING_CARRIER_MODEL
        })
        .expect("town01 has the sparring-partner placement at the pinned tile/model");

    // It classifies as an NPC carrying renderable inline dialogue.
    let inline = match classify_placement(&man_file, &man, carrier) {
        PlacementKind::Npc {
            dialog_inline: Some(inline),
            ..
        } => inline,
        other => panic!("sparring carrier should be an Npc with inline dialog, got {other:?}"),
    };
    let carrier_glyphs = printable_glyphs(&inline);
    assert!(
        carrier_glyphs >= 16,
        "carrier renders a substantial dialog line (got {carrier_glyphs} printable glyphs)"
    );

    // The carrier's *inline block* is the longest on the map: the sparring
    // menu dwarfs the village's one-line NPCs. (Byte length of the captured
    // inline buffer = first text segment through the record's bounded end.)
    let carrier_block = inline.len();
    let mut longest_other = 0usize;
    for p in &placements {
        if std::ptr::eq(p, carrier) {
            continue;
        }
        if let PlacementKind::Npc {
            dialog_inline: Some(other),
            ..
        } = classify_placement(&man_file, &man, p)
        {
            longest_other = longest_other.max(other.len());
        }
    }
    assert!(
        carrier_block > longest_other,
        "sparring carrier has the longest inline dialog block ({carrier_block} bytes) \
         vs the next longest on-map NPC ({longest_other} bytes)"
    );

    // Structural-recovery regression guard: at least a dozen town01 placements
    // recover renderable dialogue. The pre-fix opcode-`len` capture returned
    // empty for all of them.
    let npc_with_text = placements
        .iter()
        .filter(|p| {
            matches!(
                classify_placement(&man_file, &man, p),
                PlacementKind::Npc {
                    dialog_inline: Some(_),
                    ..
                }
            )
        })
        .count();
    assert!(
        npc_with_text >= 12,
        "many town01 placements recover inline dialogue (got {npc_with_text})"
    );

    eprintln!(
        "[carrier] town01 sparring partner at tile {:?} model {:#04X}: \
         {carrier_glyphs} glyphs, {carrier_block}-byte block; {npc_with_text} NPCs carry dialogue",
        RIM_ELM_SPARRING_CARRIER_TILE, RIM_ELM_SPARRING_CARRIER_MODEL,
    );
}
