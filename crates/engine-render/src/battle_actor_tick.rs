//! Per-actor **battle draw tick** - the pass that sequences an actor's tint,
//! trail and draw calls each frame, and stamps the defeated-monster grey.
//!
//! PORT: FUN_800480d8
//!
//! NOT WIRED: the calls this pass sequences are retail functions this crate
//! does not own - `FUN_8004A908` (tint / fade), `FUN_80048A08` (the TMD draw
//! pass), `FUN_8005112C` (the per-character signature effect) and
//! `FUN_801F7B88` - and the engine's battle render path draws party and
//! monster bodies directly from `legaia_engine_core`'s battle scene rather
//! than through a per-actor call sequence. So the port is the **schedule**,
//! not the calls: [`battle_actor_tick`] returns the ordered
//! [`BattleDrawStep`] list and the colour word to stamp, and a host that
//! grows those passes replays it verbatim. Nothing calls the schedule today.
//! One of the five sequenced calls now runs live by another route:
//! `FUN_80049348` (the arts after-image walk) is ported as
//! `legaia_engine_core::battle_afterimage` + `World::battle_ghost_draws`,
//! which both hosts draw per battle frame.
//!
//! Its own retail caller is **`FUN_8001ADA4`** at `8001AEF4`
//! (`jal 0x800480d8` with `a0 = s0`, under the `slti v0,v0,0xa1` guard that
//! restricts the pass to actor ids below `0xA1`) - dumped, and partly ported
//! (case `0xB` lands in the native window's field render pass). So this row
//! is not "no caller exists"; the missing retail functions are the five REFs
//! below, which are what the schedule would have to call.
//!
//! REF: FUN_8001ada4 - the dispatcher whose actor-id arm calls this pass.
//! REF: FUN_8004a908 - tint / fade pass (`BattleDrawStep::Tint`).
//! REF: FUN_80048a08 - TMD draw pass (`BattleDrawStep::Draw`).
//! REF: FUN_80049348 - arts after-image walk (`BattleDrawStep::Afterimage`).
//! REF: FUN_8005112c - per-character signature effect trigger.
//! REF: FUN_80050e74 - the raise half of the same teardown protocol
//! (`legaia_engine_vm::move_vm::flush_part_actor_pool`); the loop below is
//! the collect half.
//!
//! # The colour word is RGB, not a flag
//!
//! The pass tests `actor[+0x74] & 0x00FF_FFFF` and, on the defeated-monster
//! path, stamps `0x0080_8080`. Both constants are built by
//! `lui v?,0x80 ; ori v?,v?,0x8080` / `lui v?,0xff ; ori v?,v?,0xffff`, i.e.
//! `0x00808080` and `0x00FFFFFF` - a **24-bit RGB** field holding mid-grey, the
//! same `0x808080` the after-image ghost and the move-FX streak use. It is not
//! a `0x80808080` word.
//!
//! # Two arms off that word
//!
//! * **Word zero** (nothing tinted yet): the pass only *considers* stamping
//!   grey. It needs a monster seat (`actor[+0x5A]` in `3..=6`), the
//!   no-escape / boss byte `ctx[+0x287]` set, the debug gate byte clear, and
//!   the seated actor's `+0x21C` state to read exactly `2`. All four hold ->
//!   stamp grey, draw, tint. Any one fails -> the actor is not drawn at all
//!   this frame.
//! * **Word non-zero**: signature effect, trail flag on, after-image walk,
//!   tint; the trail flag is cleared again unless the seat is exactly `7`. Then
//!   the same four-way gate decides between "stamp grey, draw, tint" and a
//!   plain draw.
//!
//! So the difference between the arms is not *whether* the gate runs but what
//! failing it costs: on the zero arm the actor is skipped, on the non-zero arm
//! it still draws untinted.
//!
//! # The scene-clear preamble
//!
//! Ahead of all of that, a set `ctx[+0x272]` byte means the battle scene is
//! tearing down. Guarded on the effect-VM ready flag reading `0xFF`, the pass
//! shuts down the four battle overlay passes, voids every entry of the
//! `0x80`-slot effect-node table `DAT_801C90F0` whose target carries flag bit
//! `0x8`, optionally runs one more overlay hook, and clears the byte. The clear
//! happens whether or not the ready flag let the body run.
//!
//! Source: `ghidra/scripts/funcs/800480d8.txt` (disassembly).

