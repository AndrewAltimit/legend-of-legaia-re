//! The field-script half of the retail comparison corpus: which field-VM
//! contexts a retail state holds, and where in their bytecode they stand.
//!
//! Every field context is an actor node on the five SCUS actor lists
//! (`_DAT_8007C34C..`, walked by `FUN_8002519C` through the node's `+0x00`
//! link). A context's script is `(base, pc)`: `+0x90` is the absolute RAM
//! address of the record's script start and `+0x9E` the signed-16 PC into it
//! (`FUN_80039B7C` reads both at `0x80039BD4` / `0x80039BD8`), `+0x50` is the
//! record's **flat** MAN index (`FUN_8003BDE0` stores `N0 + N1 + i` for a
//! spawned partition-2 record, `FUN_8003A1E4` `N0 + i` for a placement), and
//! `+0x10 & 0x100` is the engaged bit a running interaction holds
//! (`docs/subsystems/script-vm.md`, "Engagement and the system script").
//!
//! A state caught inside a running record names the record and the PC, and
//! the seed runs the engine's own copy of the record to that phase
//! ([`ScriptGate`]) instead of sampling a fixed window after the entry. The
//! scene system script (tick `FUN_801DA51C`, not the field actor tick) and
//! the player are not candidates: the first runs every frame whatever the
//! capture shows, the second is driven by the pad.

use legaia_engine_capture::script_gate::*;
use legaia_ram_image::game_anchors;

/// The actor-list heads `FUN_8002519C` walks (`_DAT_8007C34C` onward).
const ACTOR_LIST_HEADS: u32 = 0x8007_C34C;
/// Heads scanned from [`ACTOR_LIST_HEADS`] (four-byte stride, the player
/// pointer `_DAT_8007C364` among them is not a list head but reads as one
/// node chain at worst, which the tick filter drops).
const ACTOR_LIST_SPAN: u32 = 9;
/// Per-list walk bound (the actor pool is a small fixed table).
const ACTOR_LIST_MAX: usize = 512;
/// `FUN_8003BC08`, the field actor tick that routes an engaged context to
/// the script runner.
const FIELD_ACTOR_TICK: u32 = 0x8003_BC08;
/// `FUN_801DA51C`, the scene system script's tick.
const SYSTEM_SCRIPT_TICK: u32 = 0x801D_A51C;
/// The engaged bit.
const ENGAGED: u32 = 0x100;
/// `FUN_801DC0BC`, the cutscene camera mover's tick: progress `+0x9C`
/// counts up to the duration `+0x9E`, and `+0x10 & 0x8` marks it dead the
/// frame it lands (`0x801DD238..0x801DD260`).
const CAMERA_MOVER_TICK: u32 = 0x801D_C0BC;
/// A mover's dead bit.
const DEAD: u32 = 0x8;
/// A running (engaged) field context in a retail state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetailScript {
    /// `ctx[+0x50]`, the record's flat MAN index.
    pub flat_index: u16,
    /// `ctx[+0x9E]`, the PC relative to the record's script start.
    pub pc: usize,
    /// `ctx[+0x54]`, the wait accumulator an op-`0x4A` wait counts up.
    pub wait: i16,
    /// The opcode byte at the PC.
    pub op: u8,
    /// The record's first [`RECORD_HEAD_LEN`] bytes from its script start.
    pub head: Vec<u8>,
    /// The system flags the record latched on its way to the PC
    /// ([`run_latches`] over the record's own bytes in RAM): set after the
    /// scene entry ran, so they are not what that entry saw.
    pub latches: Vec<u16>,
    /// Parked on the player's clip end-latch spin (`AD F8 08`): the player's
    /// clip cursor `+0x68`, how far the poked clip has run.
    pub player_latch: Option<i16>,
    /// Parked on a cross-context op whose target placement still walks a
    /// compass leg: the target byte and the leg's spent units `+0x54`
    /// ([`walking_target`]).
    pub walk: Option<(u8, i16)>,
}

/// The halt bit a walk arm raises on its target (`+0x10 & 0x400`), held
/// until the walk kernel lands the leg.
const HALT: u32 = 0x400;

