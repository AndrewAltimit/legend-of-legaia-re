//! Disc-gated: the raisers of the system flags the field VM's `4C C5` /
//! `4C C6` waits hold on.
//!
//! `4C C5 <idx>` holds a record until system flag `idx` is set, `4C C6` until
//! it is clear. Every shipped wait's flag is flipped by a **move-VM part** the
//! scene's own stager table carries - ext sub-op `0x1C` (`2F 1C <idx>`, set)
//! or `0x1D` (`2F 1D <idx>`, clear), the arms of `FUN_801D362C` at
//! `0x801D3DF4` / `0x801D3E08` - which the record spawns with op `0x34`
//! sub-3 before it waits. Three pins:
//!
//! - every bundle-MAN wait has its raiser in its scene's stager table: a
//!   `2F 1C <idx>` for a set-wait, a `2F 1D <idx>` for a clear-wait;
//! - `town01` P2[25]'s `B4 29 30 09` stages record 10, and that part's move
//!   program raises `0x0A` - the flag P2[25]'s `4C C5 0A 00` holds on, and
//!   which no field script on the disc sets;
//! - every partition-2 wait a cold-entered cutscene timeline reaches is
//!   passed when the record runs under the headless frame tail
//!   (`SceneHost::step_headless_frame_tail`), the step that ticks the parts.
//!
//! Skip-pass when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_core::world::World;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::{OpKey, clean_hit_offsets, man_script_spans};

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Whether `stagers` carries the move-VM ext op `2F <sub> <idx>` at a
/// halfword boundary (the move VM's operands are u16).
fn stager_carries(stagers: &[u8], sub: u16, idx: u16) -> bool {
    let want: Vec<u8> = [0x2Fu16, sub, idx]
        .iter()
        .flat_map(|h| h.to_le_bytes())
        .collect();
    stagers
        .windows(want.len())
        .enumerate()
        .any(|(i, w)| i % 2 == 0 && w == want.as_slice())
}

#[test]
fn every_system_flag_wait_has_a_stager_raiser_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    let mut names = index.cdname_scene_names();
    names.sort();
    names.dedup();
    let mut waits = 0usize;
    let mut missing = Vec::new();
    for name in names {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
            continue;
        };
        let stagers = scene
            .find_event_scripts()
            .map(|s| s.bytes.to_vec())
            .unwrap_or_default();
        for (partition, record, start, pc0, len) in man_script_spans(&man_file, &man) {
            let body = &man[start..start + len];
            for (sub, raiser) in [(0xC5u8, 0x1Cu16), (0xC6, 0x1D)] {
                let key = OpKey {
                    opcode: 0x4C,
                    sub: Some(sub),
                };
                for pc in clean_hit_offsets(body, pc0, key) {
                    let idx = u16::from_le_bytes([body[pc + 2], body[pc + 3]]);
                    waits += 1;
                    if !stager_carries(&stagers, raiser, idx) {
                        missing.push(format!(
                            "{name} P{partition}[{record}] @ {pc:#06x}: 4C {sub:02X} {idx:#x}"
                        ));
                    }
                }
            }
        }
    }
    assert!(waits >= 20, "found only {waits} system-flag waits");
    assert!(
        missing.is_empty(),
        "waits with no stager raiser: {missing:#?}"
    );
}

