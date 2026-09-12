//! **Enemy HP bars** - a red HP gauge over every living monster in battle,
//! drawn with the game's own AP-gauge primitives.
//!
//! Retail shows no HP readout for monsters at all (the one exception is the
//! Koru fight's `HP Left` percentage strip -
//! [`minigame-muscle-dome.md`](../../../docs/subsystems/minigame-muscle-dome.md)).
//! This feature adds one: for each live monster slot the injected routine
//! projects the actor's stage anchor to the screen through the billboard
//! projector every other 2-D battle sprite rides, seats a plate in a fixed
//! band along the top of the screen centred on that X, and draws the AP
//! plate's content without its blue chrome: the roster panel's `HP` label
//! chip, the meter as retail's two gouraud strips recoloured from dark-red /
//! gold to dark-red / bright-red, and the percentage numeral. The value is
//! the displayed-HP mirror `+0x172`, which steps down hit by hit inside a
//! combo. Nothing is new art: the label is a record of the system-UI icon
//! table `0x800732A4`, every primitive is emitted by a SCUS routine the
//! battle HUD already calls.
//!
//! ## Where it hooks
//!
//! The damage-number popup renderer `FUN_801DF6B8` (battle overlay 0898) runs
//! once per frame in the fighting phase from the actor-render callback
//! `FUN_800480D8` (`0x80048138`), *after* the frame's camera matrix is loaded
//! into the GTE - it is the routine that projects retail's own damage numbers.
//! Its first two words are detoured (`j` + `nop`), and the routine replays
//! them before returning to `0x801DF6C0`. Because the detour lives in PROT
//! 0898's bytes and nowhere else, a sibling slot-A image (dome, capture,
//! magic level-up) is untouched: it never carries the jump.
//!
//! The same call site is what makes the gating right for free: the popup is
//! only reached while the battle phase byte `DAT_8007BD71` is `0xFF` (not in
//! the intro ramp, not in the results sequence), and the routine additionally
//! honours the HUD-parked halfword `ctx[+0x6CE]` every retail HUD emitter
//! tests.
//!
//! ## Where the code lives - four unreferenced bodies
//!
//! The injected-code arenas in `SCUS_942.54` are full (34 bytes in fragments
//! remain - [`randomizer.md`](../../../docs/tooling/randomizer.md)), and the
//! battle overlay's image is packed. What is left is code retail never
//! reaches: routines with **no reference of any form in any image** (word,
//! `jal`, `j`, branch, `lui`+`addiu` pair - the five-form scan in
//! [`address-reference-scan.md`](../../../docs/tooling/address-reference-scan.md)).
//! The routine is laid out as four fragments over four such bodies, three in
//! PROT 0898 and one in SCUS, stitched with branches (the three overlay bodies
//! are within branch range of each other) and one `jal`:
//!
//! | Fragment | Body overwritten | Capacity |
//! |---|---|---|
//! | A - gates, slot loop, percentage | `FUN_801F2D54`, cast colour-wash pulse | 47 words |
//! | B - clamp, project the anchor | `FUN_801F463C`, learned-art predicate | 35 words |
//! | C - loop tail, epilogue | `FUN_801DBB2C`, card-slot highlight reset | 24 words |
//! | S - draw one plate (leaf, `jal` from C) | `FUN_8005126C`, battle sprite on-screen test | 52 words |
//!
//! Each body is fingerprinted at plan time (prologue words + its own `jr ra`),
//! and a fragment that would overrun its body refuses the patch. The bodies are
//! code, not zero padding, so "zero is not dead" does not apply: the evidence
//! is the reference scan, recorded per body in
//! `scripts/ci/port-catalog-ignore.toml` `[unreferenced]`.
//!
//! `FUN_8005126C` is SCUS-resident and therefore live in every mode; it is only
//! ever entered from fragment C, which exists only in PROT 0898. No other mod
//! claims any of the four bodies, so this composes with every arena feature.
//!
//! ## Traps honoured
//!
//! - **Load-delay slot**: no instruction reads a register in the slot after
//!   the load that writes it (asserted by a static scan in the tests).
//! - **`mflo` / `divu` spacing**: the R3000 leaves `lo` undefined when a
//!   multiply or divide issues within two instructions of an `mflo`; the
//!   percentage math keeps four instructions between them.
//! - **Branch reach**: every branch is resolved against absolute VAs and
//!   checked against the 16-bit word offset; the far transfers (SCUS <-> 0898,
//!   the loop back-edge) are `j` / `jal`.
//! - **Frame ownership**: the routine opens its own `0x50`-byte frame under
//!   the popup's caller, and the leaf fragment S parks its return address in
//!   a slot of that frame (`sp+0x48`) rather than opening a second one.

use anyhow::{Context, Result, bail};

use legaia_asset::item_names;

use crate::mips::*;
use crate::shiny_seru::{Edit, OVERLAY_BASE_VA, OVERLAY_TABLE_RANGES, SCUS_TABLE_RANGES};

/// PROT entry of the battle-action overlay (0898).
pub const OVERLAY_PROT_INDEX: usize = 898;

// --- Hook site --------------------------------------------------------------

/// `FUN_801DF6B8`, the damage-number popup renderer: its first two words are
/// the detour.
pub const HOOK_VA: u32 = 0x801D_F6B8;
/// Where the routine resumes the popup (past the two displaced words).
pub const HOOK_RET_VA: u32 = HOOK_VA + 8;
/// `addiu sp,sp,-0x70` - the popup's frame open.
const HOOK_W0: u32 = 0x27BD_FF90;
/// `lui t0,0x8008`.
const HOOK_W1: u32 = 0x3C08_8008;

// --- The four bodies ---------------------------------------------------------

/// Fragment A home: `FUN_801F2D54`, the cast colour-wash intensity pulse
/// (`0x801F2D54..0x801F2E10`, next routine's `addiu sp` at `0x801F2E10`).
pub const FRAG_A_VA: u32 = 0x801F_2D54;
const FRAG_A_END_VA: u32 = 0x801F_2E10;
const FRAG_A_W0: u32 = 0x3C05_8008; // lui a1,0x8008
const FRAG_A_W1: u32 = 0x8CA4_BD24; // lw a0,-0x42dc(a1)
const FRAG_A_JR_VA: u32 = 0x801F_2E08;
const FRAG_A_NEXT_PROLOGUE: u32 = 0x27BD_FFC0; // addiu sp,sp,-0x40 at FRAG_A_END_VA

/// Fragment B home: `FUN_801F463C`, the "has this seat learned art N"
/// predicate (`0x801F463C..0x801F46C8`).
pub const FRAG_B_VA: u32 = 0x801F_463C;
const FRAG_B_END_VA: u32 = 0x801F_46C8;
const FRAG_B_W0: u32 = 0x00A0_4821; // move t1,a1
const FRAG_B_W1: u32 = 0x0000_3021; // move a2,zero
const FRAG_B_JR_VA: u32 = 0x801F_46C0;

