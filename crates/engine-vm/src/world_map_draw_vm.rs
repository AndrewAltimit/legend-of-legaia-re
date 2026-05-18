//! Overlay-resident move-VM extension dispatcher (`FUN_801D362C`).
//!
//! PORT: FUN_801D362C
//!
//! `FUN_801D362C` is the dispatcher reached from the move-VM
//! (`FUN_80023070`) when the outer move-VM opcode is `0x2F` (overlay
//! extension). It walks one inner sub-opcode and returns an advance
//! count (in u16 halfwords) that the move-VM adds to its PC.
//!
//! The same function exists in **many** overlays at the same RAM
//! address (`overlay_world_map`, `overlay_world_map_top`,
//! `overlay_world_map_walk`, `overlay_0897` field, `overlay_dialog_mc4`,
//! `overlay_dialog_typing`, `overlay_cutscene_dialogue`,
//! `overlay_cutscene_mapview`); each overlay supplies its own
//! contents in the 61-entry JT at `0x801CE868`. This file ports the
//! `overlay_world_map` flavour - the JT-advance counts here are
//! derived from that overlay's handlers. Other overlays share most
//! advance counts (the JT structure is the same) but may dispatch to
//! different sub-handler bodies.
//!
//! ## Bytecode layout
//!
//! Every instruction has this shape:
//!
//! ```text
//!   u16 word_0      (branch-target / next-pc hint, read by some ops)
//!   u16 opcode      (0x00..=0x3C, read at `s3 + 0x2`)
//!   u16 arg0        (read at `s3 + 0x4`)
//!   u16 arg1        ...
//!   ...
//! ```
//!
//! The total size of the instruction varies per opcode and equals the
//! `li s2, N` exit-slot value of each handler (the advance is in u16
//! halfwords). Out-of-range opcodes (`>= 0x3D`) advance by 1 halfword
//! so the dispatcher can resync after a corrupt instruction.
//!
//! Note: when the dispatcher is invoked through the move-VM op 0x2F
//! escape, the outer move-VM op byte sits at `s3 + 0x0` (so `word_0`
//! = `[0x2F, 0x00]`). When the world-map controller calls
//! `FUN_801D362C` directly, `word_0` is just whatever the caller put
//! there - typically a default-next branch hint used by ops 0x13 /
//! 0x14 (the conditional-skip ops that `j 0x801d4838`).
//!
//! ## Scrolling-strip opcodes (sub-ops 0x2B..0x2E)
//!
//! Four sub-opcodes drive the per-scanline POLY_FT4 strip emitter
//! `FUN_801D31B0` and its slab/UV/GPU-mode state:
//!
//! | sub-op | role                                                          | size |
//! | ------ | ------------------------------------------------------------- | ---- |
//! | 0x2B   | Set slab UV bounds: writes `op[2..5]` to `slab[+0x18..+0x1E]` | 6    |
//! | 0x2C   | Invoke per-scanline POLY_FT4 strip emitter (`FUN_801D31B0`)   | 7    |
//! | 0x2D   | Increment slab UV bounds by `op[2..5]`                        | 6    |
//! | 0x2E   | Build GP0 TPage/CLUT packet from `op[2..]`                    | 13   |
//!
//! `FUN_801D31B0` is shared across many overlays (dialog, cutscene,
//! 0897 field, world-map) - it is **not** a continent-specific
//! function. The 832-byte body emits horizontal POLY_FT4 strips
//! parameterised by the slab descriptor at `actor[+0x9C]` (UV bounds,
//! tpage, clut, line height). Dialog overlays use it for scrolling
//! text-strip backgrounds; the world-map overlay variant has been
//! observed mapped to op-0x2C in the JT but has not been observed
//! dispatched during world-map render in any captured state (the
//! bulk continent prims in the world-map's prim pool come from a
//! different emitter that is still under investigation).
//!
//! The remaining 54 unique opcodes are advance-only stubs by default.
//! Engines that want to track e.g. flag writes can override the host
//! callbacks. Either way, the VM walks every byte of real bytecode
//! correctly because every advance count comes straight from the
//! decompiled `li s2, N` exit slots.
//!
//! ## Provenance
//!
//! See `ghidra/scripts/funcs/overlay_world_map_801d362c.txt` for the
//! full decompilation and
//! `ghidra/scripts/funcs/world_map_vm_jt_overlay_world_map.bin.txt`
//! for the jump-table at `0x801CE868`.

