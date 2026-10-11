//! Property ramps: the per-actor ramp scheduler and the zone ramp.

use super::*;

/// Destination width of a scheduler slot - retail's `slot+0x18`. Only
/// [`RampKind::U16`] is reachable from the motion VM's `0x0D`; the others
/// exist because the same pool carries sound and render-bank ramps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampKind {
    /// `kind 1` - `sb`.
    U8,
    /// `kind 2` - `sh`. The heading channel.
    U16,
    /// `kind 3` - three packed 8-bit lanes lerped independently.
    Rgb,
    /// `kind 4` - `sw`.
    U32,
}

/// One live slot of the `0x801C66A0` pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ramp {
    /// Opaque host tag for the destination (retail stores a raw pointer at
    /// `slot+0x04`). The motion VM's heading channel uses [`RAMP_DEST_HEADING`].
    pub dest: u32,
    /// Host tag for the owning actor (retail `slot+0x00`), for
    /// [`RampScheduler::free_owner`].
    pub owner: u32,
    /// Install-time value, `slot+0x08`.
    pub start: i32,
    /// Endpoint, `slot+0x0C`. The terminal tick stores this verbatim.
    pub end: i32,
    /// Total duration, `slot+0x10` - never rewritten.
    pub total: i32,
    /// Countdown, `slot+0x14`.
    pub remaining: i32,
    pub kind: RampKind,
}

/// Destination tag the `0x0D` install uses for `&actor+0x26`.
///
/// The tags below are the other actor fields this VM's tween ops install
/// ramps on. Each tag is literally retail's byte offset within the actor
/// record, which is what keeps the mapping checkable against the
/// disassembly.
pub const RAMP_DEST_HEADING: u32 = 0x26;

/// `0x15`'s destination - `actor+0x24`, the X Euler angle.
pub const RAMP_DEST_PITCH: u32 = 0x24;

/// `0x16`'s destination - `actor+0x28`, the Z Euler angle.
pub const RAMP_DEST_ROLL: u32 = 0x28;

/// `0x14`'s destination - `actor+0x72`, the uniform render scale.
pub const RAMP_DEST_SCALE: u32 = 0x72;

/// `0x0C`'s colour lane - `actor+0x74`, the packed RGB tint (`kind 3`).
pub const RAMP_DEST_TINT: u32 = 0x74;

/// `0x0C`'s mode lane - `actor+0x78`, the draw-mode word. Retail installs
/// it as `kind 4` (a `sw`) over a field it reads back with `lhu`; the port
/// keeps the 16-bit reading.
pub const RAMP_DEST_BLEND: u32 = 0x78;

/// A value the scheduler produced this tick, and where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RampWrite {
    pub dest: u32,
    pub owner: u32,
    /// The value retail stores. Truncate per [`RampWrite::kind`] at the
    /// destination; the heading channel takes the low 16 bits raw.
    pub value: i32,
    pub kind: RampKind,
    /// `true` on the tick the slot retired (the endpoint write).
    pub finished: bool,
}

/// The 63-slot ramp pool - retail `0x801C66A0`, installer `FUN_8003C5F0`,
/// ticker `FUN_80036D80`.
#[derive(Debug, Clone, Default)]
pub struct RampScheduler {
    slots: Vec<Option<Ramp>>,
    /// Retail's `DAT_80073ED0` pool-exhausted counter, incremented instead
    /// of installing when every slot is busy.
    pub overflow: u32,
}

impl RampScheduler {
    pub fn new() -> Self {
        Self {
            slots: vec![None; RAMP_SLOTS],
            overflow: 0,
        }
    }

    /// `FUN_8003C5F0`: first-free scan, or bump [`RampScheduler::overflow`]
    /// and drop the request. Returns the slot index on success.
    pub fn install(&mut self, ramp: Ramp) -> Option<usize> {
        if self.slots.is_empty() {
            self.slots = vec![None; RAMP_SLOTS];
        }
        match self.slots.iter().position(Option::is_none) {
            Some(i) => {
                self.slots[i] = Some(ramp);
                Some(i)
            }
            None => {
                self.overflow = self.overflow.wrapping_add(1);
                None
            }
        }
    }

    /// Free every slot owned by `owner` - the port's stand-in for retail's
    /// "owner actor has `+0x10 & 8`" despawn sweep.
    pub fn free_owner(&mut self, owner: u32) {
        for s in self.slots.iter_mut() {
            if s.is_some_and(|r| r.owner == owner) {
                *s = None;
            }
        }
    }

