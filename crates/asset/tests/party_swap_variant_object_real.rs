//! Disc-gated oracle for the swapped player file's **equipment-variant**
//! objects (`party_swap::playerize`).
//!
//! A section's surplus objects are not spare decoration. The first one is
//! the `0xFF` **variant**, and the per-frame pass `FUN_8004CCD4` installs
//! it *into its attach bone's own channel* of the render node's model
//! table whenever a window of the playing entry's `+0xA4..+0xAB` track is
//! open - so a variant that carries no geometry does not merely drop an
//! ornament, it **deletes that bone's part** for those frames. Retail's
//! own variant is the attach bone's mesh again, which is why the swap has
//! to put one there too.
//!
//! The entry the **Spirit** command plays is one of the entries that
//! declares such a window: the battle command dispatcher `FUN_801D0748`
//! stages anim id `0x10` at `0x801D16B0` (the `0x4000` = Cross arm, which
//! also writes action category `4`), and `FUN_8004AD80` materializes id
//! `0x10` from art-bank record `0`. On the USA disc only **Noa's** file
//! declares live windows, and her record 0 declares `+0xA8 = [1, 53]` over
//! a 58-frame clip - so whichever sibling the mapping puts in Noa's slot
//! lost a hand for 53 frames of every Spirit.
//!
//! Two controls keep the claim honest: the retail files must show the same
//! mirror (so "mirrors its bone" is retail's shape, not this test's
//! invention), and the `0xFE` extras - which have pose channels of their
//! own and draw *alongside* the hand - must stay empty.
//!
//! NB this test is about which MESH a channel draws. It says nothing about
//! where the channel is posed; joint continuity is
//! `party_swap_idle_continuity_real.rs`.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::party_swap::{self, PlayerRig, playerize};
use legaia_asset::{battle_char_assembly, battle_data_pack};
use legaia_tmd::encode::{ModelObject, decode_model};

fn prot_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted/PROT", "../../extracted/PROT"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
}

/// Every retail player file carries exactly two `0xFF` variants - one per
/// arm section - and the post-pass sorts them last, so they are the final
/// two objects of the assembled model.
const VARIANTS: usize = 2;

const SIBLINGS: [(u16, &str); 3] = [(162, "Gi"), (163, "Che"), (164, "Lu")];

/// Equipment ids to assemble at. `0` is the default build; the others are
/// low ids every section carries, so the variants are exercised on more
/// than one section record.
const EQUIP: [[u8; 5]; 3] = [[0; 5], [1; 5], [3; 5]];

fn prim_count(o: &ModelObject) -> usize {
    o.groups.iter().map(|g| g.prims.len()).sum()
}

/// The assembled model, its skeleton bone count, and the attach bone of
/// each object.
struct Assembled {
    model: Vec<ModelObject>,
    bones: usize,
    anm_bones: Vec<u8>,
}

fn assemble(file: &[u8], equipped: &[u8; 5]) -> Option<Assembled> {
    let pack = battle_data_pack::parse(file).ok()?;
    let asm = battle_char_assembly::assemble_character(file, &pack, equipped).ok()?;
    let tmd = legaia_tmd::parse(&asm.tmd).ok()?;
    let model = decode_model(&tmd, &asm.tmd).ok()?;
    let bones = battle_char_assembly::idle_battle_animation(file)
        .ok()??
        .part_count;
    Some(Assembled {
        model,
        bones,
        anm_bones: asm.anm_bones,
    })
}

/// The entries of a player file that open an equipment-variant window,
/// as `(what, pair, start, end)`. Window layout per `FUN_8004CCD4`:
/// entry `+0xA4 + (pair * 2 + w) * 2` is `[start, end]`, live when
/// `end != 0`.
fn live_windows(file: &[u8]) -> Vec<(String, usize, u8, u8)> {
    let mut out = Vec::new();
    let Ok(rec0) = battle_char_assembly::decode_record0(file) else {
        return out;
    };
    let mut scan = |what: String, entry: usize| {
        for pair in 0..2usize {
            for w in 0..2usize {
                let at = entry + 0xA4 + (pair * 2 + w) * 2;
                let Some(win) = rec0.get(at..at + 2) else {
                    continue;
                };
                if win[1] != 0 {
                    out.push((what.clone(), pair, win[0], win[1]));
                }
            }
        }
    };
    for slot in 0..battle_char_assembly::ACTION_SLOT_COUNT {
        let Some(w) = rec0.get(slot * 4..slot * 4 + 4) else {
            continue;
        };
        let off = u32::from_le_bytes(w.try_into().unwrap()) as usize;
        if off != 0 && off + 0xAC <= rec0.len() {
            scan(format!("action slot {slot:#x}"), off);
        }
    }
    if let Ok(bank) = battle_char_assembly::art_animation_bank(&rec0) {
        for rec in &bank {
            scan(format!("art bank row {}", rec.index), rec.entry_offset);
        }
    }
    out
}

struct Host {
    file: Vec<u8>,
    rig: &'static PlayerRig,
    who: &'static str,
    slot: usize,
}

