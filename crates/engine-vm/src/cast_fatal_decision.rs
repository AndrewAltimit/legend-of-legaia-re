//! PROT 0954 (**Fatal Decision**) - the capture-class roulette, tick body
//! `0x801F6A58`, reached straight from the `0x801CF56C` arm (no trampoline).
//!
//! A thirteen-arm `ctx[+0x279]` switch (arms `0..=12` plus the terminal
//! `0xFF`), read off the disassembly of the image at the slot-B base
//! `0x801F69D8`:
//!
//! | arm | what |
//! |---:|---|
//! | 0 | the backdrop record `0x801F85DC`, the caster turned to its victim, both seats' animation rate halved, every party seat and every live monster hidden, the caster shown, a cut behind the caster, cue `0x149` |
//! | 1 | (gated) a pull-out on the caster |
//! | 2 | (gated) a cut to the victim; the victim shown and the caster hidden |
//! | 3 | a push on the victim |
//! | 4 | (gated) the **wheel fill**: eight outcome slots, one icon record per slot |
//! | 5 | the icons spiral out to the ring while it spins; the stop prompt for a party victim |
//! | 6 | the ring spins until the confirm button or the countdown stops it |
//! | 7 | the spin decelerates and snaps to a slot; the landing cue |
//! | 8 | (gated) a push on the victim |
//! | 9 | the ring opens out, the landed icon grows and sinks; the outcome's name banner |
//! | 10 | (gated) the icons retire; the **outcome** applies to the victim |
//! | 11 | the victim's reaction clip, turned to face the caster |
//! | 12 | waits for the victim's clip to settle |
//! | `0xFF` | the animation rates restored, the caster shown; the body returns `0` |
//!
//! The module countdown is the word `0x801F9018`, drained `scalar * delta`
//! by the gated arms; the spin angle is `0x801F9010`, the spread radius
//! `0x801F901C`, the eight slots `0x801F9020`, the icon handles `0x801F9040`
//! and the landed index `0x801F9080`.
//!
//! **Who stops the wheel.** Arm 6 holds while the countdown is positive and
//! the packed pad edge `_DAT_8007B874 | _DAT_8007B938` misses the confirm
//! mask `_DAT_800846D0` (`0x801F76B0..0x801F76E4`). The countdown it holds
//! on is set by arm 5 on the victim's seat: `scalar * 1200` for a party
//! victim - with the "press the button" prompt up in the message bar - and
//! `scalar << 6` for a monster victim. Fatal Decision is a monster's cast,
//! so the player stops the wheel on their own party member; a monster
//! victim's wheel stops itself.
//!
//! The camera's `FUN_801D5854(caster, 8)` framing calls in arms 11 and 12
//! and the full heal's CLUT reload and effect-list spawn (`FUN_800583C8`,
//! `FUN_801E22C8`) are not ported here.
//!
//! PORT: overlay_cast_fatal_decision_0954_801f6a58 (PROT 0954; every arm: the camera, the wheel, the stop, the sixteen outcomes)

use crate::battle_action::motion::sin12;
use crate::cast_module_camera::{ModuleSeat, ModuleShot, SPEED_SCALAR, heading};
use crate::cast_module_ticks::CastModuleCtx;

/// The band entry.
pub const FATAL_DECISION_ENTRY: u32 = 954;
/// The tick body.
pub const FATAL_DECISION_BODY: u32 = 0x801F_6A58;
/// The module countdown word.
pub const FATAL_DECISION_COUNTDOWN: u32 = 0x801F_9018;

/// Vsyncs per engine tick, the `*(0x1F800393)` frame delta a retail battle
/// frame carries (the engine ticks once per vsync).
pub const DELTA: i32 = crate::cast_module_camera::DELTA_PER_TICK;

/// Wheel slots.
pub const WHEEL_SLOTS: usize = 8;
/// Angle between two slots (`addiu ..,0x200` in every placement loop).
pub const WHEEL_STEP: i32 = 0x200;