#[test]
fn town01_record_10_raises_flag_0x0a_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    let scene = Scene::load(&index, "town01").expect("load town01");
    let stagers = scene
        .find_event_scripts()
        .expect("town01 event scripts")
        .bytes
        .to_vec();
    let mut world = World {
        clock: legaia_engine_vm::frame_clock::FrameClock {
            frame_step: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    world.install_field_stagers(&stagers);
    // P2[25] clears the flag before it fires the animation.
    world.system_flag_clear(0x0A);
    // `B4 29 30 09`: op 0x34 sub-3 stages record `arg + 1`.
    assert!(world.spawn_ambient_record(10, [0, 0, 0]));
    assert!(
        !world.system_flag_test(0x0A),
        "the record waits 0x1E before it raises the flag"
    );
    let mut ticks = 0;
    while !world.system_flag_test(0x0A) && ticks < 120 {
        world.tick_ambient_fx();
        ticks += 1;
    }
    assert!(
        world.system_flag_test(0x0A),
        "record 10's `2F 1C 0A` never ran in {ticks} ticks"
    );
    // The part then spins on `2F 13 0A` (branch back while set) until the
    // script's own `60 0A` clears the flag, and runs on past it.
    let live = world.ambient.fx.iter().filter(|p| !p.finished).count();
    world.system_flag_clear(0x0A);
    for _ in 0..8 {
        world.tick_ambient_fx();
    }
    assert!(!world.system_flag_test(0x0A));
    assert_ne!(
        world.ambient.fx.iter().filter(|p| !p.finished).count(),
        live,
        "clearing 0x0A releases the part's spin (it spawns its tail child)"
    );
}

/// Every partition-2 wait is **released at runtime** on the path a host
/// runs it: enter the scene, install the record as the cutscene timeline,
/// and tick it the way a headless driver does - [`SceneHost::tick`] then
/// [`SceneHost::step_headless_frame_tail`], whose VRAM step ticks the
/// ambient move-VM parts the record stages - with confirm toggling for its
/// dialogue. A wait the timeline reaches must be passed, never parked on
/// until the record ends: `garmel` P2[12] parks on `4C C6 0A 00` until the
/// part `B4 1D 30 01` staged clears `0x0A`, and a driver without the tail
/// held it there for good.
///
/// A record that never reaches its wait from a cold scene entry (it needs
/// story state, or leaves the scene first) is reported and not counted.
/// Partition-1 waits are talk scripts run by the inline-dialogue runner,
/// not the timeline; they are left to the census above.
#[test]
fn every_reached_p2_system_flag_wait_is_released_or_skip() {
    use legaia_engine_scene_host::scene_host::SceneHost;
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    let mut names = index.cdname_scene_names();
    names.sort();
    names.dedup();
    let cross = legaia_engine_system::input::PadButton::Cross.mask();
    let mut passed = 0usize;
    let mut stuck = Vec::new();
    for name in names {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
            continue;
        };
        for (partition, record, start, pc0, len) in man_script_spans(&man_file, &man) {
            if partition != 2 {
                continue;
            }
            let body = &man[start..start + len];
            let mut waits: Vec<usize> = [0xC5u8, 0xC6]
                .into_iter()
                .flat_map(|sub| {
                    clean_hit_offsets(
                        body,
                        pc0,
                        OpKey {
                            opcode: 0x4C,
                            sub: Some(sub),
                        },
                    )
                })
                .collect();
            if waits.is_empty() {
                continue;
            }
            waits.sort_unstable();
            let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
            if host.enter_field_scene(&name, 0).is_err() {
                continue;
            }
            assert!(
                host.world
                    .install_cutscene_timeline_record(&man_file, &man, 2, record, false),
                "{name} P2[{record}] installs"
            );
            let mut last_pc = pc0;
            let mut max_pc = 0usize;
            let mut ticks = 0u32;
            while ticks < 12_000 {
                host.world
                    .input
                    .set_pad(if ticks.is_multiple_of(2) { cross } else { 0 });
                if host.tick().is_err() {
                    break;
                }
                let _ = host.step_headless_frame_tail();
                ticks += 1;
                let Some(pc) = host
                    .world
                    .cutscene
                    .timeline
                    .as_ref()
                    .filter(|tl| !tl.done)
                    .map(|tl| tl.pc)
                else {
                    break;
                };
                last_pc = pc;
                max_pc = max_pc.max(pc);
                if waits.iter().all(|&w| max_pc > w) {
                    break;
                }
            }
            for &w in &waits {
                let what = if max_pc > w {
                    passed += 1;
                    "released"
                } else if last_pc == w {
                    stuck.push(format!("{name} P2[{record}] @ {w:#06x} ({ticks} ticks)"));
                    "HELD"
                } else {
                    "not reached"
                };
                eprintln!(
                    "{name} P2[{record}] @ {w:#06x}: {what} (last pc {last_pc:#06x}, {ticks} ticks)"
                );
            }
        }
    }
    assert!(passed >= 5, "only {passed} waits reached and released");
    assert!(
        stuck.is_empty(),
        "timeline waits held until the record ended: {stuck:#?}"
    );
}