/// A gate op aimed at a placement that is still walking: the op at the PC
/// is a cross-context one (`0x80` set, the target byte after it), and the
/// node whose flat index `+0x50` is that byte carries the halt bit with a
/// compass walk (`B7` / `C1`, ops `0x37` / `0x41`) seated on its `+0x94`.
/// The dispatcher prologue (`0x801DE90C..0x801DE944`) returns such an op at
/// its own PC every frame until the leg lands, so the PC says nothing about
/// how far the leg has run; its `+0x54` does. `town0c`'s queen-bee beat
/// (`rim_elm_queen_bee_battle`) is parked on `C1 3B 02 41` with flat 59
/// eight units into the `C1 3B 02 46` leg before it.
fn walking_target(ram: &[u8], nodes: &[u32], op: u8, target: Option<u8>) -> Option<(u8, i16)> {
    if op & 0x80 == 0 {
        return None;
    }
    let target = target.filter(|&t| t != 0xF8 && t != 0xFB)?;
    nodes.iter().find_map(|&n| {
        if game_anchors::u32_at(ram, n + 0x0C) != FIELD_ACTOR_TICK
            || game_anchors::u16_at(ram, n + 0x50) != u16::from(target)
            || game_anchors::u32_at(ram, n + 0x10) & HALT == 0
        {
            return None;
        }
        let seat = game_anchors::u32_at(ram, n + 0x94);
        let walk_op = in_ram(seat).then(|| game_anchors::u8_at(ram, seat))?;
        (walk_op & 0x80 != 0 && matches!(walk_op & 0x7F, 0x37 | 0x41))
            .then(|| (target, game_anchors::i16_at(ram, n + 0x54)))
    })
}

/// The player's clip end-latch spin: the cross-context local-flag test
/// `AD F8 08`, which holds the record until the player's clip latches its
/// end bit.
fn on_player_latch(op: u8, operands: Option<Vec<u8>>) -> bool {
    op == 0xAD && operands.is_some_and(|o| o == [0xF8, 0x08])
}

/// The field-script observables of one retail state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetailScripts {
    /// Engaged contexts other than the system script and the player, in
    /// actor-list order.
    pub running: Vec<RetailScript>,
    /// Display frames left on a live camera-mover glide, when one is in
    /// flight (duration `+0x9E` less progress `+0x9C`).
    pub glide_left: Option<i32>,
    /// How many vsyncs the displayed frame is older than the RAM
    /// ([`crate::retail_compare_battle::display_lag_vsyncs`]): the field
    /// double-buffers the same way the battle does.
    pub display_lag: u16,
    /// The scene system script's PC (`+0x9E` of the node ticked by
    /// `FUN_801DA51C`): where the entry script's last pass parked.
    pub system_pc: Option<usize>,
    /// The field pager mid-page: its typing row's window index and reveal
    /// counter, while `_DAT_801F2734` is the typing state `0x0B`
    /// ([`pager_typing`]).
    pub typing: Option<(u8, i32)>,
}

/// The pager state word `_DAT_801F2734` (field overlay).
const PAGER_STATE: u32 = 0x801F_2734;
/// The reveal counter `_DAT_801F2748`: units of the typing row drawn.
const PAGER_REVEAL: u32 = 0x801F_2748;
/// The row table's last index `_DAT_801F3530`: the row being typed.
const PAGER_LAST_ROW: u32 = 0x801F_3530;
/// Pager state `0x0B`: typing the row at the last index.
const PAGER_TYPING: i32 = 0x0B;

/// The pager's typing row and reveal counter when the state holds a page
/// still being typed (`FUN_801D84D0` state `0x0B`, `0x801D89B8`).
pub fn pager_typing(ram: &[u8]) -> Option<(u8, i32)> {
    if game_anchors::u32_at(ram, PAGER_STATE) as i32 != PAGER_TYPING {
        return None;
    }
    let row = u8::try_from(game_anchors::u32_at(ram, PAGER_LAST_ROW)).ok()?;
    let counter = game_anchors::u32_at(ram, PAGER_REVEAL) as i32;
    Some((row, counter))
}

fn in_ram(p: u32) -> bool {
    (p & 0xFFE0_0000) == 0x8000_0000
}

fn bytes_at(ram: &[u8], va: u32, len: usize) -> Option<Vec<u8>> {
    let lo = (va & 0x1F_FFFF) as usize;
    ram.get(lo..lo + len).map(<[u8]>::to_vec)
}