/// Fragment C home: `FUN_801DBB2C`, the card-slot highlight reset
/// (`0x801DBB2C..0x801DBB8C`; `FUN_801DBB8C` follows).
pub const FRAG_C_VA: u32 = 0x801D_BB2C;
const FRAG_C_END_VA: u32 = 0x801D_BB8C;
const FRAG_C_W0: u32 = 0x3C03_8008; // lui v1,0x8008
const FRAG_C_W1: u32 = 0x8C62_BD24; // lw v0,-0x42dc(v1)
const FRAG_C_JR_VA: u32 = 0x801D_BB84;

/// Fragment S home: `FUN_8005126C`, the battle sprite on-screen test
/// (`0x8005126C..0x8005133C`, SCUS).
pub const FRAG_S_VA: u32 = 0x8005_126C;
const FRAG_S_END_VA: u32 = 0x8005_133C;
const FRAG_S_W0: u32 = 0x27BD_FFC8; // addiu sp,sp,-0x38
const FRAG_S_W1: u32 = 0x3C03_801D; // lui v1,0x801d
const FRAG_S_JR_VA: u32 = 0x8005_1334;
const FRAG_S_NEXT_PROLOGUE: u32 = 0x27BD_FFE0; // addiu sp,sp,-0x20 at FRAG_S_END_VA

// --- Retail routines and globals the fragments use -------------------------

/// `*(u32*)0x8007BD24` - the battle context pointer (`0` outside battle).
pub const CTX_PTR_VA: u32 = 0x8007_BD24;
/// `ctx[+0x6CE]` - the HUD-parked / results-phase halfword every HUD emitter
/// early-outs on.
pub const CTX_HUD_PARKED_OFF: u16 = 0x6CE;
/// `DAT_801C9370` - the eight-slot battle actor pointer table.
pub const ACTOR_TABLE_VA: u32 = 0x801C_9370;
/// First and one-past-last monster slot (`3..=6`; slot 7 is the "none"
/// sentinel `FUN_801DB8B4` returns).
pub const MONSTER_SLOT_FIRST: u32 = 3;
pub const MONSTER_SLOT_END: u32 = 7;
/// Actor record fields.
pub const ACTOR_HP_OFF: u16 = 0x14C;
pub const ACTOR_HP_MAX_OFF: u16 = 0x14E;
/// `actor[+0x172]` - the **displayed** HP mirror. Live HP `+0x14C` is
/// committed once at the end of a player art out of the per-action total
/// (`actor[+0x00]`, `FUN_801EC3E4`), but each hit credits the pending delta
/// `+0x10` and the drain `FUN_80047430` applies it to this mirror the same
/// frame on a monster slot (no ramp - retail never drew it). Reading it is
/// what makes the bar step down hit by hit inside a combo; `+0x14C` stays the
/// liveness test.
pub const ACTOR_HP_SHOWN_OFF: u16 = 0x172;
pub const ACTOR_ANCHOR_X_OFF: u16 = 0x3C;
pub const ACTOR_ANCHOR_Y_OFF: u16 = 0x3E;
pub const ACTOR_ANCHOR_Z_OFF: u16 = 0x40;
/// `actor[+0x21C]` - `0xFF` while the actor is hidden by a summon fade.
pub const ACTOR_HIDDEN_OFF: u16 = 0x21C;
pub const ACTOR_HIDDEN_VALUE: u16 = 0xFF;

/// `FUN_800195A8(&svec, hw, hh, angle, &xy0, &xy1, &xy2, &xy3) -> depth` - the
/// billboard projector; corners come back as packed `(i16 x, i16 y)`.
pub const PROJECT_FN: u32 = 0x8001_95A8;
/// The projector's return is the OT bucket (`SZ3 >> 2 >> ot_shift`), zero
/// or negative for a point behind the camera. Retail's popup does not gate
/// on it at all (its `slti v0,0x80` at `0x801DF828` is a near-clip fixup that
/// widens the quad, not a visibility test); the routine skips the bar only
/// when the depth is not positive.
/// `FUN_8002C0B0(x, y, value)` - the AP-gauge content: two gouraud strips of
/// `value/2` px from `x+0x1B` plus the value numerals at `x+0x50`.
pub const GAUGE_FN: u32 = 0x8002_C0B0;
/// `FUN_8002C488(x, y, icon)` - one system-UI icon-table sprite at `(x, y)`.
pub const ICON_FN: u32 = 0x8002_C488;
/// Scratchpad primitive write cursor (`_DAT_1F8003A0`).
pub const PRIM_CURSOR_VA: u32 = 0x1F80_03A0;

/// The one icon-table record drawn: the roster panel's `HP` label chip. The
/// AP plate's blue chrome (trough `0x32`, value box `0x69`, end cap `0x6A`)
/// is deliberately left out - the plate is the label, the meter and the
/// numeral.
pub const ICON_HP_LABEL: u16 = 0x07; // (208,86) 16x10, sub-palette 1

/// Plate geometry, relative to the plate origin `(x, y)`, keeping the AP
/// plate's own offsets ([`field-menu.md`](../../../docs/subsystems/field-menu.md)):
/// the label at `+4`, the meter at `+0x1B..+0x4D` (the gauge primitive's own
/// span) and the numeral at `+0x50`.
pub const LABEL_DX: u16 = 4;
pub const LABEL_DY: u16 = 3;
/// Plate width (label to the last numeral cell), used to centre it on the
/// anchor.
pub const PLATE_W: u16 = 0x5C;
/// Screen Y of the first row's top edge. The plates sit in a band along the
/// top of the screen, one 16-px row per monster slot (slot 3 on the first
/// row), and track their monsters horizontally: only the projected X is
/// used. A head-anchored plate collides with a retail widget for some monster
/// size in every phase (the Begin / Reselect prompt at `y 85..100` for a
/// small monster, the party panels for a tall one), and a single shared row
/// piles up when an attack camera zooms in and the monsters' X converge; a
/// row per slot under the acting-actor plaque (`y 8..28`) is clear of both.
pub const PLATE_BAND_Y: u16 = 28;
/// Row pitch in the band.
pub const PLATE_ROW_PITCH: u16 = 16;

/// Meter colours: retail's dark end `(0x80,0x20,0x10)` stays; the gold end
/// `(0xC0,0xA0,0x40)` becomes this red. Packed as the GP0 colour word
/// `[r, g, b, code]`.
pub const METER_BRIGHT_RGB: (u8, u8, u8) = (0xE8, 0x38, 0x28);
/// GP0 command byte of the gauge quads (`0x39`, shaded quad).
pub const GAUGE_CODE: u8 = 0x39;