/// The backdrop record arm 0 spawns.
pub const OPEN_RECORD: u32 = 0x801F_85DC;
/// The first of the sixteen outcome icon records (`0x801F86DC + id * 0x4C`,
/// the jump table at the image head picks one per slot).
pub const OUTCOME_RECORD_BASE: u32 = 0x801F_86DC;
/// Stride of the icon records.
pub const OUTCOME_RECORD_STRIDE: u32 = 0x4C;
/// The three records arm 7 spawns when the wheel lands.
pub const STOP_RECORDS: [u32; 3] = [0x801F_8B9C, 0x801F_8C08, 0x801F_8C68];
/// The outcome-name strings, `0x28` bytes apart, one per outcome id.
pub const OUTCOME_NAME_BASE: u32 = 0x801F_8D50;
/// Stride of the outcome-name strings.
pub const OUTCOME_NAME_STRIDE: u32 = 0x28;
/// The stop prompt arm 5 puts in the message bar.
pub const PROMPT_TEXT: u32 = 0x801F_8CC8;
/// The gold outcome's message.
pub const GOLD_TEXT: u32 = 0x801F_8CF4;
/// What follows the item name in the steal outcome's message.
pub const STOLEN_SUFFIX_TEXT: u32 = 0x801F_8D34;
/// The steal outcome's message when nothing could be taken.
pub const NO_EFFECT_TEXT: u32 = 0x801F_8D44;

/// Arm 0's cue (`FUN_8004FCC8(0x149)`).
pub const OPEN_CUE: u16 = 0x149;
/// Arm 7's landing cue.
pub const STOP_CUE: u16 = 0x14A;
/// Arm 7's landing cue when the slot is the full heal.
pub const FULL_HEAL_STOP_CUE: u16 = 0x14B;

/// The neutral tint word `0x20080200` the arms show a seat with.
pub const NEUTRAL_TINT: u32 = 0x2008_0200;
/// The render flag arm 0 shows the caster with.
pub const CASTER_RENDER_FLAG: u8 = 6;
/// The render flag that hides a seat.
pub const HIDDEN_RENDER_FLAG: u8 = 0xFF;

/// The playing-clip id arm 12 waits for on a party victim left at zero HP.
pub const PARTY_DOWN_CLIP: u8 = 8;

/// The port's bound on arm 12's wait, in ticks. Retail waits on the victim's
/// clip with no bound; an engine clip that never reports the id retail
/// waits for would hold the band for good, so the port lets go.
pub const SETTLE_TICK_LIMIT: u16 = 600;

/// The sixteen outcomes, by slot value - the jump table at `0x801F6A18`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FatalOutcome {
    /// `0x801F831C`: nothing.
    Nothing = 0,
    /// HP and max HP halved.
    HalveHp = 1,
    /// HP to zero.
    Death = 2,
    /// MP and max MP halved.
    HalveMp = 3,
    /// Status `0x0001` (Venom).
    Venom = 4,
    /// Status `0x0002` (Toxic).
    Toxic = 5,
    /// The ATK pair halved.
    HalveAtk = 6,
    /// Both defence pairs halved.
    HalveDef = 7,
    /// Status `0x0400` (Numb), the queued action cancelled.
    Numb = 8,
    /// Status `0x0038` - all three Rot limbs.
    Rot = 9,
    /// Status `0x0004` (Stone), the queued action cancelled.
    Stone = 10,
    /// Status `0x1000` (Curse).
    Curse = 11,
    /// Every status bit cleared and HP to max.
    FullHeal = 12,
    /// MP and max MP to zero.
    DrainMp = 13,
    /// One bag item destroyed.
    StealItem = 14,
    /// A tenth of the party's gold.
    GoldTithe = 15,
}

impl FatalOutcome {
    /// The outcome a slot value names; `None` past the table.
    pub fn from_slot(v: u32) -> Option<Self> {
        use FatalOutcome::*;
        Some(match v {
            0 => Nothing,
            1 => HalveHp,
            2 => Death,
            3 => HalveMp,
            4 => Venom,
            5 => Toxic,
            6 => HalveAtk,
            7 => HalveDef,
            8 => Numb,
            9 => Rot,
            10 => Stone,
            11 => Curse,
            12 => FullHeal,
            13 => DrainMp,
            14 => StealItem,
            15 => GoldTithe,
            _ => return None,
        })
    }

    /// The outcome's icon record.
    pub fn record(self) -> u32 {
        OUTCOME_RECORD_BASE + u32::from(self as u8) * OUTCOME_RECORD_STRIDE
    }

