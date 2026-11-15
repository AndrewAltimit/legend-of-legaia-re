//! The battle message banner's element ids and its raised line - the
//! top-of-screen widget screen elements `0x59` / `0x65` / `0x66` share.
//! `engine-core`'s `world::battle::message_banner` raises and retires it.

use crate::battle_state::BattleState;

/// Screen element of the Seru-absorb banner.
pub const ABSORB_BANNER_ELEMENT: u8 = 0x59;

/// Screen element of the magic-level-increased banner.
pub const MAGIC_LEVEL_BANNER_ELEMENT: u8 = 0x65;

/// Screen element of the timed message (the counterattack line, "No
/// effect.").
pub const TIMED_MESSAGE_ELEMENT: u8 = 0x66;

/// The counterattack line, `s_Counterattack_successful_801CED18`.
pub const COUNTER_MESSAGE_VA: u32 = 0x801C_ED18;

/// The hold the counterattack swap seeds (`li v0,0x78` / `sw v0,0x6964` at
/// `0x801E3650` / `0x801E3658`).
pub const COUNTER_MESSAGE_HOLD: i32 = 0x78;

/// One raised message banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleMessageBanner {
    /// The screen element that raised it (`0x59` / `0x65` / `0x66`) - the
    /// unload that retires it names the same id.
    pub element: u8,
    /// The composed line (retail's `ctx + 0x1F9` buffer, or the overlay
    /// string a timed message points at).
    pub text: String,
    /// The timed message's remaining hold `0x801F6964`, in frames; `0` for
    /// the two elements that stay until their own unload.
    pub hold: i32,
    /// The element's glide between its two seats, a tracked-widget record
    /// like every other raise and unload of the spawner `FUN_801D8DE8`
    /// registers (`FUN_801DB7B0` at `0x801D93D8`): a raise (`mode 0`)
    /// spawns at seat A `(16, -24)` and glides down onto seat B `(16, 14)`,
    /// an unload (`mode 1`) glides back up and the line goes once it lands.
    /// `None` once the raise has landed.
    pub glide: Option<legaia_engine_battle_vm::battle_commit_log::LogLaunch>,
}

/// Seat A / seat B rows of the three message elements' records (`0x59`,
/// `0x65`, `0x66`), off the placement table.
pub const MESSAGE_BANNER_SEATS_Y: (i32, i32) = (-24, 14);

impl BattleMessageBanner {
    /// A freshly raised line: spawned at seat A, gliding to seat B.
    pub fn raised(element: u8, text: String, hold: i32) -> Self {
        Self {
            element,
            text,
            hold,
            glide: Some(legaia_engine_battle_vm::battle_commit_log::LogLaunch::new(
                false,
            )),
        }
    }

    /// Whether the line is gliding back out after its unload.
    pub fn leaving(&self) -> bool {
        self.glide.is_some_and(|g| g.inbound)
    }

    /// The line's row offset from seat B this frame: `FUN_801D9BBC`'s linear
    /// step from seat A down onto seat B on the raise, from seat B back up to
    /// seat A on the unload, `0` at rest.
    pub fn dy(&self) -> i32 {
        let (a, b) = MESSAGE_BANNER_SEATS_Y;
        match self.glide.as_ref() {
            Some(g) if g.inbound => {
                if g.settled() {
                    a - b
                } else {
                    (a - b) * i32::from(g.elapsed) / i32::from(g.total.max(1))
                }
            }
            g => legaia_engine_battle_vm::battle_commit_log::LogLaunch::seat_dy(g, a, b),
        }
    }
}

impl BattleState {
    /// Stage the magic-level line and raise element `0x65` - the port's seat
    /// of `FUN_801F452C`'s compose followed by `FUN_801E70BC`'s raise.
    pub fn raise_magic_level_banner(&mut self, text: String) {
        self.message_banner = Some(BattleMessageBanner::raised(
            MAGIC_LEVEL_BANNER_ELEMENT,
            text,
            0,
        ));
    }

    /// The `FUN_801D8DE8(element, 1)` unload of the line `element` raised:
    /// the record glides back up to seat A, and the line goes once it lands
    /// ([`Self::retire_landed_message_banner`]).
    ///
    /// REF: FUN_801D8DE8 (`0x801D935C..0x801D93D8`, a mode-1 spawn at seat B)
    pub fn unload_message_banner(&mut self, element: u8) {
        if let Some(b) = self.message_banner.as_mut()
            && b.element == element
            && !b.leaving()
        {
            b.glide = Some(legaia_engine_battle_vm::battle_commit_log::LogLaunch::new(
                true,
            ));
        }
    }

    /// Drop a line whose unload glide has landed, and the record of a raise
    /// that has.
    pub fn retire_landed_message_banner(&mut self) {
        let Some(b) = self.message_banner.as_mut() else {
            return;
        };
        match b.glide {
            Some(g) if g.settled() && g.inbound => self.message_banner = None,
            Some(g) if g.settled() => b.glide = None,
            _ => {}
        }
    }

    /// The battle frame driver's hold countdown for the timed message
    /// (`FUN_80046A20`, `0x80047058..0x80047098`): run after the action-SM
    /// step, `hold -= DAT_1F800393` while it is positive, and the element is
    /// unloaded (`FUN_801D8DE8(0x66, 1)`) once it reaches `0`.
    ///
    /// PORT: FUN_80046A20 (`0x80047058..0x80047098`, the `0x801F6964` hold)
    pub fn tick_timed_message(&mut self) {
        // Per battle pass by the frame step, so one a vsync tick
        // ([`crate::hud_glides::BATTLE_PASS_STEP_PER_TICK`]).
        let step = i32::from(crate::hud_glides::BATTLE_PASS_STEP_PER_TICK);
        let Some(b) = self.message_banner.as_mut() else {
            return;
        };
        if b.element != TIMED_MESSAGE_ELEMENT || b.hold <= 0 {
            return;
        }
        b.hold -= step;
        if b.hold <= 0 {
            self.unload_message_banner(TIMED_MESSAGE_ELEMENT);
        }
    }
}