/// `actor + 0x10` bit that skips the whole tick.
pub const FLAG_ACTOR_INERT: u32 = 0x8;
/// Mask retail applies to the colour word `actor + 0x74`.
pub const COLOUR_MASK: u32 = 0x00FF_FFFF;
/// The mid-grey RGB stamped onto a defeated monster.
pub const DEFEATED_GREY: u32 = 0x0080_8080;
/// Effect-node table slot count (`DAT_801C90F0`).
pub const EFFECT_NODE_SLOTS: usize = 0x80;
/// Flag bit an effect-node target must carry to be voided during teardown.
pub const EFFECT_NODE_VOID_BIT: u32 = 0x8;
/// Effect-VM ready flag value that lets the teardown body run.
pub const EFFECT_VM_READY: u8 = 0xFF;
/// Seat value that keeps the trail flag (`actor + 0x6A`) latched on.
pub const TRAIL_LATCH_SEAT: i16 = 7;
/// `actor + 0x21C` value that marks a monster seat as defeated.
pub const DEFEATED_STATE: u8 = 2;
/// Monster seats, i.e. the `3..=6` window the grey stamp is gated on.
pub const MONSTER_SEATS: std::ops::RangeInclusive<i16> = 3..=6;

/// One call the tick makes, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleDrawStep {
    /// `FUN_8005112C` - per-character signature effect trigger.
    SignatureEffect,
    /// `FUN_80049348` - arts after-image / motion-trail walk.
    Afterimage,
    /// `FUN_8004A908` - tint / fade pass.
    Tint,
    /// `FUN_80048A08` - TMD draw pass.
    Draw,
}

/// The battle-scene teardown preamble's verdict.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleTeardown {
    /// `true` when `ctx[+0x272]` was set, so the byte gets cleared.
    pub requested: bool,
    /// `true` when the effect-VM ready flag also let the shutdown body run.
    pub ran: bool,
    /// `true` when the extra overlay hook (`FUN_801F7B88`) fires - gated on
    /// `_DAT_8007BDC0 != 0`.
    pub extra_hook: bool,
}

/// The retail actor + context fields the tick reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleActorView {
    /// `actor + 0x10` flag word.
    pub flags: u32,
    /// `actor + 0x74` colour word.
    pub colour: u32,
    /// `actor + 0x5A` seat index.
    pub seat: i16,
}

/// The gates that live outside the actor.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleTickGates {
    /// `ctx + 0x287` - the no-escape / boss byte.
    pub no_escape: bool,
    /// `gp + 0x9F5` (`0x8007BD0D`) - a debug byte that must be clear.
    pub debug_hold: bool,
    /// `*(DAT_801C9370 + seat*4) + 0x21C` - the seated actor's state byte.
    pub seat_state: u8,
}

/// The tick's result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleActorTick {
    /// Teardown verdict; runs before any draw step.
    pub teardown: BattleTeardown,
    /// Calls in retail order.
    pub steps: Vec<BattleDrawStep>,
    /// The value retail leaves in `actor + 0x74`.
    pub colour: u32,
    /// The value retail leaves in `actor + 0x6A` (the trail flag), or `None`
    /// when the pass never touches it. Retail raises it to `1` *before* the
    /// after-image walk and only lowers it after the following tint, so the
    /// walk always observes `1`; this field is the settled value.
    pub trail_flag: Option<u16>,
    /// `true` when the actor was inert (`+0x10 & 8`) and nothing ran.
    pub inert: bool,
}