    /// The outcome's name string.
    pub fn name_va(self) -> u32 {
        OUTCOME_NAME_BASE + u32::from(self as u8) * OUTCOME_NAME_STRIDE
    }
}

/// The three monsters whose wheel arm 4 fills (`0x8007BD0C[ctx[+0x13] - 3]`),
/// and the slot range each draws from: `rand() % 8`, `% 12`, `% 16`
/// (`0x801F6FE4..0x801F70A4`).
pub fn wheel_span(caster_monster: u8) -> Option<u32> {
    match caster_monster {
        0x77 => Some(8),
        0x78 => Some(12),
        0x79 => Some(16),
        _ => None,
    }
}

/// Arm 4's fill (`0x801F6FB4..0x801F71DC`).
///
/// Per slot, one `rand() % span` for a listed caster (any other caster keeps
/// the slot's previous word, `image` - zero in the image); a Stone slot
/// (`10`) becomes a blank when the victim already carries a Rot bit
/// (`status & 0x38`). If no slot came out blank, one more draw blanks slot
/// `rand() % 8`. A **monster** victim (target code `>= 3`) then has four
/// outcomes remapped: Stone to Death, Rot to Halve HP, Steal to Halve ATK and
/// Gold to Halve DEF - the party-only effects swapped for monster ones.
pub fn fill_wheel(
    caster_monster: u8,
    victim_status: u16,
    victim_is_monster: bool,
    image: [u32; WHEEL_SLOTS],
    mut rand: impl FnMut() -> u32,
) -> [u32; WHEEL_SLOTS] {
    let mut slots = image;
    let span = wheel_span(caster_monster);
    let mut blanks = 0;
    for slot in &mut slots {
        if let Some(n) = span {
            *slot = rand_mod(rand(), n);
        }
        if victim_status & 0x38 != 0 && *slot == FatalOutcome::Stone as u32 {
            *slot = 0;
        }
        if *slot == 0 {
            blanks += 1;
        }
    }
    if blanks == 0 {
        slots[rand_mod(rand(), WHEEL_SLOTS as u32) as usize] = 0;
    }
    if victim_is_monster {
        for slot in &mut slots {
            *slot = match *slot {
                10 => 2,
                9 => 1,
                14 => 6,
                15 => 7,
                v => v,
            };
        }
    }
    slots
}

/// `rand() % n` as the module computes it: a signed remainder of the BIOS
/// `rand()`'s non-negative return.
fn rand_mod(r: u32, n: u32) -> u32 {
    ((r as i32) % (n as i32)) as u32
}

/// The `0x2AAAAAAB` multiply-high divide the placement loops use:
/// `(hi(v * 0x2AAAAAAB) >> shift) - (v >> 31)`.
fn div6_shift(v: i32, shift: u32) -> i16 {
    let hi = ((i64::from(v) * 0x2AAA_AAAB) >> 32) as i32;
    ((hi >> shift) - (v >> 31)) as i16
}

/// An icon's `(+0x14, +0x16)` while the ring opens (arms 5 and 9): the sine
/// and cosine samples of its angle scaled by the spread `r`, `/ 6 >> 10`.
pub fn spread_offset(angle: i32, r: i32) -> [i16; 2] {
    let a = (angle & 0xFFF) as u16;
    let s = i32::from(sin12(a));
    let c = i32::from(sin12(a.wrapping_add(0x400)));
    [
        div6_shift(s.wrapping_mul(r), 10),
        div6_shift(c.wrapping_mul(r), 10),
    ]
}

/// An icon's `(+0x14, +0x16)` on the spinning ring (arms 6 and 7): the
/// samples `/ 6 >> 3`, a radius of `85`.
pub fn ring_offset(angle: i32) -> [i16; 2] {
    let a = (angle & 0xFFF) as u16;
    let s = i32::from(sin12(a));
    let c = i32::from(sin12(a.wrapping_add(0x400)));
    [div6_shift(s, 3), div6_shift(c, 3)]
}

/// The truncating `x - (x / 0x200) * 0x200` arm 7 measures a slot boundary
/// with (`bgez; addiu 0x1ff; sra 9; sll 9; subu`).
fn slot_rem(x: i32) -> i32 {
    let b = if x < 0 { x + 0x1FF } else { x };
    x - ((b >> 9) << 9)
}

