//! Per-actor **battle draw tick** - the pass that decides whether a battle
//! body is drawn this frame, with which colour, and in what order its tint,
//! trail and draw calls run.
//!
//! PORT: FUN_800480d8
//!
//! Live through `legaia_engine_core::world::World::battle_actor_draw_plan`,
//! which both play hosts call once per battle body per frame (the native
//! window's battle actor pass and the browser play page's
//! `play_battle_actor_transforms` / `play_battle_actor_cursor`). The engine
//! runs the calls this pass sequences by other routes - the tint through
//! [`crate::battle_actor_tint`], the after-image walk `FUN_80049348` through
//! `World::battle_ghost_draws`, the draw itself through each host's mesh
//! pass. So the port is the **decision**: [`battle_actor_tick`] returns the
//! ordered [`BattleDrawStep`] list and the colour word the draw sees, and
//! [`BattleActorTick::drawn`] is what the hosts gate the body on.
//!
//! Source: `ghidra/scripts/funcs/800480d8.txt` (disassembly), with its caller
//! `ghidra/scripts/funcs/8001ada4.txt`.
//!
//! REF: FUN_8001ada4 - the render dispatcher whose mode-2 arm calls this pass.
//! REF: FUN_8004a908 - tint / fade pass (`BattleDrawStep::Tint`).
//! REF: FUN_80048a08 - TMD draw pass (`BattleDrawStep::Draw`).
//! REF: FUN_80049348 - arts after-image walk (`BattleDrawStep::Afterimage`).
//! REF: FUN_8005112c - per-character signature effect trigger.
//! REF: FUN_80050e74 - the raise half of the effect-node void protocol
//! (`move_vm::flush_part_actor_pool`); the sweep below is the collect half.
//!
//! # Who calls it, and the gate in front of it
//!
//! The render dispatcher `FUN_8001ADA4` switches on `actor[+0x56]` through
//! the jump table at `0x8001042C`; render mode `2` lands on `0x8001AEE0`,
//! which calls this pass (`jal 0x800480d8` at `0x8001AEF4`) only when
//! `actor[+0x34] >= 0xA1` (`slti v0,v0,0xa1` / `bne` at `0x8001AEE8`). That
//! word is the actor's **view-space depth**: the dispatcher head writes it
//! with an `MVMVA` of `actor[+0x14]` (`cop2 0x480012`, `swc2` of MAC1..3 to
//! `+0x2C..+0x34`, `0x8001AE0C..0x8001AE34`) when `actor[+0x10] & 0x80` is
//! set, and parks it at `0x7D0` otherwise. So the `0xA1` test is a near-plane
//! reject - a body at or behind 160 view units is skipped - not an "actor id
//! below `0xA1`" window. The head also skips any node with `+0x10 & 0xA`.
//! [`dispatch_draws`] is that gate.
//!
//! # The colour word is RGB, not a flag
//!
//! The pass tests `actor[+0x74] & 0x00FF_FFFF` and, on the grey path, stamps
//! `0x0080_8080`. Both constants are built by `lui v?,0x80 ; ori
//! v?,v?,0x8080` / `lui v?,0xff ; ori v?,v?,0xffff`, i.e. `0x00808080` and
//! `0x00FFFFFF` - a **24-bit RGB** field holding mid-grey. It is not a
//! `0x80808080` word.
//!
//! # Two arms off that word
//!
//! The tint pass runs first and rewrites `+0x74`; a seated actor whose colour
//! lanes (`+0x04`) are zero leaves only the top byte. Then:
//!
//! * **Word zero**: the pass only *considers* stamping grey. It needs the
//!   seat `actor[+0x5A]` in `3..=6` (`lhu`, `addiu -3`, `sltiu 4` at
//!   `0x800481D4..0x800481E0`), the no-escape byte `ctx[+0x287]` set,
//!   `gp[+0x9F5]` clear, and the seated actor's `+0x21C` state to read
//!   exactly `2`. `gp[+0x9F5]` is `0x8007BD0D`, the formation's **second
//!   monster id** (the encounter loader stores slot 1 there,
//!   `docs/formats/encounter.md`; it reads `0` in every captured one-monster
//!   battle and the second id in every multi-monster one), so the gate is
//!   "a defeated monster in a lone-monster scripted fight". All four hold -> stamp grey, draw, tint. Any
//!   one fails -> the actor is **not drawn** this frame.
//! * **Word non-zero**: signature effect, trail flag on, after-image walk,
//!   tint; the trail flag is cleared again unless the seat is exactly `7`.
//!   The same gate follows with one difference - the seat test here is
//!   `lh` / `slti 3` (`0x80048278..0x80048280`), a signed `seat >= 3` with no
//!   upper bound - and it decides between "stamp grey, draw, tint" and a
//!   plain draw.
//!
//! So on the zero arm failing the gate costs the draw; on the non-zero arm
//! the body still draws in its own colour.
//!
//! # The per-frame global passes
//!
//! Ahead of all of that, a set `ctx[+0x272]` byte runs the battle's
//! once-per-frame global passes. Its one writer is the battle frame driver
//! `FUN_80046A20` (`sb v1,0x272(v0)` with `v1 = 1` at `0x80047104`, every
//! frame the frame gate `gp[+0x330]` reads negative), and the first
//! non-inert body drawn consumes it (`sb zero` at `0x800481B0`). Under the
//! battle-running signal `DAT_8007BD71 == 0xFF` it calls the effect-VM walker
//! `FUN_801E0080`, the cast census `FUN_801E09F8`, the damage-number popup
//! `FUN_801DF6B8` and `FUN_801E2524`, voids every entry of the `0x80`-slot
//! effect-node table `DAT_801C90F0` whose target carries flag bit `0x8`, and
//! runs PROT 0920's `FUN_801F7B88` while `_DAT_8007BDC0 != 0`. This is not a
//! scene teardown: the end sequence raises `0xFE`, which closes the arm (the
//! exec-breakpoint measurement in `docs/subsystems/cast-module.md` enters it
//! once per rendered frame until the victory). The engine runs each of those
//! passes from its own frame tick; [`battle_frame_passes`] is the gate.

