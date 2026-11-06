//! Disc-gated: the field party readout honours the scratchpad menu lock.
//!
//! `FUN_801D0D38` suppresses the readout while `_DAT_1F800394 & 0x0800_0000`
//! is set (`0x801D0D98..0x801D0DAC`), the menu lock field-VM op `2E 1B`
//! raises (`town0e`, `urudre1`). Drives the shared per-frame HUD step both
//! play hosts run (`field_screens::field_party_hud_rearm` /
//! `field_party_hud_tick`) over a real `town01` entry, standing still, with
//! the lock bit raised as that op raises it, and asserts no frame draws while
//! it holds - and that the same scene draws once the bit is dropped.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_engine_field::world_map_panel_actors::HudDecision;
use legaia_engine_field::world_map_panel_host::FieldPartyHud;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_screens::field_screens::{field_party_hud_rearm, field_party_hud_tick};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn step(host: &mut SceneHost, hud: &mut FieldPartyHud, latch: &mut Option<String>) -> bool {
    host.world.set_pad(0);
    host.tick().expect("tick");
    field_party_hud_rearm(hud, latch, host);
    let suppressed =
        legaia_engine_world_views::world_map_panel_host::field_hud_suppressed(&host.world, false);
    field_party_hud_tick(hud, &host.world, suppressed, None);
    matches!(hud.decision(), Some(HudDecision::Draw { .. }))
}

#[test]
fn the_menu_lock_keeps_the_party_readout_off() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");
    let lock = legaia_engine_core::world::FIELD_MENU_LOCK_BIT;
    let mut hud = FieldPartyHud::new();
    let mut latch = None;
    // `2E 1B` sets the bit in the scratchpad word (`StoryFlagState::story_flags`).
    host.world.flags.story_flags |= lock;
    let mut locked_frames = 0;
    for _ in 0..300 {
        host.world.flags.story_flags |= lock;
        let drew = step(&mut host, &mut hud, &mut latch);
        locked_frames += 1;
        assert!(!drew, "the readout drew under the menu lock");
    }
    // Control: the same scene, the lock dropped, draws.
    host.world.flags.story_flags &= !lock;
    let drew = (0..200).any(|_| {
        host.world.flags.story_flags &= !lock;
        step(&mut host, &mut hud, &mut latch)
    });
    assert!(drew, "without the lock the readout comes up");
    eprintln!(
        "[ok] town01: {locked_frames} locked frames, no readout; it draws once the lock drops"
    );
}
