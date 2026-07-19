//! Disc-gated: the Rim Elm spar's 4-option menu now decodes as an inline picker.
//!
//! The menu is an `0x29` 4-option MES inline picker in the sparring partner's
//! dialogue, whose **index-2 entry ("...practice with...") starts the fight** -
//! pinned live (`autorun_tetsu_confirm.lua` + `autorun_tetsu_picker_data.lua`).
//! It uses the **immediate-labels** form (no post-page continuation byte), which
//! the picker decoder previously rejected. This test boots `town01`, scans every
//! field-NPC dialogue for a 4-option picker, and asserts one exists whose option
//! 2 label reads as the spar choice - proving `scan_pickers` now recognises it.
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::path::PathBuf;

use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// US build: dialogue glyph bytes are ASCII, so a label decodes to readable text.
fn label_text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .filter(|&&b| (0x20..0x7f).contains(&b))
        .map(|&b| b as char)
        .collect()
}

#[test]
fn rim_elm_spar_menu_decodes_as_a_four_option_picker() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live(
            "town01",
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");
    assert_eq!(session.host.world.mode, SceneMode::Field);

    // Scan every field-NPC dialogue for a 4-option picker.
    let mut four_opt = Vec::new();
    for (slot, bytes) in &session.host.world.field_npc_dialog {
        for p in legaia_mes::scan_pickers(bytes) {
            if p.n == 4 {
                let labels: Vec<String> = p.options.iter().map(|o| label_text(&o.label)).collect();
                eprintln!("[slot {slot}] 4-option picker @0x{:X}: {labels:?}", p.open);
                four_opt.push((p.clone(), labels));
            }
        }
    }

    assert!(
        !four_opt.is_empty(),
        "town01 must now decode a 4-option picker (the spar menu); the immediate-labels \
         form was previously rejected by parse_picker_at"
    );

    // The spar picker's index-2 option is the training fight - its label mentions
    // practising / fighting / sparring.
    let spar = four_opt.iter().find(|(_, labels)| {
        labels.get(2).is_some_and(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("practice")
                || l.contains("fight")
                || l.contains("spar")
                || l.contains("train")
        })
    });
    let (picker, labels) = spar.expect(
        "a 4-option picker whose index-2 label is the training-fight choice (e.g. \"...practice...\")",
    );
    eprintln!(
        "spar picker: option 2 = {:?}, jump_target(2) = {:?}",
        labels[2],
        picker.jump_target(2)
    );
    assert!(
        picker.jump_target(2).is_some(),
        "index-2 option resolves to a branch target (the arm-the-fight path)"
    );
}

/// End-to-end disc proof of the disc-derived fight option: at least one town01
/// field-NPC dialogue is a spar menu whose fight option
/// ([`legaia_engine_core::world::spar_menu_of`]) is chosen from the
/// scripted-battle install `3E FF 04` sitting in that option's branch - **not**
/// from any English label. This is the translation-independent path.
///
/// Contingency (see lane report): if `3E FF 04` is not present in the buffer
/// `spar_menu_of` receives (the truncated `dialog_inline`), the derivation must
/// instead run over the full `partition_record_span(..,1,10)` body. This test
/// keys on `field_npc_dialog` - the same buffer the live carrier path feeds -
/// so a failure here is the signal to widen that buffer.
#[test]
fn rim_elm_spar_fight_option_is_disc_derived_from_the_battle_install() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    session
        .enter_field_live(
            "town01",
            &FieldLiveOpts {
                live_loop: true,
                ..Default::default()
            },
        )
        .expect("enter field live");

    // Find a field dialogue the shipped derivation reads as a spar menu, and
    // confirm its fight option's branch carries the exact Tetsu install.
    let mut proven = false;
    for (slot, bytes) in &session.host.world.field_npc_dialog {
        let Some((n, fight_option)) = legaia_engine_core::world::spar_menu_of(bytes) else {
            continue;
        };
        assert_eq!(n, 4, "the spar menu is a 4-option picker");
        // Recover the picker to resolve the fight option's branch target.
        let picker = legaia_mes::scan_pickers(bytes)
            .into_iter()
            .find(|p| p.n == 4)
            .expect("the spar picker re-decodes");
        let t = picker
            .jump_target(fight_option)
            .expect("the fight option resolves to a branch target");
        let branch = &bytes[t.min(bytes.len())..];
        eprintln!(
            "[slot {slot}] disc-derived fight_option = {fight_option}, branch @0x{t:X} \
             starts {:02X?}",
            &branch[..branch.len().min(8)]
        );
        assert!(
            branch.windows(3).any(|w| w == [0x3E, 0xFF, 0x04]),
            "the disc-derived fight option's branch installs the lone-Tetsu formation \
             (`3E FF 04`) - the derivation is keyed on this op, not an English label"
        );
        proven = true;
    }

    assert!(
        proven,
        "town01 must present a spar menu whose fight option is derived from the \
         `3E FF 04` scripted-battle install (contingency: widen the buffer to the \
         full P1[10] partition body - see lane report)"
    );
}