/// `actor + 0x10` bit that skips the whole tick.
pub const FLAG_ACTOR_INERT: u32 = 0x8;
/// `actor + 0x10` bits the render dispatcher skips a node on.
pub const DISPATCH_SKIP_MASK: u32 = 0xA;
/// `actor + 0x10` bit that makes the dispatcher transform the node's
/// position; clear, the depth word is parked at [`PARKED_VIEW_DEPTH`].
pub const FLAG_TRANSFORM: u32 = 0x80;
/// The depth the dispatcher parks an untransformed node at (`li v0,0x7d0`).
pub const PARKED_VIEW_DEPTH: i32 = 0x7D0;
/// Smallest view depth the mode-2 arm draws at (`slti v0,v0,0xa1`).
pub const NEAR_REJECT_DEPTH: i32 = 0xA1;
/// Mask retail applies to the colour word `actor + 0x74`.
pub const COLOUR_MASK: u32 = 0x00FF_FFFF;
/// The mid-grey RGB stamped onto a gated monster.
pub const DEFEATED_GREY: u32 = 0x0080_8080;
/// Effect-node table slot count (`DAT_801C90F0`).
pub const EFFECT_NODE_SLOTS: usize = 0x80;
/// Flag bit an effect-node target must carry to be voided.
pub const EFFECT_NODE_VOID_BIT: u32 = 0x8;
/// `DAT_8007BD71` value that lets the per-frame passes run (battle running).
pub const BATTLE_RUNNING: u8 = 0xFF;
/// Seat value that keeps the trail flag (`actor + 0x6A`) latched on.
pub const TRAIL_LATCH_SEAT: i16 = 7;
/// `actor + 0x21C` value the grey gate requires.
pub const DEFEATED_STATE: u8 = 2;

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

/// The per-frame global-pass preamble's verdict.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleFramePasses {
    /// `true` when `ctx[+0x272]` was set, so the byte gets cleared.
    pub requested: bool,
    /// `true` when the battle-running signal also let the passes run.
    pub ran: bool,
    /// `true` when PROT 0920's hook (`FUN_801F7B88`) fires - gated on
    /// `_DAT_8007BDC0 != 0`.
    pub extra_hook: bool,
}

/// The retail actor fields the tick reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleActorView {
    /// `actor + 0x10` flag word.
    pub flags: u32,
    /// `actor + 0x74` colour word, as the tint pass left it.
    pub colour: u32,
    /// `actor + 0x5A` seat index.
    pub seat: i16,
}

/// The gates that live outside the actor.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleTickGates {
    /// `ctx + 0x287` - the no-escape byte.
    pub no_escape: bool,
    /// `gp + 0x9F5` (`0x8007BD0D`) non-zero - the formation has a second
    /// monster; the gate needs it clear.
    pub second_monster: bool,
    /// `*(DAT_801C9370 + seat*4) + 0x21C` - the seated actor's state byte.
    pub seat_state: u8,
}

