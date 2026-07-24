//! Actor / sprite script VM, ported clean-room from `FUN_801D6628`.
//!
//! PORT: FUN_801D6628, FUN_800319A8, FUN_800326AC, FUN_80035334, FUN_800357FC
//! PORT: FUN_800358C0, FUN_80035978, FUN_80035A4C
//!
//! `FUN_801D6628` lives in the title-screen / field overlay loaded into the
//! `0x801C0000+` window at runtime (see `docs/tooling/overlay-capture.md`). It is
//! the first script VM identified in retail Legaia. It is small (612 bytes,
//! 13 opcodes, 68 callers) and well-bounded - the smallest target we have for
//! a runtime-faithful port.
//!
//! ## Bytecode layout
//!
//! Each instruction is exactly 4 bytes:
//!
//! ```text
//!   byte 0:  opcode
//!   byte 1:  operand_b - typically an actor id
//!   bytes 2-3: operand_w - little-endian u16, typically a packed (x, y)
//! ```
//!
//! Execution stops on opcode `0x00`. Opcodes outside the `1..=0xD` valid range
//! are no-ops (the original used `sltiu v0, v1-1, 0xd` to dispatch).
//!
//! ## Opcode summary (per `funcs/overlay_801d6628.txt`)
//!
//! | op   | name                | semantics                                          |
//! | ---- | ------------------- | -------------------------------------------------- |
//! | 0x00 | `End`               | Terminate the program.                             |
//! | 0x01 | `SpawnDefault`      | Ensure actor exists, snap to default position,     |
//! |      |                     | conditional clear of `field20` if subobj equal.    |
//! | 0x02 | `SpawnAt`           | Ensure actor exists, snap to packed `operand_w`.   |
//! | 0x03 | `SetField1d`        | Write low byte of `operand_w` to actor `field1d`.  |
//! | 0x04 | `DeleteSprite`      | Delete the sprite for `operand_b`.                 |
//! | 0x05 | `GlobalUpdate`      | Tick the global sprite system.                     |
//! | 0x06 | `ClearField20`      | Clear actor `field20` if actor exists.             |
//! | 0x07 | `Nop`               | No-op (case 7 falls through to default).           |
//! | 0x08 | `Effect`            | Trigger actor effect.                              |
//! | 0x09 | `MotionAt`          | Ensure actor exists, motion to packed `operand_w`  |
//! |      |                     | (or default position if `operand_w == 0`).         |
//! | 0x0A | `EffectMotion`      | If actor exists: capture subobj-derived target,    |
//! |      |                     | trigger effect, respawn, then motion to target.    |
//! | 0x0B | reserved Nop        | Falls through to default.                          |
//! | 0x0C | reserved Nop        | Falls through to default.                          |
//! | 0x0D | reserved Nop        | Falls through to default.                          |
//!
//! ## Packed-position encoding
//!
//! When a position comes from `operand_w`, the original code uses:
//!
//! ```text
//!   x = (operand_w >> 7) & 0x1FE
//!   y =  operand_w       & 0xFF
//! ```
//!
//! Note the `0x1FE` mask - `x` is even-aligned at 9-bit precision. This matches
//! how the runtime quantises actor positions in field coordinates.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` or any overlay live in this crate. The Ghidra
//! decompilation is the *spec*, not source. The `Host` trait abstracts every
//! call the original made into the SCUS sprite engine - implementations of
//! that trait belong to the engine layer.
//!
//! Tests use hand-authored synthetic bytecode (no Sony bytes).

#![forbid(unsafe_code)]

