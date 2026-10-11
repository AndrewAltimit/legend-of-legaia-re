//! The field overlay's three probe-offset tables - the first 192 bytes of its
//! data segment, bound to the instructions that read them.
//!
//! PROT 0897 (link base `0x801CE818`) opens its data segment at `0x801F21B4`
//! with 16-byte rows of signed `(dx, dz)` halfword pairs. The block is not one
//! table: three consumers form three distinct bases into it, and each base is
//! where one table starts.
//!
//! | Table | Rows | Consumer (`lui` / `addiu` pair) | What it probes |
//! |---|---|---|---|
//! | [`ACTOR_PROBE_VA`] | 6 | `0x801CFE74` (`FUN_801CFE4C`), `0x801D5A70` (`FUN_801D5A68`) | actor-collision points, `dir * 0x10` |
//! | [`WALL_PROBE_VA`] | 4 | `0x801CFEE8`, `0x801CFFC0`, `0x801D009C` (`FUN_801CFE4C`) | leading-edge wall points, `dir * 0x10` |
//! | [`FACING_PROBE_VA`] | 2 | `0x801D0834` | the eight-point interact compass, one `(dx, dz)` per heading |
//!
//! Each extent is the distance to the next independently formed base, and the
//! last one ends where `0x801F2274` - a `lw`/`sw` scalar - begins. Rows `4..5`
//! of the actor table have no reader of their own on the locomotion path; they
//! sit inside the extent the `dir * 0x10` index can reach and before the next
//! formed base, and are the table's, not a fourth structure.
//!
//! The row layout and the per-direction values are documented in
//! `docs/subsystems/field-locomotion.md`; the engine carries them as
//! `FIELD_ACTOR_PROBES` / `FIELD_WALL_PROBES` / `FIELD_FACING_PROBES`.

/// Link base of the field overlay (PROT 0897).
pub const OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// PROT extraction index of the field overlay.
pub const OVERLAY_PROT_INDEX: u32 = 897;

/// Actor-collision probe table (`DAT_801F21B4`).
pub const ACTOR_PROBE_VA: u32 = 0x801F_21B4;
/// Rows in the actor-collision table.
pub const ACTOR_PROBE_ROWS: usize = 6;
/// Leading-edge wall probe table (`DAT_801F2214`).
pub const WALL_PROBE_VA: u32 = 0x801F_2214;
/// Rows in the wall table.
pub const WALL_PROBE_ROWS: usize = 4;
/// Interact facing-probe compass (`DAT_801F2254`).
pub const FACING_PROBE_VA: u32 = 0x801F_2254;
/// Rows in the facing compass (eight `(dx, dz)` points, four per row).
pub const FACING_PROBE_ROWS: usize = 2;
/// Bytes per row: four `(i16 dx, i16 dz)` pairs.
pub const ROW_BYTES: usize = 16;

/// `(table VA, rows, lui sites that form it)`, in address order.
pub const TABLES: [(u32, usize, &[u32]); 3] = [
    (
        ACTOR_PROBE_VA,
        ACTOR_PROBE_ROWS,
        &[0x801C_FE74, 0x801D_5A70],
    ),
    (
        WALL_PROBE_VA,
        WALL_PROBE_ROWS,
        &[0x801C_FEE8, 0x801C_FFC0, 0x801D_009C],
    ),
    (FACING_PROBE_VA, FACING_PROBE_ROWS, &[0x801D_0834]),
];

fn word(image: &[u8], va: u32) -> Option<u32> {
    let off = va.checked_sub(OVERLAY_BASE_VA)? as usize;
    image
        .get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Does the `lui` at `site` pair with an `addiu` (within four words) that
/// forms `va`?
fn forms(image: &[u8], site: u32, va: u32) -> bool {
    let hi = (va.wrapping_add(0x8000) >> 16) as u16;
    let lo = va as u16;
    let Some(lui) = word(image, site) else {
        return false;
    };
    if lui >> 26 != 0x0F || lui as u16 != hi {
        return false;
    }
    let rt = (lui >> 16) & 0x1F;
    (1..=4).any(|k| {
        word(image, site + 4 * k)
            .is_some_and(|w| w >> 26 == 0x09 && (w >> 21) & 0x1F == rt && w as u16 == lo)
    })
}

/// Re-derive every table's base from its consumers. Empty when the image is
/// the retail field overlay; one line per disagreement otherwise.
pub fn check(image: &[u8]) -> Vec<String> {
    let mut errs = Vec::new();
    for (va, rows, sites) in TABLES {
        for &site in sites {
            if !forms(image, site, va) {
                errs.push(format!("{va:#010x}: no lui/addiu pair at {site:#010x}"));
            }
        }
        let end = va - OVERLAY_BASE_VA + (rows * ROW_BYTES) as u32;
        if end as usize > image.len() {
            errs.push(format!("{va:#010x}: runs past the image"));
        }
    }
    errs
}

/// One table's rows as `(dx, dz)` pairs, four per row. `None` if the image is
/// too short.
pub fn read(image: &[u8], va: u32, rows: usize) -> Option<Vec<[(i16, i16); 4]>> {
    let off = va.checked_sub(OVERLAY_BASE_VA)? as usize;
    let bytes = image.get(off..off + rows * ROW_BYTES)?;
    Some(
        bytes
            .as_chunks::<ROW_BYTES>()
            .0
            .iter()
            .map(|r| {
                let h = |i: usize| i16::from_le_bytes([r[2 * i], r[2 * i + 1]]);
                [(h(0), h(1)), (h(2), h(3)), (h(4), h(5)), (h(6), h(7))]
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_tile_the_block_in_address_order() {
        let mut at = ACTOR_PROBE_VA;
        for (va, rows, _) in TABLES {
            assert_eq!(va, at);
            at = va + (rows * ROW_BYTES) as u32;
        }
        assert_eq!(at, 0x801F_2274);
    }
}