#![allow(clippy::needless_range_loop)]

/// Maximum sub-opcode value (`sltiu v0, v1, 0x3D` at `0x801D3654`).
pub const MAX_SUB_OPCODE: u16 = 0x3D;

/// One halfword = 2 bytes. Move-VM PC advances are expressed in u16 units.
pub const HALFWORD: usize = 2;

/// Slab descriptor offsets relative to the actor's `+0x9C` base.
///
/// These are the fields the per-scanline POLY_FT4 strip emitter
/// (`FUN_801D31B0`) reads. The drawing VM mutates the UV bounds
/// (`+0x18..+0x1E`) via sub-ops 0x2B / 0x2D; everything else is set up
/// either by the controller or by sub-op 0x2E (TPage/CLUT packet build).
pub mod slab {
    /// `slab[+0x14] = tpage` (PSX GPU TPage word).
    pub const TPAGE: usize = 0x14;
    /// `slab[+0x16] = clut` (PSX GPU CLUT word).
    pub const CLUT: usize = 0x16;
    /// `slab[+0x18..+0x1E]` = scrolling UV bounds (u16 x4).
    pub const UV_BOUNDS_START: usize = 0x18;
    pub const UV_BOUNDS_END: usize = 0x1E;
}

/// Result of dispatching a single sub-opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepResult {
    /// Number of u16 halfwords consumed by this instruction.
    ///
    /// Equivalent to the move-VM's `param_3` (the value `s2` is loaded
    /// with before the function returns); the move-VM adds this to its
    /// PC.
    pub size_u16: u16,
    /// True when the dispatched sub-opcode was out of range
    /// (`>= MAX_SUB_OPCODE`).
    pub out_of_range: bool,
}

/// Canonical advance count (in halfwords) for every recognised sub-opcode.
///
/// `None` for sub-opcodes `>= MAX_SUB_OPCODE` (treated as out-of-range
/// by the original; advance by 1).
///
/// Sourced from the `li s2, N` exit-slot in each handler in
/// `FUN_801D362C`.
pub fn canonical_size(sub_op: u16) -> Option<u16> {
    Some(match sub_op {
        0x00 => 16, // explicit `li s2, 0x10` halt-skip
        0x01 => 2,
        0x02 => 2,
        0x03 => 2,
        0x04 => 3,
        0x05 => 5,
        0x06 => 7,
        0x07 => 7,
        0x08 => 2,
        0x09 => 2,
        0x0A => 3,
        0x0B => 3,
        0x0C => 3,
        0x0D => 3,
        0x0E => 11,
        0x0F => 2,
        0x10 => 2,
        0x11 => 2,
        0x12 => 8,
        0x13 => 4,
        0x14 => 4,
        0x15 => 2,
        0x16 => 2,
        0x17 => 8,
        0x18 => 5,
        0x19 => 8,
        0x1A => 8,
        0x1B => 5,
        0x1C => 3,
        0x1D => 3,
        0x1E => 4,
        0x1F => 5,
        0x20 => 5,
        0x21 => 5,
        0x22 => 5,
        0x23 => 6,
        0x24 => 8,
        0x25 => 3,
        0x26 => 3,
        0x27 => 3,
        0x28 => 5,
        0x29 => 5,
        0x2A => 8,
        0x2B => 6,
        0x2C => 7,
        0x2D => 6,
        0x2E => 13,
        0x2F => 3,
        0x30 => 5,
        0x31 => 3,
        0x32 => 3,
        0x33 => 6,
        0x34 => 3,
        0x35 => 3,
        0x36 => 4,
        0x37 => 4,
        0x38 => 4,
        0x39 => 4,
        0x3A => 3,
        0x3B => 4,
        0x3C => 6,
        _ => return None,
    })
}

