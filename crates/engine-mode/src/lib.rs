//! The game-mode seat: the master mode word every retail overlay hands the
//! frame through, as one `World`-free state machine.
//!
//! Retail runs 28 game modes off one word (`_DAT_8007B83C`) and a per-mode
//! `(init, frame)` handler pair; the seat ([`mode::ModeSeat`]) owns that word,
//! the boot chain that reaches the title, and the stage each mode is in. It
//! is not field-specific - the field is three of the modes - so it sits below
//! `legaia-engine-field`, whose camera only reads [`mode::SceneMode`].
//!
//! [`minigame_entry`] is the mode-entry side of the minigame modes and
//! [`card_request`] the leaf that asks the seat for the card / menu mode;
//! [`sfx_residency`] is what each mode's initialiser does to the SPU region
//! VAB slots `2` and `6` share.
//! See the crate README for the layering.

#![forbid(unsafe_code)]

pub mod card_request;
pub mod minigame_entry;
pub mod mode;
pub mod sfx_residency;
