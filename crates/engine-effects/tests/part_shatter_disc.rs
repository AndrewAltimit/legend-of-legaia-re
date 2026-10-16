//! Disc-gated: a shipped move-VM part that arms op `0x11` grows its own
//! `+0x7A` and is drawn through the shatter renderer.
//!
//! PROT 0905's arm-5 glow `0x801F8494` is the case the cast-sequence oracle
//! caught: retail holds it alive through arm 8 with `+0x7A` climbing, drawn
//! as flying shards, where a whole disc would grow over the screen. The
//! test stages the record from the disc, ticks it, and pins the chain
//! *program -> `+0x7A` -> de-indexed shard mesh*, no Sony bytes.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_engine_effects::effect_default_arm::{default_arm_draw, default_arm_vram_mesh};
use legaia_engine_effects::summon::SummonScene;
use legaia_engine_vm::move_vm::MoveHost;
use legaia_overlay_images::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_prot::archive::Archive;

struct LutHost;
impl MoveHost for LutHost {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let a = f64::from(index) * std::f64::consts::TAU / 4096.0;
        ((a.sin() * 4096.0) as i16, (a.cos() * 4096.0) as i16)
    }
}

fn prot() -> Option<PathBuf> {
    ["extracted", "../../extracted", "../extracted"]
        .iter()
        .map(|b| PathBuf::from(b).join("PROT.DAT"))
        .find(|p| p.is_file())
}

#[test]
fn vera_glow_breaks_up_through_its_own_7a() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    const GLOW: u32 = 0x801F_8494;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive.entries[905].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0905");
    let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
    let off = (GLOW - SUMMON_OVERLAY_LINK_BASE) as usize;
    let parts: Vec<_> = summon_overlay::parse_records_at(
        &bytes,
        &overlay
            .parts
            .iter()
            .map(|p| p.record_off)
            .chain([off])
            .collect::<Vec<_>>(),
    )
    .into_iter()
    .filter(|p| p.record_off == off)
    .collect();
    assert_eq!(parts.len(), 1, "the glow record parses");
    let mut scene = SummonScene::spawn_parts(&[], &bytes, 0, [0; 3]);
    scene.push_parts(&parts, &bytes, [0; 3], [0; 3]);
    let mut host = LutHost;
    let mut grew = None;
    for frame in 0..120 {
        scene.tick(&mut host, 8);
        let part = &scene.parts[0];
        assert!(!part.finished, "the glow's program holds (WAIT 4095)");
        if part.state.field_7a != 0 && grew.is_none() {
            grew = Some(frame);
        }
    }
    let first = grew.expect("op 0x11 grows +0x7A");
    eprintln!("[ok] PROT 0905 glow: +0x7A live from frame {first}");
    let state = &scene.parts[0].state;
    let mesh = default_arm_draw(state).expect("a default-arm node").mesh;
    // The shatter draws one corner per index: no vertex is shared.
    assert_eq!(mesh.positions.len(), mesh.indices.len());
    // ...two shards a quad: the shatter leaves the dispatcher, so the glow's
    // bank-3 split (sixteen triangles a quad) does not apply. With the word
    // cleared the same node is the split disc again.
    let mut whole = state.clone();
    whole.field_7a = 0;
    let disc = default_arm_vram_mesh(&whole).expect("whole disc");
    assert_eq!(disc.positions.len(), 8 * mesh.positions.len());
}
