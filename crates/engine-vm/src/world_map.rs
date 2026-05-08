//! World-map entity state machine, ported clean-room from `FUN_801DA51C`
//! (overlay_world_map.bin base `0x801C0000`).
//!
//! One instance of [`WorldMapEntityCtx`] exists per on-map entity (NPCs,
//! town-portal tiles, monster spawn zones). The retail engine stores the
//! state in a per-entity record; the engine-side host trait bridges between
//! this SM and whatever data structure the engine uses.
//!
//! ## State machine
//!
//! ```text
//!   Idle (0) ──encounter──► Idle (0, with encounter handler invoked)
//!   Idle (0) ──interact──►  Idle (0, with interact handler invoked)
//!   Idle (0) ──[SM sets]──► Activating (1)
//!   Activating (1) ──countdown=0──► Transitioning (2)
//!   Transitioning (2/3) ──────────► Terminal (4)
//! ```
//!
//! States 2 and 3 share the same handler body (fall-through in the original C
//! switch). State 4 is a terminal stop state — the entity stops ticking.
//!
//! ## Source
//!
//! `ghidra/scripts/funcs/801da51c.txt` (decompiled from `overlay_world_map.bin`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityState {
    Idle = 0,
    Activating = 1,
    Transitioning = 2,
    Terminal = 4,
}

impl EntityState {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => EntityState::Idle,
            1 => EntityState::Activating,
            2 | 3 => EntityState::Transitioning,
            _ => EntityState::Terminal,
        }
    }
}

/// Per-entity SM state. Corresponds to fields within the world-map entity
/// record at the following offsets (from `FUN_801DA51C`):
///
/// - `state`    ← `entity[+0x8A]` (`i16`)
/// - `pad_flags`← `entity[+0x10]` (`u32`): bit `0x80000` = walking-blocked; bit `0x100` = interact-cooldown
/// - `field_88` ← `entity[+0x88]` (`u16`)
#[derive(Debug, Clone, Default)]
pub struct WorldMapEntityCtx {
    /// Movement-state discriminant. Values 0..=4 are defined; see
    /// [`EntityState`].
    pub state: u16,
    /// Packed pad / flag bits for this entity. The SM mutates bits `0x80000`
    /// and `0x100`. Bit semantics per `FUN_801DA51C`:
    /// - `0x80000` = walking / movement-blocked flag (set in Activating state, cleared on scene transition)
    /// - `0x100` = interaction-cooldown flag (set once per interaction cycle)
    pub pad_flags: u32,
    /// Auxiliary field. Cleared by the SM on state advances (from `+0x88`).
    pub field_88: u16,
}

/// Engine-side callbacks consumed by [`step`].
///
/// Each method documents the retail global / call it replaces.
pub trait WorldMapEntityHost {
    /// `_DAT_8007b868 == 0`. When the door/portal is closed this gate is set
    /// non-zero and the whole SM body is skipped (only the post-SM interaction
    /// path below state-0 still runs when the gate is open elsewhere).
    fn activation_gate_open(&self) -> bool;

    /// `DAT_8007b604` — signed encounter-rate countdown shared across all
    /// entities. Decremented in the Idle state; the SM reads and writes it
    /// via the two methods below.
    fn encounter_countdown(&self) -> i8;
    fn set_encounter_countdown(&mut self, v: i8);

    /// `DAT_8007b5f8 != 0` — encounter-rate flag. When zero, encounters are
    /// disabled regardless of the countdown reaching zero.
    fn encounter_enabled(&self) -> bool;

    /// Called when the countdown hits zero and encounters are enabled.
    /// Wraps `FUN_801D9E1C(entity, resolver_result)`. The `resolver_result`
    /// is the return value of `FUN_800243F0` (BGM/asset resolver) at the
    /// start of the tick.
    fn on_encounter(&mut self, entity_idx: usize, resolver_result: u32);

    /// Called during the Activating → Transitioning advance when the
    /// countdown drains to zero. The engine should copy any pending scene
    /// data and set up the transition. Wraps the block starting with
    /// `func_0x8004313c()` in case 1.
    fn on_activating(&mut self, entity_idx: usize);