/// Engine-side callbacks for the overlay-resident move-VM extension VM.
///
/// The four `slab_*` / `gpu_*` hooks correspond to the scrolling-strip
/// opcodes (0x2B..0x2E); default impls are no-ops so an engine can
/// run the VM in "advance-only" mode (validating that every byte
/// parses) before wiring up the renderer.
pub trait WorldMapDrawHost {
    /// Sub-op 0x2B - set slab UV bounds.
    ///
    /// Writes `args` directly to the slab descriptor at offsets
    /// `+0x18..+0x1E` (origin u + origin v + end u + end v). The slab
    /// descriptor lives at `actor[+0x9C]` in the original.
    fn slab_uv_set(&mut self, _args: [u16; 4]) {}

    /// Sub-op 0x2D - increment slab UV bounds by `args`.
    ///
    /// Used by scroll/anim ticks to advance the texture window per
    /// frame. The wrap is unsigned u16 wrap (handler is `lhu / addu /
    /// sh`).
    fn slab_uv_inc(&mut self, _args: [u16; 4]) {}

    /// Sub-op 0x2C - invoke the per-scanline POLY_FT4 strip emitter
    /// (`FUN_801D31B0`).
    ///
    /// The full instruction is 7 halfwords (14 bytes) - including the
    /// outer `0x2F` op + inner `0x2C` sub-op header at +0..+4. Args
    /// `op[2..7]` (5 halfwords) are the emitter parameters; their
    /// precise role lives inside `FUN_801D31B0` and is not relevant
    /// to the VM walk. `FUN_801D31B0` is a shared scrolling-strip
    /// helper used by dialog / cutscene / 0897 field / world-map
    /// overlays, not a continent-specific function.
    fn emit_strip(&mut self, _args: [u16; 5]) {}

    /// Sub-op 0x2E - build GP0 TPage/CLUT packet.
    ///
    /// Reads 11 args, packs a 5-word GP0 `0x4000_0000` draw-mode packet
    /// (TPage + CLUT + tex window + draw area) and links it into the
    /// OT chain. Engines that own the GPU state stream consume this;
    /// the default no-op is fine for VM-only walking.
    fn gpu_draw_mode(&mut self, _args: [u16; 11]) {}
}

/// Decode the opcode at the current PC.
///
/// `bytecode` must start at the world-map VM PC: `s3 + 0` is `word_0`
/// (branch hint), `s3 + 2` is the opcode that drives dispatch.
///
/// Returns `None` if `bytecode` has fewer than 4 bytes (no room for
/// the `word_0` + opcode header).
pub fn peek_sub_op(bytecode: &[u8]) -> Option<u16> {
    if bytecode.len() < 4 {
        return None;
    }
    Some(u16::from_le_bytes([bytecode[2], bytecode[3]]))
}

/// Read `word_0` (the branch-target / next-pc hint) at PC.
pub fn peek_word0(bytecode: &[u8]) -> Option<u16> {
    if bytecode.len() < 2 {
        return None;
    }
    Some(u16::from_le_bytes([bytecode[0], bytecode[1]]))
}

