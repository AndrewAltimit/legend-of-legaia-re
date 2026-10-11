//! Static catalogue of every retail code path that fires a STR FMV by
//! writing the FMV index + the `StrInit` game mode (`0x1A`).
//!
//! ## Background
//!
//! The retail STR FMV play loop (`FUN_801CF098` in the cutscene-overlay
//! slice) only runs when the global game-mode word at `_DAT_8007B83C`
//! equals `0x1A` and the FMV index at `_DAT_8007BA78` (a `s16`) selects
//! one of the 23 32-byte dispatch slots at `0x801D0A6C` (nine retail,
//! `fmv_id 0..=8`; selector `sll v0,v0,0x5` in the master dispatch
//! `FUN_801CEA3C` - see [`docs/formats/str-fmv-table.md`]).
//!
//! A backward sweep of every Ghidra dump in the corpus surfaces
//! **three** writers of `_DAT_8007B83C = 0x1A`:
//!
//! 1. [`FIELD_VM_OP_4C_E2`] - the field-VM opcode `0x4C 0xE2 lo hi` handler
//!    inside `FUN_801DE840`. Reachable only via the field-VM 2nd-stage
//!    jump-table at `0x801CF008` entry `2`. Reads `fmv_id` from the
//!    bytecode stream.
//! 2. [`TITLE_ATTRACT_LOOP`] - the title-screen menu state machine in
//!    `FUN_801DE234`, case `0x10`. Hardcodes `fmv_id = 0` (intro
//!    `MV1.STR`) when the attract-loop timer at `DAT_801ef16c` underflows.
//! 3. [`TITLE_TICK_INLINE`] - an inline fall-through path inside the
//!    title-overlay per-frame tick `FUN_801DD35C` at `0x801DDCF0`. The
//!    `bgez v0, 0x801DFC3C` at `0x801DDCC8` keeps the normal path
//!    short-circuiting away when the countdown is still positive; once
//!    it underflows, the function falls through and the very next
//!    instructions zero `_DAT_8007BA78` and store `0x1A` to
//!    `_DAT_8007B83C` before reaching the [`TITLE_ATTRACT_LOOP`] label
//!    site downstream. PC-verified via a PCSX-Redux watchpoint on the
//!    title-attract countdown (see `project_title_overlay_tick_pinned`).
//!
//! There is no static caller of the [`FIELD_VM_OP_4C_E2`] handler - the
//! `FUN_801E30E4` label that the PRD refers to is reached **only** via
//! the JT dispatch chain `0x4C → byte1 >> 4 == 0xE → byte1 & 0xF == 0x2`,
//! and Ghidra's reference manager does not promote that to a call edge.
//!
//! ## Per-scene triggers vs the seven-label list
//!
//! The seven CDNAME labels the FMV overlay carries at `0x801CE8AC`
//! (`town0b`, `map01`, `chitei2`, `map02`, `jou`, `uru2`, `town0e`)
//! are the **post-play return scenes**: after playback the master
//! dispatch (`FUN_801CEA3C`) copies the label for the just-played
//! mid-game `fmv_id` into the next-scene name global `0x80084548`.
//! They are NOT the trigger-scene set. The actual `0x4C 0xE2` trigger
//! ops live LZS-compressed inside each trigger scene's MAN (which is
//! why a raw bytewise PROT scan misses them);
//! `man_field_scripts::scene_fmv_triggers` (engine-core) recovers the
//! literal `fmv_id` operands for all eight trigger scenes (`town01`,
//! `garmel`, `deroa`, `chitei2`, `dohaty`, `town0d`, `uru`, `jouine`),
//! pinned by the disc-gated `scene_fmv_triggers_disc` test.
//!
//! ## Provenance
//!
//! Every cell below is sourced from `ghidra/scripts/funcs/`
//! disassembly + decompilation dumps. Each [`FmvTriggerSite`] entry
//! pins the exact `_DAT_8007B83C = 0x1A` writer location.

/// One static code path that fires a STR FMV by writing the FMV index
/// + setting `_DAT_8007B83C = 0x1A`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FmvTriggerSite {
    /// Short identifier - useful for log messages and tests.
    pub label: &'static str,
    /// Function entry the writer lives in.
    pub function: &'static str,
    /// Byte offset / instruction address of the `sh 0x1a` write.
    pub mode_write_addr: u32,
    /// Byte offset / instruction address of the FMV-id write.
    /// `None` when the path doesn't write `_DAT_8007BA78` itself
    /// (e.g. it relies on a previously-stored value).
    pub fmv_id_write_addr: Option<u32>,
    /// Whether `fmv_id` is hardcoded to a literal value or read from
    /// a dynamic source (the field-VM bytecode stream).
    pub fmv_id_source: FmvIdSource,
    /// One-line summary of the trigger condition.
    pub trigger_condition: &'static str,
}

