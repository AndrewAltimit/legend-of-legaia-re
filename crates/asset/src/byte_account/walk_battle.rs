//! Battle-side walkers: monster archive, `summon.dat` / `readef.DAT`, player battle files.
//! Split out of `byte_account.rs`.

use super::*;

// --- monster archive (PROT 0867) ------------------------------------------

pub(super) fn walk_monster_archive(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    use crate::monster_archive::SLOT_STRIDE;
    let slots = buf.len() / SLOT_STRIDE;
    let mut populated = 0usize;
    let mut raw_tims = 0usize;
    let mut fill_slots = 0usize;
    for id in 1..=slots {
        let slot = (id - 1) * SLOT_STRIDE;
        // The loader transfers the whole 0x14000-byte slot whatever the block
        // costs; everything past the LZS stream's own terminator is declared
        // slack. See `claim_slot_fill`.
        let before = sink.claims.len();
        claim_slot_fill(
            buf,
            sink,
            slot,
            slot + SLOT_STRIDE,
            format!("slot {id} fill past the block"),
        );
        fill_slots += usize::from(sink.claims.len() > before);
        let Some(dec_size) = legaia_bytes::u32_le(buf, slot).map(|v| v as usize) else {
            continue;
        };
        // The archive's trailing slots do not hold a `[u32 dec_size][LZS]`
        // monster block at all: their first word is the PSX TIM magic and the
        // slot head is a raw TIM, zero-padded to the stride. Claiming the head
        // word as a `dec_size` there would be reading the magic as a length.
        if dec_size == 0x10 {
            if let Some(hit) = buf
                .get(slot..slot + SLOT_STRIDE)
                .map(crate::tim_scan::scan_buffer)
                .and_then(|hits| hits.into_iter().find(|h| h.offset == 0))
            {
                sink.claim(
                    slot,
                    slot + hit.byte_len,
                    OWNER_TIM,
                    format!(
                        "slot {id} raw TIM {}x{} {}bpp",
                        hit.width, hit.height, hit.bpp
                    ),
                );
                raw_tims += 1;
            }
            continue;
        }
        if !(0x4C..=SLOT_STRIDE * 8).contains(&dec_size) {
            continue;
        }
        sink.claim(slot, slot + 4, OWNER_HEADER, format!("slot {id} dec_size"));
        let detail = format!("monster id {id} block ({dec_size} B)");
        let Some(block) = take_lzs(sink, buf, slot + 4, dec_size, &detail) else {
            sink.note(format!("monster id {id}: LZS decode failed"));
            continue;
        };
        populated += 1;
        sink.nest(
            opts,
            depth,
            format!("lzs@{:#x} monster id {id}", slot + 4),
            &block,
            Walker::MonsterBlock,
        );
    }
    sink.note(format!(
        "{populated} of {slots} {SLOT_STRIDE:#x}-byte slots carry a decodable block; \
         {raw_tims} carry a raw TIM at the slot head instead; \
         {fill_slots} end in fill the loader transfers and never reads"
    ));
}

/// Offset of the per-action animation stream inside a spell/action entry
/// (`docs/formats/monster-animation.md`).
pub(super) const MONSTER_ANIM_STREAM_OFFSET: usize = 0x8C;
/// Bytes per part record in the packed stream (six 12-bit fields).
pub(super) const MONSTER_ANIM_PART_STRIDE: usize = 9;

