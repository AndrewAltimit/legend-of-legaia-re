//! `legaia-engine-capture` - standing the engine where a retail capture
//! stood.
//!
//! A retail save state is a frame taken somewhere inside a script, a fight or
//! a camera move. To compare the engine against it the engine has to be put
//! at the same phase, and two processes do that: the headless comparison
//! (`legaia-parity`), which reads the phase out of the state's RAM, and the
//! `play-window` image child it launches, which reads the same phase back
//! from its environment. This crate is the vocabulary the two share: the
//! seed records, the gates the engine is run to, the pad drives that reach
//! them, and each one's environment form in both directions.
//!
//! Everything here is engine-side. Nothing reads an emulator state or a RAM
//! image - deriving a seed from retail RAM is `legaia-parity`'s - so a play
//! host may link it.
//!
//! - [`phase_gate`] - [`PhaseGate`](phase_gate::PhaseGate), the point in an
//!   in-flight battle action a capture sits at.
//! - [`battle_drive`] - [`BattleDrive`](battle_drive::BattleDrive), the pad
//!   path that reaches a battle capture past the round prompt.
//! - [`script_gate`] - [`ScriptGate`](script_gate::ScriptGate), a field
//!   script's phase, and the drive that resumes its record.
//! - [`bar_seeds`], [`hud_glide`], [`camera_align`] - a fight's bars, HUD
//!   glides and camera tween at the capture instant.
//! - [`cast_parts`] - a captured cast's live parts, seated over the replay's
//!   own on the capture frame.
//! - [`ghost_ring`] - a captured body's after-image ghosts, seated on its
//!   history ring on the capture frame.
//! - [`formation_cell`] - [`FormationCell`](formation_cell::FormationCell), a
//!   fight named by its formation cell where no MAN row carries it.
//! - [`field_seeds`] - the field-side seeds: object clips, walkers, morphs,
//!   fog, sprite arms, CLUT cyclers, scroller rects, panel, camera block.

pub mod bar_seeds;
pub mod battle_drive;
pub mod camera_align;
pub mod cast_parts;
pub mod clip_cursors;
pub mod effect_children;
pub mod field_seeds;
pub mod formation_cell;
pub mod ghost_ring;
pub mod ground_snap;
pub mod hud_glide;
pub mod melee_hits;
pub mod phase_gate;
pub mod script_gate;

/// Engine ticks run between seeding a session from a capture and sampling
/// it.
pub const SETTLE_TICKS: u64 = 60;