/// Dispatch a single sub-opcode against the host. Returns the advance
/// count to feed back to the move-VM.
///
/// `bytecode` is the full move-VM bytecode buffer starting at the
/// current move-VM PC (i.e. the outer `0x2F` byte is at `bytecode[0]`).
/// Sub-op handlers read args from `+4(s3), +6(s3), ...` in the
/// original, which corresponds to `bytecode[4..]` here.
pub fn step<H: WorldMapDrawHost + ?Sized>(host: &mut H, bytecode: &[u8]) -> StepResult {
    let Some(sub_op) = peek_sub_op(bytecode) else {
        return StepResult {
            size_u16: 1,
            out_of_range: true,
        };
    };
    if sub_op >= MAX_SUB_OPCODE {
        return StepResult {
            size_u16: 1,
            out_of_range: true,
        };
    }
    // Helper: read u16 LE at byte offset `b` (i16 reads are sign-extended
    // by the caller).
    let read_u16 = |b: usize| -> u16 {
        if b + 2 > bytecode.len() {
            return 0;
        }
        u16::from_le_bytes([bytecode[b], bytecode[b + 1]])
    };

    match sub_op {
        // 0x2B - slab UV bounds set. args at +4..+0xC.
        0x2B => {
            let a = [read_u16(4), read_u16(6), read_u16(8), read_u16(0xA)];
            host.slab_uv_set(a);
        }
        // 0x2C - invoke scanline strip emitter. 5 arg halfwords at +4..+0xE.
        0x2C => {
            let a = [
                read_u16(4),
                read_u16(6),
                read_u16(8),
                read_u16(0xA),
                read_u16(0xC),
            ];
            host.emit_strip(a);
        }
        // 0x2D - slab UV bounds increment.
        0x2D => {
            let a = [read_u16(4), read_u16(6), read_u16(8), read_u16(0xA)];
            host.slab_uv_inc(a);
        }
        // 0x2E - GP0 draw-mode packet. 11 args at +4..+0x1A.
        0x2E => {
            let mut a = [0u16; 11];
            for i in 0..11 {
                a[i] = read_u16(4 + i * 2);
            }
            host.gpu_draw_mode(a);
        }
        // Every other sub-op is an advance-only stub - the move-VM
        // still progresses, but the side effects (flag writes, GPU
        // state mutations, etc.) are not modelled by this minimal port.
        // The advance count comes from the JT-derived
        // `canonical_size` table.
        _ => {}
    }
    let size = canonical_size(sub_op).unwrap_or(1);
    StepResult {
        size_u16: size,
        out_of_range: false,
    }
}

/// Walk `bytecode` linearly through the world-map drawing VM,
/// dispatching every opcode into `host`. Returns walk statistics.
///
/// This is a straight-line walker: it does NOT follow `word_0`
/// branch hints. Stops when:
///  - `max_steps` is exhausted (default cap = `bytecode.len()` steps),
///  - PC would run off the end of the buffer,
///  - an opcode `>= 0x3D` is hit (out-of-range terminator).
///
/// Useful for "does this buffer parse end-to-end" validation and for
/// counting render-class ops. For real execution the engine should
/// model branch flow + the move-VM bridge.
pub fn walk<H: WorldMapDrawHost + ?Sized>(host: &mut H, bytecode: &[u8]) -> WalkSummary {
    walk_with_limit(host, bytecode, bytecode.len())
}

/// As [`walk`], but with an explicit step cap (some bytecodes have
/// terminator opcodes we haven't decoded; the cap stops runaway).
pub fn walk_with_limit<H: WorldMapDrawHost + ?Sized>(
    host: &mut H,
    bytecode: &[u8],
    max_steps: usize,
) -> WalkSummary {
    let mut pc = 0usize;
    let mut steps = 0usize;
    let mut out_of_range = 0usize;
    while pc + 4 <= bytecode.len() && steps < max_steps {
        let r = step(host, &bytecode[pc..]);
        if r.out_of_range {
            out_of_range += 1;
            break; // halt at unrecognised opcode
        }
        pc += (r.size_u16 as usize) * HALFWORD;
        steps += 1;
    }
    WalkSummary {
        steps_walked: steps,
        final_pc: pc,
        terminated_out_of_range: out_of_range > 0,
    }
}

