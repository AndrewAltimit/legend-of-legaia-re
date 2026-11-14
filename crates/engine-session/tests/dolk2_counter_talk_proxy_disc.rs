//! A shop counter hands the action button across to its keeper.
//!
//! `dolk2`'s market keepers (P1[11] arms, P1[12] items, P1[13] the inn)
//! stand a tile behind their counters, out of the facing probe's reach from
//! the front (64 units ahead, a 72-unit box). The counters are placed
//! objects of the interact class (`31 1E`), bound to P0[26..=28], and each
//! bind record carries one op, `B1 <keeper> 08`: the touched mark the touch
//! post `FUN_801D5B5C` raises, on which the context runner `FUN_80039B7C`
//! steps the keeper's record. The facing probe meets the counter's own
//! 80-unit box, so retail opens the shop from in front of the counter. The
//! port stepped the mark on the counter's context and opened nothing.
//!
//! Disc-gated: skips without `LEGAIA_DISC_BIN` or an extracted tree.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The arms dealer, P1[11], behind the counter on tile `(73, 84)`.
const ARMS_DEALER: u8 = 11;

#[test]
fn pressing_at_the_counter_talks_to_the_keeper() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "dolk2".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open session");
    s.host.world.toggles.use_vm_dialogue = true;
    for f in [0x142u16, 0x178, 0x179, 0x26A, 0x27C, 0x27D] {
        s.host.world.system_flag_set(f);
    }
    s.enter_field_live("dolk2", &FieldLiveOpts::default())
        .expect("enter dolk2");
    s.set_host_drains_queues(true);
    for _ in 0..10 {
        s.tick().expect("tick");
    }
    // In front of the counter (centre `(9408, 10816)`, the keeper at
    // `(9408, 10944)`), where its box stops the walk, facing Z+.
    {
        let w = &mut s.host.world;
        let slot = usize::from(w.player_actor_slot.expect("player"));
        let ms = &mut w.actors[slot].move_state;
        ms.world_x = 9408;
        ms.world_z = 10674;
        ms.render_26 = 0;
        assert_eq!(
            w.field_interact_probe_slot(),
            None,
            "the keeper is out of reach"
        );
        assert_eq!(
            w.field_interact_prop_anchor(),
            Some((73, 84)),
            "the counter is in reach"
        );
    }
    let mut talked = false;
    for t in 0..30u32 {
        let pad = if t == 2 { PadButton::Cross.mask() } else { 0 };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        let w = &s.host.world;
        if w.dialog.last_field_interact == Some((0, ARMS_DEALER))
            && w.dialog
                .inline
                .as_ref()
                .is_some_and(|id| id.prop_anchor.is_none())
        {
            talked = true;
            break;
        }
    }
    assert!(
        talked,
        "the press at the counter opens the arms dealer's talk"
    );
}
