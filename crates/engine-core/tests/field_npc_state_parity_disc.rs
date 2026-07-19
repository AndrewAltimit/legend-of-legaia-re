//! Disc + save-library gated BREADTH oracle: every catalogued **field-mode**
//! retail capture (mednafen `.mcr` + PCSX-Redux `.sstate` library backups,
//! enumerated from `scripts/scenarios.toml`) versus a cold engine entry into
//! the same scene with the capture's story-flag bank seeded byte-for-byte.
//!
//! Goes beyond the town01 entry-position oracle
//! (`field_npc_entry_positions_disc.rs`): it sweeps EVERY library state whose
//! RAM says `game_mode == 0x03` on a non-worldmap scene - mid-beat states,
//! post-battle states, interiors, later chapters - and compares, per retail
//! partition-1 actor node (`_DAT_8007C354` list, `+0x50 = N0 + placement`):
//!
//! - **visibility**: retail parked at the `(0x7F,0x7F)` off-map sentinel
//!   <-> engine parked (the spawn-prologue despawn arrangement);
//! - **position**: both placed -> within the patrol-locality bound
//!   (`NPC_ROUTE_LOCALITY`; retail walkers roam their authored local route
//!   between capture and comparison, the seat is what the pre-run pins);
//! - **heading**: diagnostic-only (`+0x26`, retail `0` = Z-, engine `0` = Z+):
//!   a captured walker faces whichever way it last moved, so heading drift is
//!   expected dynamics, not a seat bug - reported, never gated;
//! - **story flags**: after the engine's scene entry (which runs the real MAN
//!   entry script + spawn prologues against the seeded bank), the engine's
//!   `DAT_80085758` mirror must still equal the capture's bank bit-for-bit.
//!   Retail captured mid-scene has already run the same entry script, so any
//!   engine-side flip is a divergence.
//!
//! Known, classified divergences are pinned in [`KNOWN_DIVERGENCES`] so the
//! gate stays green while the open items remain visible in the log.
//!
//! Skips (passes) when `LEGAIA_DISC_BIN`, `extracted/`, the scenario
//! manifest, or the library backups are missing - CI runs without disc data.

use std::path::{Path, PathBuf};

use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost, is_world_map_scene};
use legaia_mednafen::ScenarioManifest;
use legaia_mednafen::scenarios::library_backup_for;

const RAM_MASK: u32 = 0x001F_FFFF;
/// Field-actor list sentinel head (the `FUN_8003BC08` tick class).
const ACTOR_LIST_HEAD_VA: u32 = 0x8007_C354;
/// Active-scene CDNAME label (8 bytes).
const SCENE_NAME_VA: u32 = 0x8007_050C;
/// Game-mode register (`0x03` = field free-roam).
const GAME_MODE_VA: u32 = 0x8007_B83C;
/// System story-flag bank (`DAT_80085758`, MSB-first bit order).
const FLAG_BANK_VA: u32 = 0x8008_5758;
const FLAG_BANK_LEN: usize = 0x1100;

/// Divergences surveyed, classified, and deliberately tolerated. Each entry
/// keeps the gate green while documenting exactly what still diverges and
/// why. Key = `(scenario label, divergence key)`; the key is stable
/// (`vis:<pi>` / `pos:<pi>` / `flag:<idx>` / `enter`), and a key ending in
/// `*` prefix-matches (for whole-cluster capture-context classifications).
struct KnownDivergence {
    label: &'static str,
    key: &'static str,
    /// `a-reported` = engine bug with retail evidence, owned by the
    /// field-VM/stepping lane; `b` = expected dynamics / capture context a
    /// cold scene entry cannot know; `c` = open RE thread - see the note.
    class: &'static str,
    note: &'static str,
}

