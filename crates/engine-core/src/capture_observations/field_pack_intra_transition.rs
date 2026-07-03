/// Scene-bundle pool base. Each pool slot is 16 bytes:
/// `[u32 scene_id][u32 reserved][char name[8]]`. Slots 0 and 1 both
/// carry the active / pending scene name.
pub const SCENE_NAME_TABLE_ADDR: u32 = 0x80084540;

/// Stride between the two scene-bundle pool slots.
pub const SCENE_NAME_SLOT_STRIDE: u32 = 0x10;

/// Offset of the 8-byte CDNAME label inside one scene-bundle pool slot.
pub const SCENE_NAME_OFFSET_IN_SLOT: u32 = 0x08;

/// Maximum length of a CDNAME label inside a pool slot (8 bytes,
/// null-padded).
pub const SCENE_NAME_MAX_LEN: usize = 8;

/// Old field-pack base (`town01` intro Rim Elm settled state).
pub const PREV_BASE: u32 = 0x80139530;

/// New field-pack base (`town0c` Rim Elm normal scene; matches
/// the settled `town0c` value once the loader completes).
pub const NEXT_BASE: u32 = 0x800A25F0;

/// Read the CDNAME label from one of the two scene-bundle pool slots
/// (`slot` is 0 or 1). Returns the trimmed label if it parses as
/// printable ASCII, otherwise `None`.
pub fn read_pool_slot_name(main_ram: &[u8], slot: u32) -> Option<String> {
    if slot > 1 {
        return None;
    }
    let base = SCENE_NAME_TABLE_ADDR + slot * SCENE_NAME_SLOT_STRIDE + SCENE_NAME_OFFSET_IN_SLOT;
    let off = (base - 0x80000000) as usize;
    let bytes = main_ram.get(off..off + SCENE_NAME_MAX_LEN)?;
    let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    if nul == 0 || !bytes[..nul].iter().all(|&b| b.is_ascii_graphic()) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes[..nul]).into_owned())
}

/// Detect whether `main_ram` is captured mid-transition: the
/// scene-bundle pool's slot-0 name disagrees with the scene name
/// implied by the field-pack base pointer's last-known value.
/// Returns `(pool_label, recovered_base_value)` only when both
/// readings succeed AND they disagree about which scene is loaded.
pub fn detect_mid_transition(main_ram: &[u8]) -> Option<(String, u32)> {
    let label = read_pool_slot_name(main_ram, 0)?;
    let base = super::field_pack_load::recover_base(main_ram)?;
    // The settled pre-transition state (`town01`) has
    // label="town01" + base=PREV_BASE. The mid-transition state
    // (`town0c`) has label="town0c" + base=PREV_BASE - the label
    // has flipped, the base has not. We surface that case.
    if label != "town01" && base == PREV_BASE {
        return Some((label, base));
    }
    if label != "town0c" && base == NEXT_BASE {
        return Some((label, base));
    }
    None
}
