//! Entry points that load bytecode into the VMs, seat actors, and step the move
//! VM and the battle action state machine.

use super::*;

/// The run band's wait the Spikefish flute's flee hands the action SM
/// (`ctx[+0x6D8] = 0x3C`, PROT 0925 arm 8).
const FLUTE_FLEE_WAIT_FRAMES: i16 = 0x3C;

impl World {
    /// Set / clear the move-VM bytecode for `slot`. `None` clears the
    /// buffer; subsequent ticks won't run the move VM on this actor.
    pub fn set_move_bytecode(&mut self, slot: usize, bytecode: Option<Vec<u16>>) {
        if slot < self.move_vm.bytecode.len() {
            self.move_vm.bytecode[slot] = bytecode.unwrap_or_default();
        }
    }

    /// Set bit `idx` in the shared system flag bank. `idx >> 3` is the byte
    /// offset; the bit mask is `0x80 >> (idx & 7)` (MSB-first, mirroring the
    /// SCUS helper at `FUN_8003CE08`). The bank grows lazily as needed.
    pub fn system_flag_set(&mut self, idx: u16) {
        let byte = (idx >> 3) as usize;
        if byte >= self.flags.system_flags.len() {
            self.flags.system_flags.resize(byte + 1, 0);
        }
        self.flags.system_flags[byte] |= 0x80u8 >> (idx & 7);
    }

    /// Clear bit `idx` in the shared system flag bank. See [`system_flag_set`].
    /// Out-of-bounds clears are no-ops (the bit is already zero).
    ///
    /// [`system_flag_set`]: World::system_flag_set
    pub fn system_flag_clear(&mut self, idx: u16) {
        let byte = (idx >> 3) as usize;
        if byte < self.flags.system_flags.len() {
            self.flags.system_flags[byte] &= !(0x80u8 >> (idx & 7));
        }
    }

    /// Test bit `idx` in the shared system flag bank. Returns `false` for
    /// indices past the currently-grown size.
    /// The flag test the encounter condition walk (`FUN_801D9E1C`) asks:
    /// [`Self::system_flag_test`], except that with
    /// [`crate::world::WorldToggles::fortress_encounter_fix`] on, the Floating
    /// Castle's stale flag [`crate::world::FORTRESS_STALE_FLAG`] reads clear in
    /// the Fortress scenes ([`crate::world::FORTRESS_ENCOUNTER_SCENES`]). The
    /// walk then takes the group after the castle's copied "no battles" group,
    /// as the `--fortress-encounter-fix` disc patch makes it do.
    pub fn encounter_condition_flag_test(&self, idx: u16) -> bool {
        if self.toggles.fortress_encounter_fix
            && idx == crate::world::FORTRESS_STALE_FLAG
            && crate::world::FORTRESS_ENCOUNTER_SCENES.contains(&self.active_scene_label.as_str())
        {
            return false;
        }
        self.system_flag_test(idx)
    }

    pub fn system_flag_test(&self, idx: u16) -> bool {
        let byte = (idx >> 3) as usize;
        if byte < self.flags.system_flags.len() {
            self.flags.system_flags[byte] & (0x80u8 >> (idx & 7)) != 0
        } else {
            false
        }
    }

    /// Replace the field-VM bytecode buffer + reset PC. Engines call this
    /// when entering a new field scene (loading the scene's per-event
    /// script) to start interpretation from the beginning.
    pub fn load_field_script(&mut self, bytecode: Vec<u8>) {
        self.field_bytecode = bytecode;
        self.field_pc = 0;
        self.field_ctx = FieldCtx::default();
        self.field_vm.system_pass_open = true;
    }

    /// Load a field-VM bytecode buffer and begin interpretation at `pc`
    /// instead of 0.
    ///
    /// Used to run a MAN-resolved **scene-entry system script** (retail
    /// `FUN_8003ab2c`, context channel `0xFB`): the buffer is the MAN slice
    /// taken from the script block's start, and `pc` is the first opcode's
    /// offset into that slice (past the `[local-count][locals][record-header]`
    /// prefix). Slicing from the script start keeps relative jumps wrapping
    /// against the slice base (index 0), matching the retail
    /// `buffer_base = script_start` convention. See
    /// [`legaia_engine_scene::scene::Scene::field_man_entry_script`].
    ///
    /// REF: FUN_8003ab2c (the port lives in `legaia_scene_formats::man_section`).
    pub fn load_field_script_at(&mut self, bytecode: Vec<u8>, pc: usize) {
        self.field_bytecode = bytecode;
        self.field_pc = pc;
        self.field_ctx = FieldCtx::default();
        self.field_vm.system_pass_open = true;
    }