const fn colour_word(rgb: (u8, u8, u8), code: u8) -> u32 {
    u32::from_le_bytes([rgb.0, rgb.1, rgb.2, code])
}

/// The bright colour word with the GP0 code byte (quad 2's first colour).
pub const METER_BRIGHT_WITH_CODE: u32 = colour_word(METER_BRIGHT_RGB, GAUGE_CODE);
/// The bright colour word without a code byte (the other three slots).
pub const METER_BRIGHT_PLAIN: u32 = colour_word(METER_BRIGHT_RGB, 0);

/// Routine frame layout (bytes from `sp` after the frame opens).
const FRAME: u16 = 0x50;
const F_OUTPTR: u16 = 0x10; // 4 words: the projector's out-pointers
const F_SVEC: u16 = 0x20; // SVECTOR (x, y, z, pad)
const F_XY: u16 = 0x28; // projected (i16 x, i16 y)
const F_CURSOR: u16 = 0x2C; // primitive cursor before the gauge call
const F_RA: u16 = 0x30;
const F_S0: u16 = 0x34;
const F_S1: u16 = 0x38;
const F_S3: u16 = 0x3C;
const F_S4: u16 = 0x40;
const F_S5: u16 = 0x44;
const F_LEAF_RA: u16 = 0x48; // fragment S parks its return here

// --- A label-resolving fragment assembler -----------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Label {
    LoopHead,
    ANext,
    AExit,
    BStart,
    CStart,
    Next,
    Exit,
    PctCapped,
    PctFloored,
}

#[derive(Clone)]
enum Ins {
    W(u32),
    /// A PC-relative branch: encoder takes the word offset.
    Br(fn(i16) -> u32, Label),
    J(Label),
}

fn beq_to(rs: u32, rt: u32) -> fn(i16) -> u32 {
    // The encoder closures must be plain fns; pick the pair here.
    match (rs, rt) {
        (ZERO, ZERO) => |o| beq(ZERO, ZERO, o),
        (V0, ZERO) => |o| beq(V0, ZERO, o),
        (S1, ZERO) => |o| beq(S1, ZERO, o),
        (S5, ZERO) => |o| beq(S5, ZERO, o),
        (V0, V1) => |o| beq(V0, V1, o),
        _ => unreachable!("no beq encoder for that register pair"),
    }
}

fn bne_to(rs: u32, rt: u32) -> fn(i16) -> u32 {
    match (rs, rt) {
        (V0, ZERO) => |o| bne(V0, ZERO, o),
        (S5, ZERO) => |o| bne(S5, ZERO, o),
        _ => unreachable!("no bne encoder for that register pair"),
    }
}

struct Fragment {
    va: u32,
    end_va: u32,
    ins: Vec<Ins>,
    /// Labels defined in this fragment, by instruction index.
    labels: Vec<(Label, usize)>,
}

impl Fragment {
    fn new(va: u32, end_va: u32) -> Self {
        Self {
            va,
            end_va,
            ins: Vec::new(),
            labels: Vec::new(),
        }
    }
    fn label(&mut self, l: Label) {
        self.labels.push((l, self.ins.len()));
    }
    fn w(&mut self, word: u32) {
        self.ins.push(Ins::W(word));
    }
    fn br(&mut self, enc: fn(i16) -> u32, l: Label) {
        self.ins.push(Ins::Br(enc, l));
    }
    fn j_to(&mut self, l: Label) {
        self.ins.push(Ins::J(l));
    }
    fn capacity_words(&self) -> usize {
        ((self.end_va - self.va) / 4) as usize
    }
}

