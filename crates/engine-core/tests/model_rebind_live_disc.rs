//! Disc-gated: a scripted mesh re-bind resolves to real TMD bytes.
//!
//! The wiring this pins is the one the host-drift page used to call blocked:
//! the scripted-motion VM's op `0x0E` records a model id on the world
//! (`World::field_npc_live_model`), and each host turns that id into a mesh
//! through the scene's own model bank
//! ([`legaia_engine_core::model_bank::SceneModelBank::tmd_bytes`]) - the
//! native window in `upload_assets`, the browser through its
//! `play_npc_live_model` export. Both read the same world field and call the
//! same resolver, so pinning the resolver pins both.
//!
//! `koin3` is the scene the census names for the re-bind: 100 authored sites
//! over 10 distinct targets, none of them a model some placement already
//! binds, and all of them inside an LZS-compressed bundle descriptor that a
//! magic scan over the scene's raw entries cannot see.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_core::man_field_scripts::scene_man_carriers;
use legaia_engine_core::model_bank::SceneModelBank;
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost};
use std::collections::BTreeSet;
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

/// Every distinct op-`0x0E` operand the scene's tail-section-1 streams carry,
/// decoded the way the op itself reads it (`[0E, lo, hi]`, signed halfword).
fn authored_swap_targets(index: &ProtIndex, scene: &Scene) -> BTreeSet<i16> {
    let mut out = BTreeSet::new();
    for carrier in scene_man_carriers(index, scene) {
        let man = &carrier.payload;
        let Ok(man_file) = legaia_asset::man_section::parse(man) else {
            continue;
        };
        for rec in legaia_asset::man_motion::motion_records(man, &man_file) {
            for var in legaia_asset::man_motion::stream_variants(man, &rec) {
                let mut pc = var.code_offset;
                while pc < var.code_end && pc < man.len() {
                    let op = man[pc];
                    let Some(w) = legaia_asset::man_motion::op_width(op) else {
                        break;
                    };
                    if op == 0x0E && pc + 2 < man.len() {
                        out.insert(i16::from_le_bytes([man[pc + 1], man[pc + 2]]));
                    }
                    pc += w;
                }
            }
        }
    }
    out
}

#[test]
fn a_scripted_rebind_resolves_to_a_parseable_mesh() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open extracted");
    host.enter_field_scene("koin3", 0).expect("enter koin3");
    let scene = host.scene.as_ref().expect("scene loaded");

    // The bank the hosts resolve through is the one the host holds, rebuilt
    // on entry - not a second one built here.
    let bank = &host.model_bank;
    assert!(
        bank.len() > 1,
        "koin3's model bank collapsed to {} - `SceneResources::tmds` would report 1 \
         because it cannot see a TMD inside an LZS bundle descriptor",
        bank.len()
    );

    let targets = authored_swap_targets(&host.index, scene);
    assert!(!targets.is_empty(), "koin3 authors no op 0x0E operand");

    let mut resolved = 0usize;
    for &id in &targets {
        let Some(raw) = bank.tmd_bytes(scene, id) else {
            panic!("op 0x0E target {id} has no bytes in koin3's model bank");
        };
        let tmd = legaia_tmd::parse(&raw).unwrap_or_else(|e| {
            panic!(
                "op 0x0E target {id} resolved to {} bytes that do not parse as a TMD: {e:#}",
                raw.len()
            )
        });
        assert!(
            !tmd.objects.is_empty(),
            "op 0x0E target {id} parsed to a TMD with no objects"
        );
        resolved += 1;
    }
    eprintln!(
        "[model rebind] koin3: {resolved} of {} targets resolve to a parseable TMD",
        targets.len()
    );
    assert_eq!(resolved, targets.len());

    // The world field each host reads: install one of the authored targets on
    // a real placement slot and read it back the way both hosts do.
    let id = *targets.iter().next().expect("a target");
    let slot = 0u8;
    host.world.set_field_npc_live_model(slot, id);
    assert_eq!(
        host.world.field_npc_live_model(slot),
        Some(id),
        "the host-facing read is the slot space the effect arm writes"
    );
    assert!(
        SceneModelBank::build(scene).tmd_bytes(scene, id).is_some(),
        "a bank rebuilt from the same scene resolves the same id"
    );
}