    // Live: `World::install_field_player` calls this on the world-owned
    // `locomotion.player_scale_ramps` scheduler, which - unlike the
    // per-placement schedulers `World::seed_field_npc_ambient` rebuilds - does
    // outlive a scene, so the reset has a real boundary to sit on and a ramp
    // the last scene left running cannot resize the player on this one.
    //
    // That seat is the engine's, not a transcription of a retail call site:
    // `find-address-word-refs.py --prot 8003cda8` finds no reference of any
    // form (`jal`, `j`, branch, word, `lui` pair) to `0x8003CDA8` in SCUS or
    // in any PROT entry, and the instruction before it is the previous
    // routine's `jr ra` delay slot, so nothing falls into it either. The
    // routine is shipped but unreferenced; "scene-entry reset" names what it
    // does, not who calls it.
    /// PORT: FUN_8003CDA8 - the scene-entry pool reset.
    ///
    /// Retail zeroes the busy word `+0x1E` of all **64** raw slots, clears
    /// slot 0's `+0x1A` / `+0x1C`, then writes `_DAT_801C66BE = 1` - slot 0's
    /// own `+0x1E` - so the header re-arms as occupied and slots 1..63 come
    /// back free. That asymmetric last store is why the pool's usable count
    /// is [`RAMP_SLOTS`] (63) and not 64; a port that clears all 64 and stops
    /// hands out the list header as an allocatable slot.
    ///
    /// The routine ends with an actor sweep (`actor_free(&DAT_800742EC,
    /// _DAT_8007C34C)`) that retires the pool's own driver actor. The port
    /// has no such driver - the host ticks the scheduler directly - so that
    /// half has no counterpart here; see [`RampScheduler::free_owner`] for
    /// the owner-scoped sweep the engine does model.
    ///
    /// Nothing carries across a scene boundary: `overflow` is reset too,
    /// because retail's `DAT_80073ED0` counter is diagnostic and a stale
    /// count would misreport the new scene.
    pub fn reset_pool(&mut self) {
        self.slots.clear();
        self.slots.resize(RAMP_SLOTS, None);
        self.overflow = 0;
    }

    pub fn active(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// `true` while a ramp is driving `dest`.
    pub fn is_driving(&self, dest: u32) -> bool {
        self.slots.iter().any(|s| s.is_some_and(|r| r.dest == dest))
    }

    /// One frame of `FUN_80036D80`. `speed` is `_DAT_1F800393`.
    pub fn tick(&mut self, speed: u8) -> Vec<RampWrite> {
        let mut out = Vec::new();
        for slot in self.slots.iter_mut() {
            let Some(mut r) = *slot else { continue };
            r.remaining -= i32::from(speed);
            let (value, finished) = if r.remaining <= 0 {
                (r.end, true)
            } else {
                (lerp_remaining(r.start, r.end, r.remaining, r.total), false)
            };
            let value = match r.kind {
                RampKind::Rgb => rgb_lerp(r.start, r.end, r.remaining, r.total, finished),
                _ => value,
            };
            out.push(RampWrite {
                dest: r.dest,
                owner: r.owner,
                value,
                kind: r.kind,
                finished,
            });
            *slot = if finished { None } else { Some(r) };
        }
        out
    }
}

/// Retail's scalar lerp: `end + (start - end) * remaining / total`, with a
/// truncating (toward-zero) divide. Reproduces `0x80036E00..0x80036E50`.
pub(super) fn lerp_remaining(start: i32, end: i32, remaining: i32, total: i32) -> i32 {
    if total == 0 {
        return end;
    }
    end + (start - end).saturating_mul(remaining) / total
}

/// `kind 3`: the lerp run on three packed 8-bit lanes (`0x80036E54..0x80036F60`).
/// Present for completeness - the motion VM never installs this kind.
///
/// The lanes are **not** three copies of [`lerp_remaining`]. Only red runs
/// on the plain byte (`andi 0xff`, then `& 0xff` on the result). Green runs
/// on the byte still scaled by 256 (`andi 0xff00`), and blue on bits 16..23
/// the same way after both words are shifted down by 8 (`sra 8` then
/// `andi 0xff00`); each scaled result is masked back to `& 0xff00`. The
/// divide truncates the *scaled* difference toward zero and the mask then
/// floors, so a decreasing green or blue lane lands one below the plain-byte
/// lerp whenever the quotient has a fraction: start `0`, end `10`, one third
/// remaining gives red `7` but green `6`. The result's top byte is zero.
pub(super) fn rgb_lerp(start: i32, end: i32, remaining: i32, total: i32, finished: bool) -> i32 {
    if finished {
        return end;
    }
    if total == 0 {
        return end;
    }
    let lerp = |s: i32, e: i32| -> i32 {
        (s - e)
            .wrapping_mul(remaining)
            .wrapping_div(total)
            .wrapping_add(e)
    };
    let red = lerp(start & 0xFF, end & 0xFF) & 0xFF;
    let green = lerp(start & 0xFF00, end & 0xFF00) & 0xFF00;
    let (s8, e8) = (start >> 8, end >> 8);
    let blue = (lerp(s8 & 0xFF00, e8 & 0xFF00) & 0xFF00) << 8;
    red | green | blue
}

// ---------------------------------------------------------------------------
// The player-zone ramp actor (`FUN_80037018`)
// ---------------------------------------------------------------------------

/// Destination width of a [`ZoneRamp`] - retail's `slot+0x8C`.
///
/// Deliberately **not** [`RampKind`]. The two width ladders disagree on kind
/// `3`: the pool tick `FUN_80036D80` gives it the three-lane packed-RGB lerp,
/// while `FUN_80037018`'s `slti 3` / `slti 5` pair sends `3` and `4` to the
/// same `sw`. Reusing one enum for both would import an RGB lerp this
/// routine never performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneRampWidth {
    /// `kind 1` - `sb`.
    U8,
    /// `kind 2` - `sh`.
    U16,
    /// `kind 3` **and** `kind 4` - `sw`.
    U32,
}