/// One pass of arm 7's snap (`0x801F7758..0x801F77D8`): step the angle back
/// `delta * 4`, and land it on the slot boundary it crossed.
pub fn snap_step(rot: i32, delta: i32) -> i32 {
    if rot & 0x1FF == 0 {
        return rot;
    }
    let before = slot_rem(rot) as i16 as i32;
    let next = rot - delta * 4;
    let after = slot_rem(next);
    if before < after { next - after } else { next }
}

/// The slot that sits at angle `0x800` once the ring is aligned: the `i`
/// whose `i << 9` equals `(0x800 - rot) & 0xFFF`.
pub fn landing_slot(rot: i32) -> Option<usize> {
    let at = (0x800 - rot) & 0xFFF;
    (0..WHEEL_SLOTS).find(|&i| (i as i32) << 9 == at)
}

/// The module-resident words the body carries between ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FatalDecisionState {
    /// `0x801F9018`.
    pub countdown: i32,
    /// `0x801F9010`, the spin angle.
    pub rot: i32,
    /// `0x801F901C`, the spread radius.
    pub spread: i32,
    /// `0x801F9020`, the eight slots.
    pub slots: [u32; WHEEL_SLOTS],
    /// `0x801F9080`, the landed slot index.
    pub landed: usize,
    /// The port's arm-12 wait counter ([`SETTLE_TICK_LIMIT`]).
    pub settle_ticks: u16,
}

/// What the body reads off the battle each tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FatalDecisionView {
    /// `actor_table[ctx[+0x13]]`.
    pub caster: ModuleSeat,
    /// `actor_table[caster[+0x1DD]]`.
    pub victim: ModuleSeat,
    /// The victim's seat y `+0x3E`.
    pub victim_seat_y: i16,
    /// The victim's body radius, `*(victim[+0x22C]) + 0x58`.
    pub victim_radius: i16,
    /// `ctx[+0x6D0]`, the framing depth (a signed halfword).
    pub depth: i16,
    /// The caster's formation monster id.
    pub caster_monster: u8,
    /// The victim sits on a party seat (target code `< 3`).
    pub victim_is_party: bool,
    /// The confirm edge this tick.
    pub confirm: bool,
    /// The victim's `+0x16E` status word.
    pub victim_status: u16,
    /// The victim's `+0x14C`.
    pub victim_hp: u16,
    /// The victim's playing clip `+0x1D9`.
    pub victim_playing: u8,
    /// The victim's tint word `+0x04`.
    pub victim_tint: u32,
}

/// One record a pass spawns, anchored at the origin with no rotation (every
/// record of this module is camera-relative).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatalSpawn {
    /// The record's slot-B VA.
    pub record: u32,
    /// The wheel slot the record is the icon of.
    pub wheel_slot: Option<usize>,
    /// The spawn's fourth argument, the part's render scale `+0x72`: `0x20`
    /// for an icon (`addiu a3,zero,0x20` at `0x801F7368`), whose sprite is
    /// authored `0x1000` wide, and `0x1000` for every other record.
    pub scale: u16,
}

/// The render scale arm 4 spawns each icon at.
pub const ICON_SPAWN_SCALE: u16 = 0x20;
/// The render scale every other spawn of the module passes.
pub const UNIT_SPAWN_SCALE: u16 = 0x1000;

/// A seat's tint word and render flag, as an arm stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatRender {
    pub tint: u32,
    pub flag: u8,
}