fn hosts(dir: &std::path::Path) -> Vec<Host> {
    [
        ("0863_edstati3.BIN", &party_swap::RIG_VAHN_GALA, "Vahn", 0),
        ("0864_edstati3.BIN", &party_swap::RIG_NOA, "Noa", 1),
        (
            "0865_battle_data.BIN",
            &party_swap::RIG_VAHN_GALA,
            "Gala",
            2,
        ),
    ]
    .into_iter()
    .map(|(f, rig, who, slot)| Host {
        file: std::fs::read(dir.join(f)).expect("read player file"),
        rig,
        who,
        slot,
    })
    .collect()
}

/// The premise the defect rests on: the Spirit command's entry really does
/// open a variant window, so an empty variant really is visible in game.
#[test]
fn the_spirit_entry_opens_an_equipment_variant_window() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut spirit_hosts = Vec::new();
    for host in hosts(&dir) {
        let windows = live_windows(&host.file);
        eprintln!(
            "== {}: {} live equipment-variant window(s)",
            host.who,
            windows.len()
        );
        for (what, pair, s, e) in &windows {
            eprintln!("   {what}: pair {pair} frames [{s}, {e}]");
        }
        // Art-bank row 0 is anim id `0x10` - what `FUN_801D0748` stages
        // for the Spirit command.
        if windows.iter().any(|(what, ..)| what == "art bank row 0") {
            spirit_hosts.push(host.who);
        }
    }
    assert!(
        !spirit_hosts.is_empty(),
        "no host's Spirit entry (art-bank row 0) opens a variant window - the \
         premise of party_swap_variant_object_real has gone stale; re-derive it \
         before trusting the fix it guards"
    );
    eprintln!("Spirit opens a variant window on: {spirit_hosts:?}");
}

/// The defect: a swapped player file whose variant objects are empty
/// deletes the attach bone's part for every frame a window is open.
#[test]
fn every_swapped_variant_object_mirrors_its_attach_bone() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT or LEGAIA_DISC_BIN missing");
        return;
    };
    let archive = std::fs::read(dir.join("0867_battle_data.BIN")).expect("read archive");
    let mut failures: Vec<String> = Vec::new();

    for host in hosts(&dir) {
        // Retail control: the shape the swap has to reproduce.
        for equipped in EQUIP {
            let Some(a) = assemble(&host.file, &equipped) else {
                continue;
            };
            for k in a.model.len().saturating_sub(VARIANTS)..a.model.len() {
                let bone = a.anm_bones[k] as usize;
                assert!(
                    !a.model[k].vertices.is_empty()
                        && prim_count(&a.model[k]) == prim_count(&a.model[bone]),
                    "{} retail eq {equipped:?}: variant object {k} is not its attach bone \
                     {bone}'s mesh ({} verts / {} prims against {} / {}) - the control this \
                     oracle compares against no longer holds",
                    host.who,
                    a.model[k].vertices.len(),
                    prim_count(&a.model[k]),
                    a.model[bone].vertices.len(),
                    prim_count(&a.model[bone]),
                );
            }
        }

        for (id, name) in SIBLINGS {
            let swapped = playerize::playerize_player_file(
                &host.file,
                host.file.len(),
                host.rig,
                &archive,
                id,
                host.slot,
            )
            .unwrap_or_else(|e| panic!("{name} -> {}: playerize: {e:#}", host.who));

            for equipped in EQUIP {
                let Some(a) = assemble(&swapped.file, &equipped) else {
                    continue;
                };
                let first_variant = a.model.len().saturating_sub(VARIANTS);
                for k in a.bones..a.model.len() {
                    let bone = a.anm_bones[k] as usize;
                    let (obj, attach) = (&a.model[k], &a.model[bone]);
                    if k >= first_variant {
                        // `0xFF`: the render pass swaps this INTO the
                        // bone's channel, so it must BE the bone's mesh.
                        if obj.vertices != attach.vertices || prim_count(obj) != prim_count(attach)
                        {
                            failures.push(format!(
                                "{name} -> {} eq {equipped:?}: variant object {k} carries \
                                 {} verts / {} prims where its attach bone {bone} carries \
                                 {} / {} - every frame a variant window is open, that bone \
                                 draws this instead",
                                host.who,
                                obj.vertices.len(),
                                prim_count(obj),
                                attach.vertices.len(),
                                prim_count(attach),
                            ));
                        }
                    } else if !obj.vertices.is_empty() {
                        // `0xFE`: an extra part with a pose channel of its
                        // own. Filling it would draw a second copy of the
                        // bone's mesh at a different pose.
                        failures.push(format!(
                            "{name} -> {} eq {equipped:?}: extra-part object {k} is not \
                             empty ({} verts) - it draws alongside bone {bone}, not \
                             instead of it",
                            host.who,
                            obj.vertices.len(),
                        ));
                    }
                }
                eprintln!(
                    "** {name} -> {} eq {equipped:?}: {} objects, {} bones, variants {} \
                     mirror their attach bones",
                    host.who,
                    a.model.len(),
                    a.bones,
                    first_variant,
                );
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} variant-object hole(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
