//! Menu input → save / load round-trip integration: drive the
//! `engine-vm::menu` state machine through Idle → StatusTop → save commit
//! → write bytes → load bytes back into a fresh World and assert party
//! state survived the round-trip.
//!
//! What this proves:
//!  - `engine-vm::menu::step` reaches every transition the integration
//!    needs (Idle → StatusTop → SavePickSlot via host-driven state mutation,
//!    SavePickSlot → SaveWriting → SaveDone).
//!  - A `MenuHost` impl can use the save crate to serialise + deserialise a
//!    `Party` without lifetime issues.
//!  - `World::save_party` / `World::load_party` round-trip under the same
//!    flow the menu UI will eventually drive.
//!
//! No disc data, no font, no winit. The menu host is a closure-shaped
//! struct that owns a borrow of the World + a mutable scratch slot for
//! the serialised save bytes.

use legaia_engine_core::world::World;
use legaia_engine_vm::menu::{MenuCtx, MenuHost, MenuInput, MenuState, open, step};
use legaia_save::{CharacterRecord, Party};

/// A menu host that:
/// 1. Says StatusTop has 2 items (the "Save" row at index 0 and a dummy
///    "Cancel" row at index 1).
/// 2. On commit at StatusTop slot 0, transitions ctx into SavePickSlot.
/// 3. On commit at SavePickSlot, captures `World::save_party` into its
///    `serialised` buffer and bumps ctx into SaveWriting.
/// 4. On step into SaveWriting (cursor, Cross again), bumps ctx into
///    SaveDone.
/// 5. On commit at SaveDone, transitions to Closing.
struct SaveFlowHost<'a> {
    world: &'a mut World,
    /// Bytes captured by the save commit, populated when the host walks
    /// through SavePickSlot. Re-used by the load test.
    serialised: Vec<u8>,
    /// State byte the host wants ctx to take next frame (separate from
    /// ctx.state because step()'s default arm doesn't allow the host to
    /// directly mutate state - we patch ctx.state outside step()).
    next_state: Option<u8>,
}

impl MenuHost for SaveFlowHost<'_> {
    fn screen_item_count(&self, state: MenuState) -> u8 {
        match state {
            MenuState::StatusTop => 2,
            _ => 1,
        }
    }
    fn commit(&mut self, state: MenuState, slot: u8) {
        match (state, slot) {
            (MenuState::StatusTop, 0) => {
                self.next_state = Some(MenuState::SavePickSlot.as_byte());
            }
            (MenuState::SavePickSlot, _) => {
                let party = self.world.save_party();
                self.serialised = party.write();
                self.next_state = Some(MenuState::SaveWriting.as_byte());
            }
            (MenuState::SaveWriting, _) => {
                self.next_state = Some(MenuState::SaveDone.as_byte());
            }
            (MenuState::SaveDone, _) => {
                self.next_state = Some(MenuState::Closing.as_byte());
            }
            _ => {}
        }
    }
}

fn build_party_with_two_chars() -> Party {
    let mut a = CharacterRecord::zeroed();
    let mut hms = a.hp_mp_sp();
    hms.hp_cur = 80;
    hms.hp_max = 100;
    hms.mp_cur = 30;
    a.set_hp_mp_sp(hms);

    let mut b = CharacterRecord::zeroed();
    let mut hms = b.hp_mp_sp();
    hms.hp_cur = 200;
    hms.hp_max = 220;
    hms.mp_cur = 0;
    b.set_hp_mp_sp(hms);

    let mut p = Party::zeroed(2);
    p.members[0] = a;
    p.members[1] = b;
    p
}

#[test]
fn menu_save_flow_serialises_world_party() {
    let mut world = World::default();
    let party = build_party_with_two_chars();
    world.load_party(party);

    let mut host = SaveFlowHost {
        world: &mut world,
        serialised: Vec::new(),
        next_state: None,
    };
    let mut ctx = MenuCtx::default();

    open(&mut ctx);
    // After open, one tick to roll Idle → StatusTop.
    step(&mut host, &mut ctx, MenuInput::default());
    assert_eq!(ctx.state, MenuState::StatusTop.as_byte());

    // Cross on Save row (cursor 0 by default).
    step(
        &mut host,
        &mut ctx,
        MenuInput {
            cross: true,
            ..Default::default()
        },
    );
    if let Some(next) = host.next_state.take() {
        ctx.state = next;
        ctx.frame = 0;
    }
    assert_eq!(ctx.state, MenuState::SavePickSlot.as_byte());

    // Cross on SavePickSlot - captures the save bytes.
    step(
        &mut host,
        &mut ctx,
        MenuInput {
            cross: true,
            ..Default::default()
        },
    );
    if let Some(next) = host.next_state.take() {
        ctx.state = next;
        ctx.frame = 0;
    }
    assert_eq!(ctx.state, MenuState::SaveWriting.as_byte());
    assert!(
        !host.serialised.is_empty(),
        "save commit should have populated bytes"
    );
    let serialised = host.serialised.clone();

    // SaveWriting → SaveDone → Closing → Deactivate → Closed.
    for _ in 0..4 {
        step(
            &mut host,
            &mut ctx,
            MenuInput {
                cross: true,
                ..Default::default()
            },
        );
        if let Some(next) = host.next_state.take() {
            ctx.state = next;
            ctx.frame = 0;
        }
    }
    // Wait through Closing's frame timer (16 frames) then Deactivate.
    for _ in 0..32 {
        step(&mut host, &mut ctx, MenuInput::default());
    }
    assert_eq!(ctx.state, MenuState::Closed.as_byte());

    // Round-trip: parse the serialised bytes and verify HP/MP survived.
    let restored = Party::parse(&serialised).expect("parse saved party bytes");
    assert_eq!(restored.members.len(), 2);
    assert_eq!(restored.members[0].hp_mp_sp().hp_cur, 80);
    assert_eq!(restored.members[0].hp_mp_sp().hp_max, 100);
    assert_eq!(restored.members[1].hp_mp_sp().hp_cur, 200);

    // Load back into a fresh world and verify the actor mirrors picked up
    // the HP from the save (the load_party path the engine uses).
    let mut fresh = World::default();
    fresh.load_party(restored);
    assert_eq!(fresh.actors[0].battle.hp, 80);
    assert_eq!(fresh.actors[1].battle.hp, 200);
}

#[test]
fn menu_cancel_path_closes_without_saving() {
    let mut world = World::default();
    let mut host = SaveFlowHost {
        world: &mut world,
        serialised: Vec::new(),
        next_state: None,
    };
    let mut ctx = MenuCtx::default();
    open(&mut ctx);
    step(&mut host, &mut ctx, MenuInput::default());
    // Triangle on StatusTop → Closing.
    step(
        &mut host,
        &mut ctx,
        MenuInput {
            triangle: true,
            ..Default::default()
        },
    );
    assert_eq!(ctx.state, MenuState::Closing.as_byte());
    // Drain frames so Closing → Deactivate → Closed.
    for _ in 0..40 {
        step(&mut host, &mut ctx, MenuInput::default());
    }
    assert_eq!(ctx.state, MenuState::Closed.as_byte());
    assert!(host.serialised.is_empty(), "cancel must not save");
}