    /// Load one event-script record into the field VM, skipping the leading
    /// `0xFFFF 0x0000` frame-divider sentinel when present.
    ///
    /// Records pulled from `scene_event_scripts` / `scene_scripted_asset_table`
    /// containers commonly open with the 4-byte sentinel; the field VM's
    /// dispatcher in retail consumes the sentinel as a record-start marker
    /// rather than an opcode (the high bit + low-7-bits 0x7F would otherwise
    /// hit the "UNFIND INDICATION" default arm). The exact dispatcher prelude
    /// hasn't been fully traced, so this skip is heuristic - revise once
    /// `FUN_801DE840`'s outer loop is captured.
    pub fn load_field_record(&mut self, record_bytes: &[u8]) {
        const FRAME_DIVIDER: [u8; 4] = [0xFF, 0xFF, 0x00, 0x00];
        let pc = if record_bytes.starts_with(&FRAME_DIVIDER) {
            4
        } else {
            0
        };
        self.field_bytecode = record_bytes.to_vec();
        self.field_pc = pc;
        self.field_ctx = FieldCtx::default();
        self.field_vm.system_pass_open = true;
    }

    /// Activate a slot and return a mutable reference to the actor, keeping
    /// whatever state it holds - a test and viewer convenience. The retail
    /// allocator's initialisation is [`Actor::init_allocated`].
    ///
    /// REF: FUN_80020DE0
    pub fn spawn_actor(&mut self, slot: usize) -> &mut Actor {
        let a = &mut self.actors[slot];
        a.active = true;
        a
    }

    /// Ensure the slot at `id` is initialized with the supplied default
    /// position and active. Idempotent.
    ///
    /// Preserves `tmd_binding` and `active_animation` across the reset so
    /// that `init_scene_animations` bindings survive the first field-VM
    /// actor-spawn opcode.
    pub fn ensure_actor(&mut self, id: u8, default_pos: ActorVmPosition) -> &mut Actor {
        let a = &mut self.actors[id as usize];
        if !a.active {
            let tmd_binding = a.tmd_binding;
            let active_animation = a.active_animation.take();
            *a = Actor::new();
            a.tmd_binding = tmd_binding;
            a.active_animation = active_animation;
            a.active = true;
        }
        a.default_pos = default_pos;
        a
    }

    /// Pre-bind every actor slot to its scene resources before the field VM
    /// spawns actors. Wires:
    ///
    /// - `actor.tmd_binding = slot_idx` (direct 1:1 ordering: the retail
    ///   `FUN_8001E890` loop registers TMDs in pack offset-table order -
    ///   actor K → TMD slot K).
    /// - `actor.active_animation` seeded from ANM record 0 (idle) when an
    ///   ANM pack is present for that slot.
    ///
    /// Because `ensure_actor` preserves these fields across resets, the
    /// bindings survive the first field-VM actor-spawn opcode.
    pub fn init_scene_animations(
        &mut self,
        resources: &legaia_engine_scene::scene_resources::SceneResources,
    ) {
        for (i, actor) in self.actors.iter_mut().enumerate() {
            if i < resources.tmds.len() {
                actor.tmd_binding = Some(i);
            }
            if actor.active_animation.is_none()
                && let Some(anm) = resources.anm_pack_for_actor(i)
                && let Some(rec_bytes) = anm.record_bytes(0)
            {
                let bone_count = resources
                    .tmds
                    .get(i)
                    .map(|t| t.tmd.objects.len())
                    .unwrap_or(1)
                    .max(1);
                if let Ok(player) = AnimPlayer::new(rec_bytes.to_vec(), bone_count) {
                    actor.active_animation = Some(player);
                }
            }
        }
    }

