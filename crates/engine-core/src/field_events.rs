//! Event queue emitted by the field VM through the world's `FieldHost`
//! implementation.
//!
//! Most field-VM opcodes only have meaningful side-effects in the host -
//! BGM dispatch lives in audio, dialog opens a UI overlay, money / inventory
//! / party manipulation update game state. The retail engine called into
//! its loader / audio / UI layers directly; in the clean-room port we route
//! every such call through a [`FieldEvent`] pushed onto
//! [`crate::world::World::pending_field_events`].
//!
//! Engines drain the queue once per frame after [`crate::world::World::tick`]
//! returns and apply the events to their own subsystems. Tests inspect it to
//! verify the VM emitted what they expected.
//!
//! Read [`docs/subsystems/script-vm.md`] for the per-opcode semantic notes.

use legaia_engine_vm::field::CameraParam;

/// One side-effect the field VM requested this frame. Variants mirror the
/// `FieldHost` callbacks one-to-one - see [`legaia_engine_vm::field::FieldHost`]
/// for the per-opcode citation.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldEvent {
    /// Field-VM op 0x35 (BGM control).
    Bgm { text_id: u16, sub_op: u8 },
    /// Field-VM op 0x39 (`GIVE_ITEM`): the player was given one of `item_id`
    /// (treasure chest / scripted gift). The world also adds it to inventory.
    GiveItem { item_id: u8 },
    /// Field-VM op 0x3F (open dialog box).
    OpenDialog {
        text_id: u16,
        inline: Vec<u8>,
        world_x: u16,
        world_z: u16,
        depth_id: u8,
    },
    /// Emitted when [`crate::world::World`]'s field-VM dialog-advance host
    /// hook (`op 0x4C n5 sub-4`) clears `current_dialog` in response to a
    /// just-pressed Cross/Circle. Engines that mirror dialog state in
    /// their own HUD drain this to fade the dialog box out.
    DialogDismissed,
    /// Field-VM op 0x3A (add or subtract money). Already sign-extended
    /// from the 24-bit operand.
    AddMoney { delta: i32 },
    /// Field-VM op 0x3B (set inventory slot count).
    SetItemCount { slot_byte: u8, count: u8 },
    /// Field-VM op 0x3C (party_add).
    PartyAdd { char_id: u8, accepted: bool },
    /// Field-VM op 0x3D (party_remove).
    PartyRemove { char_id: u8 },
    /// Field-VM op 0x3E `op0 < 100` arm (field interaction trigger).
    FieldInteract { interact_id: u8, slot: u8 },
    /// Field-VM op 0x4F (scene register write).
    SceneRegisterWrite {
        slot_10: u8,
        slot_12: u8,
        slot_14: u8,
    },
    /// Field-VM op 0x4C sub-0 (set party leader).
    SetPartyLeader { leader_id: u8 },
    /// Field-VM op 0x45 sub-CONFIGURE (camera params).
    CameraConfigure {
        params: Vec<CameraParam>,
        apply_trigger: u16,
        mode: u8,
    },
    /// Field-VM op 0x45 sub-LOAD (camera payload).
    CameraLoad { payload: Vec<u8> },
    /// Field-VM op 0x45 sub-SAVE (snapshot camera scratch).
    CameraSave,
    /// Field-VM op 0x45 sub-APPLY (apply + read-back).
    CameraApply,
    /// Field-VM op 0x4B (multi-keyframe animation setup).
    SetupAnimation {
        count: u8,
        base_id: u8,
        frames: Vec<u8>,
    },
    /// Field-VM op 0x46 long-form render config (RGB + packed).
    RenderCfgLong { b1: u8, b2: u8, b3: u8, b4: u8 },
    /// Field-VM op 0x46 short-form render config.
    RenderCfgShort { r: u8, g: u8, b: u8, packed: u8 },
    /// Field-VM op 0x44 (spawn a MAN partition-2 record as a new context).
    SpawnRecord { global_index: u8 },
    /// Effect-anim trigger (op cluster around 0x32 / 0x4E).
    EffectAnimTrigger { arg: u8 },
    /// Field-VM op 0x36 (scene fade) - the host returned this fade was
    /// applied. `op0_word` / `op1_word` are the raw 16-bit operands so
    /// engines can re-decode mode bits.
    SceneFade { op0_word: u16, op1_word: u16 },
    /// Field-VM op 0x34 sub-0 (effect-global colour fade, `FUN_801E1FB0`).
    /// `op0`'s low bit selects direction; `rgb` is the wash colour (all-zero
    /// clears the active fade).
    ColorFade { op0: u8, rgb: [u8; 3] },
    /// Menu-control op 0x4C sub-1.
    MenuCtrl { op0: u8, payload: [u8; 5] },
    /// Menu-refresh op (any sub-op that requested a reload).
    MenuRefresh,
    /// Field-VM op 0x23 (move_to). Includes the decoded world coords.
    MoveTo {
        world_x: u16,
        world_z: u16,
        is_player: bool,
    },
    /// Field-VM op 0x2C (exec_move) - the move-table consumer.
    ExecMove { move_id: u8 },
    /// Field-VM op `0x4C 0xE2` (FMV trigger).
    ///
    /// The retail handler at `0x801E30E4` writes the s16 operand to
    /// `_DAT_8007BA78` (the runtime FMV index, used by the master
    /// dispatch `FUN_801CEA3C` to select a 32-byte slot from the FMV
    /// dispatch table at `0x801D0A6C`) and kicks the next-game-mode
    /// global `_DAT_8007B83C` to `0x1A` (game mode 26 = StrInit). On
    /// retail, indices 0..=8 select the nine retail movie slots -
    /// `MV1`/`MV2`/four `MV3` frame-range segments/`MV4`/`MV5`/`MV6`,
    /// every movie on the disc. Engines that want to actually play
    /// the FMV should pop this event, resolve the index to a STR file
    /// (via `legaia_asset::fmv_dispatch::FmvTable` when disc bytes are
    /// available, else the static
    /// [`crate::cutscene::fmv_index_to_str_filename`] map), and kick
    /// whatever STR/MDEC playback path they have. Engines without an
    /// FMV path can drop the event - the field VM doesn't require any
    /// host-side response.
    FmvTrigger { fmv_id: i16 },
    /// Emitted by [`crate::world::World::install_scripted_encounter`] when the
    /// field VM forwards a `+0x94` record pointer (op 0x34 sub-2 capture) and
    /// the scripted-encounter consumer is armed. `record` is the bounded
    /// bytecode window the VM captured ( `[flag][_][_][count][ids..]`, at most
    /// 8 bytes). Engines can drain this to log / visualize the install; the
    /// formation is already registered + armed by the time the event surfaces.
    ScriptedEncounter { record: Vec<u8> },
    /// Field-VM op `0x4C 0x80` (actor allocator, halt-acquire prelude).
    ///
    /// The dispatcher has already routed through the halt-acquire predicate
    /// (`which = 0x80`) and performed the ctx mutation; this event surfaces
    /// the spawn request to engines. `records` is the list of child-actor
    /// bytecode streams split out of the script's `tail` via the retail
    /// `FUN_8003CA38` packet-length walker (mirrored by
    /// [`legaia_engine_vm::field_helpers::packet_length`]). The parent
    /// script's PC has already advanced past the opcode header; the records
    /// themselves remain embedded in the bytecode buffer and become the
    /// spawned actors' own bytecode (retail stores the pointer at
    /// `actor[+0x90]`).
    ActorAllocate { records: Vec<Vec<u8>> },
    /// Emitted by [`crate::world::World::materialize_actor_spawns`] - the
    /// engine-side consumer that drains queued [`Self::ActorAllocate`]
    /// records and instantiates each as an actor slot.
    ///
    /// `slot` is the index into `World::actors` that received the
    /// allocation. `record` is the bytecode the spawned actor's
    /// `actor[+0x4C]` record-pointer references (cloned onto the slot's
    /// [`crate::world::Actor::spawn_record`]). `kind` / `variant` mirror
    /// the FUN_801D77F4 `actor[+0x3C]` / `actor[+0x3E]` writes - both zero
    /// until a record encoding is pinned.
    ///
    /// Engines drain this when they want to attach rendering / animation
    /// state to the spawned actor; the spawn itself is already complete by
    /// the time the event surfaces.
    ActorSpawned {
        slot: u8,
        kind: u16,
        variant: u16,
        record: Vec<u8>,
    },
    /// Emitted by [`crate::world::World::materialize_actor_spawns`] when an
    /// actor-allocate request cannot be served because every slot from
    /// `spawn_start_slot..MAX_ACTORS` is already active. Mirrors the retail
    /// "pool exhausted: bail silently" branch of `FUN_801D77F4` (the
    /// instantiator returns when `FUN_80020DE0` hands back zero). The
    /// dropped record is included so engines can log / diagnose.
    ActorSpawnFailed { record: Vec<u8> },
    /// A world-map portal entity reached its scene-transition state. `slot` is
    /// the overworld entity index; `target_map` is the scene the portal leads
    /// to (the per-portal target map id the entity was configured with via
    /// [`crate::world::WorldMapEntityConfig::Portal`]). Engines drain this to
    /// load `target_map` and leave the overworld. Carries the richer target id
    /// the generic [`Self::FieldInteract`] cannot.
    WorldMapTransition { target_map: u16, slot: u8 },
}