/// The program: every fragment with its label definitions.
fn program() -> Vec<Fragment> {
    let ctx_hi = hi(CTX_PTR_VA);
    let ctx_lo = lo(CTX_PTR_VA);
    let tab_hi = hi(ACTOR_TABLE_VA);
    let tab_lo = lo(ACTOR_TABLE_VA);

    // ---- Fragment A: gates, slot loop head, percentage --------------------
    let mut a = Fragment::new(FRAG_A_VA, FRAG_A_END_VA);
    a.w(addiu(SP, SP, (-(FRAME as i16)) as u16)); // open the frame
    a.w(sw(RA, SP, F_RA));
    a.w(sw(S0, SP, F_S0));
    a.w(sw(S1, SP, F_S1));
    a.w(sw(S3, SP, F_S3));
    a.w(sw(S4, SP, F_S4));
    a.w(sw(S5, SP, F_S5));
    a.w(lui(V0, ctx_hi));
    a.w(lw(V0, V0, ctx_lo)); // v0 = ctx
    a.w(nop()); // (load delay)
    a.br(beq_to(V0, ZERO), Label::AExit); // no battle context
    a.w(nop()); // (branch delay)
    a.w(lh(V0, V0, CTX_HUD_PARKED_OFF));
    a.w(nop()); // (load delay)
    a.br(bne_to(V0, ZERO), Label::AExit); // HUD parked / results phase
    a.w(nop()); // (branch delay)
    a.w(addiu(V0, SP, F_XY)); // the projector's four out-pointers
    a.w(sw(V0, SP, F_OUTPTR));
    a.w(sw(V0, SP, F_OUTPTR + 4));
    a.w(sw(V0, SP, F_OUTPTR + 8));
    a.w(sw(V0, SP, F_OUTPTR + 12));
    a.w(addiu(S0, ZERO, MONSTER_SLOT_FIRST as u16)); // s0 = slot
    a.label(Label::LoopHead);
    a.w(lui(V0, tab_hi));
    a.w(addiu(V0, V0, tab_lo));
    a.w(sll(V1, S0, 2));
    a.w(addu(V0, V0, V1));
    a.w(lw(S1, V0, 0)); // s1 = actor
    a.w(nop()); // (load delay)
    a.br(beq_to(S1, ZERO), Label::ANext); // empty seat
    a.w(nop()); // (branch delay)
    a.w(lhu(S5, S1, ACTOR_HP_OFF)); // s5 = live HP
    a.w(nop()); // (load delay)
    a.br(beq_to(S5, ZERO), Label::ANext); // dead
    a.w(nop()); // (branch delay)
    a.w(lbu(V0, S1, ACTOR_HIDDEN_OFF));
    a.w(addiu(V1, ZERO, ACTOR_HIDDEN_VALUE));
    a.br(beq_to(V0, V1), Label::ANext); // hidden by a summon fade
    a.w(lhu(S5, S1, ACTOR_HP_SHOWN_OFF)); // (branch delay) s5 = displayed HP - moves per hit
    a.w(addiu(V0, ZERO, 100));
    a.w(multu(S5, V0)); // lo = shown * 100 (one instruction past the load)
    a.w(mflo(V0));
    a.br(beq_to(ZERO, ZERO), Label::BStart); // splice -> B
    a.w(lhu(V1, S1, ACTOR_HP_MAX_OFF)); // (branch delay) v1 = max HP
    a.label(Label::ANext);
    a.j_to(Label::Next); // trampolines: Next / Exit live in C
    a.w(nop());
    a.label(Label::AExit);
    a.j_to(Label::Exit);
    a.w(nop());

    // ---- Fragment B: percentage clamp, project the anchor -----------------
    let mut b = Fragment::new(FRAG_B_VA, FRAG_B_END_VA);
    b.label(Label::BStart);
    b.w(nop()); // (load delay for v1; also 4 instructions past the mflo)
    b.w(divu(V0, V1)); // lo = hp*100 / max
    b.w(mflo(S5)); // s5 = pct
    b.w(slti(V0, S5, 101));
    b.br(bne_to(V0, ZERO), Label::PctCapped);
    b.w(nop()); // (branch delay)
    b.w(addiu(S5, ZERO, 100)); // cap at 100
    b.label(Label::PctCapped);
    b.br(bne_to(S5, ZERO), Label::PctFloored);
    b.w(nop()); // (branch delay)
    b.w(addiu(S5, ZERO, 1)); // a living monster always shows a sliver
    b.label(Label::PctFloored);
    b.w(lhu(V0, S1, ACTOR_ANCHOR_X_OFF));
    b.w(lhu(V1, S1, ACTOR_ANCHOR_Z_OFF));
    b.w(sh(V0, SP, F_SVEC)); // svec.x
    b.w(lhu(V0, S1, ACTOR_ANCHOR_Y_OFF));
    b.w(sh(V1, SP, F_SVEC + 4)); // svec.z
    b.w(nop()); // (load delay)
    b.w(sh(V0, SP, F_SVEC + 2)); // svec.y
    b.w(addiu(A0, SP, F_SVEC));
    b.w(addu(A1, ZERO, ZERO)); // half-width 0
    b.w(addu(A2, ZERO, ZERO)); // half-height 0
    b.w(jal(PROJECT_FN));
    b.w(addu(A3, ZERO, ZERO)); // (branch delay) no spin
    b.w(sll(V0, V0, 16));
    b.w(sra(V0, V0, 16));
    b.br(|o| blez(V0, o), Label::ANext); // behind the camera (SZ saturates to 0)
    b.w(nop()); // (branch delay)
    b.w(lh(S3, SP, F_XY)); // s3 = screen x of the anchor
    b.w(addiu(V0, S0, (-(MONSTER_SLOT_FIRST as i16)) as u16)); // row = slot - 3
    b.w(sll(V0, V0, 4)); // * 16 px
    b.w(addiu(S4, V0, PLATE_BAND_Y)); // s4 = this slot's row in the band
    b.w(addiu(S3, S3, (-((PLATE_W / 2) as i16)) as u16)); // centre the plate on it
    b.br(beq_to(ZERO, ZERO), Label::CStart); // splice -> C
    b.w(nop()); // (branch delay)

    // ---- Fragment C: draw, loop tail, epilogue ----------------------------
    let mut c = Fragment::new(FRAG_C_VA, FRAG_C_END_VA);
    c.label(Label::CStart);
    c.w(jal(FRAG_S_VA)); // draw_plate(s3, s4, s5)
    c.w(nop()); // (branch delay)
    c.label(Label::Next);
    c.w(addiu(S0, S0, 1));
    c.w(slti(V0, S0, MONSTER_SLOT_END as i16));
    c.br(beq_to(V0, ZERO), Label::Exit);
    c.w(nop()); // (branch delay)
    c.j_to(Label::LoopHead);
    c.w(nop()); // (branch delay)
    c.label(Label::Exit);
    c.w(lw(RA, SP, F_RA));
    c.w(lw(S0, SP, F_S0));
    c.w(lw(S1, SP, F_S1));
    c.w(lw(S3, SP, F_S3));
    c.w(lw(S4, SP, F_S4));
    c.w(lw(S5, SP, F_S5));
    c.w(addiu(SP, SP, FRAME)); // close the frame
    c.w(HOOK_W0); // replay `addiu sp,sp,-0x70`
    c.w(j(HOOK_RET_VA));
    c.w(HOOK_W1); // (branch delay) replay `lui t0,0x8008`

    // ---- Fragment S: draw one plate (leaf) --------------------------------
    let mut s = Fragment::new(FRAG_S_VA, FRAG_S_END_VA);
    s.w(sw(RA, SP, F_LEAF_RA));
    s.w(addu(A0, S3, ZERO));
    s.w(addu(A1, S4, ZERO));
    s.w(addu(A2, S5, ZERO));
    s.w(lui(T0, imm_hi(PRIM_CURSOR_VA)));
    s.w(lw(T1, T0, imm_lo(PRIM_CURSOR_VA))); // t1 = cursor before the gauge
    s.w(jal(GAUGE_FN));
    s.w(sw(T1, SP, F_CURSOR)); // (branch delay; one instruction past the load)
    s.w(lw(T1, SP, F_CURSOR));
    s.w(lui(T2, imm_hi(METER_BRIGHT_WITH_CODE)));
    s.w(ori(T2, T2, imm_lo(METER_BRIGHT_WITH_CODE)));
    s.w(lui(T3, imm_hi(METER_BRIGHT_PLAIN)));
    s.w(ori(T3, T3, imm_lo(METER_BRIGHT_PLAIN)));
    s.w(sw(T3, T1, 0x14)); // quad 1: rgb2 (gold -> red)
    s.w(sw(T3, T1, 0x1C)); // quad 1: rgb3
    s.w(sw(T2, T1, 0x24 + 0x04)); // quad 2: rgb0 (carries the GP0 code)
    s.w(sw(T3, T1, 0x24 + 0x0C)); // quad 2: rgb1
    s.w(addiu(A0, S3, LABEL_DX));
    s.w(addiu(A1, S4, LABEL_DY));
    s.w(jal(ICON_FN));
    s.w(addiu(A2, ZERO, ICON_HP_LABEL)); // (branch delay)
    s.w(lw(RA, SP, F_LEAF_RA));
    s.w(nop()); // (load delay)
    s.w(jr(RA));
    s.w(nop()); // (branch delay)

    vec![a, b, c, s]
}

/// One assembled fragment: its VA and words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    pub va: u32,
    pub words: Vec<u32>,
}