/// Decide the teardown preamble.
pub fn battle_scene_teardown(
    clear_requested: bool,
    effect_vm_ready: u8,
    extra_hook_armed: bool,
) -> BattleTeardown {
    let ran = clear_requested && effect_vm_ready == EFFECT_VM_READY;
    BattleTeardown {
        requested: clear_requested,
        ran,
        extra_hook: ran && extra_hook_armed,
    }
}

/// Which effect-node slots the teardown voids: every non-null entry whose
/// target's flag word carries [`EFFECT_NODE_VOID_BIT`].
pub fn voided_effect_nodes(node_flags: &[Option<u32>]) -> Vec<usize> {
    node_flags
        .iter()
        .take(EFFECT_NODE_SLOTS)
        .enumerate()
        .filter_map(|(i, f)| match f {
            Some(f) if f & EFFECT_NODE_VOID_BIT != 0 => Some(i),
            _ => None,
        })
        .collect()
}

/// The four-way gate both arms share: seat in `3..=6`, no-escape byte set,
/// debug byte clear, seat state exactly [`DEFEATED_STATE`].
pub fn defeated_grey_gate(seat: i16, gates: &BattleTickGates) -> bool {
    MONSTER_SEATS.contains(&seat)
        && gates.no_escape
        && !gates.debug_hold
        && gates.seat_state == DEFEATED_STATE
}