pub mod actor_alloc;
pub mod actor_tick;
pub mod ambient_motion;
pub mod anim_vm;
pub mod battle_action;
pub mod battle_camera;
pub mod battle_cast_census;
pub mod battle_cast_cue;
pub mod battle_cast_dispatch;
pub mod battle_cue_group;
pub mod battle_cursor_pose;
pub mod battle_damage_wrappers;
pub mod battle_formulas;
pub mod battle_gauge;
pub mod battle_gauge_rearm;
pub mod battle_helpers;
pub mod battle_hp_bar;
pub mod battle_intro_particles;
pub mod battle_intro_styles;
pub mod battle_intro_swirl;
pub mod battle_intro_tiles;
pub mod battle_intro_transition;
pub mod battle_separation;
pub mod battle_stream_slot;
pub mod battle_target_group;
pub mod camera_mover;
pub mod camera_rel_actor;
pub mod code_lock_actor;
pub mod cutscene_trigger;
pub mod dev_equip_commit;
pub mod effect_vm;
pub mod escape_timer;
pub mod field;
/// The field-VM bytecode disassembler now lives in the Track-1 `asset` crate
/// (it is a side-effect-free width/format decoder); re-exported here so the
/// engine's existing `legaia_engine_vm::field_disasm` / `crate::field_disasm`
/// paths keep working.
pub use legaia_asset::field_disasm;
pub mod field_actor_billboard;
pub mod field_actor_reflect;
pub mod field_helpers;
pub mod field_ledge_hop_arc;
pub mod field_passive_hud;
pub mod field_state_pick;
pub mod menu;
pub mod menu_actor_seed;
pub mod motion_pause;
pub mod motion_vm;
pub mod move_buffer;
pub mod move_no_effect_guard;
pub mod move_vm;
pub mod move_vm_overlay_ext;
pub mod panel_backread_loader;
pub mod prim_dispatch;
pub mod scus_battle_helpers;
pub mod scus_core_helpers;
pub mod status_effects;
pub mod title_overlay;
pub mod title_prim;
pub mod travel_art_actor;
pub mod vdf_morph;
pub mod vram_rect_copy;
pub mod world_map;

pub mod world_map_clut_fade;
pub mod world_map_dev_menu;
pub mod world_map_dim;
pub mod world_map_horizon;
pub mod world_map_overlay;
pub mod world_map_panel;
pub mod world_map_panel_actors;
pub mod world_map_particle_burst;

/// Width of one bytecode instruction in bytes.
pub const INSN_SIZE: usize = 4;

/// Decoded instruction.
///
/// `opcode` is kept as the raw byte rather than the [`Opcode`] enum so that
/// reserved-range / out-of-range bytes round-trip identically - the runtime
/// treats `0x0B`..`0xFF` the same as `0x07` (no-op fall-through).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Insn {
    pub opcode: u8,
    pub operand_b: u8,
    pub operand_w: u16,
}

impl Insn {
    /// Decode one instruction from a 4-byte slice.
    pub fn decode(bytes: [u8; INSN_SIZE]) -> Self {
        Self {
            opcode: bytes[0],
            operand_b: bytes[1],
            operand_w: u16::from_le_bytes([bytes[2], bytes[3]]),
        }
    }

    /// Encode this instruction back to 4 bytes (round-trips [`decode`]).
    pub fn encode(self) -> [u8; INSN_SIZE] {
        let [lo, hi] = self.operand_w.to_le_bytes();
        [self.opcode, self.operand_b, lo, hi]
    }

    /// Decode the packed-position fields from `operand_w`.
    pub fn packed_position(self) -> Position {
        Position {
            x: ((self.operand_w >> 7) & 0x1FE) as i16,
            y: (self.operand_w & 0xFF) as i16,
        }
    }
}

/// Symbolic opcode names for documentation, debugging, and disassembly.
///
/// The runtime dispatches on the raw byte; reserved or out-of-range opcodes
/// are no-ops. Only the valid range is enumerated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    End = 0x00,
    SpawnDefault = 0x01,
    SpawnAt = 0x02,
    SetField1d = 0x03,
    DeleteSprite = 0x04,
    GlobalUpdate = 0x05,
    ClearField20 = 0x06,
    Nop = 0x07,
    Effect = 0x08,
    MotionAt = 0x09,
    EffectMotion = 0x0A,
}

impl Opcode {
    /// Decode the opcode byte. Returns `None` for reserved/out-of-range bytes
    /// (treated as `Nop` by the runtime).
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Opcode::End),
            0x01 => Some(Opcode::SpawnDefault),
            0x02 => Some(Opcode::SpawnAt),
            0x03 => Some(Opcode::SetField1d),
            0x04 => Some(Opcode::DeleteSprite),
            0x05 => Some(Opcode::GlobalUpdate),
            0x06 => Some(Opcode::ClearField20),
            0x07 => Some(Opcode::Nop),
            0x08 => Some(Opcode::Effect),
            0x09 => Some(Opcode::MotionAt),
            0x0A => Some(Opcode::EffectMotion),
            _ => None,
        }
    }
}