/// Assemble the routine: resolve every label to an absolute VA, encode the
/// branches (refusing one out of 16-bit reach) and check each fragment fits
/// its body.
pub fn assemble() -> Result<Vec<Assembled>> {
    let frags = program();
    let mut label_va = std::collections::HashMap::new();
    for f in &frags {
        for &(l, idx) in &f.labels {
            if label_va.insert(l, f.va + 4 * idx as u32).is_some() {
                bail!("enemy-hp-bar: label {l:?} defined twice");
            }
        }
    }
    let mut out = Vec::new();
    for f in &frags {
        if f.ins.len() > f.capacity_words() {
            bail!(
                "enemy-hp-bar: fragment at {:#x} is {} words, body holds {}",
                f.va,
                f.ins.len(),
                f.capacity_words()
            );
        }
        let mut words = Vec::with_capacity(f.ins.len());
        for (i, ins) in f.ins.iter().enumerate() {
            let pc = f.va + 4 * i as u32;
            let word = match ins {
                Ins::W(w) => *w,
                Ins::Br(enc, l) => {
                    let target = *label_va
                        .get(l)
                        .ok_or_else(|| anyhow::anyhow!("enemy-hp-bar: unresolved {l:?}"))?;
                    let delta = (i64::from(target) - i64::from(pc + 4)) / 4;
                    let off = i16::try_from(delta).map_err(|_| {
                        anyhow::anyhow!(
                            "enemy-hp-bar: branch at {pc:#x} to {l:?} ({target:#x}) out of reach"
                        )
                    })?;
                    enc(off)
                }
                Ins::J(l) => {
                    let target = *label_va
                        .get(l)
                        .ok_or_else(|| anyhow::anyhow!("enemy-hp-bar: unresolved {l:?}"))?;
                    if (target ^ pc) & 0xF000_0000 != 0 {
                        bail!("enemy-hp-bar: j at {pc:#x} to {target:#x} crosses a segment");
                    }
                    j(target)
                }
            };
            words.push(word);
        }
        out.push(Assembled { va: f.va, words });
    }
    Ok(out)
}

/// A planned enemy-HP-bar injection: the same-size edits, nothing written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyHpBarInjection {
    /// Same-size edits (`None` target = `SCUS_942.54`, `Some(idx)` = PROT entry).
    pub edits: Vec<Edit>,
    /// Words the routine occupies in the battle overlay.
    pub overlay_words: usize,
    /// Words the routine occupies in SCUS.
    pub scus_words: usize,
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn scus_off(scus: &[u8], va: u32) -> Result<usize> {
    item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("enemy-hp-bar: can't resolve SCUS VA {va:#x}"))
}

fn expect_scus(scus: &[u8], va: u32, expect: u32, what: &str) -> Result<()> {
    let off = scus_off(scus, va)?;
    let got = read_word(scus, off)
        .with_context(|| format!("enemy-hp-bar: SCUS_942.54 too short at {va:#x}"))?;
    if got != expect {
        bail!(
            "enemy-hp-bar: {what}: SCUS {va:#x} = {got:#010x}, expected {expect:#010x} \
             (unrecognized build, or another patch holds it - nothing written)"
        );
    }
    Ok(())
}

fn overlay_off(va: u32) -> Result<usize> {
    if va < OVERLAY_BASE_VA {
        bail!("enemy-hp-bar: {va:#x} is below the battle overlay base");
    }
    Ok((va - OVERLAY_BASE_VA) as usize)
}

fn expect_overlay(overlay: &[u8], va: u32, expect: u32, what: &str) -> Result<()> {
    let off = overlay_off(va)?;
    let got = read_word(overlay, off)
        .with_context(|| format!("enemy-hp-bar: PROT 0898 too short at {va:#x}"))?;
    if got != expect {
        bail!(
            "enemy-hp-bar: {what}: PROT 0898 {va:#x} = {got:#010x}, expected {expect:#010x} \
             (unrecognized build, or another patch holds it - nothing written)"
        );
    }
    Ok(())
}

fn assert_not_in_tables(va: u32, len: u32, ranges: &[(u32, u32)], what: &str) -> Result<()> {
    let end = va.saturating_add(len);
    for &(a, b) in ranges {
        if va < b && a < end {
            bail!(
                "enemy-hp-bar: {what} {va:#x}..+{len} overlaps live table {a:#x}..{b:#x} - refusing"
            );
        }
    }
    Ok(())
}

