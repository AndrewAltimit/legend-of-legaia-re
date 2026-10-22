//! The shared box emit and its off-display guard.

// ---------------------------------------------------------------------
// Shared box emit
// ---------------------------------------------------------------------

/// Bottom clip the guarded box emit applies (`slti v0,s0,0xf1`).
///
/// The 320x240 display's last line is 239, so a `y` of 240 already
/// reserves nothing; retail's bound admits 240 and rejects 241.
pub const BOX_EMIT_MAX_Y: i32 = 0xF0;

/// The box a guarded fill would emit, or `None` when it is clipped away.
///
/// `FUN_801E4140` takes six arguments, runs nothing when the `y` argument
/// exceeds `0xF0`, and otherwise calls the fill-state setter `FUN_80034B6C`
/// and then the box writer `FUN_8002C69C(x, y, w, h)`. That single
/// comparison is the whole routine - it is a bottom-of-screen guard on an
/// otherwise unconditional pair of calls.
///
/// The decompiled C renders the first call as `func_0x80034b6c()` with no
/// arguments; the disassembly shows `a0` / `a1` untouched between the
/// prologue and the `jal`, so it in fact receives the caller's first two
/// arguments - the dropped-register-argument artifact, not a niladic call.
/// Its one caller passes `(0x44, 0x02202020, …)`, i.e. a mode selector
/// and a packed RGB word, so this pair is a **shaded colour fill**, not the
/// gold 9-slice window border.
///
/// PORT: FUN_801E4140
/// REF: FUN_80034b6c - the fill-state setter this guard gates, which takes
/// the caller's mode selector and packed RGB word. Not ported.
/// REF: FUN_8002c69c - the box writer, which inflates its own rect by 8px on
/// every side. Not ported; the hosts draw their window chrome from the UI
/// atlas instead.
// REPLACED-BY: nothing is owed a port - retail never runs the call either.
// The routine has exactly one reference on the disc: the `jal` at `0x801DDEE8`
// in PROT 0899 (`find-address-word-refs.py 801e4140 --prot --home 0899`, 1234
// images: word 0, jal 1, j 0, branch 0, lui 0; `find-gp-relative-refs.py --va
// 0x801E4140`: no hits). That call sits in the title tick `FUN_801DD35C`'s
// sub-mode `0x02` arm (jump table `0x801CF244` slot 2 = `0x801DDDFC`, body
// through `0x801DDF2C`), which retail never enters: `init.pak` raises the entry
// word `_DAT_8007BB00` before the hand-off, so `Init` takes its `0x11` arm, and
// the shared epilogue rewrites a left-over `0x02` to `0x10` (capture-confirmed;
// see `legaia_engine_title::title_overlay`). The arm's box - mode `0x44`, RGB word
// `0x02202020`, rect `(0x7C, 0x6B, 0x3C, 0x1A)` behind its two rows - is behind
// a constant `y` of `0x6B` besides, so even a host that entered the arm would
// never see the guard clip.
pub fn guarded_box_rect(x: i32, y: i32, w: i32, h: i32) -> Option<(i32, i32, i32, i32)> {
    (y <= BOX_EMIT_MAX_Y).then_some((x, y, w, h))
}