/// One player-zone ramp actor's record, as `FUN_80037018` reads it off the
/// actor it is the `+0x0C` handler of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneRamp {
    /// `+0x80` - the value at the low edge of the window.
    pub start: i32,
    /// `+0x84` - the value at the high edge.
    pub end: i32,
    /// `+0x88` / `+0xC8` - the X gate. Outside it the actor does nothing;
    /// X takes no part in the interpolation.
    pub x_lo: i16,
    pub x_hi: i16,
    /// `+0x8A` / `+0xCA` - the Z window. Both the gate **and** the ramp
    /// parameter: the output is where the player's Z sits inside it.
    pub z_lo: i16,
    pub z_hi: i16,
    /// `+0x8C` - destination width selector, kept raw so the out-of-range
    /// arm stays reachable.
    pub kind: i32,
}

/// What one tick of a [`ZoneRamp`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneRampTick {
    /// A gate rejected: nothing is written and the actor stays live.
    Idle,
    /// Store `value` through the actor's `+0x94` destination pointer.
    Write { value: i32, width: ZoneRampWidth },
    /// `kind` is none of `1..=4`: retail sets its **own** `+0x10 |= 8` yield
    /// bit, so the actor retires instead of writing.
    Retire,
    /// `z_lo == z_hi`. Retail divides by the zero span and executes
    /// `break 0x1C00`, the MIPS divide-by-zero trap. The port reports it
    /// rather than reproducing a CPU exception; no shipped record is built
    /// this way.
    DivideByZero,
}

/// PORT: FUN_80037018 - the **player-zone ramp actor** tick.
///
/// The spawner is `FUN_8003C6A4`, reached from the field VM's op `0x43`
/// sub-3..6: it allocates from the pool descriptor at `&DAT_80074304`, whose
/// `+8` word is this routine's VA, and fills in exactly the `+0x80..+0xCA`
/// fields read below. The port's spawn side is
/// `legaia_engine_field::register_ramp::spawn_register_ramp`, and the
/// destination pointer is modelled as one of four named camera-configuration
/// registers rather than a raw address. `engine-field::actor_handler` also
/// knows this VA as the target of the MAN loader's retire sweep, which is
/// what clears the records at scene entry.
///
/// Not a slot of the [`RampScheduler`] pool, despite the family resemblance:
/// its fields live at `+0x80..+0xCA` of a full actor record (the pool's slots
/// are `0x20` bytes), it is dispatched as an actor `+0x0C` handler rather
/// than walked by `FUN_80036D80`, and it interpolates on the **player's
/// position** instead of on a countdown. Nothing decrements here, so the
/// value tracks the player and can run backwards; the actor never retires
/// itself on a completed ramp. What clears them is the scene MAN loader's
/// second inlined retire sweep (`FUN_8003AEB0` at `0x8003B414`), which is
/// keyed on this very VA - see `legaia_engine_field::actor_handler`.
///
/// Two global gates come first, both shared with the field touch dispatch:
/// the player's engaged flag (`_DAT_8007C364[+0x10] & 0x80000`) and the
/// scratch system lock (`_DAT_1F800394 & 0x400`). Then the AABB test, X
/// before Z. The interpolation is
///
/// ```text
/// value = start + (end - start) * (player.z - z_lo) / (z_hi - z_lo)
/// ```
///
/// with MIPS `mult`/`mflo` (low word, wrapping) and a toward-zero `div`. The
/// second overflow guard retail emits (`v1 == i32::MIN && span == -1`) is
/// unreachable: the Z gates have already forced `z_lo <= player.z <= z_hi`,
/// so the span is non-negative before the divide.
pub fn zone_ramp_tick(
    ramp: &ZoneRamp,
    player_x: i16,
    player_z: i16,
    player_engaged: bool,
    system_lock: bool,
) -> ZoneRampTick {
    if player_engaged || system_lock {
        return ZoneRampTick::Idle;
    }
    if player_x < ramp.x_lo || ramp.x_hi < player_x {
        return ZoneRampTick::Idle;
    }
    if player_z < ramp.z_lo || ramp.z_hi < player_z {
        return ZoneRampTick::Idle;
    }
    let span = i32::from(ramp.z_hi) - i32::from(ramp.z_lo);
    if span == 0 {
        return ZoneRampTick::DivideByZero;
    }
    let elapsed = i32::from(player_z) - i32::from(ramp.z_lo);
    let value = ramp
        .end
        .wrapping_sub(ramp.start)
        .wrapping_mul(elapsed)
        .wrapping_div(span)
        .wrapping_add(ramp.start);
    let width = match ramp.kind {
        1 => ZoneRampWidth::U8,
        2 => ZoneRampWidth::U16,
        3 | 4 => ZoneRampWidth::U32,
        _ => return ZoneRampTick::Retire,
    };
    ZoneRampTick::Write { value, width }
}