/// The comparison model behind class `b`: a catalogued capture is a MID-VISIT
/// snapshot. The engine side reproduces the retail FRESH-ENTRY arrangement
/// (the `FUN_8003A1E4` spawn-prologue pre-run against the capture's flag
/// bank); anything a mid-visit beat re-arranged after retail's own entry -
/// walk-on choreography seats/parks, opening/ending timeline hides, post-
/// battle staging - legitimately differs. Where possible each note cites a
/// sibling capture proving the fresh-entry arrangement (e.g. the rikuroa
/// `pre_caruban` state pins retail's own entry seats byte-equal to the
/// engine's).
const KNOWN_DIVERGENCES: &[KnownDivergence] = &[
    KnownDivergence {
        label: "rim_elm_zoom_intro",
        key: "vis:*",
        class: "b",
        note: "mid-opening-timeline capture: the fly-in choreography still hides these \
               villagers; the engine's free-roam entry seats them exactly where the s3/s4 \
               free-roam captures show retail seats them",
    },
    KnownDivergence {
        label: "vahn_walks_out",
        key: "vis:*",
        class: "b",
        note: "same mid-opening-timeline park cohort as rim_elm_zoom_intro",
    },
    KnownDivergence {
        label: "octam_to_sebucus_worldmap",
        key: "vis:30",
        class: "a-reported",
        note: "engine pre-run slice ends after the seat arm's no-mask 4C-70 wall paint \
               (engine-vm menu_ctrl nibble-7 sub-0/1 returns Yield); retail FUN_8003A1E4 \
               breaks only on an executed 0x21 NOP and reaches the `23 2A 70` seat -> \
               (5440,14400). Flag gate 0x1D4 evaluates identically in both. Field-VM slice \
               model fix - reported to the stepping lane",
    },
    KnownDivergence {
        label: "ending_vignette_fullscreen",
        key: "vis:*",
        class: "b",
        note: "mid-ending-choreography capture (three of the walkers are mid-step); the \
               vignette timeline seats these actors, a cold entry parks them",
    },
    KnownDivergence {
        label: "ending_panel_corner",
        key: "vis:*",
        class: "b",
        note: "same ending-vignette choreography seats, later beat",
    },
    KnownDivergence {
        label: "ending_vignette_biron",
        key: "vis:*",
        class: "b",
        note: "Biron-monastery ending vignette choreography seats",
    },
    KnownDivergence {
        label: "s1_newgame_field",
        key: "pos:7",
        class: "b",
        note: "opdeene opening-timeline actor captured mid-beat away from its entry seat",
    },
    KnownDivergence {
        label: "s1_newgame_field",
        key: "flag:*",
        class: "b",
        note: "opening-latch timing skew, reported to the timeline lane: the engine's \
               opdeene entry latches 414/1320 and clears 418 during scene entry, retail \
               performs the same writes a few beats into the opening timeline (the s3 \
               free-roam capture holds the same end-state: 414 set, 418 clear)",
    },
    KnownDivergence {
        label: "rikuroa_post_caruban",
        key: "vis:*",
        class: "b",
        note: "post-Caruban staging keeps the rescue-party actors placed for the rest of \
               the visit; with 0x142 latched both retail's and the engine's spawn prologue \
               stop at the 0x142 arm's 0x21 NOP with the actor at its (127,127) header \
               park, so a fresh re-entry parks them in retail too",
    },
    KnownDivergence {
        label: "rikuroa_post_genesis_tree",
        key: "vis:4",
        class: "b",
        note: "a mid-visit beat parked this cutscene-only actor; its authored header tile \
               is (0,0) and retail's own fresh-entry arrangement stands it at (64,64) - \
               the pre_caruban capture shows the retail node exactly there",
    },
    KnownDivergence {
        label: "dolk2_market_noa",
        key: "vis:*",
        class: "b",
        note: "market-crowd beat swap: a mid-visit choreography record parks the day \
               cohort (their prologues seat them at market tiles, then the beat hides \
               them) and seats the crowd cohort 53..60 (bare idle-loop prologues, \
               (127,127) header parks). The capture's bank still has the P1[2] spawn \
               latch 0x2FE clear, so the swap ran through another path; a fresh-entry \
               dolk2 capture would close the remaining question",
    },
    KnownDivergence {
        label: "dolk2_market_noa",
        key: "pos:2",
        class: "b",
        note: "walker captured mid-route 896 units off its prologue seat (both placed)",
    },
    KnownDivergence {
        label: "chapter2_garmel_pre_zeto",
        key: "vis:3",
        class: "c",
        note: "pre-Zeto staging places P1[3]/P1[4] next to the player; cold entry parks \
               them. Needs the garmel staging record traced (or a fresh-entry garmel \
               capture) to decide whether a beat or the prologue seats them",
    },
    KnownDivergence {
        label: "chapter2_garmel_pre_zeto",
        key: "vis:4",
        class: "c",
        note: "sibling of vis:3 - same staging cohort",
    },
];

fn known(label: &str, key: &str) -> Option<&'static KnownDivergence> {
    KNOWN_DIVERGENCES.iter().find(|k| {
        k.label == label
            && match k.key.strip_suffix('*') {
                Some(prefix) => key.starts_with(prefix),
                None => k.key == key,
            }
    })
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn first_existing(cands: &[&str]) -> Option<PathBuf> {
    cands.iter().map(PathBuf::from).find(|p| p.exists())
}