/// 2D actor position. Stored as `i16` to match the SCUS sprite engine's
/// signed-short coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub x: i16,
    pub y: i16,
}

impl Position {
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }
}

/// Errors the VM can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    /// Reached the end of the bytecode buffer without seeing an `End` opcode.
    Unterminated { at: usize, length: usize },
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::Unterminated { at, length } => write!(
                f,
                "bytecode ran past end at offset {at} (length {length}) with no End opcode"
            ),
        }
    }
}

impl std::error::Error for VmError {}

/// Engine-side callbacks the VM dispatches into.
///
/// Each method documents the SCUS function it replaces. Implementations live
/// in the engine layer (eventually `crates/engine-core` or a sibling) and are
/// free to back actors with whatever data structure makes sense - the VM
/// itself never inspects actor state, only invokes these queries.
pub trait Host {
    /// Equivalent of `FUN_80035334(actor_id)` - does this actor currently
    /// have an active sprite/state?
    fn actor_exists(&self, actor_id: u8) -> bool;

    /// Lookup the per-actor "default position" entry. The original consults a
    /// 16-byte-stride table at `0x801E4738` indexed by `actor_id` and reads
    /// two `i16` shorts at offsets `+4` and `+6` of that entry.
    ///
    /// The byte layout of that table is overlay-resident Sony data; engines
    /// must populate their default-position table from a separate channel
    /// (extracted asset, scene config, ...) and not from the executable.
    fn default_position(&self, actor_id: u8) -> Position;

    /// Equivalent of `FUN_800326ac(actor_id, table_entry_ptr)` - spawn the
    /// actor at its default position. Called when [`actor_exists`] returns
    /// `false` and we need the actor to come into existence.
    ///
    /// [`actor_exists`]: Host::actor_exists
    fn spawn(&mut self, actor_id: u8, default_position: Position);

    /// Equivalent of `FUN_800357fc(actor_id, x, y)` - snap actor position.
    fn set_position(&mut self, actor_id: u8, position: Position);

    /// Equivalent of `FUN_800358c0(actor_id, x, y)` - start a motion / glide
    /// to the supplied position.
    fn start_motion(&mut self, actor_id: u8, target: Position);

    /// Equivalent of `FUN_80035978(actor_id)` - delete the sprite associated
    /// with this actor.
    fn delete_sprite(&mut self, actor_id: u8);

    /// Equivalent of `FUN_80035a4c()` - tick whatever global sprite-system
    /// state advances per VM `GlobalUpdate` instruction.
    fn global_update(&mut self);

    /// Equivalent of `FUN_800319a8(actor_id)` - trigger the actor's effect
    /// (visual flash / ripple / spawn sound - the original handler is in the
    /// effect subsystem, undecoded so far).
    fn actor_effect(&mut self, actor_id: u8);

    /// Opaque write to the actor's `field1d` byte (`SetField1d`).
    ///
    /// Field semantics aren't yet reverse-engineered; this is a placeholder
    /// abstraction that future work will rename / type once the actor struct
    /// is decoded. Bytecode programs use it as a small per-actor flag store.
    fn set_field_1d(&mut self, actor_id: u8, value: u8);

    /// Opaque clear of the actor's `field20` 16-bit slot (`ClearField20`).
    /// See note on [`Host::set_field_1d`].
    fn clear_field_20(&mut self, actor_id: u8);

    /// Test the conditional that drives `SpawnDefault`'s post-snap clear.
    ///
    /// In the original, after `set_position` the runtime reads the actor's
    /// `subobj` pointer at offset `+0x24`. If non-null and both
    /// `subobj[+6] == subobj[+0xA]` and `subobj[+8] == subobj[+0xC]`, it
    /// clears `field20`.
    ///
    /// Implementations return `true` to perform the clear, `false` to skip.
    fn snap_clear_condition(&self, actor_id: u8) -> bool;