/// Every node on the actor lists, each once, in list order.
pub fn actor_nodes(ram: &[u8]) -> Vec<u32> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for k in 0..ACTOR_LIST_SPAN {
        let mut p = game_anchors::u32_at(ram, ACTOR_LIST_HEADS + 4 * k);
        let mut n = 0;
        while in_ram(p) && n < ACTOR_LIST_MAX && seen.insert(p) {
            out.push(p);
            p = game_anchors::u32_at(ram, p);
            n += 1;
        }
    }
    out
}

impl RetailScripts {
    /// Read the field contexts of a retail state's RAM.
    pub fn from_ram(ram: &[u8]) -> Self {
        let player = game_anchors::player_ptr(ram);
        let mut out = Self {
            display_lag: crate::retail_compare_battle::display_lag_vsyncs(ram),
            typing: pager_typing(ram),
            ..Self::default()
        };
        let nodes = actor_nodes(ram);
        for &node in &nodes {
            let tick = game_anchors::u32_at(ram, node + 0x0C);
            let flags = game_anchors::u32_at(ram, node + 0x10);
            let id = game_anchors::u16_at(ram, node + 0x50);
            if tick == CAMERA_MOVER_TICK && flags & DEAD == 0 && out.glide_left.is_none() {
                let left = i32::from(game_anchors::i16_at(ram, node + 0x9E))
                    - i32::from(game_anchors::i16_at(ram, node + 0x9C));
                out.glide_left = Some(left.max(0));
                continue;
            }
            let base = game_anchors::u32_at(ram, node + 0x90);
            if !in_ram(base) {
                continue;
            }
            if tick == SYSTEM_SCRIPT_TICK && out.system_pc.is_none() {
                out.system_pc = usize::try_from(game_anchors::i16_at(ram, node + 0x9E)).ok();
                continue;
            }
            let head = bytes_at(ram, base, RECORD_HEAD_LEN);
            if tick != FIELD_ACTOR_TICK || flags & ENGAGED == 0 || Some(node) == player {
                continue;
            }
            let pc = game_anchors::i16_at(ram, node + 0x9E);
            let (Ok(pc), Some(head)) = (usize::try_from(pc), head) else {
                continue;
            };
            let op = game_anchors::u8_at(ram, base.wrapping_add(pc as u32));
            let player_latch =
                on_player_latch(op, bytes_at(ram, base.wrapping_add(pc as u32 + 1), 2))
                    .then(|| player.map(|p| game_anchors::i16_at(ram, p + 0x68)))
                    .flatten();
            let walk = walking_target(
                ram,
                &nodes,
                op,
                bytes_at(ram, base.wrapping_add(pc as u32 + 1), 1).map(|b| b[0]),
            );
            out.running.push(RetailScript {
                flat_index: id,
                pc,
                wait: game_anchors::i16_at(ram, node + 0x54),
                op,
                player_latch,
                walk,
                head,
                latches: bytes_at(ram, base, pc + 1)
                    .map(|body| run_latches(&body, pc))
                    .unwrap_or_default(),
            });
        }
        out
    }
}

/// The engine context running the same record as `script`, if any.
pub fn find_engine_context<'a>(
    contexts: &'a [EngineContext],
    script: &RetailScript,
) -> Option<&'a EngineContext> {
    contexts.iter().find(|c| c.head == script.head)
}

/// The gate for a retail state's first running context, if any.
pub fn script_gate_from_retail(scripts: &RetailScripts) -> Option<ScriptGate> {
    scripts.running.first().map(|s| ScriptGate {
        flat_index: s.flat_index,
        head: s.head.clone(),
        pc: s.pc,
        wait: s.wait.max(0),
        op: s.op,
        glide_left: scripts.glide_left,
        retire_watch: std::cell::Cell::new((false, false)),
        system_pc: scripts.system_pc,
        resumed_early: std::cell::Cell::new(false),
        typing: scripts.typing.filter(|_| s.op & 0x7F < 0x20),
        latch: s.player_latch,
        walk: s.walk,
    })
}