/// What one pass of the body did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FatalDecisionPass {
    /// The body returned `0`.
    pub done: bool,
    /// The `FUN_801D829C` shot the arm armed.
    pub shot: Option<ModuleShot>,
    /// Records spawned this pass.
    pub spawns: Vec<FatalSpawn>,
    /// Arm 7 retired the backdrop (`flags |= 8` on its handle).
    pub retire_backdrop: bool,
    /// Arm 10 retired the eight icons.
    pub retire_wheel: bool,
    /// Each icon's `(+0x14, +0x16)` this pass; `None` for an icon the pass
    /// did not place.
    pub place: [Option<[i16; 2]>; WHEEL_SLOTS],
    /// Arm 9's landed icon: `(slot, d)` with `+0x72 += d` and `+0x16 += d`.
    pub grow: Option<(usize, i16)>,
    /// The `FUN_8004FCC8` cue the arm raised.
    pub cue: Option<u16>,
    /// `Some(true)` opens the stop prompt, `Some(false)` closes it.
    pub prompt: Option<bool>,
    /// Arm 9's outcome-name banner.
    pub banner: Option<FatalOutcome>,
    /// Arm 7's white flash (`FUN_80024E80`).
    pub flash: bool,
    /// Arm 0 hid every party seat and every live monster.
    pub hide_all: bool,
    /// The caster's new heading.
    pub caster_facing: Option<u16>,
    /// The victim's new heading.
    pub victim_facing: Option<u16>,
    /// The caster's tint and render flag.
    pub caster_render: Option<SeatRender>,
    /// The victim's tint and render flag.
    pub victim_render: Option<SeatRender>,
    /// Arm 0 halved both seats' animation rates (`srl 1`).
    pub halve_rates: bool,
    /// Arm `0xFF` doubled them back (`sll 1`).
    pub restore_rates: bool,
    /// Arm 10 applied this outcome.
    pub applied: Option<FatalOutcome>,
    /// Arm 11 staged the victim's reaction.
    pub react: bool,
}

/// What the body asks of its battle.
pub trait FatalDecisionHost {
    /// One BIOS `rand()`.
    fn rand(&mut self) -> u32;
    /// Apply one outcome to the victim. Returns whether the victim's reaction
    /// arm runs (the outcomes whose arm falls to `0x801F8530`), or the body
    /// skips it (`0x801F831C`).
    fn apply_outcome(&mut self, outcome: FatalOutcome) -> bool;
}

fn shot(focus: ModuleSeat, angles: [i16; 2], tr: [i16; 2], frames: u16) -> Option<ModuleShot> {
    Some(ModuleShot {
        angles: [angles[0], angles[1], 0],
        tr: [0, tr[0], tr[1]],
        focus: [focus.x.wrapping_neg(), 0, focus.z.wrapping_neg()],
        frames,
    })
}

fn yaw(k: i32, facing: u16) -> i16 {
    (k - i32::from(facing)) as i16
}