    /// Called in states 2 / 3 (and the fall-through from state 1) to
    /// perform the actual scene transition. Wraps `func_0x8003ce34(0x35)`,
    /// `_DAT_8007b5f4 = 1`, fade-globals, `_DAT_8007b83c = 8`.
    fn on_scene_transition(&mut self, entity_idx: usize);

    /// `_DAT_1f800394 & 0x8000` — dialog / menu is active. When set, the
    /// post-SM interaction check is suppressed.
    fn dialog_active(&self) -> bool;

    /// `_DAT_8007c364[+0x10] & 0x80000` — player's movement-blocked flag.
    /// When set alongside the entity's interact-cooldown being clear, the
    /// interaction check is skipped.
    fn player_walking(&self) -> bool;

    /// Called when the SM determines the entity should be interacted with.
    /// Wraps `func_0x80039b7c(entity)`.
    fn on_interact(&mut self, entity_idx: usize);

    /// `_DAT_8007b6b0 == -1000`. Sentinel check run after the interact call.
    fn encounter_counter_is_sentinel(&self) -> bool;

    /// `_DAT_8007b6b0 = 0`. Clears the encounter counter when the sentinel
    /// was detected.
    fn clear_encounter_counter(&mut self);
}

/// Step one frame of the world-map entity state machine.
///
/// `entity_idx` is the engine's slot index for this entity (passed through to
/// the host callbacks for context).
pub fn step<H: WorldMapEntityHost>(entity_idx: usize, ctx: &mut WorldMapEntityCtx, host: &mut H) {
    if host.activation_gate_open() {
        let countdown = host.encounter_countdown();
        match ctx.state {
            0 => {
                // Idle: decrement encounter countdown; fire encounter when it
                // hits 0 and the encounter rate is enabled.
                if countdown == 0 && host.encounter_enabled() {
                    host.on_encounter(entity_idx, 0);
                } else {
                    host.set_encounter_countdown(countdown.saturating_sub(1));
                }
            }
            1 => {
                // Activating: set the entity-blocked flag on the player, then
                // drain the countdown. When it hits 0, advance to state 2 and
                // fall through to the Transitioning handler.
                ctx.pad_flags |= 0x80000;
                if countdown > 0 {
                    host.set_encounter_countdown(countdown - 1);
                    return;
                }
                host.on_activating(entity_idx);
                ctx.field_88 = 0;
                ctx.scene_data_consumed();
                ctx.state += 1; // 1 → 2, fall through below
                host.set_encounter_countdown(host.encounter_countdown()); // re-read
                // FALLTHROUGH to states 2/3 below:
                host.on_scene_transition(entity_idx);
                ctx.state = 4;
                ctx.pad_flags &= !0x80000;
                ctx.field_88 = 0;
                return;
            }
            2 | 3 => {
                // Transitioning: initiate scene change and move to Terminal.
                host.on_scene_transition(entity_idx);
                ctx.state = 4;
                ctx.pad_flags &= !0x80000;
                ctx.field_88 = 0;
                return;
            }
            _ => {
                // State 4 (Terminal) and any out-of-range: nothing to do.
            }
        }
    }

    // Post-SM: interaction check only while Idle.
    // Runs regardless of activation_gate_open (the gate only gates the SM
    // body above, not this path — per the original C structure).
    if ctx.state == 0 && !host.dialog_active() {
        let blocked = (ctx.pad_flags & 0x80000) != 0;
        if !blocked {
            let cooldown_set = (ctx.pad_flags & 0x100) != 0;
            let player_not_walking = !host.player_walking();
            if cooldown_set || player_not_walking {
                ctx.pad_flags |= 0x100;
                host.on_interact(entity_idx);
            }
            if host.encounter_counter_is_sentinel() {
                host.clear_encounter_counter();
            }
        }
    }
}

impl WorldMapEntityCtx {
    fn scene_data_consumed(&mut self) {
        // Marks entity[+0x94] as consumed (pointer cleared). In the retail
        // engine this zeroes a pointer; here it is a no-op since the engine
        // side owns the scene-data reference through the host.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecHost {
        gate_open: bool,
        countdown: i8,
        encounter_en: bool,
        dialog: bool,
        player_walk: bool,
        encounter_sentinel: bool,
        pub events: Vec<String>,
    }

