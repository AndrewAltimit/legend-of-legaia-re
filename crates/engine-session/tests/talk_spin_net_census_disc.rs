//! Disc-gated executing census: every placement talk whose record waits on
//! an actor's or a clip's signal (`2D` / `AD <id> <bit>`, the `+0x62` spin)
//! is run through the inline dialogue runner on a live session, and every
//! wait that runs into the runner's spin net (`INLINE_SPIN_PARK_TIMEOUT`)
//! is listed.
//!
//! Retail has no net: the dispatcher re-runs the test every frame until the
//! bit lands. A wait that reaches the port's net is a signal retail raises
//! and the port does not - `ropeway` P1[45]'s morph waits on placement `42`
//! were one (`ropeway_ruins_report_disc.rs`). The talk is driven with a
//! confirm press every eight frames, so a picker takes its first answer and
//! only that branch is walked.
//!
//! `LEGAIA_TALK_SPIN_SCENES=a,b` narrows the sweep and
//! `LEGAIA_TALK_SPIN_FLAGS=1C9,..` raises system flags before scene entry, to
//! reach a talk its story gate parks. A placement parked on the hide tile is
//! skipped. Report-only by default; `LEGAIA_TALK_SPIN_STRICT=1` fails on any hit
//! outside [`KNOWN`].
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::{Path, PathBuf};

use legaia_engine_field::inline_dialogue::INLINE_SPIN_PARK_TIMEOUT;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, decode, partition_record_span};
use legaia_scene_formats::man_section;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Talks that still reach the net at the default flags, as
/// `(scene, placement, record pc)`.
const KNOWN: &[(&str, u8, usize)] = &[];

fn scene_names(extracted: &Path) -> Vec<String> {
    let Ok(cdname) = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();
    names
}

/// A comma-separated environment list (empty when unset).
fn env_list(key: &str) -> Vec<String> {
    std::env::var(key)
        .map(|v| {
            v.split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn open(extracted: &Path, scene: &str) -> Option<NativeSession> {
    let cfg = BootConfig {
        scene: scene.into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(extracted, &cfg).ok()?;
    for f in env_list("LEGAIA_TALK_SPIN_FLAGS") {
        if let Ok(f) = u16::from_str_radix(f.trim_start_matches("0x"), 16) {
            s.host.world.system_flag_set(f);
        }
    }
    s.host.world.toggles.use_vm_dialogue = true;
    s.enter_field_live(scene, &FieldLiveOpts::default()).ok()?;
    // The hosts bind every placement's clip cursor at entry; a talk's
    // `2D 08` waits on the talker's own clip.
    let _ = legaia_engine_session::scene_actors::bind_scene_actors(&mut s.host);
    s.set_host_drains_queues(true);
    for _ in 0..20 {
        s.tick().ok()?;
    }
    Some(s)
}

/// Partition-1 placements whose record decodes an `LFlag.Test` (op `0x2D`,
/// either form) anywhere in a linear walk.
fn spin_talks(s: &NativeSession) -> Vec<u8> {
    let Some(man) = s.host.world.field_vm.channels_man.clone() else {
        return Vec::new();
    };
    let Ok(file) = man_section::parse(&man) else {
        return Vec::new();
    };
    let count = file
        .header
        .partition_counts
        .get(1)
        .copied()
        .unwrap_or(0)
        .max(0) as usize;
    let mut out = Vec::new();
    for record in 1..count.min(256) {
        let Some((start, pc0, len)) = partition_record_span(&file, &man, 1, record) else {
            continue;
        };
        let Some(body) = man.get(start..start + len) else {
            continue;
        };
        let mut pc = pc0;
        let mut spins = false;
        while pc < body.len() {
            let Ok(insn) = decode(body, pc) else { break };
            if matches!(
                insn.info,
                InsnInfo::LFlag {
                    kind: FlagKind::Test,
                    ..
                }
            ) {
                spins = true;
                break;
            }
            pc += insn.size.max(1);
        }
        if spins {
            out.push(record as u8);
        }
    }
    out
}

#[derive(Debug, Default)]
struct Hit {
    max_park: u32,
    pc: usize,
    op: Vec<u8>,
    ended: bool,
    /// The talker's live clip id and whether the world binds a cursor to it.
    clip: (Option<u8>, bool),
}

fn run(extracted: &Path, scene: &str, slot: u8) -> Option<Hit> {
    let mut s = open(extracted, scene)?;
    let &(nx, nz) = s.host.world.npcs.positions.get(&slot)?;
    // A placement its prologue parked on the hide tile (`23 7F 7F`) is out
    // of the player's reach at these flags.
    if nx >= 0x7F * 128 || nz >= 0x7F * 128 {
        return None;
    }
    s.host
        .world
        .seat_player_at_tile((nx >> 7) as u8, ((nz >> 7) as u8).saturating_add(1));
    for _ in 0..4 {
        s.tick().ok()?;
    }
    s.host.world.trigger_field_interact(0, slot);
    let label = s.host.world.active_scene_label.clone();
    let mut hit = Hit {
        clip: (
            s.host.world.field_npc_live_anim(usize::from(slot)),
            s.host.world.npc_clip_cursor_bound(slot),
        ),
        ..Hit::default()
    };
    let mut talked = false;
    for t in 0..8000u32 {
        let busy = s.host.world.dialogue_owns_input();
        talked |= busy;
        if (talked && !busy) || s.host.world.active_scene_label != label {
            hit.ended = true;
            break;
        }
        if let Some(id) = s.host.world.dialog.inline.as_ref()
            && id.park_frames > hit.max_park
        {
            hit.max_park = id.park_frames;
            hit.pc = id.pc;
            hit.op = id.bytecode.get(id.pc..id.pc + 3).unwrap_or(&[]).to_vec();
        }
        let pad = if busy && t % 8 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().ok()?;
    }
    Some(hit)
}

#[test]
fn talk_spin_net_census() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let (mut runs, mut hits) = (0, Vec::new());
    let only = env_list("LEGAIA_TALK_SPIN_SCENES");
    for scene in scene_names(&extracted) {
        if !only.is_empty() && !only.contains(&scene) {
            continue;
        }
        let Some(s) = open(&extracted, &scene) else {
            continue;
        };
        let talks = spin_talks(&s);
        drop(s);
        for slot in talks {
            let Some(h) = run(&extracted, &scene, slot) else {
                continue;
            };
            runs += 1;
            if h.max_park >= INLINE_SPIN_PARK_TIMEOUT {
                eprintln!(
                    "NET {scene} P1[{slot}] pc=+0x{:04X} op={:02X?} ended={} clip={:?}",
                    h.pc, h.op, h.ended, h.clip
                );
                hits.push((scene.clone(), slot, h.pc));
            }
        }
    }
    eprintln!(
        "[talk-spin] {runs} talks run, {} reached the net",
        hits.len()
    );
    if only.is_empty() {
        assert!(runs > 20, "the census reached too few talks: {runs}");
    }
    if std::env::var_os("LEGAIA_TALK_SPIN_STRICT").is_some() {
        let unknown: Vec<_> = hits
            .iter()
            .filter(|(sc, sl, pc)| !KNOWN.contains(&(sc.as_str(), *sl, *pc)))
            .collect();
        assert!(
            unknown.is_empty(),
            "talks reached the spin net: {unknown:?}"
        );
    }
}