/// One pass of the body over phase `ctx.phase`.
///
/// PORT: overlay_cast_fatal_decision_0954_801f6a58 (the arm switch)
pub fn fatal_decision_tick(
    st: &mut FatalDecisionState,
    ctx: &mut CastModuleCtx,
    view: &FatalDecisionView,
    host: &mut dyn FatalDecisionHost,
) -> FatalDecisionPass {
    let scalar = SPEED_SCALAR;
    let mut p = FatalDecisionPass::default();
    let advance = |ctx: &mut CastModuleCtx| ctx.phase = ctx.phase.wrapping_add(1);
    let v = view.victim;
    let depth = i32::from(view.depth);
    // The `subu; bgtz` gate of the counted arms.
    let gated = |st: &mut FatalDecisionState| {
        st.countdown -= scalar * DELTA;
        st.countdown > 0
    };
    match ctx.phase {
        0 => {
            p.spawns.push(FatalSpawn {
                record: OPEN_RECORD,
                wheel_slot: None,
                scale: UNIT_SPAWN_SCALE,
            });
            let facing = heading(v, view.caster).wrapping_add(0x800) & 0xFFF;
            p.caster_facing = Some(facing);
            p.halve_rates = true;
            p.shot = shot(
                view.caster,
                [-0x20, yaw(0x800, facing)],
                [0x400, (depth / 3) as i16],
                0x20,
            );
            st.countdown = scalar << 6;
            p.hide_all = true;
            p.caster_render = Some(SeatRender {
                tint: NEUTRAL_TINT,
                flag: CASTER_RENDER_FLAG,
            });
            ctx.ctx_278 = 3;
            advance(ctx);
            p.cue = Some(OPEN_CUE);
            st.rot = 0;
        }
        1 => {
            if gated(st) {
                return p;
            }
            p.shot = shot(
                view.caster,
                [0x40, yaw(0x800, view.caster.facing)],
                [0x700, (depth / 2) as i16],
                0x80,
            );
            st.countdown += scalar << 7;
            advance(ctx);
        }
        2 => {
            if gated(st) {
                return p;
            }
            p.shot = shot(
                v,
                [-0x20, yaw(0, view.caster.facing)],
                [
                    view.victim_seat_y.wrapping_neg().wrapping_mul(5),
                    view.victim_radius.wrapping_mul(4),
                ],
                1,
            );
            p.victim_render = Some(SeatRender {
                tint: NEUTRAL_TINT,
                flag: 0,
            });
            p.caster_render = Some(SeatRender {
                tint: 0,
                flag: HIDDEN_RENDER_FLAG,
            });
            advance(ctx);
        }
        3 => {
            p.shot = shot(
                v,
                [-0x20, yaw(0x800, v.facing)],
                [
                    view.victim_seat_y.wrapping_neg().wrapping_mul(5),
                    view.victim_radius.wrapping_mul(2),
                ],
                0x20,
            );
            st.countdown += scalar << 6;
            advance(ctx);
        }
        4 => {
            if gated(st) {
                return p;
            }
            st.slots = fill_wheel(
                view.caster_monster,
                view.victim_status,
                !view.victim_is_party,
                st.slots,
                || host.rand(),
            );
            for (i, &slot) in st.slots.iter().enumerate() {
                if let Some(o) = FatalOutcome::from_slot(slot) {
                    p.spawns.push(FatalSpawn {
                        record: o.record(),
                        wheel_slot: Some(i),
                        scale: ICON_SPAWN_SCALE,
                    });
                }
            }
            let y = i32::from(view.victim_seat_y.wrapping_neg()) * 35;
            p.shot = shot(
                v,
                [0x40, yaw(0x800, v.facing)],
                [(y / 4) as i16, view.victim_radius.wrapping_mul(3)],
                0x80,
            );
            st.spread = 0;
            st.countdown += scalar << 7;
            advance(ctx);
        }
        5 => {
            st.spread += DELTA;
            st.rot -= DELTA << 5;
            st.countdown -= scalar * DELTA;
            for (i, place) in p.place.iter_mut().enumerate() {
                *place = Some(spread_offset(st.rot + i as i32 * WHEEL_STEP, st.spread));
            }
            if st.countdown > 0 {
                return p;
            }
            if view.victim_is_party {
                p.prompt = Some(true);
                st.countdown = scalar * 1200;
            } else {
                st.countdown = scalar << 6;
            }
            st.spread = 0x80;
            advance(ctx);
        }
        6 => {
            st.rot -= DELTA << 5;
            st.countdown -= scalar * DELTA;
            for (i, place) in p.place.iter_mut().enumerate() {
                *place = Some(ring_offset(st.rot + i as i32 * WHEEL_STEP));
            }
            if st.countdown > 0 && !view.confirm {
                return p;
            }
            st.countdown = scalar << 9;
            if view.victim_is_party {
                p.prompt = Some(false);
            }
            advance(ctx);
        }
        7 => {
            let mut spinning = false;
            if st.countdown >= 0 {
                st.countdown -= 2 * scalar * DELTA;
                if st.countdown > 0 {
                    spinning = true;
                    st.rot -= if st.countdown >= 0x200 {
                        (DELTA * st.countdown) / 128
                    } else {
                        DELTA * 4
                    };
                }
            }
            if !spinning {
                st.rot = snap_step(st.rot, DELTA);
                if st.rot & 0x1FF == 0 {
                    p.flash = true;
                    st.countdown += scalar << 6;
                    p.retire_backdrop = true;
                    for record in STOP_RECORDS {
                        p.spawns.push(FatalSpawn {
                            record,
                            wheel_slot: None,
                            scale: UNIT_SPAWN_SCALE,
                        });
                    }
                    if let Some(i) = landing_slot(st.rot) {
                        st.landed = i;
                    }
                    p.cue = Some(
                        if st.slots[st.landed % WHEEL_SLOTS] == FatalOutcome::FullHeal as u32 {
                            FULL_HEAL_STOP_CUE
                        } else {
                            STOP_CUE
                        },
                    );
                    advance(ctx);
                }
            }
            for (i, place) in p.place.iter_mut().enumerate() {
                *place = Some(ring_offset(st.rot + i as i32 * WHEEL_STEP));
            }
        }
        8 => {
            if gated(st) {
                return p;
            }
            p.shot = shot(
                v,
                [-0x20, yaw(0x800, v.facing)],
                [
                    view.victim_seat_y.wrapping_mul(-7),
                    view.victim_radius.wrapping_mul(2),
                ],
                0x20,
            );
            st.countdown += scalar << 5;
            advance(ctx);
        }
        9 => {
            let at = (0x800 - st.rot) & 0xFFF;
            st.spread += DELTA * 6;
            st.countdown -= scalar * DELTA;
            for i in 0..WHEEL_SLOTS {
                if at == (i as i32) << 9 {
                    p.grow = Some((i, DELTA as i16));
                    st.landed = i;
                } else {
                    p.place[i] = Some(spread_offset(st.rot + i as i32 * WHEEL_STEP, st.spread));
                }
            }
            if st.countdown > 0 {
                return p;
            }
            st.countdown += scalar << 6;
            p.banner = FatalOutcome::from_slot(st.slots[st.landed % WHEEL_SLOTS]);
            advance(ctx);
        }
        10 => {
            if gated(st) {
                return p;
            }
            p.retire_wheel = true;
            st.countdown += scalar << 7;
            ctx.ctx_0d = 0;
            advance(ctx);
            if let Some(o) = FatalOutcome::from_slot(st.slots[st.landed % WHEEL_SLOTS]) {
                p.applied = Some(o);
                if !host.apply_outcome(o) {
                    advance(ctx);
                }
            }
        }
        11 => {
            if view.victim_status & 4 == 0 {
                p.react = true;
                p.victim_facing = Some(heading(view.caster, v).wrapping_add(0x800) & 0xFFF);
            }
            advance(ctx);
        }
        12 => {
            if st.countdown > 0 && gated(st) {
                return p;
            }
            st.settle_ticks = st.settle_ticks.saturating_add(1);
            let settled = match (view.victim_is_party, view.victim_hp != 0) {
                (true, true) | (false, true) => view.victim_playing == 0,
                (true, false) => view.victim_playing == PARTY_DOWN_CLIP,
                (false, false) => view.victim_tint == 0,
            };
            if settled || st.settle_ticks > SETTLE_TICK_LIMIT {
                ctx.phase = 0xFF;
            }
        }
        0xFF => {
            p.restore_rates = true;
            p.caster_render = Some(SeatRender {
                tint: NEUTRAL_TINT,
                flag: 0,
            });
            p.done = true;
        }
        // No arm: retail falls to the epilogue still reporting busy. No
        // phase outside the switch is reachable, so the port finishes.
        _ => p.done = true,
    }
    p
}