impl FieldEvent {
    /// One-line description for logging / asset-viewer overlays.
    pub fn summary(&self) -> String {
        match self {
            FieldEvent::Bgm { text_id, sub_op } => {
                format!("Bgm(id={text_id}, sub={sub_op})")
            }
            FieldEvent::GiveItem { item_id } => format!("GiveItem({item_id})"),
            FieldEvent::OpenDialog {
                text_id,
                inline,
                world_x,
                world_z,
                depth_id,
            } => {
                format!(
                    "OpenDialog(text={text_id}, inline={}B, x={world_x}, z={world_z}, depth={depth_id})",
                    inline.len()
                )
            }
            FieldEvent::DialogDismissed => "DialogDismissed".into(),
            FieldEvent::AddMoney { delta } => format!("AddMoney({delta})"),
            FieldEvent::SetItemCount { slot_byte, count } => {
                format!("SetItemCount(slot={slot_byte:#x}, count={count})")
            }
            FieldEvent::PartyAdd {
                char_id, accepted, ..
            } => {
                format!("PartyAdd({char_id}, accepted={accepted})")
            }
            FieldEvent::PartyRemove { char_id } => format!("PartyRemove({char_id})"),
            FieldEvent::FieldInteract { interact_id, slot } => {
                format!("FieldInteract(id={interact_id}, slot={slot})")
            }
            FieldEvent::WorldMapTransition { target_map, slot } => {
                format!("WorldMapTransition(target_map={target_map}, slot={slot})")
            }
            FieldEvent::SceneRegisterWrite {
                slot_10,
                slot_12,
                slot_14,
            } => {
                format!("SceneRegisterWrite([{slot_10:#x}, {slot_12:#x}, {slot_14:#x}])")
            }
            FieldEvent::SetPartyLeader { leader_id } => format!("SetPartyLeader({leader_id})"),
            FieldEvent::CameraConfigure {
                params,
                apply_trigger,
                mode,
            } => {
                format!(
                    "CameraConfigure({} params, apply={apply_trigger}, mode={mode})",
                    params.len()
                )
            }
            FieldEvent::CameraLoad { payload } => {
                format!("CameraLoad({}B)", payload.len())
            }
            FieldEvent::CameraSave => "CameraSave".into(),
            FieldEvent::CameraApply => "CameraApply".into(),
            FieldEvent::SetupAnimation {
                count,
                base_id,
                frames,
            } => {
                format!(
                    "SetupAnimation(count={count}, base={base_id}, frames={}B)",
                    frames.len()
                )
            }
            FieldEvent::RenderCfgLong { b1, b2, b3, b4 } => {
                format!("RenderCfgLong({b1:#x}, {b2:#x}, {b3:#x}, {b4:#x})")
            }
            FieldEvent::RenderCfgShort { r, g, b, packed } => {
                format!("RenderCfgShort(r={r}, g={g}, b={b}, packed={packed})")
            }
            FieldEvent::SpawnRecord { global_index } => format!("SpawnRecord({global_index})"),
            FieldEvent::EffectAnimTrigger { arg } => format!("EffectAnimTrigger({arg})"),
            FieldEvent::SceneFade { op0_word, op1_word } => {
                format!("SceneFade(op0={op0_word:#x}, op1={op1_word:#x})")
            }
            FieldEvent::ColorFade { op0, rgb } => {
                format!("ColorFade(op0={op0:#x}, rgb={rgb:?})")
            }
            FieldEvent::MenuCtrl { op0, payload } => {
                format!("MenuCtrl(op0={op0}, payload={:?})", payload)
            }
            FieldEvent::MenuRefresh => "MenuRefresh".into(),
            FieldEvent::MoveTo {
                world_x,
                world_z,
                is_player,
            } => {
                format!("MoveTo(x={world_x}, z={world_z}, player={is_player})")
            }
            FieldEvent::ExecMove { move_id } => format!("ExecMove({move_id})"),
            FieldEvent::FmvTrigger { fmv_id } => format!("FmvTrigger({fmv_id})"),
            FieldEvent::ScriptedEncounter { record } => {
                format!("ScriptedEncounter({}B)", record.len())
            }
            FieldEvent::ActorAllocate { records } => {
                let bytes: usize = records.iter().map(|r| r.len()).sum();
                format!("ActorAllocate(count={}, body={}B)", records.len(), bytes,)
            }
            FieldEvent::ActorSpawned {
                slot,
                kind,
                variant,
                record,
            } => {
                format!(
                    "ActorSpawned(slot={slot}, kind={kind:#x}, variant={variant:#x}, body={}B)",
                    record.len()
                )
            }
            FieldEvent::ActorSpawnFailed { record } => {
                format!("ActorSpawnFailed(body={}B)", record.len())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_handles_each_variant() {
        // Just smoke-test that none of the variants panic on summary.
        let events = vec![
            FieldEvent::Bgm {
                text_id: 1,
                sub_op: 1,
            },
            FieldEvent::GiveItem { item_id: 7 },
            FieldEvent::OpenDialog {
                text_id: 0x42,
                inline: vec![1, 2, 3],
                world_x: 10,
                world_z: 20,
                depth_id: 0,
            },
            FieldEvent::AddMoney { delta: -500 },
            FieldEvent::SetItemCount {
                slot_byte: 0x12,
                count: 99,
            },
            FieldEvent::PartyAdd {
                char_id: 3,
                accepted: true,
            },
            FieldEvent::PartyRemove { char_id: 2 },
            FieldEvent::FieldInteract {
                interact_id: 4,
                slot: 1,
            },
            FieldEvent::SceneRegisterWrite {
                slot_10: 1,
                slot_12: 2,
                slot_14: 3,
            },
            FieldEvent::SetPartyLeader { leader_id: 0 },
            FieldEvent::CameraConfigure {
                params: vec![],
                apply_trigger: 0,
                mode: 0,
            },
            FieldEvent::CameraLoad { payload: vec![] },
            FieldEvent::CameraSave,
            FieldEvent::CameraApply,
            FieldEvent::SetupAnimation {
                count: 0,
                base_id: 0,
                frames: vec![],
            },
            FieldEvent::RenderCfgLong {
                b1: 0,
                b2: 0,
                b3: 0,
                b4: 0,
            },
            FieldEvent::RenderCfgShort {
                r: 0,
                g: 0,
                b: 0,
                packed: 0,
            },
            FieldEvent::SpawnRecord { global_index: 0 },
            FieldEvent::EffectAnimTrigger { arg: 0 },
            FieldEvent::SceneFade {
                op0_word: 0,
                op1_word: 0,
            },
            FieldEvent::ColorFade {
                op0: 0,
                rgb: [0, 0, 0],
            },
            FieldEvent::MenuCtrl {
                op0: 0,
                payload: [0; 5],
            },
            FieldEvent::MenuRefresh,
            FieldEvent::MoveTo {
                world_x: 0,
                world_z: 0,
                is_player: false,
            },
            FieldEvent::ExecMove { move_id: 0 },
            FieldEvent::FmvTrigger { fmv_id: 3 },
            FieldEvent::ActorAllocate {
                records: vec![vec![0x40, 0x40], vec![0x80]],
            },
        ];
        for e in events {
            assert!(!e.summary().is_empty());
        }
    }
}