impl EnemyHpBarInjection {
    /// Plan the injection against a real `SCUS_942.54` and PROT 0898 image.
    ///
    /// Refuses - writing nothing - unless the hook site and all four host
    /// bodies carry their known retail words (so a second application, a
    /// different build, or a mod that has since claimed a body all fail
    /// closed), and every fragment fits the body it overwrites.
    pub fn plan(scus: &[u8], overlay: &[u8]) -> Result<Self> {
        // 1. Recognized build + untouched hosts.
        expect_overlay(overlay, HOOK_VA, HOOK_W0, "popup hook")?;
        expect_overlay(overlay, HOOK_VA + 4, HOOK_W1, "popup hook (+4)")?;
        expect_overlay(overlay, FRAG_A_VA, FRAG_A_W0, "fragment A home")?;
        expect_overlay(overlay, FRAG_A_VA + 4, FRAG_A_W1, "fragment A home (+4)")?;
        expect_overlay(overlay, FRAG_A_JR_VA, jr(RA), "fragment A home (jr ra)")?;
        expect_overlay(
            overlay,
            FRAG_A_END_VA,
            FRAG_A_NEXT_PROLOGUE,
            "fragment A bound (next prologue)",
        )?;
        expect_overlay(overlay, FRAG_B_VA, FRAG_B_W0, "fragment B home")?;
        expect_overlay(overlay, FRAG_B_VA + 4, FRAG_B_W1, "fragment B home (+4)")?;
        expect_overlay(overlay, FRAG_B_JR_VA, jr(RA), "fragment B home (jr ra)")?;
        expect_overlay(overlay, FRAG_C_VA, FRAG_C_W0, "fragment C home")?;
        expect_overlay(overlay, FRAG_C_VA + 4, FRAG_C_W1, "fragment C home (+4)")?;
        expect_overlay(overlay, FRAG_C_JR_VA, jr(RA), "fragment C home (jr ra)")?;
        expect_scus(scus, FRAG_S_VA, FRAG_S_W0, "fragment S home")?;
        expect_scus(scus, FRAG_S_VA + 4, FRAG_S_W1, "fragment S home (+4)")?;
        expect_scus(scus, FRAG_S_JR_VA, jr(RA), "fragment S home (jr ra)")?;
        expect_scus(
            scus,
            FRAG_S_END_VA,
            FRAG_S_NEXT_PROLOGUE,
            "fragment S bound (next prologue)",
        )?;

        // 2. Bodies stay clear of every live table (belt and braces: they are
        //    code, but the guard costs nothing).
        for (va, end) in [
            (FRAG_A_VA, FRAG_A_END_VA),
            (FRAG_B_VA, FRAG_B_END_VA),
            (FRAG_C_VA, FRAG_C_END_VA),
        ] {
            assert_not_in_tables(va, end - va, OVERLAY_TABLE_RANGES, "overlay fragment")?;
        }
        assert_not_in_tables(
            FRAG_S_VA,
            FRAG_S_END_VA - FRAG_S_VA,
            SCUS_TABLE_RANGES,
            "SCUS fragment",
        )?;

        // 3. Assemble (fit + reach are checked inside) and lay out the edits.
        let frags = assemble()?;
        let mut edits = Vec::new();
        let mut overlay_words = 0;
        let mut scus_words = 0;
        for f in &frags {
            if f.va >= OVERLAY_BASE_VA {
                overlay_words += f.words.len();
                edits.push(Edit {
                    prot_index: Some(OVERLAY_PROT_INDEX),
                    file_off: overlay_off(f.va)?,
                    bytes: words_to_bytes(&f.words),
                });
            } else {
                scus_words += f.words.len();
                edits.push(Edit {
                    prot_index: None,
                    file_off: scus_off(scus, f.va)?,
                    bytes: words_to_bytes(&f.words),
                });
            }
        }
        edits.push(Edit {
            prot_index: Some(OVERLAY_PROT_INDEX),
            file_off: overlay_off(HOOK_VA)?,
            bytes: words_to_bytes(&[j(FRAG_A_VA), nop()]),
        });
        Ok(Self {
            edits,
            overlay_words,
            scus_words,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mips_sim::Cpu;

    fn all_words() -> Vec<(u32, Vec<u32>)> {
        assemble()
            .unwrap()
            .into_iter()
            .map(|a| (a.va, a.words))
            .collect()
    }

    #[test]
    fn every_fragment_fits_its_body() {
        let frags = assemble().unwrap();
        assert_eq!(frags.len(), 4);
        let caps = [
            (FRAG_A_VA, FRAG_A_END_VA),
            (FRAG_B_VA, FRAG_B_END_VA),
            (FRAG_C_VA, FRAG_C_END_VA),
            (FRAG_S_VA, FRAG_S_END_VA),
        ];
        for (f, (va, end)) in frags.iter().zip(caps) {
            assert_eq!(f.va, va);
            let cap = ((end - va) / 4) as usize;
            assert!(
                f.words.len() <= cap,
                "{va:#x}: {} words > {cap}",
                f.words.len()
            );
            assert_eq!(va % 4, 0, "fragment VA must be word-aligned");
        }
    }

    /// The R3000 load-delay law, as a gate: no instruction may read a
    /// register in the slot right after the load that writes it. Checked over
    /// each fragment in layout order and across the two branch splices.
    #[test]
    fn no_load_delay_hazards() {
        let loaded_reg =
            |w: u32| -> Option<u32> { matches!(w >> 26, 0x20..=0x26).then_some((w >> 16) & 0x1F) };
        let reads = |w: u32, r: u32| -> bool {
            if r == 0 || w == 0 {
                return false;
            }
            let op = w >> 26;
            let rs = (w >> 21) & 0x1F;
            let rt = (w >> 16) & 0x1F;
            match op {
                0 => rs == r || ((w & 0x3F) > 0x08 && rt == r),
                2 | 3 => false,
                0x28..=0x2E => rs == r || rt == r,
                _ => rs == r,
            }
        };
        let check = |seq: &[u32], what: &str| {
            for (i, pair) in seq.windows(2).enumerate() {
                if let Some(r) = loaded_reg(pair[0]) {
                    assert!(
                        !reads(pair[1], r),
                        "{what}: load-delay hazard at word {i}: {:#010x} then {:#010x}",
                        pair[0],
                        pair[1]
                    );
                }
            }
        };
        let frags = all_words();
        for (va, words) in &frags {
            check(words, &format!("fragment {va:#x}"));
        }
        // The A -> B splice: A's delay-slot load is followed by B's first word.
        let a = &frags[0].1;
        let b = &frags[1].1;
        check(&[a[a.len() - 5], b[0]], "A->B splice");
    }

    /// `mflo` must not be followed within two instructions by a multiply or
    /// divide (the R3000 leaves `lo` undefined otherwise).
    #[test]
    fn mflo_is_not_chased_by_a_divide() {
        // Execution order across the A -> B splice.
        let frags = all_words();
        let a = &frags[0].1;
        let b = &frags[1].1;
        let mut seq: Vec<u32> = a[..a.len() - 4].to_vec(); // up to the splice delay slot
        seq.extend_from_slice(&b[..4]);
        let is_muldiv = |w: u32| w >> 26 == 0 && matches!(w & 0x3F, 0x18..=0x1B);
        let is_mflo = |w: u32| w >> 26 == 0 && (w & 0x3F) == 0x12;
        for (i, w) in seq.iter().enumerate() {
            if is_mflo(*w) {
                for k in 1..=2 {
                    if let Some(n) = seq.get(i + k) {
                        assert!(
                            !is_muldiv(*n),
                            "mflo at {i} chased by mult/div at {}",
                            i + k
                        );
                    }
                }
            }
        }
    }

    // ---- Execution model ----------------------------------------------------

    const CTX_VA: u32 = 0x800E_B654;
    const SP0: u32 = 0x801F_F000;
    const ACTORS: [u32; 4] = [0x8010_0000, 0x8010_1000, 0x8010_2000, 0x8010_3000];

    #[derive(Debug, PartialEq, Eq, Clone)]
    enum Call {
        Project {
            x: i16,
            y: i16,
            z: i16,
            hw: u32,
            hh: u32,
            angle: u32,
        },
        Gauge {
            x: i32,
            y: i32,
            value: u32,
        },
        Icon {
            x: i32,
            y: i32,
            icon: u32,
        },
    }

    /// One monster seat for the model: `(live hp, displayed hp, max, hidden, anchor)`.
    type Seat = Option<(u16, u16, u16, bool, (i16, i16, i16))>;

    struct Scene {
        /// Per monster slot 3..=6.
        slots: [Seat; 4],
        ctx: Option<u16>, // Some(hud_parked) = ctx present
        /// Projector stub: (sx, sy, depth) per call, in call order.
        projections: Vec<(i16, i16, i16)>,
    }

    fn run(scene: &Scene) -> (Cpu, Vec<Call>, Vec<Vec<u32>>) {
        let mut cpu = Cpu::new();
        for (va, words) in all_words() {
            cpu.load_words(va, &words);
        }
        // The hook site: `j A; nop` then the popup's real body continues at
        // +8 with something recognisable.
        cpu.load_words(HOOK_VA, &[j(FRAG_A_VA), nop(), 0xDEAD_BEEF]);
        if let Some(parked) = scene.ctx {
            cpu.wr32(CTX_PTR_VA, CTX_VA);
            cpu.wr16(CTX_VA + u32::from(CTX_HUD_PARKED_OFF), parked);
        }
        for (i, slot) in scene.slots.iter().enumerate() {
            let entry = ACTOR_TABLE_VA + 4 * (MONSTER_SLOT_FIRST + i as u32);
            match slot {
                None => cpu.wr32(entry, 0),
                Some((hp, shown, max, hidden, (x, y, z))) => {
                    let a = ACTORS[i];
                    cpu.wr32(entry, a);
                    cpu.wr16(a + u32::from(ACTOR_HP_OFF), *hp);
                    cpu.wr16(a + u32::from(ACTOR_HP_SHOWN_OFF), *shown);
                    cpu.wr16(a + u32::from(ACTOR_HP_MAX_OFF), *max);
                    cpu.wr8(
                        a + u32::from(ACTOR_HIDDEN_OFF),
                        if *hidden { 0xFF } else { 0 },
                    );
                    cpu.wr16(a + u32::from(ACTOR_ANCHOR_X_OFF), *x as u16);
                    cpu.wr16(a + u32::from(ACTOR_ANCHOR_Y_OFF), *y as u16);
                    cpu.wr16(a + u32::from(ACTOR_ANCHOR_Z_OFF), *z as u16);
                }
            }
        }
        // Slot 7 (sentinel) holds a live-looking record that must never draw.
        cpu.wr32(ACTOR_TABLE_VA + 4 * 7, 0x8010_7000);
        cpu.wr16(0x8010_7000 + u32::from(ACTOR_HP_OFF), 5);
        cpu.wr16(0x8010_7000 + u32::from(ACTOR_HP_SHOWN_OFF), 5);
        cpu.wr16(0x8010_7000 + u32::from(ACTOR_HP_MAX_OFF), 5);

        // Primitive cursor and a scratch arena for the gauge stub.
        let mut cursor = 0x8018_0000u32;
        cpu.wr32(PRIM_CURSOR_VA, cursor);

        cpu.pc = HOOK_VA;
        cpu.r[29] = SP0;
        cpu.r[31] = 0x8004_8140; // the popup's caller
        cpu.r[16] = 0x1111; // s-registers the popup's caller owns
        cpu.r[17] = 0x2222;
        cpu.r[19] = 0x3333;
        cpu.r[20] = 0x4444;
        cpu.r[21] = 0x5555;

        let mut calls = Vec::new();
        let mut packets = Vec::new();
        let mut proj = scene.projections.iter();
        loop {
            let pc = cpu.run_until(&[PROJECT_FN, GAUGE_FN, ICON_FN, HOOK_RET_VA]);
            if pc == HOOK_RET_VA {
                break;
            }
            let a0 = cpu.r[4];
            let a1 = cpu.r[5];
            let a2 = cpu.r[6];
            let a3 = cpu.r[7];
            let sp = cpu.r[29];
            match pc {
                PROJECT_FN => {
                    let x = cpu.rd16(a0) as i16;
                    let y = cpu.rd16(a0 + 2) as i16;
                    let z = cpu.rd16(a0 + 4) as i16;
                    calls.push(Call::Project {
                        x,
                        y,
                        z,
                        hw: a1,
                        hh: a2,
                        angle: a3,
                    });
                    let (sx, sy, depth) = *proj.next().expect("projector called too often");
                    for k in 0..4 {
                        let out = cpu.rd32(sp + 0x10 + 4 * k);
                        cpu.wr16(out, sx as u16);
                        cpu.wr16(out + 2, sy as u16);
                    }
                    cpu.r[2] = depth as i32 as u32;
                }
                GAUGE_FN => {
                    calls.push(Call::Gauge {
                        x: a0 as i32,
                        y: a1 as i32,
                        value: a2,
                    });
                    // Retail emits two 0x24-byte gouraud quads at the cursor
                    // (colour words as FUN_8002C0B0 writes them), then bumps.
                    let dark = 0x0010_2080u32;
                    let gold = 0x0040_A0C0u32;
                    let code = u32::from(GAUGE_CODE) << 24;
                    let q1 = [0x0800_0000, dark | code, 0, dark, 0, gold, 0, gold, 0];
                    let q2 = [0x0800_0000, gold | code, 0, gold, 0, dark, 0, dark, 0];
                    cpu.load_words(cursor, &q1);
                    cpu.load_words(cursor + 0x24, &q2);
                    let base = cursor;
                    cursor += 0x48;
                    cpu.wr32(PRIM_CURSOR_VA, cursor);
                    packets.push(vec![base]);
                }
                ICON_FN => calls.push(Call::Icon {
                    x: a0 as i32,
                    y: a1 as i32,
                    icon: a2,
                }),
                _ => unreachable!(),
            }
            // Return to the caller: pc = ra (v0 already set for the stub).
            cpu.pc = cpu.r[31];
        }
        // Read back every recoloured packet pair.
        let bodies: Vec<Vec<u32>> = packets
            .iter()
            .map(|p| (0..18).map(|k| cpu.rd32(p[0] + 4 * k)).collect())
            .collect();
        (cpu, calls, bodies)
    }

    fn scene_two_enemies() -> Scene {
        Scene {
            slots: [
                // Live HP 80 but the mirror already shows 50: mid-combo.
                Some((80, 50, 100, false, (-300, -172, 200))),
                Some((0, 0, 100, false, (0, 0, 0))), // dead
                Some((1, 1, 1000, false, (300, -700, 200))), // sliver
                None,
            ],
            ctx: Some(0),
            projections: vec![(60, 120, 0x20), (250, 130, 0x40)],
        }
    }

    #[test]
    fn draws_one_plate_per_living_monster() {
        let (cpu, calls, bodies) = run(&scene_two_enemies());
        // Balanced frame, popup's frame open replayed, t0 replayed.
        assert_eq!(cpu.r[29], SP0 - 0x70, "sp must be the popup's frame");
        assert_eq!(cpu.r[8], 0x8008_0000, "t0 must be the popup's lui");
        assert_eq!(cpu.r[31], 0x8004_8140, "ra must be the popup caller's");
        assert_eq!(
            (cpu.r[16], cpu.r[17], cpu.r[19], cpu.r[20], cpu.r[21]),
            (0x1111, 0x2222, 0x3333, 0x4444, 0x5555),
            "callee-saved registers must survive"
        );
        let x0 = 60 - i32::from(PLATE_W / 2);
        let y0 = i32::from(PLATE_BAND_Y); // slot 3: first row
        let x1 = 250 - i32::from(PLATE_W / 2);
        let y1 = i32::from(PLATE_BAND_Y) + 2 * i32::from(PLATE_ROW_PITCH); // slot 5
        let plate = |x: i32, y: i32, pct: u32| {
            vec![
                Call::Gauge { x, y, value: pct },
                Call::Icon {
                    x: x + i32::from(LABEL_DX),
                    y: y + i32::from(LABEL_DY),
                    icon: u32::from(ICON_HP_LABEL),
                },
            ]
        };
        let mut expect = vec![Call::Project {
            x: -300,
            y: -172,
            z: 200,
            hw: 0,
            hh: 0,
            angle: 0,
        }];
        expect.extend(plate(x0, y0, 50)); // the mirror, not live HP
        expect.push(Call::Project {
            x: 300,
            y: -700,
            z: 200,
            hw: 0,
            hh: 0,
            angle: 0,
        });
        expect.extend(plate(x1, y1, 1)); // 1/1000 floors to a 1% sliver
        assert_eq!(calls, expect);

        // The gold slots of both quads became the red, dark slots untouched,
        // and the GP0 code byte survived on quad 2's first colour.
        for body in &bodies {
            let dark = 0x0010_2080u32;
            let code = u32::from(GAUGE_CODE) << 24;
            assert_eq!(body[1], dark | code);
            assert_eq!(body[3], dark);
            assert_eq!(body[5], METER_BRIGHT_PLAIN);
            assert_eq!(body[7], METER_BRIGHT_PLAIN);
            assert_eq!(body[9 + 1], METER_BRIGHT_WITH_CODE);
            assert_eq!(body[9 + 3], METER_BRIGHT_PLAIN);
            assert_eq!(body[9 + 5], dark);
            assert_eq!(body[9 + 7], dark);
            assert_eq!(METER_BRIGHT_WITH_CODE >> 24, u32::from(GAUGE_CODE));
        }
    }

    #[test]
    fn percentage_caps_at_100_and_skips_hidden_or_behind_camera() {
        let scene = Scene {
            slots: [
                Some((500, 500, 100, false, (0, 0, 0))), // over max (a buff) -> 100
                Some((10, 10, 10, true, (0, 0, 0))),     // hidden by a summon fade
                Some((10, 10, 10, false, (0, 0, 0))),    // projector says behind the camera
                Some((33, 33, 100, false, (0, 0, 0))),
            ],
            ctx: Some(0),
            projections: vec![(10, 10, 0x10), (10, 10, 0), (10, 10, 0x220)],
        };
        let (_, calls, _) = run(&scene);
        let gauges: Vec<u32> = calls
            .iter()
            .filter_map(|c| match c {
                Call::Gauge { value, .. } => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(gauges, vec![100, 33]);
        let projections = calls
            .iter()
            .filter(|c| matches!(c, Call::Project { .. }))
            .count();
        assert_eq!(projections, 3, "hidden slot must not even be projected");
    }

    #[test]
    fn gates_draw_nothing_outside_battle_or_while_hud_is_parked() {
        for ctx in [None, Some(0x43)] {
            let scene = Scene {
                slots: scene_two_enemies().slots,
                ctx,
                projections: vec![],
            };
            let (cpu, calls, _) = run(&scene);
            assert!(calls.is_empty(), "ctx {ctx:?} drew {calls:?}");
            assert_eq!(cpu.r[29], SP0 - 0x70);
            assert_eq!(cpu.r[8], 0x8008_0000);
        }
    }

    #[test]
    fn hook_words_and_edit_targets() {
        // A synthetic pair of images carrying exactly the fingerprinted words.
        let mut overlay = vec![0u8; 0x28800];
        let mut put = |va: u32, w: u32| {
            let off = (va - OVERLAY_BASE_VA) as usize;
            overlay[off..off + 4].copy_from_slice(&w.to_le_bytes());
        };
        put(HOOK_VA, HOOK_W0);
        put(HOOK_VA + 4, HOOK_W1);
        put(FRAG_A_VA, FRAG_A_W0);
        put(FRAG_A_VA + 4, FRAG_A_W1);
        put(FRAG_A_JR_VA, jr(RA));
        put(FRAG_A_END_VA, FRAG_A_NEXT_PROLOGUE);
        put(FRAG_B_VA, FRAG_B_W0);
        put(FRAG_B_VA + 4, FRAG_B_W1);
        put(FRAG_B_JR_VA, jr(RA));
        put(FRAG_C_VA, FRAG_C_W0);
        put(FRAG_C_VA + 4, FRAG_C_W1);
        put(FRAG_C_JR_VA, jr(RA));
        let scus = synthetic_scus();
        let plan = EnemyHpBarInjection::plan(&scus, &overlay).unwrap();
        assert_eq!(plan.edits.len(), 5);
        let hook = plan.edits.last().unwrap();
        assert_eq!(hook.prot_index, Some(OVERLAY_PROT_INDEX));
        assert_eq!(hook.file_off, (HOOK_VA - OVERLAY_BASE_VA) as usize);
        assert_eq!(hook.bytes, words_to_bytes(&[j(FRAG_A_VA), nop()]));
        assert_eq!(
            plan.overlay_words + plan.scus_words,
            all_words().iter().map(|f| f.1.len()).sum::<usize>()
        );
        let s = plan.edits.iter().find(|e| e.prot_index.is_none()).unwrap();
        assert_eq!(
            s.file_off,
            item_names::file_offset_for_va(&scus, FRAG_S_VA).unwrap()
        );

        // Applying twice refuses: the hook word is no longer retail's.
        let mut patched = overlay.clone();
        for e in plan.edits.iter().filter(|e| e.prot_index.is_some()) {
            patched[e.file_off..e.file_off + e.bytes.len()].copy_from_slice(&e.bytes);
        }
        assert!(EnemyHpBarInjection::plan(&scus, &patched).is_err());
    }

    /// Dump the assembled fragments + hook as `VA HEXBYTES` lines for the
    /// PCSX-Redux RAM-injection probe (`autorun_enemy_hp_bar_inject.lua`).
    /// Run with `-- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn dump_ram_edits() {
        for (va, words) in all_words() {
            let hex: String = words_to_bytes(&words)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            println!("RAMEDIT {va:08x} {hex}");
        }
        let hook: String = words_to_bytes(&[j(FRAG_A_VA), nop()])
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        println!("RAMEDIT {HOOK_VA:08x} {hook}");
    }

    /// A SCUS image with a PS-X EXE header and the fragment-S fingerprint.
    fn synthetic_scus() -> Vec<u8> {
        let text_va = 0x8001_0000u32;
        let size = 0x6C000usize;
        let mut scus = vec![0u8; 0x800 + size];
        scus[..8].copy_from_slice(b"PS-X EXE");
        scus[0x18..0x1C].copy_from_slice(&text_va.to_le_bytes()); // t_addr
        scus[0x1C..0x20].copy_from_slice(&(size as u32).to_le_bytes()); // t_size
        let mut put = |va: u32, w: u32| {
            let off = (va - text_va) as usize + 0x800;
            scus[off..off + 4].copy_from_slice(&w.to_le_bytes());
        };
        put(FRAG_S_VA, FRAG_S_W0);
        put(FRAG_S_VA + 4, FRAG_S_W1);
        put(FRAG_S_JR_VA, jr(RA));
        put(FRAG_S_END_VA, FRAG_S_NEXT_PROLOGUE);
        scus
    }
}
