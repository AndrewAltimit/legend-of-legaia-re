//! Custom-overlay loading on retail — the vertical slice that proves we can
//! stream hand-written code from an (overwritten) pochi PROT slot into RAM and
//! execute it on real hardware, the foundation the full retail seru-trade UI
//! needs (its UI driver is far too big for the SCUS rodata gap, so it must ship
//! as a loadable overlay the way the fishing / slot-machine minigames do).
//!
//! ## The mechanism
//!
//! 1. The randomizer overwrites a **pochi-filler PROT slot** (265 exist, the
//!    largest >1 MB — reserved dev fillers with real allocated disc sectors) with
//!    a small custom overlay. Because the randomizer placed it, it knows that
//!    slot's exact start LBA + sector count from the disc TOC.
//! 2. A tiny **loader stub** in the preserved SCUS rodata gap calls the
//!    game's own synchronous CD reader [`LOADER_FN`]
//!    (`FUN_8005E4D4(sector_count, lba, dest)` — verified sync: it issues the
//!    read then waits) with those values **baked as literals**, so there is no
//!    runtime PROT-index arithmetic (the recurring ±2 index-space trap can't
//!    bite). It then `jalr`s the loaded code at [`DEST`], and on return replays
//!    the displaced hook instructions and jumps back.
//! 3. A detour at the shop-open path (field-VM op `0x49`) routes into the stub.
//!
//! ## The slice payload
//!
//! For the slice the overlay is the simplest observable: it writes a 32-bit
//! [`SENTINEL`] to [`SENTINEL_ADDR`] (a reserved cell in the SCUS rodata gap,
//! resident RAM we own) and returns. If the sentinel appears after the hook
//! fires on an emulator, the load→exec→return mechanism works on hardware; the
//! real trade UI then replaces this payload. The overlay is a position-
//! independent leaf (absolute data store + `jr ra`), so it runs correctly at any
//! load address.
//!
//! Nothing here embeds Sony bytes: the overlay + stub are the randomizer's own
//! code, and the LBA/sectors come from the user's disc.

/// The game's synchronous LBA reader `FUN_8005E4D4(a0=sector_count, a1=lba,
/// a2=dest) -> bool`. SCUS-resident (always callable). Verified from
/// `ghidra/scripts/funcs/8005e4d4.txt`: sets read position from `a1`, reads
/// `a0` sectors to `a2`, then blocks on the read-sync before returning.
pub const LOADER_FN: u32 = 0x8005_E4D4;

/// Load VA of the loader stub, in the preserved rodata gap at `0x8007AB38`
/// (`0x8007AE00`, in the free window above the flee-EXP routine `0x8007AD00`+0x100
/// and below the seru-trade config blob `0x8007AF00`).
pub const STUB_VA: u32 = 0x8007_AE00;

/// Where the custom overlay is loaded + executed. Slot B (`0x801F69D8`, the
/// summon/effect overlay region) is idle during a field shop; the slice payload
/// is a one-shot leaf so a briefly-borrowed region is fine. (The full UI will
/// use the slot-A on-demand-overlay path instead, like the minigames.)
pub const DEST: u32 = 0x801F_69D8;

/// Reserved sentinel cell in the rodata gap tail (after the 0x18-byte seru-trade
/// config blob at `0x8007AF00`), resident writable RAM we own.
pub const SENTINEL_ADDR: u32 = 0x8007_AF20;

/// The value the slice overlay writes to [`SENTINEL_ADDR`] ("SERU" trade slice).
pub const SENTINEL: u32 = 0x5E_2D_7A_DE;

// --- MIPS R3000 encoders (little-endian words) ------------------------------

const ZERO: u32 = 0;
const A0: u32 = 4;
const A1: u32 = 5;
const A2: u32 = 6;
const V0: u32 = 2;
const V1: u32 = 3;
const T0: u32 = 8;
const RA: u32 = 31;