    /// Read the motion target used by `EffectMotion`.
    ///
    /// In the original, this is `(subobj[+0xA], subobj[+0xC])` - the same
    /// pair tested in [`Host::snap_clear_condition`]. Returning `None` skips
    /// the effect+motion entirely (the original branched out via
    /// `beq a2, zero, ...` on a null subobj pointer).
    fn motion_target(&self, actor_id: u8) -> Option<Position>;
}

/// Run `bytecode` until it terminates on `End` or runs out without one.
///
/// Returns the byte offset of the terminator (so callers can resume after,
/// matching the original's `return param_1` behaviour).
pub fn run<H: Host>(host: &mut H, bytecode: &[u8]) -> Result<usize, VmError> {
    let mut pc = 0usize;
    while pc + INSN_SIZE <= bytecode.len() {
        let raw = [
            bytecode[pc],
            bytecode[pc + 1],
            bytecode[pc + 2],
            bytecode[pc + 3],
        ];
        let insn = Insn::decode(raw);
        if insn.opcode == 0 {
            return Ok(pc);
        }
        execute(host, insn);
        pc += INSN_SIZE;
    }
    Err(VmError::Unterminated {
        at: pc,
        length: bytecode.len(),
    })
}

/// Execute a single decoded instruction. Out-of-range or reserved opcodes
/// (`0x07`, `0x0B..=0xFF`) fall through silently, matching the runtime.
fn execute<H: Host>(host: &mut H, insn: Insn) {
    let actor_id = insn.operand_b;
    let packed = insn.packed_position();

    match insn.opcode {
        // 0x01 - SpawnDefault.
        // Ensure actor exists, snap to its default position, then conditionally
        // clear field20 based on subobj equality.
        0x01 => {
            let default = host.default_position(actor_id);
            if !host.actor_exists(actor_id) {
                host.spawn(actor_id, default);
            }
            host.set_position(actor_id, default);
            if host.snap_clear_condition(actor_id) {
                host.clear_field_20(actor_id);
            }
        }
        // 0x02 - SpawnAt: ensure actor exists, snap to packed operand_w.
        0x02 => {
            let default = host.default_position(actor_id);
            if !host.actor_exists(actor_id) {
                host.spawn(actor_id, default);
            }
            host.set_position(actor_id, packed);
        }
        // 0x03 - SetField1d: write low byte of operand_w to actor field1d.
        // Original: `*(char *)(iVar4 + 0x1d) = (char)*puVar6`, where puVar6
        // is the u16 at bytes 2..4 - so the WRITTEN byte is bytes[2], not
        // operand_b.
        0x03 if host.actor_exists(actor_id) => {
            host.set_field_1d(actor_id, (insn.operand_w & 0xFF) as u8);
        }
        // 0x04 - DeleteSprite: unconditional delete (no exists check).
        0x04 => host.delete_sprite(actor_id),
        // 0x05 - GlobalUpdate: tick global sprite system, ignore operands.
        0x05 => host.global_update(),
        // 0x06 - ClearField20: only act if actor exists.
        0x06 if host.actor_exists(actor_id) => host.clear_field_20(actor_id),
        // 0x07 - Nop. (Case 7 explicitly falls through to the default in the
        // original switch.)
        0x07 => {}
        // 0x08 - Effect: unconditional actor effect.
        0x08 => host.actor_effect(actor_id),
        // 0x09 - MotionAt: ensure actor exists, motion to packed operand_w
        // (or to default position if operand_w == 0).
        0x09 => {
            let default = host.default_position(actor_id);
            if !host.actor_exists(actor_id) {
                host.spawn(actor_id, default);
            }
            let target = if insn.operand_w == 0 { default } else { packed };
            host.start_motion(actor_id, target);
        }
        // 0x0A - EffectMotion: capture subobj-derived target, fire effect,
        // respawn (overwrites position with default), then motion to target.
        // No-op if the actor (or its subobj target) is missing.
        0x0A => {
            if let Some(target) = host.motion_target(actor_id) {
                host.actor_effect(actor_id);
                host.spawn(actor_id, host.default_position(actor_id));
                host.start_motion(actor_id, target);
            }
        }
        // 0x0B..=0xFF - reserved / out-of-range; runtime falls through.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recording host for VM tests. Captures every Host call as a typed event
    /// so tests can assert exact dispatch order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        Exists(u8),
        DefaultPos(u8),
        Spawn(u8, Position),
        SetPos(u8, Position),
        StartMotion(u8, Position),
        DeleteSprite(u8),
        GlobalUpdate,
        Effect(u8),
        SetField1d(u8, u8),
        ClearField20(u8),
        SnapClearCondition(u8),
        MotionTarget(u8),
    }

    #[derive(Default)]
    struct RecHost {
        events: std::cell::RefCell<Vec<Event>>,
        // Test-controllable behaviour:
        existing_actors: std::collections::HashSet<u8>,
        defaults: std::collections::HashMap<u8, Position>,
        snap_clear: std::collections::HashSet<u8>,
        motion_targets: std::collections::HashMap<u8, Position>,
    }

    impl RecHost {
        fn record(&self, e: Event) {
            self.events.borrow_mut().push(e);
        }
        fn take_events(&self) -> Vec<Event> {
            std::mem::take(&mut self.events.borrow_mut())
        }
    }

    impl Host for RecHost {
        fn actor_exists(&self, actor_id: u8) -> bool {
            self.record(Event::Exists(actor_id));
            self.existing_actors.contains(&actor_id)
        }
        fn default_position(&self, actor_id: u8) -> Position {
            self.record(Event::DefaultPos(actor_id));
            self.defaults
                .get(&actor_id)
                .copied()
                .unwrap_or(Position::new(-1, -1))
        }
        fn spawn(&mut self, actor_id: u8, p: Position) {
            self.record(Event::Spawn(actor_id, p));
        }
        fn set_position(&mut self, actor_id: u8, p: Position) {
            self.record(Event::SetPos(actor_id, p));
        }
        fn start_motion(&mut self, actor_id: u8, p: Position) {
            self.record(Event::StartMotion(actor_id, p));
        }
        fn delete_sprite(&mut self, actor_id: u8) {
            self.record(Event::DeleteSprite(actor_id));
        }
        fn global_update(&mut self) {
            self.record(Event::GlobalUpdate);
        }
        fn actor_effect(&mut self, actor_id: u8) {
            self.record(Event::Effect(actor_id));
        }
        fn set_field_1d(&mut self, actor_id: u8, value: u8) {
            self.record(Event::SetField1d(actor_id, value));
        }
        fn clear_field_20(&mut self, actor_id: u8) {
            self.record(Event::ClearField20(actor_id));
        }
        fn snap_clear_condition(&self, actor_id: u8) -> bool {
            self.record(Event::SnapClearCondition(actor_id));
            self.snap_clear.contains(&actor_id)
        }
        fn motion_target(&self, actor_id: u8) -> Option<Position> {
            self.record(Event::MotionTarget(actor_id));
            self.motion_targets.get(&actor_id).copied()
        }
    }

    /// Build a bytecode buffer from instructions terminated with End.
    fn program(insns: &[Insn]) -> Vec<u8> {
        let mut out = Vec::with_capacity((insns.len() + 1) * INSN_SIZE);
        for i in insns {
            out.extend_from_slice(&i.encode());
        }
        out.extend_from_slice(&[0u8; INSN_SIZE]); // End
        out
    }

    #[test]
    fn end_terminates_immediately() {
        let mut host = RecHost::default();
        let pc = run(&mut host, &[0u8; 4]).unwrap();
        assert_eq!(pc, 0);
        assert!(host.take_events().is_empty());
    }

    #[test]
    fn unterminated_bytecode_errors() {
        let mut host = RecHost::default();
        // SpawnAt actor 0 to (0,0), then EOF without End.
        let bc = [0x02, 0x00, 0x00, 0x00];
        let err = run(&mut host, &bc).unwrap_err();
        assert!(matches!(err, VmError::Unterminated { at: 4, length: 4 }));
    }

    #[test]
    fn insn_roundtrips_decode_encode() {
        let bytes = [0x09, 0x42, 0xCD, 0xAB];
        let insn = Insn::decode(bytes);
        assert_eq!(insn.opcode, 0x09);
        assert_eq!(insn.operand_b, 0x42);
        assert_eq!(insn.operand_w, 0xABCD);
        assert_eq!(insn.encode(), bytes);
    }

    #[test]
    fn packed_position_decodes_per_spec() {
        // operand_w = 0x1234 → x = (0x1234 >> 7) & 0x1FE, y = 0x1234 & 0xFF
        let insn = Insn {
            opcode: 0x02,
            operand_b: 0,
            operand_w: 0x1234,
        };
        let p = insn.packed_position();
        assert_eq!(p.x as u16, (0x1234u16 >> 7) & 0x1FE);
        assert_eq!(p.y as u16, 0x1234u16 & 0xFF);
    }

    #[test]
    fn spawn_default_when_actor_missing_runs_full_sequence() {
        let mut host = RecHost::default();
        host.defaults.insert(7, Position::new(100, 50));
        // Don't insert into existing_actors - exists check returns false.
        let bc = program(&[Insn {
            opcode: 0x01,
            operand_b: 7,
            operand_w: 0,
        }]);
        run(&mut host, &bc).unwrap();
        let events = host.take_events();
        assert_eq!(
            events,
            vec![
                Event::DefaultPos(7),
                Event::Exists(7),
                Event::Spawn(7, Position::new(100, 50)),
                Event::SetPos(7, Position::new(100, 50)),
                Event::SnapClearCondition(7),
                // snap_clear didn't include 7 → no ClearField20
            ]
        );
    }

    #[test]
    fn spawn_default_skips_spawn_when_actor_exists_and_clears_when_condition_set() {
        let mut host = RecHost::default();
        host.defaults.insert(7, Position::new(100, 50));
        host.existing_actors.insert(7);
        host.snap_clear.insert(7);
        let bc = program(&[Insn {
            opcode: 0x01,
            operand_b: 7,
            operand_w: 0,
        }]);
        run(&mut host, &bc).unwrap();
        let events = host.take_events();
        assert_eq!(
            events,
            vec![
                Event::DefaultPos(7),
                Event::Exists(7),
                // No Spawn event because actor exists.
                Event::SetPos(7, Position::new(100, 50)),
                Event::SnapClearCondition(7),
                Event::ClearField20(7),
            ]
        );
    }

    #[test]
    fn spawn_at_uses_packed_position() {
        let mut host = RecHost::default();
        host.defaults.insert(3, Position::new(0, 0));
        let insn = Insn {
            opcode: 0x02,
            operand_b: 3,
            operand_w: 0x4080,
        };
        let bc = program(&[insn]);
        run(&mut host, &bc).unwrap();
        let p = insn.packed_position();
        let events = host.take_events();
        assert_eq!(
            events,
            vec![
                Event::DefaultPos(3),
                Event::Exists(3),
                Event::Spawn(3, Position::new(0, 0)),
                Event::SetPos(3, p),
            ]
        );
    }

    #[test]
    fn set_field_1d_writes_operand_w_low_byte_only_when_actor_exists() {
        let mut host = RecHost::default();
        host.existing_actors.insert(5);
        // operand_w = 0xFFAB - low byte 0xAB is what gets written.
        let bc = program(&[
            Insn {
                opcode: 0x03,
                operand_b: 5,
                operand_w: 0xFFAB,
            },
            // Same opcode targeting a missing actor - should be a no-op.
            Insn {
                opcode: 0x03,
                operand_b: 6,
                operand_w: 0xFFCD,
            },
        ]);
        run(&mut host, &bc).unwrap();
        let events = host.take_events();
        assert_eq!(
            events,
            vec![
                Event::Exists(5),
                Event::SetField1d(5, 0xAB),
                Event::Exists(6),
                // No SetField1d for actor 6 - exists returned false.
            ]
        );
    }

    #[test]
    fn delete_sprite_unconditional_no_exists_check() {
        let mut host = RecHost::default();
        let bc = program(&[Insn {
            opcode: 0x04,
            operand_b: 9,
            operand_w: 0,
        }]);
        run(&mut host, &bc).unwrap();
        // Note: only DeleteSprite, no Exists call.
        assert_eq!(host.take_events(), vec![Event::DeleteSprite(9)]);
    }

    #[test]
    fn global_update_takes_no_arguments() {
        let mut host = RecHost::default();
        let bc = program(&[Insn {
            opcode: 0x05,
            operand_b: 0xAB,
            operand_w: 0xCDEF,
        }]);
        run(&mut host, &bc).unwrap();
        assert_eq!(host.take_events(), vec![Event::GlobalUpdate]);
    }

    #[test]
    fn motion_at_uses_default_when_operand_w_is_zero() {
        let mut host = RecHost::default();
        host.defaults.insert(2, Position::new(80, 40));
        host.existing_actors.insert(2);
        let bc = program(&[Insn {
            opcode: 0x09,
            operand_b: 2,
            operand_w: 0,
        }]);
        run(&mut host, &bc).unwrap();
        let events = host.take_events();
        assert_eq!(
            events,
            vec![
                Event::DefaultPos(2),
                Event::Exists(2),
                Event::StartMotion(2, Position::new(80, 40)),
            ]
        );
    }

    #[test]
    fn motion_at_uses_packed_when_operand_w_nonzero() {
        let mut host = RecHost::default();
        host.defaults.insert(2, Position::new(80, 40));
        host.existing_actors.insert(2);
        let insn = Insn {
            opcode: 0x09,
            operand_b: 2,
            operand_w: 0x2080,
        };
        let bc = program(&[insn]);
        run(&mut host, &bc).unwrap();
        let events = host.take_events();
        assert_eq!(
            events,
            vec![
                Event::DefaultPos(2),
                Event::Exists(2),
                Event::StartMotion(2, insn.packed_position()),
            ]
        );
    }

    #[test]
    fn effect_motion_skipped_when_motion_target_missing() {
        let mut host = RecHost::default();
        // No motion_targets entry for 4 → motion_target returns None.
        let bc = program(&[Insn {
            opcode: 0x0A,
            operand_b: 4,
            operand_w: 0,
        }]);
        run(&mut host, &bc).unwrap();
        assert_eq!(host.take_events(), vec![Event::MotionTarget(4)]);
    }

    #[test]
    fn effect_motion_runs_full_sequence_when_target_present() {
        let mut host = RecHost::default();
        host.defaults.insert(4, Position::new(10, 20));
        host.motion_targets.insert(4, Position::new(200, 100));
        let bc = program(&[Insn {
            opcode: 0x0A,
            operand_b: 4,
            operand_w: 0,
        }]);
        run(&mut host, &bc).unwrap();
        let events = host.take_events();
        assert_eq!(
            events,
            vec![
                Event::MotionTarget(4),
                Event::Effect(4),
                Event::DefaultPos(4),
                Event::Spawn(4, Position::new(10, 20)),
                Event::StartMotion(4, Position::new(200, 100)),
            ]
        );
    }

    #[test]
    fn reserved_and_out_of_range_opcodes_are_nops() {
        let mut host = RecHost::default();
        let bc = program(&[
            Insn {
                opcode: 0x07,
                operand_b: 1,
                operand_w: 1,
            },
            Insn {
                opcode: 0x0B,
                operand_b: 1,
                operand_w: 1,
            },
            Insn {
                opcode: 0x0C,
                operand_b: 1,
                operand_w: 1,
            },
            Insn {
                opcode: 0x0D,
                operand_b: 1,
                operand_w: 1,
            },
            Insn {
                opcode: 0xFE,
                operand_b: 1,
                operand_w: 1,
            },
        ]);
        run(&mut host, &bc).unwrap();
        assert!(host.take_events().is_empty());
    }

    #[test]
    fn end_offset_returned_matches_position_of_terminator() {
        let mut host = RecHost::default();
        let mut bc = vec![];
        bc.extend_from_slice(
            &Insn {
                opcode: 0x05,
                operand_b: 0,
                operand_w: 0,
            }
            .encode(),
        );
        bc.extend_from_slice(
            &Insn {
                opcode: 0x05,
                operand_b: 0,
                operand_w: 0,
            }
            .encode(),
        );
        bc.extend_from_slice(&[0u8; 4]); // End at offset 8
        let pc = run(&mut host, &bc).unwrap();
        assert_eq!(pc, 8);
    }

    #[test]
    fn opcode_from_byte_round_trips_valid_range() {
        for b in 0..=0x0Au8 {
            assert_eq!(Opcode::from_byte(b).unwrap() as u8, b);
        }
        // Reserved / out-of-range bytes return None.
        assert!(Opcode::from_byte(0x0B).is_none());
        assert!(Opcode::from_byte(0xFF).is_none());
    }
}