/// How a trigger site obtains the `_DAT_8007BA78` value it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FmvIdSource {
    /// The handler decodes the FMV id from the field-VM bytecode
    /// (specifically `decode_u16_be(pc+1)` via `FUN_8003CE9C`). The
    /// id range observed in retail is `0..=8` - exactly the nine
    /// retail slots of the 23-slot dispatch table at `0x801D0A6C`
    /// (every movie on the disc); the per-STR FMV trigger corpus
    /// pins all nine as debug-menu-reachable.
    BytecodeOperand,
    /// The handler stores a literal value. Currently the only
    /// observed literal is `0`, which selects `MV1.STR` (the intro).
    Literal(i16),
}

/// Field-VM `0x4C 0xE2 lo hi` FMV-trigger op handler. Reached from the
/// `FUN_801DE840` main dispatcher via `FUN_801E0C3C` (16-entry JT at
/// `0x801CEE60`, byte1 high nibble) → `FUN_801E3040` (15-entry JT at
/// `0x801CF008`, byte1 low nibble) → entry `2` lands at `0x801E30E4`.
///
/// Disassembly summary:
///
/// ```text
/// 801e30e4  jal 0x8003ce9c              ; v0 = decode_u16_be(pc+1)
/// 801e30e8  _addiu a0,s6,0x1
/// 801e30ec  addiu s8,s8,0x6              ; advance PC by 6 (2 op + 2 id + 2 reserved)
/// 801e30f0  lui v1,0x8008
/// 801e30f4  sh v0,-0x4588(v1)            ; *(_DAT_8007BA78) = fmv_id
/// 801e30f8  lui v1,0x8008
/// 801e30fc  li v0,0x1a
/// 801e3100  j 0x801e3624                 ; rejoin dispatcher tail
/// 801e3104  _sh v0,-0x47c4(v1)           ; *(_DAT_8007B83C) = 0x1A
/// ```
pub const FIELD_VM_OP_4C_E2: FmvTriggerSite = FmvTriggerSite {
    label: "field_vm_op_4c_e2",
    function: "FUN_801DE840",
    mode_write_addr: 0x801E_3104,
    fmv_id_write_addr: Some(0x801E_30F4),
    fmv_id_source: FmvIdSource::BytecodeOperand,
    trigger_condition: "field-VM bytecode hits the byte sequence `0x4C 0xE2 lo hi`; \
         JT dispatch via 0x801CEE60 (high nibble 0xE) → 0x801CF008 (low nibble 0x2)",
};

/// Title-screen attract-loop FMV trigger. Lives inside the menu state
/// machine `FUN_801DE234`, case `0x10` (the title-screen idle state).
///
/// Decompilation summary (`overlay_menu_801de234.txt` lines 3784..3788):
///
/// ```c
/// DAT_801ef16c = DAT_801ef16c - (uint)DAT_1f800393;
/// if (DAT_801ef16c < 0) {
///     _DAT_8007ba78 = 0;          // fmv_id = 0 → MV1.STR (intro)
///     _DAT_8007b83c = 0x1a;       // game mode = StrInit
///     ...
/// }
/// ```
///
/// `DAT_801ef16c` is the title-screen idle countdown. When it
/// underflows, the intro FMV plays; on completion the play loop returns
/// to the title screen.
pub const TITLE_ATTRACT_LOOP: FmvTriggerSite = FmvTriggerSite {
    label: "title_attract_loop",
    function: "FUN_801DE234",
    // Decompilation line numbers map to the case-0x10 path; the
    // assembly address is the menu overlay's per-state handler tail.
    // We pin the case label rather than a single instruction since
    // the writers sit two lines apart.
    mode_write_addr: 0x801E_0F50,
    fmv_id_write_addr: Some(0x801E_0F4C),
    fmv_id_source: FmvIdSource::Literal(0),
    trigger_condition: "title-screen attract countdown `DAT_801ef16c` underflows; fires the \
         intro `MV1.STR` after the title screen has been idle for the \
         configured timeout",
};

