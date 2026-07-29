//! Disc-gated: a mid-scene BGM change must reach the director's **playback
//! start** on the frame the field VM asks for it - with no intervening scene
//! entry.
//!
//! Field-VM op `0x35` sub-op `9` is the op a cutscene uses to change music
//! part-way through a scene. Retail's arm (field overlay `0x801E0224`) is a
//! *load barrier followed by a track select*:
//!
//! ```text
//! 801e022c  lw a0,-0x4548(v0)       ; a0 = *0x8007BAB8  (resolved index)
//! 801e0230  lw v0,-0x4564(v1)       ; v0 = *0x8007BA9C  (loaded index)
//! 801e0238  bne a0,v0,0x801dee4c    ; not settled -> `move s8,s4` = re-run this PC
//! 801e0240  jal 0x8003ce9c          ; read the u16 operand
//! 801e0254  sw v0,-0x4538(a1)       ; *0x8007BAC8 = id   <-- sub-op 1's own store
//! ```
//!
//! The store at `0x801E0254` is byte-for-byte the one sub-op 1 makes, so
//! sub-op 9 **starts** the track; the only extra is that it stalls the script
//! until the previously requested asset has landed. The port resolves BGM
//! bytes synchronously, so that barrier is always satisfied and sub-op 9 is
//! plain start.
//!
//! Reading it as "queue for the next scene entry" is what this test pins
//! against: with that reading the whole Biron Monastery (`bylon`) cutscene
//! score is inaudible in its own scene and then blares over whatever scene
//! the player walks into next.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing - CI runs
//! without disc data.

use std::path::PathBuf;

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::scene::{BgmDirector, SceneHost};

/// The scene whose cutscenes drove this: Biron Monastery. Its partition-2
/// cutscene records change music with sub-op 9 more than any other scene's.
const SCENE: &str = "bylon";

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let extracted = extracted_dir()?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.load_scene(SCENE).expect("load scene");
    Some(host)
}

/// Records only the hooks that put sound in the room. Anything routed
/// somewhere else - a deferred slot, a silent drop - leaves `started` empty,
/// which is the failure this file exists to catch. The existing corpus sweep
/// folds start and queue together, which is precisely why a deferred start
/// read there as a healthy one.
#[derive(Default)]
struct SplitDirector {
    started: Vec<u16>,
}

impl BgmDirector for SplitDirector {
    fn start(&mut self, bgm_id: u16, _seq: &[u8]) {
        self.started.push(bgm_id);
    }
    fn start_owned_vab(&mut self, bgm_id: u16, _entry: &[u8]) {
        self.started.push(bgm_id);
    }
}

/// Every `Bgm { sub_op }` op the scene's partition-2 cutscene timelines
/// carry, decoded with the opcode-aware walker (a raw byte scan would count
/// dialog text as instructions).
fn cutscene_bgm_ops(host: &SceneHost) -> Vec<(u16, u8)> {
    use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
    use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};

    let scene = host.scene.as_ref().expect("scene loaded");
    let carriers = scene_man_carriers(&host.index, scene);
    let Some(carrier) = carriers.first() else {
        return Vec::new();
    };
    let man = carrier.payload.clone();
    let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
        return Vec::new();
    };
    let n2 = *man_file.header.partition_counts.get(2).unwrap_or(&0) as usize;
    let mut out = Vec::new();
    for record in 0..n2 {
        let Some((start, pc0, len)) = partition_record_span(&man_file, &man, 2, record) else {
            continue;
        };
        let Some(body) = man.get(start..start + len) else {
            continue;
        };
        for insn in LinearWalker::new(body, pc0).flatten() {
            if let InsnInfo::Bgm { text_id, sub_op } = insn.info {
                out.push((text_id, sub_op));
            }
        }
    }
    out
}

/// The op the Maya cutscene changes music with reaches `start`, on the same
/// frame, with no scene entry in between - and the ordinary scene-entry
/// sub-op 1 still does too (the non-vacuity contrast).
#[test]
fn midscene_bgm_change_starts_playback_without_a_scene_entry() {
    let Some(mut host) = open_host() else {
        return;
    };

    // Disc-anchored: take the ids from the scene's own cutscene scripts
    // rather than from a literal, so the test cannot pass against a corpus
    // that no longer contains the op.
    let ops = cutscene_bgm_ops(&host);
    let mut sub9: Vec<u16> = ops
        .iter()
        .filter(|&&(id, sub)| sub == 9 && id >= 2000)
        .map(|&(id, _)| id)
        .collect();
    sub9.sort_unstable();
    sub9.dedup();
    assert!(
        !sub9.is_empty(),
        "scene '{SCENE}' carries no global-pool op-0x35 sub-op 9 - the test lost its subject \
         (decoded {} BGM ops total)",
        ops.len(),
    );

    for id in &sub9 {
        let mut d = SplitDirector::default();
        host.world.pending_field_events.push(FieldEvent::Bgm {
            text_id: *id,
            sub_op: 9,
        });
        let acted = host.route_bgm_events(&mut d).expect("route");
        assert_eq!(
            d.started,
            vec![*id],
            "sub-op 9 track {id} did not start on the frame it was requested \
             (acted={acted}) - a mid-scene music change is a start, not a queue \
             for the next scene",
        );
        // Nothing may be left on the queue for a later trigger to pick up.
        assert!(
            !host
                .world
                .pending_field_events
                .iter()
                .any(|e| matches!(e, FieldEvent::Bgm { sub_op: 9, .. })),
            "sub-op 9 event survived routing",
        );
    }

    // Contrast: sub-op 1 (the scene-entry start) behaves the same way, so a
    // pass above is not "every id starts because routing is a no-op".
    let id = sub9[0];
    let mut d = SplitDirector::default();
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: id,
        sub_op: 1,
    });
    host.route_bgm_events(&mut d).expect("route");
    assert_eq!(d.started, vec![id], "sub-op 1 start regressed");

    eprintln!(
        "[ok] {SCENE}: {} sub-op-9 tracks start immediately",
        sub9.len()
    );
}