const fn j(target: u32) -> u32 {
    (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
}
const fn jal(target: u32) -> u32 {
    (0x03 << 26) | ((target >> 2) & 0x03ff_ffff)
}
const fn nop() -> u32 {
    0
}
const fn lui(rt: u32, imm: u16) -> u32 {
    (0x0f << 26) | (rt << 16) | imm as u32
}
const fn ori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0d << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn addiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn sw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x2b << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn jr(rs: u32) -> u32 {
    (rs << 21) | 0x08
}
const fn jalr(rs: u32) -> u32 {
    (rs << 21) | (RA << 11) | 0x09
}

/// High 16 bits to `lui` so a following signed-`lo` access reaches `va`.
const fn hi(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}
/// Low 16 bits of `va` (the signed offset half).
const fn lo(va: u32) -> u16 {
    (va & 0xffff) as u16
}
/// Plain high half of a 32-bit immediate (no sign correction — for `lui`+`ori`).
const fn imm_hi(v: u32) -> u16 {
    (v >> 16) as u16
}
const fn imm_lo(v: u32) -> u16 {
    (v & 0xffff) as u16
}

/// Assemble the slice overlay: write [`SENTINEL`] to [`SENTINEL_ADDR`], return.
/// Position-independent (absolute store + `jr ra`), so it executes at any load
/// address. 6 instructions / 24 bytes.
pub fn assemble_sentinel_overlay() -> Vec<u32> {
    vec![
        lui(V0, imm_hi(SENTINEL)),     // v0 = SENTINEL hi
        ori(V0, V0, imm_lo(SENTINEL)), // v0 |= SENTINEL lo
        lui(V1, hi(SENTINEL_ADDR)),    // v1 = &SENTINEL_ADDR hi
        sw(V0, V1, lo(SENTINEL_ADDR)), // *SENTINEL_ADDR = v0
        jr(RA),                        // return to the stub
        nop(),                         // (branch delay)
    ]
}

/// Assemble the loader stub for an overlay at disc `lba` spanning `sectors`
/// sectors, loaded to [`DEST`] and called. `displaced` are the two hook
/// instructions to replay; `return_va` is where to jump back. Lives at
/// [`STUB_VA`]. 15 instructions / 60 bytes (fits the gap free window).
pub fn assemble_loader_stub(
    lba: u32,
    sectors: u16,
    displaced: [u32; 2],
    return_va: u32,
) -> Vec<u32> {
    vec![
        addiu(A0, ZERO, sectors),  // 0:  a0 = sector_count
        lui(A1, imm_hi(lba)),      // 1:  \ a1 = lba
        ori(A1, A1, imm_lo(lba)),  // 2:  /
        lui(A2, imm_hi(DEST)),     // 3:  \ a2 = dest
        ori(A2, A2, imm_lo(DEST)), // 4:  /
        jal(LOADER_FN),            // 5:  FUN_8005E4D4(sectors, lba, dest)
        nop(),                     // 6:  (delay)
        lui(T0, imm_hi(DEST)),     // 7:  \ t0 = dest
        ori(T0, T0, imm_lo(DEST)), // 8:  /
        jalr(T0),                  // 9:  call the loaded overlay
        nop(),                     // 10: (delay)
        displaced[0],              // 11: replay hook instr 0
        displaced[1],              // 12: replay hook instr 1
        j(return_va),              // 13: back to the hook join
        nop(),                     // 14: (delay)
    ]
}

/// The two detour words written at the hook: `j STUB_VA` then `nop`.
pub fn detour_words() -> [u32; 2] {
    [j(STUB_VA), nop()]
}

/// Number of disc sectors needed to hold `byte_len` bytes (2048-byte sectors).
pub fn sectors_for(byte_len: usize) -> u16 {
    byte_len.div_ceil(2048) as u16
}

/// Serialize a word list to a little-endian byte blob.
pub fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_overlay_writes_the_sentinel() {
        let w = assemble_sentinel_overlay();
        assert_eq!(w.len(), 6);
        // v0 = 0x5E2D7ADE via lui+ori
        assert_eq!(w[0], lui(V0, 0x5E2D));
        assert_eq!(w[1], ori(V0, V0, 0x7ADE));
        // store to SENTINEL_ADDR (0x8007AF20): hi corrects for the +0x20 lo.
        assert_eq!(w[2], lui(V1, hi(SENTINEL_ADDR)));
        assert_eq!(w[3], sw(V0, V1, lo(SENTINEL_ADDR)));
        assert_eq!(w[4], jr(RA));
        assert_eq!(w[5], 0);
    }

    #[test]
    fn loader_stub_calls_the_reader_then_the_overlay() {
        let lba = 0x0004_2A17u32;
        let sectors = 1u16;
        let displaced = [0x3c03_801du32, 0x2464_9070u32];
        let return_va = 0x801E_5A18u32;
        let s = assemble_loader_stub(lba, sectors, displaced, return_va);
        assert_eq!(s.len(), 15);

        // a0 = sectors, a1 = lba (lui+ori), a2 = DEST (lui+ori).
        assert_eq!(s[0], addiu(A0, ZERO, sectors));
        assert_eq!(s[1], lui(A1, 0x0004));
        assert_eq!(s[2], ori(A1, A1, 0x2A17));
        assert_eq!(s[3], lui(A2, imm_hi(DEST)));
        assert_eq!(s[4], ori(A2, A2, imm_lo(DEST)));

        // jal lands on the loader function.
        assert_eq!((s[5] & 0x03ff_ffff) << 2, LOADER_FN & 0x0fff_ffff);
        // jalr t0 calls the loaded overlay at DEST.
        assert_eq!(s[7], lui(T0, imm_hi(DEST)));
        assert_eq!(s[9], jalr(T0));
        // displaced pair replayed, then j back to the hook join.
        assert_eq!(s[11], displaced[0]);
        assert_eq!(s[12], displaced[1]);
        assert_eq!((s[13] & 0x03ff_ffff) << 2, return_va & 0x0fff_ffff);
    }

    #[test]
    fn detour_jumps_to_the_stub() {
        let d = detour_words();
        assert_eq!((d[0] & 0x03ff_ffff) << 2, STUB_VA & 0x0fff_ffff);
        assert_eq!(d[1], 0);
    }

    #[test]
    fn stub_fits_the_gap_free_window() {
        // The stub at 0x8007AE00 must stay below the config blob at 0x8007AF00.
        let s = assemble_loader_stub(0, 1, [0, 0], 0);
        let end = STUB_VA + (s.len() as u32) * 4;
        assert!(end <= 0x8007_AF00, "stub overruns into the config blob");
        // ...and the sentinel cell sits in the reserved tail after the blob.
        assert!((0x8007_AF18..0x8007_AF40).contains(&SENTINEL_ADDR));
    }

    #[test]
    fn sectors_for_rounds_up() {
        assert_eq!(sectors_for(1), 1);
        assert_eq!(sectors_for(2048), 1);
        assert_eq!(sectors_for(2049), 2);
        assert_eq!(sectors_for(0), 0);
    }
}