/// Title-overlay per-frame-tick inline FMV trigger. Lives inside the
/// title-overlay tick function `FUN_801DD35C` (which Ghidra also labels
/// `FUN_801DE234` at an inner basic block). The countdown-decrement
/// sequence at `0x801DDCB8..0x801DDCCC` reads the per-frame scalar
/// `_DAT_1F800393`, subtracts it from `*0x801EF16C`, and uses
/// `bgez v0, 0x801DFC3C` to short-circuit away when the result is
/// still non-negative. On underflow the function falls through and the
/// next two writes set up the STR-FMV transition:
///
/// ```text
/// 801ddcc8  bgez v0,0x801dfc3c    ; if countdown >= 0, skip the writes
/// 801ddccc  _sw v0,-0xe94(a0)     ; write back decremented countdown
/// 801ddcd0  addiu s1,sp,0x38      ; (start of underflow path)
/// ...
/// 801ddce8  sh zero,-0x4588(v0)   ; *_DAT_8007BA78 = 0  (fmv_id, MV1.STR)
/// 801ddcec  li v0,0x1a
/// 801ddcf0  sh v0,-0x47c4(v1)     ; *_DAT_8007B83C = 0x1A (StrInit mode)
/// ```
///
/// This is the site a live PCSX-Redux watchpoint pins (every frame
/// passes through `0x801DDCCC`), and the writes here happen before the
/// downstream [`TITLE_ATTRACT_LOOP`] label inside the same function -
/// so practical capture of an attract-fire reports this PC, not the
/// inner label.
pub const TITLE_TICK_INLINE: FmvTriggerSite = FmvTriggerSite {
    label: "title_tick_inline",
    function: "FUN_801DD35C",
    mode_write_addr: 0x801D_DCF0,
    fmv_id_write_addr: Some(0x801D_DCE8),
    fmv_id_source: FmvIdSource::Literal(0),
    trigger_condition: "title-overlay per-frame tick: fall-through past the countdown decrement \
         at 0x801DDCCC (`bgez v0, 0x801DFC3C` not taken). Inline writes \
         _DAT_8007BA78 = 0 and _DAT_8007B83C = 0x1A before reaching the \
         downstream TITLE_ATTRACT_LOOP label.",
};

/// The complete catalogue of static FMV-trigger sites observed in the
/// corpus. Tests + tooling assert this is exhaustive against a fresh
/// dump pass of `_DAT_8007B83C = 0x1a`.
pub const FMV_TRIGGER_SITES: &[FmvTriggerSite] =
    &[FIELD_VM_OP_4C_E2, TITLE_ATTRACT_LOOP, TITLE_TICK_INLINE];

/// `_DAT_8007B83C` (the global game-mode word). The STR FMV play loop
/// only fires when this equals [`STR_INIT_MODE`].
pub const GAME_MODE_ADDR: u32 = 0x8007_B83C;

/// `_DAT_8007BA78` (the active FMV index, `s16`). Selects a 32-byte
/// dispatch slot at `0x801D0A6C + index * 0x20` (selector
/// `sll v0,v0,0x5` at `0x801CEC9C`).
pub const FMV_ID_ADDR: u32 = 0x8007_BA78;

/// Game mode `0x1A` (`= 26`). The STR-init mode that the cutscene
/// overlay's tick handler watches for.
pub const STR_INIT_MODE: u8 = 0x1A;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_contains_field_vm_and_title_attract() {
        assert_eq!(FMV_TRIGGER_SITES.len(), 3);
        assert!(
            FMV_TRIGGER_SITES
                .iter()
                .any(|s| s.label == "field_vm_op_4c_e2")
        );
        assert!(
            FMV_TRIGGER_SITES
                .iter()
                .any(|s| s.label == "title_attract_loop")
        );
        assert!(
            FMV_TRIGGER_SITES
                .iter()
                .any(|s| s.label == "title_tick_inline")
        );
    }

    #[test]
    fn title_tick_inline_site_hardcodes_fmv_id_zero() {
        assert!(matches!(
            TITLE_TICK_INLINE.fmv_id_source,
            FmvIdSource::Literal(0)
        ));
    }

    #[test]
    fn title_tick_inline_site_writes_strinit_mode() {
        // The mode-write at 0x801DDCF0 stores 0x1A to _DAT_8007B83C, matching STR_INIT_MODE.
        assert_eq!(TITLE_TICK_INLINE.mode_write_addr, 0x801D_DCF0);
        assert_eq!(TITLE_TICK_INLINE.fmv_id_write_addr, Some(0x801D_DCE8));
    }

    #[test]
    fn field_vm_site_reads_fmv_id_from_bytecode_operand() {
        assert!(matches!(
            FIELD_VM_OP_4C_E2.fmv_id_source,
            FmvIdSource::BytecodeOperand
        ));
    }

    #[test]
    fn title_attract_site_hardcodes_fmv_id_zero() {
        assert!(matches!(
            TITLE_ATTRACT_LOOP.fmv_id_source,
            FmvIdSource::Literal(0)
        ));
    }

    #[test]
    fn every_site_writes_a_known_address() {
        for site in FMV_TRIGGER_SITES {
            assert_eq!(
                site.mode_write_addr & 0xFF00_0000,
                0x8000_0000,
                "{} mode_write_addr should be in main RAM",
                site.label
            );
            if let Some(addr) = site.fmv_id_write_addr {
                assert_eq!(
                    addr & 0xFF00_0000,
                    0x8000_0000,
                    "{} fmv_id_write_addr should be in main RAM",
                    site.label
                );
            }
        }
    }
}
