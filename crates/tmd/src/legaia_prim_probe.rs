//! Diagnostic: probe Legaia primitive sections for known-PSX-size mode
//! sequences. This is NOT a parser — it's a research tool to help
//! infer the layout. For each mode byte at offset+3 within a 4-byte
//! window in the section, look up its expected PSX-stored size and
//! see whether walking the section that way consumes exactly the size
//! of the section in exactly `n_primitive` items.
//!
//! Public results from running this on real Legaia TMDs are recorded in
//! memory; the function itself is left available for future research.

/// PSX-stored TMD primitive sizes (4-byte header + body) by mode-byte.
/// Modes for "no light source / no normal" variants only — most common
/// in optimized retail games. From PSX SDK PsyQ docs.
pub fn psx_stored_size(mode: u8) -> Option<usize> {
    match mode {
        // F3: flat-shaded triangle (no normal)
        0x20..=0x23 => Some(16),
        // F4: flat quad
        0x28..=0x2B => Some(20),
        // FT3: flat textured triangle
        0x24..=0x27 => Some(24),
        // FT4: flat textured quad
        0x2C..=0x2F => Some(28),
        // G3: gouraud triangle
        0x30..=0x33 => Some(24),
        // G4: gouraud quad
        0x38..=0x3B => Some(28),
        // GT3: gouraud textured triangle
        0x34..=0x37 => Some(36),
        // GT4: gouraud textured quad
        0x3C..=0x3F => Some(48),
        _ => None,
    }
}

/// Walk a primitive section assuming Sony PsyQ stored-prim sizes per mode
/// byte (at byte+3 of a 4-byte primitive header). Returns:
/// - Ok(walked_count) if the walk consumes exactly `section_size` bytes
/// - Err(message) otherwise
pub fn walk_psx_stored_sizes(section: &[u8], n_primitive: usize) -> Result<usize, String> {
    let mut pos = 0usize;
    let mut count = 0usize;
    while count < n_primitive {
        if pos + 4 > section.len() {
            return Err(format!(
                "prim {} header at {} past section end {}",
                count,
                pos,
                section.len()
            ));
        }
        let mode = section[pos + 3];
        let size = psx_stored_size(mode)
            .ok_or_else(|| format!("prim {} at {}: unknown mode 0x{:02X}", count, pos, mode))?;
        if pos + size > section.len() {
            return Err(format!(
                "prim {} (mode 0x{:02X}, size {}) at {} overruns section end {}",
                count,
                mode,
                size,
                pos,
                section.len()
            ));
        }
        pos += size;
        count += 1;
    }
    if pos != section.len() {
        return Err(format!(
            "consumed {}b of {}b ({} unused after {} prims)",
            pos,
            section.len(),
            section.len() - pos,
            count
        ));
    }
    Ok(count)
}