    impl WorldMapEntityHost for RecHost {
        fn activation_gate_open(&self) -> bool {
            self.gate_open
        }
        fn encounter_countdown(&self) -> i8 {
            self.countdown
        }
        fn set_encounter_countdown(&mut self, v: i8) {
            self.countdown = v;
            self.events.push(format!("countdown={v}"));
        }
        fn encounter_enabled(&self) -> bool {
            self.encounter_en
        }
        fn on_encounter(&mut self, idx: usize, _r: u32) {
            self.events.push(format!("encounter:{idx}"));
        }
        fn on_activating(&mut self, idx: usize) {
            self.events.push(format!("activating:{idx}"));
        }
        fn on_scene_transition(&mut self, idx: usize) {
            self.events.push(format!("transition:{idx}"));
        }
        fn dialog_active(&self) -> bool {
            self.dialog
        }
        fn player_walking(&self) -> bool {
            self.player_walk
        }
        fn on_interact(&mut self, idx: usize) {
            self.events.push(format!("interact:{idx}"));
        }
        fn encounter_counter_is_sentinel(&self) -> bool {
            self.encounter_sentinel
        }
        fn clear_encounter_counter(&mut self) {
            self.events.push("clear_counter".into());
        }
    }

    #[test]
    fn idle_gate_closed_skips_sm_body() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: false,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        // Gate closed → SM body skipped, no countdown change.
        assert!(!host.events.iter().any(|e| e.starts_with("countdown")));
        assert_eq!(ctx.state, 0);
    }

    #[test]
    fn idle_gate_open_decrements_countdown() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            countdown: 5,
            encounter_en: false,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert_eq!(host.countdown, 4);
        assert_eq!(ctx.state, 0);
    }

    #[test]
    fn idle_encounter_fires_at_zero() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            countdown: 0,
            encounter_en: true,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(host.events.contains(&"encounter:0".to_string()));
    }

    #[test]
    fn idle_no_encounter_when_disabled() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            countdown: 0,
            encounter_en: false,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(!host.events.iter().any(|e| e.starts_with("encounter")));
    }

    #[test]
    fn interact_fires_when_cooldown_set_and_not_blocked() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0x100,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            dialog: false,
            player_walk: true, // player walking, but cooldown already set
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(host.events.contains(&"interact:0".to_string()));
    }

    #[test]
    fn interact_fires_when_player_not_walking() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            dialog: false,
            player_walk: false, // player stopped
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(host.events.contains(&"interact:0".to_string()));
        assert_ne!(ctx.pad_flags & 0x100, 0, "cooldown flag should be set");
    }

    #[test]
    fn interact_suppressed_by_dialog() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            dialog: true,
            player_walk: false,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(!host.events.iter().any(|e| e.starts_with("interact")));
    }

    #[test]
    fn transitioning_state_advances_to_terminal() {
        let mut ctx = WorldMapEntityCtx {
            state: 2,
            pad_flags: 0x80000,
            field_88: 1,
        };
        let mut host = RecHost {
            gate_open: true,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert_eq!(ctx.state, 4, "should advance to Terminal");
        assert_eq!(ctx.pad_flags & 0x80000, 0, "walking flag cleared");
        assert_eq!(ctx.field_88, 0);
        assert!(host.events.contains(&"transition:0".to_string()));
    }

    #[test]
    fn terminal_state_does_nothing() {
        let mut ctx = WorldMapEntityCtx {
            state: 4,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            countdown: 5,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert_eq!(ctx.state, 4);
        // No SM action, no countdown change from terminal.
        assert!(!host.events.iter().any(|e| e.starts_with("countdown")));
    }

    #[test]
    fn encounter_sentinel_cleared_after_interact() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0x100,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            dialog: false,
            player_walk: true,
            encounter_sentinel: true,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(host.events.contains(&"clear_counter".to_string()));
    }
}