/// The tick's result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleActorTick {
    /// Calls in retail order.
    pub steps: Vec<BattleDrawStep>,
    /// The colour word the [`BattleDrawStep::Draw`] step sees - the tint
    /// pass's word, or [`DEFEATED_GREY`] when the gate stamped it.
    pub colour: u32,
    /// The value retail leaves in `actor + 0x6A` (the trail flag), or `None`
    /// when the pass never touches it.
    pub trail_flag: Option<u16>,
    /// `true` when the actor was inert (`+0x10 & 8`) and nothing ran.
    pub inert: bool,
}

impl BattleActorTick {
    /// Whether the body is drawn this frame.
    pub fn drawn(&self) -> bool {
        self.steps.contains(&BattleDrawStep::Draw)
    }
}

/// The render dispatcher's mode-2 gate: skip on `+0x10 & 0xA`, and reject
/// a body whose view depth is below [`NEAR_REJECT_DEPTH`]. `view_depth` is
/// the node's transformed `+0x34`; an untransformed node (`+0x10 & 0x80`
/// clear) is judged at [`PARKED_VIEW_DEPTH`].
pub fn dispatch_draws(flags: u32, view_depth: i32) -> bool {
    if flags & DISPATCH_SKIP_MASK != 0 {
        return false;
    }
    let z = if flags & FLAG_TRANSFORM != 0 {
        view_depth
    } else {
        PARKED_VIEW_DEPTH
    };
    z >= NEAR_REJECT_DEPTH
}

/// Decide the per-frame global-pass preamble.
pub fn battle_frame_passes(
    requested: bool,
    battle_signal: u8,
    extra_hook_armed: bool,
) -> BattleFramePasses {
    let ran = requested && battle_signal == BATTLE_RUNNING;
    BattleFramePasses {
        requested,
        ran,
        extra_hook: ran && extra_hook_armed,
    }
}

/// Which effect-node slots the preamble voids: every non-null entry whose
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

/// The grey gate on the **zero** arm: seat in `3..=6` (an unsigned
/// `seat - 3 < 4`), no-escape byte set, no second formation monster, seat
/// state exactly [`DEFEATED_STATE`].
pub fn grey_gate_untinted(seat: i16, gates: &BattleTickGates) -> bool {
    (seat as u16).wrapping_sub(3) < 4 && grey_gate_tail(gates)
}

/// The grey gate on the **non-zero** arm: the seat test is a signed
/// `seat >= 3` with no upper bound, the other three terms as on the zero arm.
pub fn grey_gate_tinted(seat: i16, gates: &BattleTickGates) -> bool {
    seat >= 3 && grey_gate_tail(gates)
}

fn grey_gate_tail(gates: &BattleTickGates) -> bool {
    gates.no_escape && !gates.second_monster && gates.seat_state == DEFEATED_STATE
}