/// Statistics from [`walk`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkSummary {
    pub steps_walked: usize,
    pub final_pc: usize,
    /// True if the walk stopped because an opcode `>= 0x3D` was
    /// encountered. Treated as a soft terminator.
    pub terminated_out_of_range: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct RecHost {
        events: RefCell<Vec<Event>>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Event {
        SlabSet([u16; 4]),
        SlabInc([u16; 4]),
        Strip([u16; 5]),
        DrawMode([u16; 11]),
    }

    impl WorldMapDrawHost for RecHost {
        fn slab_uv_set(&mut self, a: [u16; 4]) {
            self.events.borrow_mut().push(Event::SlabSet(a));
        }
        fn slab_uv_inc(&mut self, a: [u16; 4]) {
            self.events.borrow_mut().push(Event::SlabInc(a));
        }
        fn emit_strip(&mut self, a: [u16; 5]) {
            self.events.borrow_mut().push(Event::Strip(a));
        }
        fn gpu_draw_mode(&mut self, a: [u16; 11]) {
            self.events.borrow_mut().push(Event::DrawMode(a));
        }
    }

    fn one_op(sub: u16, args: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x002Fu16.to_le_bytes());
        out.extend_from_slice(&sub.to_le_bytes());
        for a in args {
            out.extend_from_slice(&a.to_le_bytes());
        }
        out
    }

    #[test]
    fn slab_set_decodes_four_args_and_advances_six() {
        let bc = one_op(0x2B, &[0x1234, 0x5678, 0x9ABC, 0xDEF0]);
        let mut host = RecHost::default();
        let r = step(&mut host, &bc);
        assert_eq!(r.size_u16, 6);
        assert!(!r.out_of_range);
        assert_eq!(
            host.events.into_inner(),
            vec![Event::SlabSet([0x1234, 0x5678, 0x9ABC, 0xDEF0])]
        );
    }

    #[test]
    fn emit_strip_decodes_five_args_and_advances_seven() {
        let bc = one_op(0x2C, &[1, 2, 3, 4, 5]);
        let mut host = RecHost::default();
        let r = step(&mut host, &bc);
        assert_eq!(r.size_u16, 7);
        assert_eq!(
            host.events.into_inner(),
            vec![Event::Strip([1, 2, 3, 4, 5])]
        );
    }

    #[test]
    fn slab_inc_decodes_four_args_and_advances_six() {
        let bc = one_op(0x2D, &[10, 20, 30, 40]);
        let mut host = RecHost::default();
        let r = step(&mut host, &bc);
        assert_eq!(r.size_u16, 6);
        assert_eq!(
            host.events.into_inner(),
            vec![Event::SlabInc([10, 20, 30, 40])]
        );
    }

    #[test]
    fn gpu_draw_mode_decodes_eleven_args_and_advances_thirteen() {
        let args: Vec<u16> = (0..11).collect();
        let bc = one_op(0x2E, &args);
        let mut host = RecHost::default();
        let r = step(&mut host, &bc);
        assert_eq!(r.size_u16, 13);
        let expected: [u16; 11] = std::array::from_fn(|i| i as u16);
        assert_eq!(host.events.into_inner(), vec![Event::DrawMode(expected)]);
    }

    #[test]
    fn out_of_range_sub_op_advances_one_halfword() {
        let bc = one_op(0x3D, &[]);
        let mut host = RecHost::default();
        let r = step(&mut host, &bc);
        assert_eq!(r.size_u16, 1);
        assert!(r.out_of_range);
        assert!(host.events.into_inner().is_empty());
    }

    #[test]
    fn canonical_size_covers_every_known_opcode() {
        for op in 0..MAX_SUB_OPCODE {
            assert!(canonical_size(op).is_some(), "op 0x{op:02X} unmapped");
        }
        assert!(canonical_size(MAX_SUB_OPCODE).is_none());
    }

    #[test]
    fn walk_runs_two_render_ops_in_sequence() {
        // [word_0=0x0000 op=0x2B args x4] [word_0=0x0000 op=0x2C args x5]
        let mut bc = one_op(0x2B, &[1, 2, 3, 4]);
        bc.extend_from_slice(&one_op(0x2C, &[10, 20, 30, 40, 50]));

        let mut host = RecHost::default();
        let summary = walk(&mut host, &bc);
        assert_eq!(summary.steps_walked, 2);
        assert!(!summary.terminated_out_of_range);
        // 6 halfwords + 7 halfwords = 13 halfwords = 26 bytes
        assert_eq!(summary.final_pc, 26);
        assert_eq!(
            host.events.into_inner(),
            vec![
                Event::SlabSet([1, 2, 3, 4]),
                Event::Strip([10, 20, 30, 40, 50]),
            ]
        );
    }

    #[test]
    fn walk_stops_at_out_of_range_opcode() {
        // [word_0=0x0000 op=0x3D] - out of range, walk halts.
        let bc = one_op(0x3D, &[]);
        let mut host = RecHost::default();
        let summary = walk(&mut host, &bc);
        assert!(summary.terminated_out_of_range);
        assert_eq!(summary.steps_walked, 0);
    }
}