pub(super) fn walk_monster_block(block: &[u8], sink: &mut Sink) {
    sink.claim(0, 0x4C, OWNER_RECORD, "stat record head");
    if let Some(name_off) = legaia_bytes::u32_le(block, 0).map(|v| v as usize)
        && let Some(rest) = block.get(name_off..)
    {
        let n = rest.iter().take_while(|&&b| b != 0).count();
        if n > 0 && n < 64 {
            sink.claim(name_off, name_off + n + 1, OWNER_STRING, "monster name");
        }
    }
    // TMD + texture pool: record `+0x04` / `+0x08`.
    let tmd_off = legaia_bytes::u32_le(block, 0x04).unwrap_or(0) as usize;
    if legaia_bytes::u32_le(block, tmd_off) == Some(0x8000_0002) {
        let len = crate::tmd_scan::scan_buffer(block)
            .into_iter()
            .find(|h| h.offset == tmd_off)
            .map(|h| h.byte_len);
        match len {
            Some(n) => sink.claim(tmd_off, tmd_off + n, OWNER_TMD, "monster mesh"),
            None => sink.note("TMD magic at +0x04 but tmd_scan did not size it"),
        }
    }
    let pool = legaia_bytes::u32_le(block, 0x08).unwrap_or(0) as usize;
    if pool > 0 && pool < block.len() {
        // `MonsterMesh::texture` reads `pool = &block[pool_off..]` whole: the
        // CLUT region then a 4bpp page filling the rest of the block.
        let clut_end = (pool + crate::monster_archive::CLUT_REGION_BYTES).min(block.len());
        sink.claim(pool, clut_end, OWNER_CLUT, "15 x 16-colour palettes");
        sink.claim(clut_end, block.len(), OWNER_TEXTURE, "4bpp page, 256 rows");
    }
    // Spell / action entries: `+0x4A` count, `+0x4C` offsets.
    let count = block.get(0x4A).copied().unwrap_or(0) as usize;
    if count == 0 || count > 64 {
        return;
    }
    sink.claim(
        0x4C,
        0x4C + count * 4,
        OWNER_TOC,
        format!("{count} spell offsets"),
    );
    let eff_base = (count + 0x13) * 4;
    let mut eff_max = 0usize;
    for i in 0..count {
        let Some(off) = legaia_bytes::u32_le(block, 0x4C + i * 4).map(|v| v as usize) else {
            continue;
        };
        if off == 0 || off >= block.len() {
            continue;
        }
        let head_end = (off + MONSTER_ANIM_STREAM_OFFSET).min(block.len());
        sink.claim(off, head_end, OWNER_RECORD, format!("action entry {i}"));
        for f in [0x04usize, 0x08] {
            let idx = legaia_bytes::u32_le(block, off + f).unwrap_or(0) as usize;
            eff_max = eff_max.max(idx);
        }
        // `[u8 part_count][u8 frame_count][frames * parts * 9]`
        let (parts, frames) = (
            block.get(head_end).copied().unwrap_or(0) as usize,
            block.get(head_end + 1).copied().unwrap_or(0) as usize,
        );
        if parts > 0 && frames > 0 {
            let end = head_end + 2 + frames * parts * MONSTER_ANIM_PART_STRIDE;
            if end <= block.len() {
                sink.claim(
                    head_end,
                    end,
                    OWNER_ANM,
                    format!("action {i} keyframes ({parts}p x {frames}f)"),
                );
            }
        }
    }
    if eff_max > 0 {
        sink.claim(
            eff_base,
            eff_base + eff_max * 4,
            OWNER_TOC,
            format!("effect-offset table ({eff_max} words)"),
        );
    }
}

// --- summon.dat / readef.DAT (PROT 0893 / 0894) ---------------------------

