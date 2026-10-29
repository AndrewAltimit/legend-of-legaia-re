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
}

impl BattleState {
    /// Stage the magic-level line and raise element `0x65` - the port's seat
    /// of `FUN_801F452C`'s compose followed by `FUN_801E70BC`'s raise.
    pub fn raise_magic_level_banner(&mut self, text: String) {
        self.message_banner = Some(BattleMessageBanner {
            element: MAGIC_LEVEL_BANNER_ELEMENT,
            text,
            hold: 0,
        });
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
            self.message_banner = None;
        }
    }
}
