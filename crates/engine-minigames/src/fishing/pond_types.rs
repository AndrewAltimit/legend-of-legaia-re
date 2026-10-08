//! The pond session's phase, input, event and venue types.
//! Split out of `fishing.rs`.

use super::*;

/// Which phase of a [`PondSession`] is live, mirroring the retail mode-SM
/// states (`FUN_801cf3bc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PondPhase {
    /// State `0xc`: idle at the shore, waiting for the cast press.
    Idle,
    /// State `0xd`: cast wind-up (~12 frames of camera pan).
    WindUp,
    /// State `0x14`: the casting-power oscillator, until the lock press.
    Power,
    /// States `0x1e`..`0x22`: the lure flies out and settles (the landing is
    /// the cast-counter increment).
    Flight,
    /// The pre-hook loop: band roll / cadence / strike checks per frame.
    Waiting,
    /// A fish is hooked: the reel tug-of-war.
    Hooked,
    /// The fight resolved with a landed catch.
    Landed,
    /// The fight resolved with a snapped line.
    Snapped,
}

/// One frame of player input to [`PondSession::tick`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PondInput {
    /// The held packed pad word (`_DAT_8007b850`) the pond reads: bits
    /// `0x40` (Cross / reel A) and `0x80` (Square / reel B) for the reel
    /// decoder, and the D-pad (`0x2000` right, `0x4000` down, `0x8000` left)
    /// the rod actor's lift and roll read ([`crate::fishing_actors::RodActor::drive`]).
    pub reel_mask: u32,
    /// The cast / confirm edge (Circle `0x20` in retail; `X` on both browser pages, and Space as well
    /// on the minigames page).
    pub cast_edge: bool,
    /// The strike credit's pad nudge this frame - what
    /// [`crate::fishing_actors::bite_pad_nudge`] counts off the newly-pressed
    /// word (D-pad left, D-pad right, the reel pair as one mask).
    pub edge_bonus: i32,
}

impl PondInput {
    /// One frame's input off the engine pad pair (PSX pad layout, this frame
    /// and the last) - the build `World::tick_fishing` runs for the play
    /// hosts.
    ///
    /// The reel bits are **held** Cross / Square (`_DAT_8007b850 & 0xc0`),
    /// the cast edge a **new** Circle press, and the strike credit's nudge
    /// is [`crate::fishing_actors::bite_pad_nudge`] over the newly-pressed
    /// word rotated into the packed retail layout (`_DAT_8007B874`): D-pad
    /// left, D-pad right and the reel pair as one mask, never the cast.
    pub fn from_engine_pad(pad: u16, pad_prev: u16) -> Self {
        use legaia_engine_vm::pad::PadButton as B;
        let mut reel_mask = 0u32;
        if pad & B::Cross.mask() != 0 {
            reel_mask |= REEL_A_PAD_BIT;
        }
        if pad & B::Square.mask() != 0 {
            reel_mask |= REEL_B_PAD_BIT;
        }
        // The rod's D-pad bits, in the packed layout (`_DAT_8007B850`).
        for (b, bit) in [
            (B::Right, ROD_PAD_RIGHT),
            (B::Down, ROD_PAD_DOWN),
            (B::Left, ROD_PAD_LEFT),
        ] {
            if pad & b.mask() != 0 {
                reel_mask |= bit;
            }
        }
        let pressed = pad & !pad_prev;
        PondInput {
            reel_mask,
            cast_edge: pressed & B::Circle.mask() != 0,
            edge_bonus: crate::fishing_actors::bite_pad_nudge(u32::from(pressed.rotate_right(8))),
        }
    }
}

/// A per-frame event the presentation layer reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PondEvent {
    /// A reel cadence matched: the "Good!" strike splash.
    Splash,
    /// A fish struck and hooked; the payload is the species id.
    Hooked(usize),
    /// The fight landed the fish for this many points.
    Landed(i32),
    /// The line snapped.
    Snapped,
    /// The session returned to the idle shore: a resolved fight was
    /// dismissed, or an empty line was reeled all the way in. Hosts seed the
    /// auxiliary recast banner off it.
    Recast,
}

