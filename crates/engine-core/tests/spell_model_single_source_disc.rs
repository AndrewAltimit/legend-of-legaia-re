//! Disc-gated: the battle-action state machine reads the **same** spell model
//! the live cast path does, sourced from the user's own `SCUS_942.54`.
//!
//! Two questions, one table record each. What a cast costs is the record's
//! `+3` byte, which boot folds into `World::spell_catalog`
//! (`retail_magic::seru_magic_catalog_from_scus`) and which
//! `World::cast_spell_on_slots` charges. Whether a cast is capture-class is the
//! same record's `+0` class byte, which routes both the damage-kernel pick and
//! the SM's `MagicCastBegin -> MagicCaptureBranch` branch.
//!
//! Both are asserted **through `World::tick`**, i.e. through the host the
//! engine actually installs, because the defect this guards against was a host
//! wired to a pair of tables nothing ever filled: it compiled, it was covered,
//! and it quietly made every state-machine cast free and never capture-class.
//!
//! Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::battle_action::ActionState;
use std::path::PathBuf;

fn scus() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return None;
    }
    Some(
        legaia_engine_core::DiscVfs::open(&path)
            .expect("open disc")
            .read("SCUS_942.54")
            .expect("SCUS_942.54 present"),
    )
}

/// A battle with one armed caster and both spell models installed off the disc.
fn armed_battle(scus: &[u8], spell_id: u8, mp: u16) -> World {
    let mut w = World::new();
    w.mode = SceneMode::Battle;
    w.party_count = 3;
    w.set_spell_catalog(
        legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(scus)
            .expect("SCUS parses as a PSX-EXE"),
    );
    w.install_menu_text(scus);
    for i in 0..8 {
        let a = w.spawn_actor(i);
        a.battle.liveness = 1;
        a.battle.hp = 500;
        a.battle.max_hp = 500;
        a.battle.mp = mp;
    }
    w.actors[0].battle.action_category = 2; // Magic
    w.actors[0].battle.active_target = 3;
    w.actors[0].battle.params[0] = spell_id;
    w.battle_ctx.queued_action = 2;
    w.battle_ctx.action_state = ActionState::Begin.as_byte();
    w
}

/// Tick until the SM first reaches `want`, or give up after `frames`.
///
/// Bounded on the *first* arrival deliberately: the armed action eventually
/// retires and the SM re-seeds, which would let a per-cast effect (an MP
/// debit) be counted twice by a test that just ran the clock out.
fn tick_until(w: &mut World, want: ActionState, frames: usize) -> bool {
    for _ in 0..frames {
        w.tick();
        if w.battle_ctx.action_state == want.as_byte() {
            return true;
        }
    }
    false
}

/// The MP a state-machine cast debits is the disc's `+3` byte for that spell -
/// not zero, and not a number from anywhere else.
#[test]
fn the_state_machine_charges_the_discs_own_mp_cost() {
    let Some(scus) = scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let table = legaia_asset::spell_names::SpellNameTable::from_scus(&scus)
        .expect("the retail spell table decodes");

    // Gimard - the lowest player Seru id, and the one pinned by the capture
    // save-state. Its cost comes off the disc, so a patched / localised image
    // moves the expectation with it.
    const GIMARD: u8 = 0x81;
    let disc_mp = table.mp(GIMARD).expect("Gimard has a table record") as u16;
    assert!(
        disc_mp > 0,
        "a Seru spell that costs nothing is not credible"
    );

    let mut w = armed_battle(&scus, GIMARD, 200);
    assert_eq!(
        w.spell_catalog.mp_cost(GIMARD) as u16,
        disc_mp,
        "boot folds the disc's own +3 byte into the catalog"
    );

    // `MagicPreCastWait` is the state immediately after the cast-begin debit.
    assert!(
        tick_until(&mut w, ActionState::MagicPreCastWait, 2_000),
        "the cast band must actually run"
    );
    assert_eq!(
        200 - w.actors[0].battle.mp,
        disc_mp,
        "the band debits the disc cost"
    );
    assert_eq!(w.actors[0].battle.last_mp_cost, disc_mp);
}

/// The same record's class byte routes the SM. A capture-class id reaches
/// `MagicCaptureBranch`; a Seru id on an identically-built world does not.
#[test]
fn the_discs_class_byte_routes_the_capture_branch() {
    let Some(scus) = scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // `capture_class_records` is the census of `'c'`-class ids and their
    // streamed-module indices; the engine consumes it here as the source of a
    // *real* capture id rather than hard-coding one.
    let capture = legaia_asset::spell_names::capture_class_records(&scus)
        .expect("the capture-class census decodes");
    assert!(
        !capture.is_empty(),
        "retail has capture-class spells; an empty census means the read moved"
    );
    let capture_id = capture[0].0;

    let mut w = armed_battle(&scus, capture_id, 200);
    assert!(
        tick_until(&mut w, ActionState::MagicCaptureBranch, 2_000),
        "spell {capture_id:#04x} is class 'c' on the disc, so the SM must take \
         the capture branch"
    );

    // The contrast that makes it non-vacuous: an ordinary Seru cast never
    // enters that branch, it takes the ordinary cast band.
    let mut w = armed_battle(&scus, 0x81, 200);
    assert!(
        tick_until(&mut w, ActionState::MagicPreCastWait, 2_000),
        "Gimard takes the ordinary cast band"
    );
    let mut w = armed_battle(&scus, 0x81, 200);
    assert!(
        !tick_until(&mut w, ActionState::MagicCaptureBranch, 2_000),
        "Gimard is not capture-class"
    );
}