/// Run the per-actor draw tick over the colour word the leading tint pass
/// produced.
pub fn battle_actor_tick(actor: &BattleActorView, gates: &BattleTickGates) -> BattleActorTick {
    let mut out = BattleActorTick {
        colour: actor.colour,
        ..Default::default()
    };
    if actor.flags & FLAG_ACTOR_INERT != 0 {
        out.inert = true;
        return out;
    }

    // The unconditional first tint.
    out.steps.push(BattleDrawStep::Tint);

    if actor.colour & COLOUR_MASK == 0 {
        if grey_gate_untinted(actor.seat, gates) {
            out.colour = DEFEATED_GREY;
            out.steps.push(BattleDrawStep::Draw);
            out.steps.push(BattleDrawStep::Tint);
        }
        return out;
    }

    out.steps.push(BattleDrawStep::SignatureEffect);
    out.steps.push(BattleDrawStep::Afterimage);
    out.steps.push(BattleDrawStep::Tint);
    out.trail_flag = Some(u16::from(actor.seat == TRAIL_LATCH_SEAT));

    if grey_gate_tinted(actor.seat, gates) {
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
            second_monster: false,
            seat_state: if defeated { DEFEATED_STATE } else { 0 },
        }
    }

    #[test]
    fn the_stamped_colour_is_24_bit_mid_grey() {
        assert_eq!(DEFEATED_GREY, 0x0080_8080);
        assert_eq!(DEFEATED_GREY & COLOUR_MASK, DEFEATED_GREY);
        assert_eq!(COLOUR_MASK, 0x00FF_FFFF);
    }

    #[test]
    fn the_dispatch_gate_is_a_near_plane_on_view_depth() {
        assert!(dispatch_draws(FLAG_TRANSFORM, NEAR_REJECT_DEPTH));
        assert!(!dispatch_draws(FLAG_TRANSFORM, NEAR_REJECT_DEPTH - 1));
        assert!(!dispatch_draws(FLAG_TRANSFORM, -500));
        // Untransformed nodes are judged at the parked depth.
        assert!(dispatch_draws(0, -500));
        assert!(!dispatch_draws(FLAG_TRANSFORM | 0x2, 5000));
        assert!(!dispatch_draws(FLAG_TRANSFORM | 0x8, 5000));
    }

    #[test]
    fn inert_actor_does_nothing() {
        let a = BattleActorView {
            flags: FLAG_ACTOR_INERT,
            colour: 0x123456,
            seat: 4,
        };
        let t = battle_actor_tick(&a, &gates(true));
        assert!(t.inert);
        assert!(t.steps.is_empty());
        assert!(!t.drawn());
        assert_eq!(t.colour, 0x123456);
    }

    #[test]
    fn untinted_actor_that_fails_the_gate_is_not_drawn() {
        let a = BattleActorView {
            flags: 0,
            colour: 0x8100_0000,
            seat: 4,
        };
        let t = battle_actor_tick(&a, &gates(false));
        assert_eq!(t.steps, vec![BattleDrawStep::Tint]);
        assert!(!t.drawn());
        assert!(t.trail_flag.is_none());
    }

    #[test]
    fn untinted_gated_monster_gets_grey_then_draw_then_tint() {
        let a = BattleActorView {
            flags: 0,
            colour: 0,
            seat: 5,
        };
        let t = battle_actor_tick(&a, &gates(true));
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
        let t = battle_actor_tick(&a, &gates(false));
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
            let t = battle_actor_tick(&a, &gates(false));
            assert_eq!(t.trail_flag, Some(u16::from(seat == TRAIL_LATCH_SEAT)));
        }
    }

    #[test]
    fn the_two_arms_test_the_seat_differently() {
        // Zero arm: `sltiu (seat - 3), 4` - seats 3..=6 only.
        for seat in -1i16..=8 {
            assert_eq!(
                grey_gate_untinted(seat, &gates(true)),
                (3..=6).contains(&seat),
                "untinted seat {seat}"
            );
        }
        // Non-zero arm: `slti seat, 3` - every seat from 3 up, seat 7 too.
        for seat in -1i16..=8 {
            assert_eq!(
                grey_gate_tinted(seat, &gates(true)),
                seat >= 3,
                "tinted seat {seat}"
            );
        }
    }

    #[test]
    fn a_second_formation_monster_blocks_the_grey() {
        let mut g = gates(true);
        g.second_monster = true;
        assert!(!grey_gate_untinted(4, &g));
        assert!(!grey_gate_tinted(4, &g));
    }

    #[test]
    fn seat_state_must_be_exactly_two() {
        for state in 0u8..=4 {
            let mut g = gates(true);
            g.seat_state = state;
            assert_eq!(grey_gate_untinted(4, &g), state == DEFEATED_STATE);
        }
    }

    #[test]
    fn the_frame_passes_need_the_running_signal_but_the_clear_does_not() {
        let t = battle_frame_passes(true, 0xFE, true);
        assert!(t.requested && !t.ran && !t.extra_hook);
        let t = battle_frame_passes(true, BATTLE_RUNNING, true);
        assert!(t.ran && t.extra_hook);
        let t = battle_frame_passes(false, BATTLE_RUNNING, true);
        assert!(!t.requested && !t.ran);
    }

    #[test]
    fn only_flagged_effect_nodes_are_voided() {
        let mut nodes = vec![None; 4];
        nodes[0] = Some(EFFECT_NODE_VOID_BIT);
        nodes[1] = Some(0x1);
        nodes[3] = Some(EFFECT_NODE_VOID_BIT | 0x4);
        assert_eq!(voided_effect_nodes(&nodes), vec![0, 3]);
        let nodes = vec![Some(EFFECT_NODE_VOID_BIT); EFFECT_NODE_SLOTS + 5];
        assert_eq!(voided_effect_nodes(&nodes).len(), EFFECT_NODE_SLOTS);
    }
}
