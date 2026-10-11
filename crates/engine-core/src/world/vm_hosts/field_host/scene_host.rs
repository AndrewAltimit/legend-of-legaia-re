//! `FieldSceneHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldSceneHost for FieldHostImpl<'a> {
    fn scene_transition(&mut self, map_id: u8) {
        // Record the request; SceneHost::tick drains it after the field
        // step returns so the bytecode swap doesn't invalidate the
        // borrow we're stepping through.
        self.world.pending_scene_transition = Some(map_id);
    }

    fn minigame_door_warp(&mut self, sub_id: u8) {
        // Retail's arm does two things this host can do inline - zero the
        // session-winnings accumulator `_DAT_80084440` and back the departure
        // scene up for the return trip (the backup is `FUN_80025980`'s half of
        // the same entry, and the engine keeps the field state resident, so
        // arming here is equivalent and keeps the round trip in one place).
        self.world.arm_minigame_warp();
        // The mode change itself is deferred like every other transition: the
        // field bytecode is still borrowed for this step, and entering a
        // minigame swaps the world's scene mode out from under it.
        self.world.minigames.pending_warp = Some(sub_id);
    }

    // PORT: FUN_8001FD44 (the name-based scene-change packet)
    //
    // Retail stages the destination by *name*. It saves the active buffer
    // `0x80084548` and the resolved-index word `0x80084540`, raises
    // `_DAT_1F800394 |= 0x40` (transition pending), zeroes `_DAT_8007BA98`,
    // copies the name into the staged buffer `0x8007050C` and calls
    // `FUN_8001D7F8`, which rewrites `0x80084540` with the destination's
    // index. Then it forks on `_DAT_8007B8C2` (`0x8001FDCC`):
    //
    // - `!= 0` (retail boots with it set): the old active name is backed up
    //   to `0x80084558` (the previous-scene buffer), the new name becomes the
    //   active one, and the index goes to `_DAT_8007B768` (`0x8001FDF4`);
    // - `== 0` (the dev arm, never taken on the disc): the name goes to
    //   `0x800915C8`, the saved name is restored into the active buffer, and
    //   the index goes to `gp+0x688`.
    //
    // Both arms restore `0x80084540` from the saved copy and spawn the
    // transition streaming actor (descriptor `0x80070734`, handler
    // `FUN_80021934`), writing `_DAT_8007B828 = 0x7FFF` when the pool is full.
    // The engine has no name buffers or streaming actor: the packet is this
    // deferred triple plus the arrival facing, which `SceneHost::tick` drains
    // where the retail arm's streaming actor would load the destination.
    fn scene_transition_named(&mut self, scene: &str, entry_x: u8, entry_z: u8, dir: u8) {
        // Named scene-change (op 0x3F): the destination name is inline, so no
        // map-id resolver is needed. Recorded for SceneHost::tick to drain,
        // the same deferral as the map-id path above (the bytecode swap can't
        // run while we're stepping through it). `dir` is the arrival-facing
        // compass selector the seat applies on the far side.
        self.world.pending_named_scene_transition =
            Some((scene.to_string(), entry_x, entry_z, dir));
    }

    fn is_scripted_encounter_armed(&self) -> bool {
        self.world.encounters.scripted_armed
    }

    fn install_scripted_encounter(&mut self, window: &[u8]) {
        // Queue the record window for the field-step driver to install after
        // the VM borrow ends (we can't mutate the encounter session while the
        // field bytecode is still borrowed).
        self.world.encounters.pending_scripted = Some(window.to_vec());
    }

    // PORT: FUN_8003C7EC
    fn trigger_game_over(&mut self) {
        // Field-VM op `4C EA` - the scripted game-over trigger. Retail
        // stores master mode 0x16 (22 = CARD INIT, the CARD/CONTINUE
        // title flow) + `_DAT_8007BB00 = 1` + clears `DAT_8007B750` bit 1,
        // then pauses the BGM (`FUN_800266E0(0x8007052C)` - the same
        // primitive as BGM sub-op 2). The engine's CARD/CONTINUE flow is
        // its defeat state: raise `game_over` and route a BGM pause.
        self.world.game_over = true;
        self.world.pending_field_events.push(FieldEvent::Bgm {
            text_id: 0,
            sub_op: 2,
        });
    }

    // Field-VM op `4C EC`: `_DAT_8007B5FC = FUN_801DDF48()` (`0x801E34F8`
    // `jal`, store in the `j` delay slot at `0x801E3508`) - reroll the
    // encounter step counter. No shipped script issues a clean `4C EC`
    // (field-op census), so this is reachable only from modded bytecode.
    // REF: FUN_801DDF48 (ported as region_encounter::encounter_counter_reroll)
    fn reroll_encounter_step_counter(&mut self) {
        self.world.reroll_encounter_step_counter();
    }

    fn trigger_fmv(&mut self, fmv_id: i16) {
        // Field-VM op `0x4C 0xE2` - retail handler at 0x801E30E4 writes
        // the resolved s16 to `_DAT_8007BA78` (FMV index) and pokes
        // `_DAT_8007B83C = 0x1A` (next game mode = 26 = StrInit). We
        // record the request here so the SceneHost / engine driver can
        // pop it after the field step returns and switch its scene
        // mode without invalidating the field-VM borrow.
        self.world.cutscene.pending_fmv_trigger = Some(fmv_id);
        self.world
            .pending_field_events
            .push(FieldEvent::FmvTrigger { fmv_id });
    }

    fn scripted_battle(&mut self, op0: u8, row: u8) {
        // `3E <op0> <row>` with `op0 < 100` or `op0 == 0xFF` - the
        // scripted-battle install. Retail's arm installs the per-scene MAN
        // formation-table row `row` as the SYSTEM entity's encounter record
        // (`sys_ctx[+0x8A] = 1`, `sys_ctx[+0x94] = formation_table +
        // row*stride + 1`) and requests the battle mode switch;
        // `FUN_801DA51C`'s confirm state then copies the row into the battle
        // formation cell. `op0` is never read past the `0xFF` / `< 100` test,
        // so `3E 00 02` (town0b), `3E 00 03` (stone) and `3E 01 00`
        // (jagaroom) install their rows exactly as garmel's Zeto beat's
        // `3E FF 09` does. Talking to an NPC never reaches this op: that is
        // the interaction probe's [`World::trigger_field_interact`].
        let _ = op0;
        self.world.trigger_scripted_battle(row);
    }

    fn scene_register_write(&mut self, slot_10: u8, slot_12: u8, slot_14: u8) {
        self.world
            .pending_field_events
            .push(FieldEvent::SceneRegisterWrite {
                slot_10,
                slot_12,
                slot_14,
            });
    }
}