/// The victim's stat block, as the outcome arms read and write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FatalVictim {
    /// `+0x14C`.
    pub hp: u16,
    /// `+0x14E`.
    pub max_hp: u16,
    /// `+0x150`.
    pub mp: u16,
    /// `+0x152`.
    pub max_mp: u16,
    /// `+0x10`, the HP-bar accumulator.
    pub hp_bar_delta: i32,
    /// `+0x178`, the MP readout.
    pub mp_readout: u16,
    /// `+0x16E`.
    pub status: u16,
    /// `+0x158` / `+0x15A`.
    pub atk: u16,
    pub atk_base: u16,
    /// `+0x15C` / `+0x15E`.
    pub udf: u16,
    pub udf_base: u16,
    /// `+0x160` / `+0x162`.
    pub ldf: u16,
    pub ldf_base: u16,
}

/// How an outcome's arm left, past its stores on [`FatalVictim`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutcomeStep {
    /// The arm falls to `0x801F8530`: the victim's reaction arm runs.
    pub reaction: bool,
    /// The `FUN_801F44A0` popup: `(amount, restores)`.
    pub popup: Option<(u16, bool)>,
    /// Status bits ORed into `+0x16E`.
    pub status_set: u16,
    /// The `+0x1DE == 1 && +0x16C != 0` item refund and the `+0x1DE = 0`
    /// store (Numb / Stone, `0x801F7FC0` / `0x801F8014`).
    pub cancels_action: bool,
    /// `+0x16E = 0` (the full heal).
    pub status_cleared: bool,
}

