//! Arts-gauge re-arm at art start (`FUN_801E93C8`) and the damage-number popup
//! ring push (`FUN_801F44A0`).
//!
//! Two small battle-overlay state kernels that both hang off the shared battle
//! context `_DAT_8007BD24` and neither of which touches the GPU. They are
//! grouped here because each is too small to justify a module and both are
//! called at the same moment - the frame a committed action starts.
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_battle_action_801e93c8.txt`
//! and `overlay_debug_menu_801f44a0.txt`. Behaviour summaries in
//! `docs/subsystems/arts-command-gauge.md` and
//! `docs/subsystems/minigame-muscle-dome.md`.

// ---------------------------------------------------------------------------
// FUN_801E93C8 - gauge re-arm at art start
// ---------------------------------------------------------------------------

/// The neutral per-slot arm width `FUN_801E93C8` seeds into every actor's
/// `+0x21D`. The gauge builder later overwrites it with the real per-command
/// `+0x74` cost, which is why a slot briefly reads this width.
pub const ARM_WIDTH_SEED: u8 = 8;

/// The number of actor slots the re-arm walks (`0..7`).
pub const GAUGE_SLOTS: usize = 7;

/// The per-slot gauge fields `FUN_801E93C8` rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GaugeSlots {
    /// Per-actor `+0x21C` latch. Cleared **only when it holds exactly `1`** -
    /// any other value is left alone.
    pub latch: [u8; GAUGE_SLOTS],
    /// Per-actor `+0x21D` arm width, unconditionally seeded to
    /// [`ARM_WIDTH_SEED`].
    pub arm_width: [u8; GAUGE_SLOTS],
}

impl Default for GaugeSlots {
    fn default() -> Self {
        Self {
            latch: [0; GAUGE_SLOTS],
            arm_width: [0; GAUGE_SLOTS],
        }
    }
}

/// What the active actor staged, as `FUN_801E93C8` reads it. The gate differs
/// by whether the acting slot is a party member or a monster, and the two arms
/// read different fields - so the caller resolves the read and passes the
/// answer rather than this kernel reaching into the actor pool.
#[derive(Debug, Clone, Copy)]
pub enum StagedAction {
    /// Party slot (active index `< 3`): the actor's last-staged action id
    /// `+0x1D9`. The re-arm runs only while this is `< 0x10`, i.e. a plain
    /// direction command (`0x0C..=0x0F`) rather than a materialised art or
    /// starter.
    Party { action_id: u8 },
    /// Monster slot (active index `>= 3`): the flag byte `+0x87` of the art
    /// record the staged id resolves to (`record_table[slot - 3][id]` then
    /// `+0x4C`). The re-arm runs only while this is zero.
    Monster { record_flag: u8 },
}

/// Re-arm the per-actor gauge slots when a committed action begins.
///
/// Returns `true` when the re-arm ran, which is also the condition under which
/// retail clears the context's `+0x243` byte - the caller owns that write
/// because `+0x243` lives in the battle context, not in the slot array.
///
/// The live caller is the battle-action SM's `DoneCleanup` tail
/// (`crate::battle_action`'s `rearm_action_gauge`), which is where retail
/// `jal`s it. That caller maps `+0x21C` / `+0x21D` onto the SM's
/// `BattleActor::render_flag` / `BattleActor::impact_step`, and reads the
/// party gate's `+0x1D9` off `BattleActor::current_anim` - the same byte, read
/// under a different name (the SM's field list calls `+0x1D9` the current anim
/// id, `docs/subsystems/arts-command-gauge.md` calls it the last-staged action
/// id).
///
/// PORT: FUN_801E93C8
pub fn rearm_gauge(staged: StagedAction, slots: &mut GaugeSlots) -> bool {
    let gate_open = match staged {
        StagedAction::Party { action_id } => action_id < 0x10,
        StagedAction::Monster { record_flag } => record_flag == 0,
    };
    if !gate_open {
        return false;
    }
    for i in 0..GAUGE_SLOTS {
        if slots.latch[i] == 1 {
            slots.latch[i] = 0;
        }
        slots.arm_width[i] = ARM_WIDTH_SEED;
    }
    true
}

// ---------------------------------------------------------------------------
// FUN_801F44A0 - damage-number popup ring push
// ---------------------------------------------------------------------------

/// Number of slots in the popup ring (`ctx+0x262` is masked `& 7`).
pub const POPUP_RING_SLOTS: usize = 8;

/// The 8-slot damage / number popup ring in the battle context. Retail keeps
/// the three arrays at `ctx+0x83C` (value, stride 4), `ctx+0x318` (parameter,
/// stride 2) and `ctx+0x85C` (timer, stride 4), with the write cursor at
/// `ctx+0x262` and a monotonic push counter at `ctx+0x273`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DamagePopupRing {
    /// `ctx+0x83C`: the pushed value, sign-extended from the caller's i16.
    pub value: [i32; POPUP_RING_SLOTS],
    /// `ctx+0x318`: the pushed parameter byte, stored as a halfword.
    pub param: [u16; POPUP_RING_SLOTS],
    /// `ctx+0x85C`: the per-slot ramp timer, zeroed on push. The popup
    /// renderer scales the digit sprites by it.
    pub timer: [i32; POPUP_RING_SLOTS],
    /// `ctx+0x262`: write cursor, advanced `(cursor + 1) & 7`.
    pub cursor: u8,
    /// `ctx+0x273`: total pushes, wrapping at a byte. Retail never masks it.
    pub pushed: u8,
}

impl DamagePopupRing {
    /// Push one entry into the ring.
    ///
    /// The value is the caller's sign-extended i16 (so a negative number - a
    /// heal - stores negative), the parameter is truncated to its low byte, and
    /// the slot's timer is reset to zero. The cursor then advances modulo 8 and
    /// the push counter increments, wrapping at 256.
    ///
    /// PORT: FUN_801F44A0
    pub fn push(&mut self, value: i16, param: u8) {
        let i = (self.cursor & 0x7) as usize;
        self.value[i] = value as i32;
        self.param[i] = param as u16;
        self.timer[i] = 0;
        self.cursor = (self.cursor.wrapping_add(1)) & 0x7;
        self.pushed = self.pushed.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn party_gate_admits_direction_ids_and_rejects_materialised_arts() {
        let mut s = GaugeSlots {
            latch: [1; GAUGE_SLOTS],
            arm_width: [0; GAUGE_SLOTS],
        };
        assert!(rearm_gauge(StagedAction::Party { action_id: 0x0C }, &mut s));
        assert_eq!(s.latch, [0; GAUGE_SLOTS]);
        assert_eq!(s.arm_width, [ARM_WIDTH_SEED; GAUGE_SLOTS]);

        let mut s = GaugeSlots {
            latch: [1; GAUGE_SLOTS],
            arm_width: [0; GAUGE_SLOTS],
        };
        assert!(!rearm_gauge(
            StagedAction::Party { action_id: 0x10 },
            &mut s
        ));
        // Nothing is touched on the closed gate.
        assert_eq!(s.latch, [1; GAUGE_SLOTS]);
        assert_eq!(s.arm_width, [0; GAUGE_SLOTS]);
    }

    #[test]
    fn monster_gate_reads_the_record_flag_not_the_action_id() {
        let mut s = GaugeSlots::default();
        assert!(rearm_gauge(
            StagedAction::Monster { record_flag: 0 },
            &mut s
        ));
        assert_eq!(s.arm_width, [ARM_WIDTH_SEED; GAUGE_SLOTS]);

        let mut s = GaugeSlots::default();
        assert!(!rearm_gauge(
            StagedAction::Monster { record_flag: 1 },
            &mut s
        ));
        assert_eq!(s.arm_width, [0; GAUGE_SLOTS]);
    }

    #[test]
    fn only_a_latch_of_exactly_one_is_cleared() {
        let mut s = GaugeSlots {
            latch: [0, 1, 2, 3, 1, 5, 200],
            arm_width: [0; GAUGE_SLOTS],
        };
        assert!(rearm_gauge(StagedAction::Party { action_id: 0 }, &mut s));
        assert_eq!(s.latch, [0, 0, 2, 3, 0, 5, 200]);
    }

    #[test]
    fn popup_ring_wraps_at_eight_and_zeroes_the_timer() {
        let mut r = DamagePopupRing::default();
        for i in 0..8 {
            r.push(100 + i as i16, i as u8);
        }
        assert_eq!(r.cursor, 0, "cursor wraps back to slot 0");
        assert_eq!(r.pushed, 8);
        assert_eq!(r.value[7], 107);
        assert_eq!(r.param[7], 7);

        r.timer[0] = 42;
        r.push(-5, 0xFF);
        assert_eq!(r.value[0], -5, "value is sign-extended");
        assert_eq!(r.param[0], 0xFF);
        assert_eq!(r.timer[0], 0, "push resets the slot ramp timer");
        assert_eq!(r.cursor, 1);
        assert_eq!(r.pushed, 9);
    }

    #[test]
    fn push_counter_wraps_at_a_byte() {
        let mut r = DamagePopupRing {
            pushed: 255,
            ..Default::default()
        };
        r.push(1, 0);
        assert_eq!(r.pushed, 0);
    }
}