    /// Run the actor VM bytecode against this world.
    ///
    /// Convenience wrapper around [`vm::run`] that constructs a host borrow.
    pub fn run_actor_bytecode(&mut self, bytecode: &[u8]) -> Result<usize, vm::VmError> {
        let mut host = ActorVmHostImpl { world: self };
        vm::run(&mut host, bytecode)
    }

    /// Run a window-widget program against the menu window list
    /// ([`legaia_engine_menus::menu_widget::MenuWidgetState`]) - the production route
    /// into the window-script VM (`legaia_engine_vm::run`, retail
    /// `FUN_801D6628`). `bytecode` is disc bytes resolved by
    /// [`World::install_menu_overlay_tables`].
    pub fn run_menu_widget_bytes(&mut self, bytecode: &[u8]) -> Result<usize, vm::VmError> {
        vm::run(&mut self.menu.widgets, bytecode)
    }

    /// Retail `FUN_801DAFD4`'s shop-picker open: interpret the resolved
    /// open script (`DAT_801E4E38`) over the window list. Returns `false`
    /// (and leaves the list untouched) when no overlay scripts installed.
    pub fn run_shop_widget_open(&mut self) -> bool {
        let Some(bytes) = self
            .menu
            .widget_scripts
            .as_ref()
            .map(|s| s.shop_open.clone())
        else {
            return false;
        };
        self.run_menu_widget_bytes(&bytes).is_ok()
    }

    /// Retail `FUN_801DAFD4`'s Sell transition: interpret the resolved
    /// slide-away script (`DAT_801E4E54`), closing the picker windows
    /// while the gold + vendor plates stay.
    pub fn run_shop_widget_sell_away(&mut self) -> bool {
        let Some(bytes) = self
            .menu
            .widget_scripts
            .as_ref()
            .map(|s| s.shop_sell_away.clone())
        else {
            return false;
        };
        self.run_menu_widget_bytes(&bytes).is_ok()
    }

    /// Step the move VM once for the actor at `slot`, using `bytecode` as
    /// the move buffer. Returns the [`vm::move_vm::StepResult`].
    ///
    /// Engines typically call this in a loop on each per-frame actor tick
    /// until the inner step returns `Halt` or `Wait`.
    ///
    /// Writes the host's `move_bytecode_write_u16` calls (issued by ext
    /// sub-ops 0x04 / 0x1B / 0x1E / 0x36) back to `world.move_bytecode[slot]`
    /// after step completes - see the `MoveVmHostImpl` deferred-writes map.
    pub fn step_move_vm(&mut self, slot: usize, bytecode: &[u16]) -> vm::move_vm::StepResult {
        let mut host = MoveVmHostImpl {
            world: self,
            current_slot: Some(slot),
            deferred_writes: std::collections::BTreeMap::new(),
            field_record_words: None,
            child_spawns: Vec::new(),
        };
        // The VM steps a copy: the host answers position queries out of
        // `world.actors`, and the stepping actor can be the one asked about.
        let mut actor_state = host.world.actors[slot].move_state.clone();
        let result = vm::move_vm::step(&mut host, &mut actor_state, bytecode);
        host.world.actors[slot].move_state = actor_state;
        let writes = std::mem::take(&mut host.deferred_writes);
        if !writes.is_empty()
            && let Some(buf) = self.move_vm.bytecode.get_mut(slot)
        {
            for (off, value) in writes {
                if off >= buf.len() {
                    buf.resize(off + 1, 0);
                }
                buf[off] = value;
            }
        }
        result
    }