pub(super) fn walk_summon_readef(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    use crate::summon_readef::{SLOT_BYTES, SlotKind};
    let Ok(f) = crate::summon_readef::parse(buf) else {
        sink.note("summon_readef::parse failed");
        return;
    };
    let (mut tex, mut actor, mut me, mut raw) = (0, 0, 0, 0);
    let mut fill_slots = 0usize;
    for s in &f.slots {
        let base = s.index * SLOT_BYTES;
        // Both files stream in whole `0x10800` slots whatever the slot holds;
        // the tail fill is declared slack. See `claim_slot_fill`.
        let before = sink.claims.len();
        claim_slot_fill(
            buf,
            sink,
            base,
            base + SLOT_BYTES,
            format!("slot {} fill past the content", s.index),
        );
        fill_slots += usize::from(sink.claims.len() > before);
        match &s.kind {
            SlotKind::Texture(t) => {
                tex += 1;
                sink.claim(base, base + 4, OWNER_HEADER, "texture-slot mode");
                let clut_end = base + 4 + t.clut_bytes();
                sink.claim(
                    base + 4,
                    clut_end,
                    OWNER_CLUT,
                    format!("{} CLUT row(s)", t.clut_rows),
                );
                let tex_start = base + t.texture_offset;
                sink.claim(
                    tex_start,
                    tex_start + t.texture_bytes(),
                    OWNER_TEXTURE,
                    format!("{}-halfword page", t.texture_width_halfwords),
                );
            }
            SlotKind::ActorRecord(a) => {
                actor += 1;
                sink.claim(base, base + 0x4C, OWNER_RECORD, "actor record head");
                sink.claim(
                    base + 0x4C,
                    base + 0x4C + a.part_offsets.len() * 4,
                    OWNER_TOC,
                    format!("{} part offsets", a.part_count),
                );
                if let Some(n) = a.name.as_ref().map(|n| n.len()) {
                    sink.claim(
                        base + a.name_offset,
                        base + a.name_offset + n + 1,
                        OWNER_STRING,
                        "attack name",
                    );
                }
                // Each part offset names a sub-mesh; they tile the region
                // between the part table and the texture pool.
                let mut parts: Vec<usize> = a.part_offsets.iter().map(|&o| o as usize).collect();
                parts.sort_unstable();
                parts.dedup();
                for (n, off) in parts.iter().enumerate() {
                    let end = parts
                        .get(n + 1)
                        .copied()
                        .unwrap_or(a.texture_pool_offset)
                        .max(*off);
                    sink.claim(base + off, base + end, OWNER_TMD, format!("part {n}"));
                }
                let tmd_off = base + a.tmd_offset;
                if let Some(slot) = buf.get(base..base + SLOT_BYTES) {
                    if let Some(h) = crate::tmd_scan::scan_buffer(slot)
                        .into_iter()
                        .find(|h| h.offset == a.tmd_offset)
                    {
                        sink.claim(tmd_off, tmd_off + h.byte_len, OWNER_TMD, "summon mesh");
                    }
                    sink.claim(
                        base + a.texture_pool_offset,
                        base + SLOT_BYTES,
                        OWNER_TEXTURE,
                        "texture pool",
                    );
                }
            }
            SlotKind::MeArchive { count, compressed } => {
                me += 1;
                if let Some(slot) = buf.get(base..base + SLOT_BYTES) {
                    walk_me_archive_at(slot, base, sink);
                }
                if s.index < 4 {
                    sink.note(format!(
                        "slot {}: ME archive, {count} entries ({compressed} compressed)",
                        s.index
                    ));
                }
                if let Some(slot) = buf.get(base..base + SLOT_BYTES) {
                    sink.nest(
                        opts,
                        depth,
                        format!("slot {} ME archive", s.index),
                        slot,
                        Walker::MeArchive,
                    );
                }
            }
            SlotKind::Payload => {
                raw += 1;
                // The documented raw slot: `[0x1E0 CLUT][0x8000 4bpp page]
                // [part pool to the slot end]` - the big-summon group's third
                // member (`summon_readef::RAW_SLOT_*`). Filler slots are
                // skipped so the constants are not applied to empty space.
                let Some(slot) = buf.get(base..base + SLOT_BYTES) else {
                    continue;
                };
                let nonzero = slot.iter().filter(|&&b| b != 0).count();
                if nonzero * 4 < SLOT_BYTES {
                    continue;
                }
                use crate::summon_readef::{
                    RAW_SLOT_CLUT_BYTES, RAW_SLOT_PAGE_BYTES, RAW_SLOT_PART_POOL_BYTES,
                    RAW_SLOT_PART_POOL_OFFSET,
                };
                sink.claim(
                    base,
                    base + RAW_SLOT_CLUT_BYTES,
                    OWNER_CLUT,
                    "raw slot CLUT block",
                );
                sink.claim(
                    base + RAW_SLOT_CLUT_BYTES,
                    base + RAW_SLOT_CLUT_BYTES + RAW_SLOT_PAGE_BYTES,
                    OWNER_TEXTURE,
                    "raw slot 4bpp page",
                );
                sink.claim(
                    base + RAW_SLOT_PART_POOL_OFFSET,
                    base + RAW_SLOT_PART_POOL_OFFSET + RAW_SLOT_PART_POOL_BYTES,
                    OWNER_RECORD,
                    "raw slot part pool",
                );
            }
        }
    }
    sink.note(format!(
        "{} slots: {tex} texture, {actor} actor record, {me} ME archive, {raw} unclassified; \
         {fill_slots} end in fill the stream SM transfers and never reads",
        f.slots.len()
    ));
}

