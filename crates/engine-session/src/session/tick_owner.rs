//! Which screen owns a sim tick: the one order both play hosts hand each
//! tick through.
//!
//! A play host's tick goes to the first owner that takes it. The native
//! window's `sim_tick` and the browser page's `frame_ticks` used to spell
//! that order each, in their own words, and the two orders differed (the
//! page asked the pause menu first, the window the naming prompt). The order
//! is [`BootSession::claim_tick`] now, and each host only acts on the
//! [`TickOwner`] it hands back: the parts of an owner the session can do
//! itself - step the naming prompt, take the prologue hand-off - are done
//! by the time it returns.

use legaia_engine_audio::AudioSink;
use legaia_engine_system::input::PadButton;

use super::BootSession;

/// Who owns one sim tick ([`BootSession::claim_tick`]).
///
/// Tuple variants on purpose: the host-drift gate's frame-path tier reads a
/// host's arms by their header text, and a braced pattern would hide it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickOwner {
    /// The naming prompt took the tick and has been stepped on its edge
    /// (`step_name_entry_frame`). `true` = the name committed this tick.
    NameEntry(bool),
    /// Retail's prologue intro skip (`FUN_801D1344`): a Cross press during
    /// the opening chain handed off to the named scene. The host enters it
    /// (and rebuilds whatever it draws the scene with); the tick is spent.
    PrologueSkip(&'static str),
    /// No modal owner: the host's own screens, then the field.
    Field,
}

impl<S: AudioSink> BootSession<S> {
    /// Hand one sim tick to its owner, in the one order both play hosts
    /// share. `edge` is the tick's just-pressed pad bits.
    // REF: FUN_801D1344
    pub fn claim_tick(&mut self, edge: u16) -> TickOwner {
        // The naming prompt is modal: retail parks the opening's script on
        // it, so while it is up it takes the tick.
        if self.step_name_entry_frame(edge) {
            return TickOwner::NameEntry(!self.host.world.name_entry_active());
        }
        // The prologue intro skip: while the opening chain plays with the
        // trigger bit armed, a Cross press skips the whole remaining opening.
        if let Some(target) = self
            .host
            .world
            .take_prologue_handoff(edge & PadButton::Cross.mask() != 0)
        {
            return TickOwner::PrologueSkip(target);
        }
        TickOwner::Field
    }
}
