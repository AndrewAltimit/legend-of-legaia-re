//! The slot-B kernels' views of a battle actor and the battle context, read
//! from and written back to retail-shaped bytes, so a differential compares
//! the whole record a module wrote rather than the fields it chose.

use legaia_engine_cast::cast_module_ticks::{CastActorState, CastModuleCtx};
use psx_retail_exec::Memory;

/// Every [`CastActorState`] field with a home in the record, as
/// `(offset, width)`, in the struct's order.
macro_rules! actor_fields {
    ($m:ident) => {
        $m!(
            (combo_total, 0x00, u32),
            (present_04, 0x04, u32),
            (root_speed, 0x0C, i32),
            (hp_bar_delta, 0x10, i32),
            (hp, 0x14C, u16),
            (agl, 0x154, u16),
            (agl_base, 0x156, u16),
            (atk, 0x158, u16),
            (atk_base, 0x15A, u16),
            (udf, 0x15C, u16),
            (udf_base, 0x15E, u16),
            (ldf, 0x160, u16),
            (ldf_base, 0x162, u16),
            (spd, 0x164, u16),
            (spd_base, 0x166, u16),
            (intel, 0x168, u16),
            (intel_base, 0x16A, u16),
            (init_key, 0x16C, u16),
            (flags, 0x16E, u16),
            (spirit_gauge, 0x170, u16),
            (loop_timer, 0x176, u16),
            (shatter, 0x1BA, u16),
            (playing_anim, 0x1D9, u8),
            (staged_anim, 0x1DA, u8),
            (restage, 0x1DC, u8),
            (target_code, 0x1DD, u8),
            (action_category, 0x1DE, u8),
            (queued_action, 0x1DF, u8),
            (reaction_alt, 0x1EF, u8),
            (reaction_alt2, 0x1F0, u8),
            (knockdown_anim, 0x1F1, u8),
            (reaction_gate, 0x1F2, u8),
            (loop_count, 0x21B, u8),
            (render_flag, 0x21C, u8),
            (anim_rate, 0x21D, u8),
            (render_21f, 0x21F, u8),
            (render_225, 0x225, u8)
        )
    };
}

/// Read the view of the actor record at `a`.
pub fn read_actor(m: &mut Memory, a: u32) -> CastActorState {
    let mut v = CastActorState::default();
    macro_rules! rd {
        ($(($f:ident, $o:expr, $t:ident)),*) => {
            $(v.$f = rd!(@ $t, $o);)*
        };
        (@ u32, $o:expr) => { m.rd32(a + $o) };
        (@ i32, $o:expr) => { m.rd32(a + $o) as i32 };
        (@ u16, $o:expr) => { m.rd16(a + $o) };
        (@ u8, $o:expr) => { m.rd8(a + $o) };
    }
    actor_fields!(rd);
    v
}

/// Write the view back onto `rec`, the bytes of one record.
pub fn write_actor(rec: &mut [u8], v: &CastActorState) {
    macro_rules! wr {
        ($(($f:ident, $o:expr, $t:ident)),*) => {
            $(wr!(@ $t, $o, v.$f);)*
        };
        (@ u32, $o:expr, $x:expr) => { rec[$o..$o + 4].copy_from_slice(&$x.to_le_bytes()) };
        (@ i32, $o:expr, $x:expr) => { rec[$o..$o + 4].copy_from_slice(&$x.to_le_bytes()) };
        (@ u16, $o:expr, $x:expr) => { rec[$o..$o + 2].copy_from_slice(&$x.to_le_bytes()) };
        (@ u8, $o:expr, $x:expr) => { rec[$o] = $x };
    }
    actor_fields!(wr);
}

/// Read the context view at `c`.
pub fn read_ctx(m: &mut Memory, c: u32) -> CastModuleCtx {
    CastModuleCtx {
        party_count: m.rd8(c),
        monster_count: m.rd8(c + 1),
        caster_seat: m.rd8(c + 0x13),
        ctx_278: m.rd8(c + 0x278),
        phase: m.rd8(c + 0x279),
        pass_rest: 0,
        ctx_0d: m.rd8(c + 0x0D),
        turn_cursor: m.rd8(c + 0x1A),
        ctx_27a: m.rd8(c + 0x27A),
        ctx_6d8: m.rd16(c + 0x6D8),
    }
}

/// Write the context view back onto `ctx`, the context's bytes.
pub fn write_ctx(ctx: &mut [u8], v: &CastModuleCtx) {
    ctx[0] = v.party_count;
    ctx[1] = v.monster_count;
    ctx[0x13] = v.caster_seat;
    ctx[0x278] = v.ctx_278;
    ctx[0x279] = v.phase;
    ctx[0x0D] = v.ctx_0d;
    ctx[0x1A] = v.turn_cursor;
    ctx[0x27A] = v.ctx_27a;
    ctx[0x6D8..0x6DA].copy_from_slice(&v.ctx_6d8.to_le_bytes());
}

/// The differing byte offsets of two buffers, all of them, for a message.
pub fn diffs(a: &[u8], b: &[u8]) -> Vec<String> {
    a.iter()
        .zip(b)
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, (x, y))| format!("+{i:#x}: retail {x:#04x} port {y:#04x}"))
        .collect()
}
