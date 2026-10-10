//! PROT-0900 **screen-effect widget family** - the 2D presentation layer the
//! field/event VM drives during cutscene-style sequences (iris mask, scripted
//! sprites, image panel, letterbox bands). Ported from the resident slot-B
//! overlay PROT 0900, statically decoded at its link base `0x801F69D8`.
//!
//! ## What this is (and what it is not)
//!
//! This family was the long-open "quad-emit / matrix half of `FUN_801F811C`"
//! thread. Static decode of PROT 0900 at the correct link base resolves it:
//! **there is no per-part 3D matrix in this path.** `FUN_801F811C` is the
//! per-frame handler of a 2D *screen-mask widget* - its four tweened channels
//! (`+0x3c/3e/40/42` targets against `+0x14/16/18/1a` current) are the
//! **left / top / right / bottom edges of a screen rectangle**, and the four
//! emitted quads are the black border bands framing that rectangle (an iris /
//! letterbox wipe). The genuine `RotMatrixX/Y/Z` matrix code elsewhere in
//! PROT 0900 belongs to the top-view grid-instance renderer (`FUN_801F7088` +
//! its second-cluster sibling), a separate subsystem that a live trace already
//! showed does **not** run during a player summon.
//!
//! ## Structure (all pinned from PROT 0900 file bytes at base `0x801F69D8`)
//!
//! Widgets are actors on the generic effect-actor list (`_DAT_8007C34C`),
//! allocated by SCUS `FUN_80020DE0(descriptor, list)` which binds the per-frame
//! handler from `descriptor+8` at `actor+0xc`; `FUN_8003CF04(list, handler)`
//! finds the live widget by that handler. Four 0x18-byte handler-binding
//! descriptors live at `0x801F8FE4/8FFC/9014/902C` (each `[u32 0][u16 0]
//! [u16 0xFFFF][u32 handler][u32 0][...]`):
//!
//! | kind | handler | per-frame behaviour | spawn / control API |
//! |---|---|---|---|
//! | sprite | `FUN_801F7A9C` | script-driven tweened 2D sprite (GP0 `0x64` SPRT) | `FUN_801F8004(record)` |
//! | mask | `FUN_801F811C` | 4-edge rect tween + 4 black border quads | `FUN_801F8D4C(l,t,r,b,dur)` |
//! | panel | `FUN_801F849C` | 5-channel tween + 1–2 textured 15bpp quads | `FUN_801F88FC(rec)` spawn, `FUN_801F8E6C(x,y,scale,dur)` move/scale |
//! | letterbox | `FUN_801F8A34` | 2 solid bands + 2 gradient feather quads | `FUN_801F8F28(block)` |
//!
//! The sprite handler additionally interprets a byte-coded widget script
//! (opcode byte `0x40`, sub-op at `+2` dispatched through the 5-entry table at
//! the overlay head `0x801F69D8`: kill / wait-flag-set / wait-flag-clear /
//! tween-pos+colour / tween-colour). Tweens use the shared multi-mode
//! interpolator `FUN_801DE4C8` re-evaluated **from a captured start value**
//! each frame (not iteratively from the current value).
//!
//! ## Consumers
//!
//! The spawn/control APIs are called by **field-VM op `0x43` sub-ops** -
//! sub-`0x10` sprite / `0x11` mask / `0x13` panel / `0x14` panel-move /
//! `0x15` letterbox, via the 0x43 sub-op JT at `0x801CEDA8` (`jal` sites
//! inside `FUN_801DE840` at `0x801DF918`, `0x801DF974`, `0x801DFA70`,
//! `0x801DFABC`, `0x801DFACC`). On disc only the **ten** ending-sequence
//! (`ed*`) scenes' cutscene scripts invoke them - the figure of eight this doc
//! used to give omitted `edbubu` and `eddoman` - and it is partition 2 for 310
//! of the 311 sites, the odd one being a sub-`0x11` in `edlast` partition 1.
//! The census is a disc-gated test
//! (`crates/engine-core/tests/screen_fx_widgets.rs`), which also drives one
//! real script per widget kind through the field VM; the test prints the
//! census rather than asserting its count.
//! The earlier reading that the summon stagers
//! (0910..0915) reference these functions was **VA aliasing**: those hits are
//! in-file `FUN_80021B04` part records whose addresses coincide with the 0900
//! handler VAs under the shared slot-B base.
//!
//! Capture evidence: PROT 0900 file `0x0640..0x2660` (covering this whole
//! family) is byte-resident at `0x801F7018..0x801F9038` in the fingerprinted
//! `battle_gimard_tail_fire_a` save; the function bodies are byte-identical to
//! the dance / baka-fighter overlay images (`overlay_dance_801f811c.txt` etc.).
//!
//! Retail screen parameters live in the render scratchpad block at
//! `0x1F800314`: `+0x74` (`0x1F800388`) = draw-area X origin, `+0x76`
//! (`0x1F80038A`) = Y offset, `+0x7A` (`0x1F80038E`) = screen height. The
//! engine models them as [`Screen`].
//!
//! Cross-referenced infrastructure (not ported here): the actor allocator /
//! finder pair, the field-VM caller, the part stager whose records VA-alias
//! the handlers, the grid renderer, the unaligned readers + story-flag test
//! the scripts use, and the texpage-packet builder.

mod frame;
mod host;
mod interp;
mod letterbox;
mod mask_widget;
mod panel_widget;
mod screen;
mod sprite_widget;

pub use frame::*;
pub use host::*;
pub use interp::*;
pub use letterbox::*;
pub use mask_widget::*;
pub use panel_widget::*;
pub use screen::*;
pub use sprite_widget::*;

/// Read a little-endian `i16` at `off` (the unaligned reader `FUN_8003CE9C`).
fn rs16(b: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([b[off], b[off + 1]])
}

/// Read a little-endian `u24` at `off` (the unaligned reader `FUN_8003CEB8`).
/// The widget scripts use it for packed RGB.
fn ru24(b: &[u8], off: usize) -> [u8; 3] {
    [b[off], b[off + 1], b[off + 2]]
}

#[cfg(test)]
mod tests;