    /// Run one battle-action state-machine step.
    pub fn step_battle(&mut self) -> StepOutcome {
        // Anim commit first (the retail frame order: FUN_8004AD80 ran last
        // frame after the SM staged its id), so the SM observes converged
        // `current_anim` / cleared `ADVANCE_DONE` for clip-less actors and
        // sees in-flight staged clips otherwise.
        self.commit_staged_battle_anims();
        // A new action opens: the previous action's cast-module scene does not
        // carry into it ([`Self::retire_cast_scene_at_action_begin`]).
        if self.battle_ctx.action_state
            == legaia_engine_battle_vm::battle_action::ActionState::Begin.as_byte()
        {
            self.retire_cast_scene_at_action_begin();
        }
        // Step against a clone of the context, then write it back. The
        // previous raw-pointer alias required that no host method ever
        // touch `world.battle_ctx`; `BattleActionHost::ui_element` now
        // legitimately READS `battle_ctx.active_actor` (the retail effect
        // spawn is seated at the acting actor), so the alias is gone.
        // Host methods still must not WRITE `world.battle_ctx` - the
        // write-back would clobber it.
        let mut ctx = self.battle_ctx.clone();
        let pre_state = ctx.action_state;
        let pre_banner = ctx.levelup_banner_element;
        let pre_cursor = ctx.turn_cursor;
        let mut host = BattleHostImpl { world: self };
        let out = legaia_engine_battle_vm::battle_action::step(&mut host, &mut ctx);
        // One host path does write the context: the summon stager's strike
        // folds the cast inside the step, and the fold's level check stamps
        // the banner element `ctx[+0x26]` (`sb v0,0x26(v1)` at `0x801E723C`,
        // `World::accrue_summon_spell_xp`). Carry that store over the
        // write-back, unless the step itself wrote the byte; clobbered, the
        // Done band seeded its `0x3C` hold instead of `0x96` and its teardown
        // never unloaded the banner.
        let host_banner = self.battle_ctx.levelup_banner_element;
        // The second: a cast module's turn steal bumps the turn cursor
        // `ctx[+0x1A]` from inside the same stager call (PROT 0955's arms,
        // PROT 0925's round tail - `World::run_cast_module_code` writes it
        // back). The SM never writes the cursor in a summon-band state, so
        // a moved host value is the module's and survives the write-back.
        let host_cursor = self.battle_ctx.turn_cursor;
        self.battle_ctx = ctx;
        if host_banner != pre_banner && self.battle_ctx.levelup_banner_element == pre_banner {
            self.battle_ctx.levelup_banner_element = host_banner;
        }
        if host_cursor != pre_cursor && self.battle_ctx.turn_cursor == pre_cursor {
            self.battle_ctx.turn_cursor = host_cursor;
        }
        // The third: the Spikefish flute's flee (`World::spikefish_outcome`)
        // stores the run band's wait state and its countdown from inside
        // the module, and the SM carries the escape outcome on
        // `absorbed_seru` (`run::run_wait`).
        if std::mem::take(&mut self.casting.module_flee) {
            self.battle_ctx.action_state =
                legaia_engine_battle_vm::battle_action::ActionState::RunWait.as_byte();
            self.battle_ctx.frame_timer = FLUTE_FLEE_WAIT_FRAMES;
            self.battle_ctx.absorbed_seru = 1;
        }
        // The frame driver counts the timed message's hold down straight
        // after the SM step (`FUN_80046A20`, `jal 0x801E295C` then the
        // `0x801F6964` block).
        self.tick_timed_message();
        self.track_block_approach_terms(pre_state, &out);
        // The strike chain's exit re-arms the steal latch: the arm that
        // writes `0x1F` into the action state at `0x801E3A7C` clears
        // `ctx[+0x27]` in its jump's delay slot (`sb zero, 0x16(s5)` at
        // `0x801E3A84`, `s5 = ctx + 0x11`), for monsters' chains as well as
        // the party's - so each attacking action gets one steal roll.
        self.apply_battle_frame_step_seed();
        if let StepOutcome::Transition { from, to } = out {
            self.battle.steal.on_action_transition(from, to);
            // A new action's seed ends whatever flight held the last one's
            // effect lists.
            if to == legaia_engine_battle_vm::battle_action::ActionState::ActionSeed.as_byte() {
                self.casting.homing_holds_lists = None;
            }
        }
        if let StepOutcome::Transition { from, to } = out
            && log::log_enabled!(log::Level::Debug)
        {
            let slot = self.battle_ctx.active_actor;
            let (latch, queued, cursor) = self
                .actors
                .get(usize::from(slot))
                .map(|a| {
                    (
                        a.battle.flag_bits.0,
                        a.battle.queued_anim,
                        a.battle.strike_index,
                    )
                })
                .unwrap_or_default();
            log::debug!(
                "battle sm: slot {slot} {from:#04x} -> {to:#04x} (flags {latch:#04x}, queued {queued:#04x}, cursor {cursor:#04x})"
            );
        }
        out
    }
}