/// `srl 1`, then `+1` when that left zero - the halving with a floor of one
/// every stat arm uses.
pub fn halve_floor_one(x: u16) -> u16 {
    let h = x >> 1;
    if h == 0 { 1 } else { h }
}

/// The stat-block half of an outcome arm (`0x801F7DC4..0x801F838C`). Steal
/// and gold reach outside the victim and are the host's; this returns their
/// step with no stores.
pub fn apply_outcome_to(victim: &mut FatalVictim, outcome: FatalOutcome) -> OutcomeStep {
    use FatalOutcome::*;
    let stone = victim.status & 4 != 0;
    let mut step = OutcomeStep {
        reaction: true,
        ..Default::default()
    };
    match outcome {
        Nothing => step.reaction = false,
        HalveHp | Death | HalveMp | DrainMp if stone => step.reaction = false,
        HalveHp => {
            let d = ((u32::from(victim.hp) + 1) >> 1) as u16;
            step.popup = Some((d, false));
            victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(i32::from(d));
            victim.hp = victim.hp.wrapping_sub(d);
            victim.max_hp = halve_floor_one(victim.max_hp);
        }
        Death => {
            let d = victim.hp;
            step.popup = Some((d, false));
            victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(i32::from(d));
            victim.hp = 0;
        }
        HalveMp => {
            let d = ((u32::from(victim.mp) + 1) >> 1) as u16;
            step.popup = Some((d, false));
            victim.mp_readout = victim.mp_readout.wrapping_add(d);
            victim.mp = victim.mp.wrapping_sub(d);
            victim.max_mp = halve_floor_one(victim.max_mp);
        }
        DrainMp => {
            let d = victim.mp;
            step.popup = Some((d, false));
            victim.mp_readout = victim.mp_readout.wrapping_add(d);
            victim.max_mp = 0;
            victim.mp = 0;
        }
        Venom => step.status_set = 0x0001,
        Toxic => step.status_set = 0x0002,
        Rot => step.status_set = 0x0038,
        Curse => step.status_set = 0x1000,
        Numb | Stone => {
            step.status_set = if outcome == Numb { 0x0400 } else { 0x0004 };
            step.cancels_action = true;
            step.reaction = false;
        }
        HalveAtk => {
            victim.atk = halve_floor_one(victim.atk);
            victim.atk_base = halve_floor_one(victim.atk_base);
        }
        HalveDef => {
            victim.udf = halve_floor_one(victim.udf);
            victim.ldf = halve_floor_one(victim.ldf);
            victim.udf_base = halve_floor_one(victim.udf_base);
            victim.ldf_base = halve_floor_one(victim.ldf_base);
        }
        FullHeal => {
            step.status_cleared = true;
            let d = victim.max_hp.wrapping_sub(victim.hp);
            step.popup = Some((d, true));
            victim.hp_bar_delta = victim.hp_bar_delta.wrapping_sub(i32::from(d));
            victim.hp = victim.hp.wrapping_add(d);
            step.reaction = false;
        }
        StealItem | GoldTithe => {}
    }
    victim.status = if step.status_cleared {
        0
    } else {
        victim.status | step.status_set
    };
    step
}

/// The gold outcome (`0x801F8330`): `gold -= gold / 10`, a signed divide of
/// the word at `0x8008459C`.
pub fn gold_tithe(gold: i32) -> i32 {
    gold - gold / 10
}

/// Read the string at `va` out of the module image (linked at the slot-B
/// base), up to its terminating NUL. A `0xCE` / `0xCF` escape carries one
/// operand byte the terminator test does not see - the stop prompt's
/// button glyph is `CE 00` - so the walk steps over it, as the glyph-count
/// walk does (`legaia_font::glyph_count`).
pub fn module_string(image: &[u8], va: u32) -> Option<&[u8]> {
    let off = va.checked_sub(legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE)? as usize;
    let tail = image.get(off..)?;
    let mut i = 0;
    while i < tail.len() && tail[i] != 0 {
        i += if matches!(tail[i], 0xCE | 0xCF) { 2 } else { 1 };
    }
    Some(&tail[..i.min(tail.len())])
}

#[cfg(test)]
mod tests;