fn u32_at(ram: &[u8], va: u32) -> u32 {
    let o = (va & RAM_MASK) as usize;
    u32::from_le_bytes(ram[o..o + 4].try_into().unwrap())
}

fn u16_at(ram: &[u8], va: u32) -> u16 {
    let o = (va & RAM_MASK) as usize;
    u16::from_le_bytes(ram[o..o + 2].try_into().unwrap())
}

fn i16_at(ram: &[u8], va: u32) -> i16 {
    u16_at(ram, va) as i16
}

fn u8_at(ram: &[u8], va: u32) -> u8 {
    ram[(va & RAM_MASK) as usize]
}

/// Both coordinates in the far-corner park region (`tile 0x7F` = world
/// `16256..=16320`): the actor is off-field, not a visible placement.
fn is_parked(x: i16, z: i16) -> bool {
    x >= 0x3F00 && z >= 0x3F00
}

/// One retail field-actor node lifted from a capture's RAM.
#[derive(Debug, Clone, Copy)]
struct RetailActor {
    /// `actor[+0x50]`: flat MAN record index (`N0 + placement` for
    /// partition-1 NPCs).
    id50: u8,
    x: i16,
    z: i16,
    /// `actor[+0x26]` render heading, 12-bit, retail `0` = Z-.
    heading: u16,
    /// `actor[+0x10]` flags word (`0x100` script-engaged, `0x400`
    /// motion-VM walking, `0x80000` move-disabled).
    flags: u32,
}

/// Everything the oracle needs from one capture.
struct RetailCapture {
    label: String,
    scene: String,
    game_mode: u8,
    actors: Vec<RetailActor>,
    flag_bank: Vec<u8>,
    player_pos: Option<(i16, i16)>,
}

fn scene_name_from_ram(ram: &[u8]) -> String {
    let mut s = String::new();
    for i in 0..8 {
        let b = u8_at(ram, SCENE_NAME_VA + i);
        if !(0x20..0x7f).contains(&b) {
            break;
        }
        s.push(b as char);
    }
    s
}

fn capture_from_ram(label: &str, ram: &[u8]) -> RetailCapture {
    let head = u32_at(ram, ACTOR_LIST_HEAD_VA);
    let mut actors = Vec::new();
    if (head & 0xFFE0_0000) == 0x8000_0000 {
        let mut node = u32_at(ram, head);
        let mut hops = 0;
        while node != 0 && node != head && (node & 0xFFE0_0000) == 0x8000_0000 && hops < 192 {
            actors.push(RetailActor {
                id50: u8_at(ram, node + 0x50),
                x: i16_at(ram, node + 0x14),
                z: i16_at(ram, node + 0x18),
                heading: u16_at(ram, node + 0x26) & 0xFFF,
                flags: u32_at(ram, node + 0x10),
            });
            node = u32_at(ram, node);
            hops += 1;
        }
    }
    let bank_off = (FLAG_BANK_VA & RAM_MASK) as usize;
    let player = {
        let p = u32_at(ram, 0x8007_C364);
        ((p & 0xFFE0_0000) == 0x8000_0000).then(|| (i16_at(ram, p + 0x14), i16_at(ram, p + 0x18)))
    };
    RetailCapture {
        label: label.to_string(),
        scene: scene_name_from_ram(ram),
        game_mode: u8_at(ram, GAME_MODE_VA),
        actors,
        flag_bank: ram[bank_off..bank_off + FLAG_BANK_LEN].to_vec(),
        player_pos: player,
    }
}

/// Load a library backup's main RAM, whichever emulator captured it.
fn load_backup_ram(path: &Path) -> Option<Vec<u8>> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("sstate") => {
            let st = legaia_pcsxr::SaveState::from_path(path).ok()?;
            Some(st.main_ram().to_vec())
        }
        _ => {
            let st = legaia_mednafen::SaveState::from_path(path).ok()?;
            let ram = st.main_ram().ok()?;
            Some(ram.to_vec())
        }
    }
}

/// A single comparison finding.
struct Divergence {
    label: String,
    scene: String,
    key: String,
    detail: String,
}