/// The gate the capture's **displayed frame** sits at:
/// [`script_gate_from_retail`] with the wait taken back by the display lag.
/// The RAM channels are sampled on the RAM's phase; the frame on the TV is
/// two game frames older. `minigame_dance_pcsx` is parked `21` vsyncs into
/// the `0x4A` after a subtractive white walk-in (`34 01 FF FF FF 1E`, `27`
/// vsyncs after the eighth a white blend-2 target loses) on a step-`3`
/// frame: gated on the RAM's wait the engine frame was darkened six vsyncs
/// past the one retail shows.
pub fn displayed_script_gate_from_retail(scripts: &RetailScripts) -> Option<ScriptGate> {
    let mut g = script_gate_from_retail(scripts)?;
    g.wait = (i32::from(g.wait) - i32::from(scripts.display_lag)).max(0) as i16;
    // Two pager calls back: each typing call adds `min(frame step, 4)` units
    // (`0x801D8A08..0x801D8A70`), and the lag is two game frames.
    if let Some((_, counter)) = g.typing.as_mut() {
        *counter = (*counter - i32::from(scripts.display_lag.min(8))).max(1);
    }
    if let Some(left) = g.glide_left.as_mut() {
        *left += i32::from(scripts.display_lag);
    }
    // A leg spends one unit a vsync, so the displayed frame's leg had
    // `display_lag` units fewer behind it.
    if let Some((_, accum)) = g.walk.as_mut() {
        *accum = (i32::from(*accum) - i32::from(scripts.display_lag)).max(0) as i16;
    }
    Some(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A RAM image with one actor list: the scene system context (tick
    /// `FUN_801DA51C`) and one engaged spawned record, linked from the first
    /// list head. Only the record runs.
    #[test]
    fn engaged_contexts_are_read_off_the_actor_lists() {
        fn w32(ram: &mut [u8], va: u32, v: u32) {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        fn w16(ram: &mut [u8], va: u32, v: u16) {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
        }
        let mut ram = vec![0u8; 0x20_0000];
        let (sys, rec) = (0x8010_0000u32, 0x8010_1000u32);
        let (sys_base, rec_base) = (0x8012_0000u32, 0x8012_1000u32);
        w32(&mut ram, ACTOR_LIST_HEADS, sys);
        w32(&mut ram, sys, rec);
        w32(&mut ram, sys + 0x0C, 0x801D_A51C);
        w32(&mut ram, sys + 0x10, ENGAGED);
        w16(&mut ram, sys + 0x50, 0xFB);
        w32(&mut ram, sys + 0x90, sys_base);
        w32(&mut ram, rec + 0x0C, FIELD_ACTOR_TICK);
        w32(&mut ram, rec + 0x10, ENGAGED);
        w16(&mut ram, rec + 0x50, 13);
        w16(&mut ram, rec + 0x54, 48);
        w32(&mut ram, rec + 0x90, rec_base);
        w16(&mut ram, rec + 0x9E, 7);
        // `koin3` P2[6]'s run onto its last wait: `55 9C`, `51 34`, then a
        // `4A` the PC is parked on.
        let body = (rec_base & 0x1F_FFFF) as usize;
        ram[body..body + 7].copy_from_slice(&[0x55, 0x9C, 0x51, 0x34, 0x4A, 0x1E, 0x00]);
        ram[body + 7] = 0x4A;
        ram[(sys_base & 0x1F_FFFF) as usize] = 0x24;
        let s = RetailScripts::from_ram(&ram);
        assert_eq!(s.running.len(), 1);
        let r = &s.running[0];
        assert_eq!((r.flat_index, r.pc, r.wait, r.op), (13, 7, 48, 0x4A));
        assert_eq!(r.latches, vec![0x59C, 0x134]);
        // A non-adaptive step (mode word clear) is one vsync a frame, so the
        // displayed frame is two vsyncs into the wait behind the RAM.
        assert_eq!(s.display_lag, 2);
        assert_eq!(script_gate_from_retail(&s).map(|g| g.wait), Some(48));
        assert_eq!(
            displayed_script_gate_from_retail(&s).map(|g| g.wait),
            Some(46)
        );
        // A disengaged context is not running.
        w32(&mut ram, rec + 0x10, 0);
        assert!(RetailScripts::from_ram(&ram).running.is_empty());
    }

    /// A gate op aimed at a placement still walking the compass leg the
    /// record armed carries that leg's spent units; a target with no leg on
    /// its `+0x94`, or one whose halt bit is down, carries nothing.
    #[test]
    fn a_gate_held_behind_a_walking_placement_carries_the_leg() {
        fn w32(ram: &mut [u8], va: u32, v: u32) {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        fn w16(ram: &mut [u8], va: u32, v: u16) {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
        }
        let mut ram = vec![0u8; 0x20_0000];
        let (walker, seat) = (0x8010_2000u32, 0x8012_2000u32);
        w32(&mut ram, walker + 0x0C, FIELD_ACTOR_TICK);
        w32(&mut ram, walker + 0x10, HALT);
        w16(&mut ram, walker + 0x50, 59);
        w16(&mut ram, walker + 0x54, 8);
        w32(&mut ram, walker + 0x94, seat);
        // `town0c` `P1[21]`: `C1 3B 02 46` seated on flat 59.
        let o = (seat & 0x1F_FFFF) as usize;
        ram[o..o + 4].copy_from_slice(&[0xC1, 0x3B, 0x02, 0x46]);
        let nodes = [walker];
        assert_eq!(walking_target(&ram, &nodes, 0xC1, Some(59)), Some((59, 8)));
        // Not a cross-context op, the player, or another target.
        assert_eq!(walking_target(&ram, &nodes, 0x41, Some(59)), None);
        assert_eq!(walking_target(&ram, &nodes, 0xC1, Some(0xF8)), None);
        assert_eq!(walking_target(&ram, &nodes, 0xC1, Some(58)), None);
        // A seated rotate is not a walk.
        ram[o] = 0xB8;
        assert_eq!(walking_target(&ram, &nodes, 0xC1, Some(59)), None);
        ram[o] = 0xC1;
        // The landing drops the halt bit.
        w32(&mut ram, walker + 0x10, 0);
        assert_eq!(walking_target(&ram, &nodes, 0xC1, Some(59)), None);
        // The displayed frame is the display lag's vsyncs into the leg less.
        let scripts = RetailScripts {
            running: vec![RetailScript {
                flat_index: 57,
                pc: 0xD9,
                wait: 0,
                op: 0xC1,
                head: vec![0x25],
                latches: Vec::new(),
                player_latch: None,
                walk: Some((59, 8)),
            }],
            display_lag: 4,
            ..RetailScripts::default()
        };
        assert_eq!(
            script_gate_from_retail(&scripts).and_then(|g| g.walk),
            Some((59, 8))
        );
        assert_eq!(
            displayed_script_gate_from_retail(&scripts).and_then(|g| g.walk),
            Some((59, 4))
        );
    }

    /// A text gate caught while the pager types (state `0x0B`) carries the
    /// typing row and reveal counter, taken two pager calls back for the
    /// displayed frame; a gate parked on `AD F8 08` carries the player's
    /// clip cursor.
    #[test]
    fn a_mid_page_text_gate_and_a_player_latch_gate_carry_their_progress() {
        fn w32(ram: &mut [u8], va: u32, v: u32) {
            let o = (va & 0x1F_FFFF) as usize;
            ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        let mut ram = vec![0u8; 0x20_0000];
        assert_eq!(pager_typing(&ram), None);
        w32(&mut ram, PAGER_STATE, 0x0B);
        w32(&mut ram, PAGER_LAST_ROW, 1);
        w32(&mut ram, PAGER_REVEAL, 19);
        assert_eq!(pager_typing(&ram), Some((1, 19)));
        let script = |op: u8| RetailScript {
            flat_index: 9,
            pc: 64,
            wait: 0,
            op,
            head: vec![0x1F],
            latches: Vec::new(),
            player_latch: (op == 0xAD).then_some(219),
            walk: None,
        };
        let scripts = RetailScripts {
            running: vec![script(0x1F)],
            display_lag: 4,
            typing: pager_typing(&ram),
            ..RetailScripts::default()
        };
        assert_eq!(
            script_gate_from_retail(&scripts).and_then(|g| g.typing),
            Some((1, 19))
        );
        assert_eq!(
            displayed_script_gate_from_retail(&scripts).and_then(|g| g.typing),
            Some((1, 15))
        );
        // The pager word means nothing to a gate that is not on text.
        let scripts = RetailScripts {
            running: vec![script(0xAD)],
            ..scripts
        };
        let g = script_gate_from_retail(&scripts).unwrap();
        assert_eq!((g.typing, g.latch), (None, Some(219)));
        assert!(on_player_latch(0xAD, Some(vec![0xF8, 0x08])));
        assert!(!on_player_latch(0xAD, Some(vec![0x31, 0x08])));
    }
}