pub(super) fn walk_me_archive_at(slot: &[u8], base: usize, sink: &mut Sink) {
    let Ok(me) = crate::me_archive::parse(slot) else {
        return;
    };
    let n = me.len();
    sink.claim(
        base,
        base + 4 + n * 8,
        OWNER_TOC,
        format!("ME toc, {n} entries"),
    );
    for i in 0..n {
        if let Some(body) = me.raw_body(i) {
            // `raw_body` borrows the slot, so the offset is recoverable by
            // pointer arithmetic against the slot's own start.
            let off = body.as_ptr() as usize - slot.as_ptr() as usize;
            sink.claim(
                base + off,
                base + off + body.len(),
                OWNER_ANM,
                format!("ME entry {i}"),
            );
        }
    }
}

// --- player battle files ---------------------------------------------------

pub(super) fn walk_battle_data_pack(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    let Some(pack) = crate::battle_data_pack::detect(buf) else {
        sink.note("battle_data_pack::detect returned None");
        return;
    };
    sink.claim(0, 0x10, OWNER_HEADER, "desc_off + CLUT offsets + budget");
    // record[0] is an LZS stream between the header and the table.
    if let Some(dec) = legaia_bytes::u32_le(buf, 12).map(|v| v as usize)
        && take_lzs(sink, buf, 0x10, dec, "record[0] (art records)").is_none()
    {
        sink.note("record[0] LZS decode failed");
    }
    let n = pack.records.len();
    let table_end = pack.table_offset + (n + 1) * 12;
    sink.claim(
        pack.table_offset,
        table_end,
        OWNER_TOC,
        format!("{n} [id, offset, size] entries + terminator"),
    );
    // Between the descriptor table's terminator and the compressed-data
    // section the file carries zero fill, and both ends of it are declared:
    // the table ends where its own terminator does, and the data section
    // begins where the pack's `data_base` plus the first descriptor's offset
    // says. Nothing reads between them - the loader seeks each slot by
    // descriptor - so it is slack the container states, not an unwalked
    // region. Claimed only when every byte of it is zero, which is the guard
    // that stops a short walk from buying the gap.
    let data_start = pack
        .records
        .iter()
        .map(|r| pack.data_base + r.data_offset as usize)
        .min()
        .unwrap_or(table_end);
    if data_start > table_end
        && buf
            .get(table_end..data_start)
            .is_some_and(|g| g.iter().all(|&b| b == 0))
    {
        sink.claim(
            table_end,
            data_start,
            OWNER_PAD,
            "descriptor-table to data-section slack",
        );
    }
    for (i, r) in pack.records.iter().enumerate() {
        let off = pack.data_base + r.data_offset as usize;
        sink.claim(off, off + 4, OWNER_HEADER, format!("slot {i} dec_size"));
        let Some(dec) = legaia_bytes::u32_le(buf, off).map(|v| v as usize) else {
            continue;
        };
        let detail = format!("equip slot {i} (id {:#x})", r.id);
        if let Some(out) = take_lzs(sink, buf, off + 4, dec, &detail) {
            sink.nest(
                opts,
                depth,
                format!("slot {i} id {:#x}", r.id),
                &out,
                Walker::Generic,
            );
        }
        // The slot's declared footprint is sector-aligned slack the container
        // itself accounts for.
        sink.claim(
            off,
            off + r.size as usize,
            OWNER_PAD,
            format!("slot {i} declared footprint"),
        );
    }
}