/// Run the per-actor draw tick.
pub fn battle_actor_tick(
    actor: &BattleActorView,
    gates: &BattleTickGates,
    teardown: BattleTeardown,
) -> BattleActorTick {
    let mut out = BattleActorTick {
        colour: actor.colour,
        ..Default::default()
    };
    if actor.flags & FLAG_ACTOR_INERT != 0 {
        out.inert = true;
        return out;
    }
    out.teardown = teardown;

    // The unconditional first tint.
    out.steps.push(BattleDrawStep::Tint);

    if actor.colour & COLOUR_MASK == 0 {
        // Untinted arm: the gate decides between "grey, draw, tint" and
        // drawing nothing at all this frame.
        if defeated_grey_gate(actor.seat, gates) {
            out.colour = DEFEATED_GREY;
            out.steps.push(BattleDrawStep::Draw);
            out.steps.push(BattleDrawStep::Tint);
        }
        return out;
    }

    // Tinted arm.
    out.steps.push(BattleDrawStep::SignatureEffect);
    out.steps.push(BattleDrawStep::Afterimage);
    out.steps.push(BattleDrawStep::Tint);
    out.trail_flag = Some(u16::from(actor.seat == TRAIL_LATCH_SEAT));

    if defeated_grey_gate(actor.seat, gates) {
        out.colour = DEFEATED_GREY;
        out.steps.push(BattleDrawStep::Draw);
        out.steps.push(BattleDrawStep::Tint);
    } else {
        out.steps.push(BattleDrawStep::Draw);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gates(defeated: bool) -> BattleTickGates {
        BattleTickGates {
            no_escape: defeated,
            debug_hold: false,
            seat_state: if defeated { DEFEATED_STATE } else { 0 },
        }
    }

    #[test]
    fn the_stamped_colour_is_24_bit_mid_grey() {
        // Guards against the `0x80808080` reading: the constant is built by
        // `lui 0x80 ; ori 0x8080`.
        assert_eq!(DEFEATED_GREY, 0x0080_8080);
        assert_eq!(DEFEATED_GREY & COLOUR_MASK, DEFEATED_GREY);
        assert_eq!(COLOUR_MASK, 0x00FF_FFFF);
    }

    #[test]
    fn inert_actor_does_nothing() {
        let a = BattleActorView {
            flags: FLAG_ACTOR_INERT,
            colour: 0x123456,
            seat: 4,
        };
        let t = battle_actor_tick(&a, &gates(true), BattleTeardown::default());
        assert!(t.inert);
        assert!(t.steps.is_empty());
        assert_eq!(t.colour, 0x123456);
    }

    #[test]
    fn untinted_actor_that_fails_the_gate_is_not_drawn() {
        let a = BattleActorView {
            flags: 0,
            colour: 0,
            seat: 4,
        };
        let t = battle_actor_tick(&a, &gates(false), BattleTeardown::default());
        assert_eq!(t.steps, vec![BattleDrawStep::Tint]);
        assert_eq!(t.colour, 0);
        assert!(t.trail_flag.is_none());
    }

    #[test]
    fn untinted_defeated_monster_gets_grey_then_draw_then_tint() {
        let a = BattleActorView {
            flags: 0,
            colour: 0,
            seat: 5,
        };
        let t = battle_actor_tick(&a, &gates(true), BattleTeardown::default());
        assert_eq!(
            t.steps,
            vec![
                BattleDrawStep::Tint,
                BattleDrawStep::Draw,
                BattleDrawStep::Tint
            ]
        );
        assert_eq!(t.colour, DEFEATED_GREY);
    }

    #[test]
    fn tinted_actor_always_draws_even_when_the_gate_fails() {
        let a = BattleActorView {
            flags: 0,
            colour: 0x203040,
            seat: 1,
        };
        let t = battle_actor_tick(&a, &gates(false), BattleTeardown::default());
        assert_eq!(
            t.steps,
            vec![
                BattleDrawStep::Tint,
                BattleDrawStep::SignatureEffect,
                BattleDrawStep::Afterimage,
                BattleDrawStep::Tint,
                BattleDrawStep::Draw,
            ]
        );
        assert_eq!(t.colour, 0x203040);
    }

    #[test]
    fn trail_flag_latches_only_on_seat_seven() {
        for seat in 0i16..=8 {
            let a = BattleActorView {
                flags: 0,
                colour: 1,
                seat,
            };
            let t = battle_actor_tick(&a, &gates(false), BattleTeardown::default());
            assert_eq!(
                t.trail_flag,
                Some(u16::from(seat == TRAIL_LATCH_SEAT)),
                "seat {seat}"
            );
        }
    }

    #[test]
    fn only_seats_three_through_six_can_take_the_grey() {
        for seat in -1i16..=8 {
            let want = (3..=6).contains(&seat);
            assert_eq!(defeated_grey_gate(seat, &gates(true)), want, "seat {seat}");
        }
    }

    #[test]
    fn debug_hold_byte_blocks_the_grey() {
        let mut g = gates(true);
        g.debug_hold = true;
        assert!(!defeated_grey_gate(4, &g));
    }

    #[test]
    fn seat_state_must_be_exactly_two() {
        for state in 0u8..=4 {
            let mut g = gates(true);
            g.seat_state = state;
            assert_eq!(defeated_grey_gate(4, &g), state == DEFEATED_STATE);
        }
    }

    #[test]
    fn teardown_body_needs_the_ready_flag_but_the_clear_does_not() {
        let t = battle_scene_teardown(true, 0x11, true);
        assert!(t.requested);
        assert!(!t.ran);
        assert!(!t.extra_hook);
        let t = battle_scene_teardown(true, EFFECT_VM_READY, true);
        assert!(t.ran);
        assert!(t.extra_hook);
        let t = battle_scene_teardown(false, EFFECT_VM_READY, true);
        assert!(!t.requested);
        assert!(!t.ran);
    }

    #[test]
    fn only_flagged_effect_nodes_are_voided() {
        let mut nodes = vec![None; 4];
        nodes[0] = Some(EFFECT_NODE_VOID_BIT);
        nodes[1] = Some(0x1);
        nodes[3] = Some(EFFECT_NODE_VOID_BIT | 0x4);
        assert_eq!(voided_effect_nodes(&nodes), vec![0, 3]);
    }

    #[test]
    fn effect_node_sweep_stops_at_the_table_length() {
        let nodes = vec![Some(EFFECT_NODE_VOID_BIT); EFFECT_NODE_SLOTS + 5];
        assert_eq!(voided_effect_nodes(&nodes).len(), EFFECT_NODE_SLOTS);
    }
}