/// The venue-faithful fishing session: the retail cast -> wait -> strike ->
/// fight -> score loop over the disc's species / spawn / cadence tables and
/// the save block's persistent lure, rod, cast-counter and point record.
///
/// Composes the pinned kernels ([`CastPower`], [`ReelCadence`], [`BandCheck`],
/// [`band4_gate`], [`spawn_species`], [`TensionGauge`] via the fight,
/// [`FishingRecord`]) with the reconstruction glue each doc-comment marks
/// (flight timing, the record reel-down rate, the snap-at-max-tension loss).
#[derive(Debug, Clone)]
pub struct PondSession {
    /// The 10-record species table (disc rodata).
    pub species: Vec<FishingSpecies>,
    /// This venue's `8 x 8` spawn page (disc rodata).
    pub spawn: Vec<[u32; 8]>,
    /// Venue: `0` Buma pond, `1` Vidna pond (`DAT_801d90d0`).
    pub venue: usize,
    /// Persistent equipped-lure row (`_DAT_80084450`, 0..=2).
    pub lure: u32,
    /// Persistent rod stat (`_DAT_80084454`, 0..=2).
    pub rod: i32,
    /// Persistent lifetime cast counter (`_DAT_80084460`).
    pub casts: i32,
    /// Persistent point record (`_DAT_8008444C` / `58` / `5C`).
    pub record: FishingRecord,
    /// Persistent one-time prize bitmask (`_DAT_8008446C`).
    pub purchased_mask: u32,

    pub(super) phase: PondPhase,
    pub(super) cast: CastPower,
    pub(super) cadence: ReelCadence,
    pub(super) band: BandCheck,
    pub(super) rng: BiosRand,
    /// Phase-local frame counter (wind-up / flight).
    pub(super) timer: i32,
    /// Line record (`DAT_801d927c`); seeded from the locked cast power.
    pub(super) line_record: i32,
    /// Line depth (`DAT_801d9298`).
    pub(super) depth: i32,
    /// Fish lateral offset during the fight (dart push accumulator).
    pub(super) lateral: i32,
    pub(super) fight_species: Option<usize>,
    pub(super) fish: FishAi,
    pub(super) gauge: TensionGauge,
    /// Accumulated fight strength (`DAT_801d91b8`).
    pub(super) strength: i32,
    /// Points awarded by the last landed catch.
    pub(super) last_award: i32,
    /// The landed catch's result-plate counter (the result actor's `+0x1A`
    /// in `FUN_801D5298`): `0` on the landing, `+4` per frame step, held at
    /// `0x1000`. [`PondSession::catch_result`] derives the plate's brightness
    /// and the name's rise from it.
    pub(super) result_ramp: i32,
    pub(super) events: Vec<PondEvent>,
    /// The venue the lure is cast into, once a host attaches one.
    pub(super) venue_map: Option<PondVenue>,
    /// The live cast lure (`0x801D9174` / `0x801D918C`), from the cast lock
    /// until the line is reeled back in.
    pub(super) lure_actor: Option<crate::fishing_actors::LureActor>,
    /// Last frame's lure probe - the water class the strike credit and the
    /// hooked fish's weight come off.
    pub(super) lure_probe: crate::fishing_actors::LureProbe,
    /// The venue's hub menu, while it is up ([`crate::fishing_hub`]).
    pub(crate) hub: Option<crate::fishing_hub::FishingHub>,
    /// The rod actor (`FUN_801D1C5C`), from the cast lock until its recover
    /// swing retires it.
    pub(super) rod_actor: Option<crate::fishing_actors::RodActor>,
    /// This frame's line, once a host has projected the fish end
    /// ([`Self::line_frame`]); cleared by every [`Self::tick`].
    pub(super) line: Option<crate::fishing_actors::FishingLine>,
    /// Whether [`Self::line_frame`] has run since the last tick.
    pub(super) line_latched: bool,
    /// The fish end's last unclipped screen point (`0x801D9198`), which the
    /// next frame's rod yaw is measured against.
    pub(super) line_fish_prev: Option<(i16, i16)>,
}

/// The venue bytes a host attaches so the cast lure has a world to land in.
///
/// Retail reads both regions off the one resident scene buffer
/// (`*_DAT_1F8003EC`); the port keeps the same buffer and the parsed
/// `+0x10000` region block beside it, because
/// [`legaia_engine_vm::field_regions::RegionTable`] borrows rather than owns.
#[derive(Debug, Clone)]
pub struct PondVenue {
    /// The venue scene's `.MAP` buffer.
    pub map: Vec<u8>,
    /// Its `+0x10000` region block (`legaia_engine_core::scene::Scene::field_map_region_block`).
    pub region_block: Option<Vec<u8>>,
    /// The angler's world `x` - the point the cast offsets from.
    pub anchor_x: i16,
    /// The angler's world `z`.
    pub anchor_z: i16,
    /// The angler's facing (`actor[+0x26]`), the polar offset's angle.
    pub facing: i16,
    /// The venue scene's three rods and their bend - the geometry the
    /// line's rod end is a vertex of ([`crate::fishing_actors::RodMesh`]).
    pub rod_mesh: Option<crate::fishing_actors::RodMesh>,
}

/// Wind-up frames before the power meter opens (state `0xd`: ~12 frames).
pub const WINDUP_FRAMES: i32 = 12;

/// Flight frames before the lure settles (state `0x1e` waits for the line
/// animation counter to reach `0x14`).
pub const FLIGHT_FRAMES: i32 = 0x14;