#[allow(clippy::too_many_lines)]
#[test]
fn catalogued_field_states_match_retail_npc_and_flag_state() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Some(manifest_path) = first_existing(&[
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ]) else {
        eprintln!("[skip] scenarios manifest missing");
        return;
    };
    let Some(library) =
        first_existing(&["saves/library", "../saves/library", "../../saves/library"])
    else {
        eprintln!("[skip] saves library missing");
        return;
    };
    // The PCSX-Redux RAM locator needs the SCUS anchor bytes.
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: single-threaded test setup before any save load.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");
    let verbose = std::env::var_os("LEGAIA_NPC_PARITY_VERBOSE").is_some();

    // ---- Enumerate: every field-phase scenario with a resolvable backup. ----
    let mut captures: Vec<RetailCapture> = Vec::new();
    for scn in &manifest.scenarios {
        if scn.phase.as_deref() != Some("field") {
            continue;
        }
        let Some(fp) = scn.backup_fingerprint.as_deref() else {
            continue;
        };
        let Some(path) = library_backup_for("mednafen", &library, fp)
            .or_else(|| library_backup_for("pcsx-redux", &library, fp))
        else {
            continue;
        };
        let Some(ram) = load_backup_ram(&path) else {
            eprintln!("[warn] {}: backup {} unreadable", scn.label, path.display());
            continue;
        };
        let cap = capture_from_ram(&scn.label, &ram);
        // Field free-roam states only: other modes (scene-load gaps, ending
        // vignette machinery) run a different actor arrangement.
        if cap.game_mode != 0x03 {
            if verbose {
                eprintln!(
                    "[note] {}: game_mode {:#x} != 0x03 field - skipped",
                    cap.label, cap.game_mode
                );
            }
            continue;
        }
        if cap.scene.is_empty() {
            eprintln!("[warn] {}: no readable scene name - skipped", cap.label);
            continue;
        }
        // The kingdom overworlds run the world-map entity SM
        // (`FUN_801DA51C`), not the field NPC placement model.
        if is_world_map_scene(&cap.scene) {
            if verbose {
                eprintln!(
                    "[note] {}: worldmap scene {} - skipped",
                    cap.label, cap.scene
                );
            }
            continue;
        }
        if let Some(exp) = scn.expected_active_scene.as_deref()
            && exp != cap.scene
        {
            eprintln!(
                "[warn] {}: manifest expects scene {exp}, RAM says {} - using RAM",
                cap.label, cap.scene
            );
        }
        captures.push(cap);
    }
    if captures.is_empty() {
        eprintln!("[skip] no field-mode library captures resolvable");
        return;
    }

    let locality = legaia_engine_core::man_field_scripts::NPC_ROUTE_LOCALITY;
    let mut divergences: Vec<Divergence> = Vec::new();
    let mut tolerated = 0usize;
    let mut states_compared = 0usize;
    let mut total_slots = 0usize;
    let mut total_parked_both = 0usize;
    let mut heading_checked = 0usize;
    let mut heading_drift = 0usize;

    for cap in &captures {
        // ---- Engine side: seed the capture's flag bank, cold-enter. ----
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
        host.world.system_flags = cap.flag_bank.clone();
        if let Err(err) = host.enter_field_scene(&cap.scene, 0) {
            divergences.push(Divergence {
                label: cap.label.clone(),
                scene: cap.scene.clone(),
                key: "enter".into(),
                detail: format!("enter_field_scene failed: {err:#}"),
            });
            continue;
        }
        states_compared += 1;

        let scene = host.scene.as_ref().expect("scene loaded");
        let placements = match scene.field_actor_placements(&host.index) {
            Ok(Some(p)) => p,
            _ => {
                if verbose {
                    eprintln!("[note] {} ({}): no MAN placements", cap.label, cap.scene);
                }
                Vec::new()
            }
        };
        let n0 = scene
            .field_man_payload(&host.index)
            .ok()
            .flatten()
            .and_then(|man| {
                legaia_asset::man_section::parse(&man)
                    .ok()
                    .map(|mf| mf.header.partition_counts[0].max(0) as u8)
            })
            .unwrap_or(0);

        let mut slots = 0usize;
        let mut parked_both = 0usize;
        for ra in &cap.actors {
            if ra.id50 < n0 {
                continue; // partition-0 object actors / specials
            }
            let pi = ra.id50 - n0;
            let Some(p) = placements.iter().find(|p| p.index == pi as usize) else {
                continue;
            };
            let (ex, ez) = host
                .world
                .field_npc_positions
                .get(&pi)
                .copied()
                .unwrap_or((p.world_x, p.world_z));
            slots += 1;
            let (rp, ep) = (is_parked(ra.x, ra.z), is_parked(ex, ez));
            if rp && ep {
                parked_both += 1;
                continue;
            }
            if rp != ep {
                let key = format!("vis:{pi}");
                let detail = format!(
                    "retail {} at ({},{}), engine {} at ({ex},{ez}) [rflags {:#x}]",
                    if rp { "PARKED" } else { "placed" },
                    ra.x,
                    ra.z,
                    if ep { "PARKED" } else { "placed" },
                    ra.flags
                );
                match known(&cap.label, &key) {
                    Some(k) => {
                        tolerated += 1;
                        eprintln!(
                            "[known-{}] {} ({}) {key}: {detail} - {}",
                            k.class, cap.label, cap.scene, k.note
                        );
                    }
                    None => divergences.push(Divergence {
                        label: cap.label.clone(),
                        scene: cap.scene.clone(),
                        key,
                        detail,
                    }),
                }
                continue;
            }
            let (dx, dz) = (
                (ex as i32 - ra.x as i32).abs(),
                (ez as i32 - ra.z as i32).abs(),
            );
            if dx.max(dz) > locality {
                let key = format!("pos:{pi}");
                let detail = format!(
                    "engine seat ({ex},{ez}) is {dx}/{dz} units from retail ({},{})",
                    ra.x, ra.z
                );
                match known(&cap.label, &key) {
                    Some(k) => {
                        tolerated += 1;
                        eprintln!(
                            "[known-{}] {} ({}) {key}: {detail} - {}",
                            k.class, cap.label, cap.scene, k.note
                        );
                    }
                    None => divergences.push(Divergence {
                        label: cap.label.clone(),
                        scene: cap.scene.clone(),
                        key,
                        detail,
                    }),
                }
                continue;
            }
            // Heading: diagnostic only. Engine convention 0 = Z+, retail
            // 0 = Z- (engine = retail + 0x800 mod 0x1000); a walker keeps its
            // last travel direction, so drift is expected dynamics.
            if let Some(&eh) = host.world.field_npc_headings.get(&pi) {
                heading_checked += 1;
                let expected = ((ra.heading + 0x800) & 0xFFF) as i16;
                if (eh & 0xFFF) != expected {
                    heading_drift += 1;
                    if verbose {
                        eprintln!(
                            "[hdg] {} ({}) pi {pi}: engine {:#x}, retail {:#x} (engine-frame {:#x}) [rflags {:#x}]",
                            cap.label,
                            cap.scene,
                            eh & 0xFFF,
                            ra.heading,
                            expected,
                            ra.flags
                        );
                    }
                }
            }
        }
        total_slots += slots;
        total_parked_both += parked_both;

        // ---- Story-flag bank: engine entry must be a no-op vs the capture. ----
        let bank = &host.world.system_flags;
        for (byte, (&e, &r)) in bank.iter().zip(cap.flag_bank.iter()).enumerate() {
            if e == r {
                continue;
            }
            for bit in 0..8 {
                let mask = 0x80u8 >> bit;
                if (e ^ r) & mask != 0 {
                    let idx = byte * 8 + bit;
                    let key = format!("flag:{idx}");
                    let detail = format!(
                        "engine {} flag {idx} ({:#x}) during entry; retail bank holds {}",
                        if e & mask != 0 { "SET" } else { "CLEARED" },
                        idx,
                        (r & mask != 0) as u8
                    );
                    match known(&cap.label, &key) {
                        Some(k) => {
                            tolerated += 1;
                            eprintln!(
                                "[known-{}] {} ({}) {key}: {detail} - {}",
                                k.class, cap.label, cap.scene, k.note
                            );
                        }
                        None => divergences.push(Divergence {
                            label: cap.label.clone(),
                            scene: cap.scene.clone(),
                            key,
                            detail,
                        }),
                    }
                }
            }
        }

        let walking = cap.actors.iter().filter(|a| a.flags & 0x400 != 0).count();
        let engaged = cap.actors.iter().filter(|a| a.flags & 0x100 != 0).count();
        eprintln!(
            "[state] {} ({}): {} retail nodes, {slots} placement slots ({parked_both} parked in \
             both), {walking} walking, {engaged} engaged, player {:?}",
            cap.label,
            cap.scene,
            cap.actors.len(),
            cap.player_pos
        );
    }

    eprintln!(
        "[sweep] {states_compared} field states compared, {total_slots} placement slots \
         ({total_parked_both} parked in both), heading checked {heading_checked} \
         (drift {heading_drift}), {tolerated} known divergences tolerated, {} open",
        divergences.len()
    );
    for d in &divergences {
        eprintln!(
            "  DIVERGENCE {} ({}) {}: {}",
            d.label, d.scene, d.key, d.detail
        );
    }

    // Non-vacuity: the sweep really covered a broad corpus.
    assert!(
        states_compared >= 5,
        "expected >=5 comparable field states, got {states_compared}"
    );
    assert!(
        total_slots >= 100,
        "expected >=100 compared placement slots, got {total_slots}"
    );
    assert!(
        divergences.is_empty(),
        "{} unclassified retail-vs-engine divergence(s) - see the log",
        divergences.len()
    );
}
