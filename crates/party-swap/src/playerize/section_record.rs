//! Equipment-section record surgery: picking and aliasing variant objects,
//! rewriting a section record, and splicing a rebuilt TMD into it.

use super::super::*;

/// Object index of a section's **equipment-variant** object, given its
/// attach count and object count - `None` when the section has no surplus.
///
/// A section's object list is `[attach_count bone objects][surplus]`, and
/// the splice `FUN_800536BC` tags the surplus `0xFF` (the **first**) /
/// `0xFE` (the rest). The two tags are different animals:
///
/// * **`0xFF` - the equipment VARIANT.** The post-pass `FUN_80053898`
///   sorts it past every drawn channel and records the preceding object's
///   bone as its attach channel; `FUN_800513F0` then snapshots the
///   `(default, variant)` object-pointer pair, and the per-frame pass
///   `FUN_8004CCD4` installs one or the other **into the attach bone's own
///   channel** of the render node's model table - the variant while a
///   window of the playing entry's `+0xA4..+0xAB` track is open (or
///   unconditionally, on its extra-channel escape), the default
///   otherwise. So the variant is not a separate part hanging off the
///   hand: it IS the hand, for those frames.
/// * **`0xFE` - an extra animated part**, seated directly after the
///   skeleton bones and posed by a pose channel of its own (the
///   extra-channel swings). It draws alongside the hand, not instead
///   of it.
///
/// Leaving both empty therefore reads very differently. An empty `0xFE`
/// simply drops the equipment visual, which is what the swap wants. An
/// empty `0xFF` **deletes the attach bone's hand** for every frame a
/// window is open - the channel's model pointer is replaced, so the baked
/// part it was drawing is gone, not merely un-decorated.
///
/// Retail's own variant is the same mesh as its attach bone (byte-equal
/// vertices and prims on Vahn / Gala / Terra; same topology, alternate
/// vertices on Noa), which is why the swap never wants a hole here. Live
/// windows exist only in **Noa's** file on the USA disc, and the entry the
/// **Spirit** command plays - art-bank record 0, staged as anim id `0x10`
/// by the command dispatcher `FUN_801D0748` at `0x801D16B0` - is one of
/// them (`+0xA8 = [1, 53]` over a 58-frame clip, pair 1 = the armB hand).
pub fn variant_object(attach_count: usize, nobj: usize) -> Option<usize> {
    (attach_count > 0 && nobj > attach_count).then_some(attach_count)
}

/// Point the attach bone's object-table entry at the variant's, so the two
/// share one copy of the geometry.
///
/// The variant has to carry the same mesh as its attach bone, and the
/// budget will not pay for a second copy: the sections repeat per
/// equipment id (Vahn's arm sections alone hold 30 records), and duplicating
/// a hand into every one of them costs Gala's PROT entry enough to force
/// the whole band down to half texture resolution. Two object-table
/// entries reading one data region cost nothing, and nothing can tell:
/// both the retail splice `FUN_800536BC` and the port's
/// `battle_char_assembly` relocate every entry by one per-section delta and
/// then address the data purely through it, and the retail draw pass reads
/// `prim_top` / `n_primitive` off the entry it was handed.
///
/// The direction matters. The geometry is emitted in the **variant's**
/// slot and the bone's entry is aliased FORWARD onto it, never the other
/// way round: the variant is the section's last object whenever there is
/// exactly one surplus, and `rewrite_section_record` reads a retail TMD's
/// length off its last object's `normal_top`, so an entry aliased backwards
/// would leave that reading short of the real end.
pub fn alias_variant_onto_bone(tmd: &mut [u8], variant: usize) -> Result<()> {
    let entry = |i: usize| legaia_tmd::HEADER_SIZE + i * legaia_tmd::OBJECT_SIZE;
    let (src, dst) = (entry(variant), entry(variant - 1));
    if src + legaia_tmd::OBJECT_SIZE > tmd.len() {
        bail!("variant object {variant} past the encoded object table");
    }
    let copy: Vec<u8> = tmd[src..src + legaia_tmd::OBJECT_SIZE].to_vec();
    tmd[dst..dst + legaia_tmd::OBJECT_SIZE].copy_from_slice(&copy);
    Ok(())
}

/// Rebuild one section record around the swapped geometry + pool.
pub(super) fn rewrite_section_record(
    decoded: &[u8],
    channel_geom: &BTreeMap<u8, ModelObject>,
    pool_block: &[u8],
    section: usize,
    anchor_section: &mut Option<usize>,
) -> Result<Vec<u8>> {
    let u32at = |o: usize| -> Result<u32> {
        legaia_bytes::u32_le(decoded, o).ok_or_else(|| anyhow::anyhow!("short record at +{o:#x}"))
    };
    let frame_off = u32at(0)? as usize;
    let body_end = u32at(0xC)? as usize;
    if body_end > decoded.len() || frame_off < 0x14 || frame_off >= body_end {
        bail!("section record header out of range");
    }
    // Loader frame.
    let attach_count = decoded[frame_off] as usize;
    let tmd_off = frame_off + 0xC;
    let tmd = legaia_tmd::parse(&decoded[tmd_off..]).context("section TMD")?;
    let nobj = tmd.objects.len();
    if attach_count > nobj {
        bail!("attach_count {attach_count} > nobj {nobj}");
    }
    let bone_ids: Vec<u8> = decoded[frame_off + 1..frame_off + 1 + attach_count].to_vec();

    // New object list. A section's objects have three roles (see
    // [`variant_object`]): the first `attach_count` bind to skeleton bones
    // and wear the baked Delilas part for theirs; the FIRST surplus is the
    // section's `0xFF` equipment VARIANT of the last attached bone and has
    // to wear that same part, because the render pass swaps it INTO that
    // bone's channel; any further surplus is a `0xFE` extra animated part
    // with a pose channel of its own and stays empty.
    //
    // The attach bone and its variant share one copy of the geometry: it
    // is emitted in the VARIANT's slot and the bone's own slot goes out
    // empty, its object-table entry aliased onto the variant's below.
    let variant = variant_object(attach_count, nobj);
    // DROP every `0xFE` extra past the variant instead of emitting it
    // empty. The extras carry the equipment visuals the swap discards,
    // and their mere PRESENCE is live-dangerous: the runtime retag
    // records the extra's variant-pair ordinal ("0xFF objects seen so
    // far", `ctx+0x240+slot`), and with two variant-carrying sections
    // ahead of the extra that ordinal is 2 - past the TWO-pair
    // snapshot - so the per-frame variant pass `FUN_8004CCD4` pins an
    // out-of-range pair, reading the NEIGHBOUR SLOT's pair block and
    // installing a foreign object pointer into a garbage channel.
    // Measured live on the `delilas_gi_spirit_streaks_v2` state: during
    // Noa-slot Spirit (variant window [1,53] active) channel 4 drew a
    // foreign object as a screen-crossing beam; hiding obj4 alone
    // removed it. With no extra emitted there is no ordinal to record
    // and the pin stays on pair 0, whose default is our own alias -
    // benign by construction.
    //
    // ... with one exception, discovered live as the SECOND Spirit-streak
    // class: dropping every extra also deletes the assembled model's
    // `100+`-tag node, and the charge's late-phase streamer effect anchors
    // on it - with no such object the anchor resolves to garbage and the
    // effect's trail fan smears across the screen for the drawn frame at
    // the aura's ray phase (a fan of semi-transparent quads with 2-3
    // vertices on the actor and 1-2 at a repeated garbage point, pinned in
    // the ct124 display list of the transition capture). So the FIRST
    // VARIANT-CARRYING section (and only it) re-emits exactly one `0xFE`
    // extra, EMPTY: an empty `0xFE` draws nothing, but its object-table
    // entry exists, so tag-100 lookups resolve. Variant-ness is a
    // per-section invariant of the player files (every record of a section
    // agrees), so the latch below picks the same section for every
    // equipment layout, and the recorded ordinal is exactly 1 (that
    // section's own `0xFF` is the only one preceding it in splice order) -
    // safely inside the two-pair snapshot the old beam bug overflowed,
    // pinning pair 1, which is an identity alias by construction.
    let emit_anchor = variant.is_some() && anchor_section.is_none_or(|a| a == section);
    if emit_anchor {
        *anchor_section = Some(section);
    }
    let emit_count = variant.map_or(nobj, |v| v + 1) + usize::from(emit_anchor);
    // Prim-less stub for a slot whose geometry the swap leaves out. A
    // TRULY empty object is live-dangerous: the charge aura's streamer
    // effect samples authored VERTEX INDICES of specific objects (Noa's
    // hair part and the weapon extra carry 49 / 56 vertices on the retail
    // assembly), and against a zero-vertex pool those reads return
    // whatever bytes sit there - two constant garbage points that the
    // effect's trail fan then stretches across the whole screen (the
    // "Spirit streak": mesh-textured quads anchored on the actor with
    // 1-2 vertices flung to the same far point every run). A stub keeps
    // the pool real: `n` vertices at the object origin - posed by the
    // part's own channel they all land on its socket, so a sampled
    // streamer anchors ON the body - and zero primitives, so nothing is
    // ever drawn.
    let stub = |n: usize| ModelObject {
        vertices: vec![[0i16; 3]; n.max(1)],
        groups: Vec::new(),
        scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
    };
    let mut objects: Vec<ModelObject> = Vec::with_capacity(emit_count);
    for k in 0..emit_count {
        let bone = match variant {
            Some(v) if k + 1 == v => None,
            Some(v) if k == v => bone_ids.last().copied(),
            _ if k < attach_count => bone_ids.get(k).copied(),
            _ => None,
        };
        let obj = match bone {
            Some(b) => channel_geom.get(&b).cloned().unwrap_or_else(|| {
                // Retail geometry existed in this slot; keep its vertex
                // count samplable (see `stub` above).
                stub(tmd.objects.get(k).map_or(0, |o| o.vertices.len()))
            }),
            // The variant hole before the alias copies over it, or the
            // streamer-anchor extra: the extra gets a generous samplable
            // pool, the hole is overwritten below either way.
            None if k >= nobj || k + 1 != variant.unwrap_or(usize::MAX) => stub(64),
            None => ModelObject {
                vertices: Vec::new(),
                groups: Vec::new(),
                scale: legaia_tmd::encode::LEGAIA_OBJECT_SCALE,
            },
        };
        objects.push(obj);
    }
    let mut new_tmd = encode(&objects).context("encode section TMD")?;
    if let Some(v) = variant {
        alias_variant_onto_bone(&mut new_tmd, v).context("alias the equipment-variant object")?;
    }
    splice_record_tmd(decoded, &new_tmd, pool_block)
}

/// Rebuild a section record around a replacement TMD and pool:
/// `[0x00..tmd verbatim][new TMD][swing / attach tail shifted whole][pool]`,
/// with the header's swing-record offsets, `body_end`, attach-object
/// offsets and the loader frame's `data_size` fixed up, and the upload
/// flag forced on (a retail flag-0 default would leave the band tile
/// stale). `decoded` is the retail record the header, loader frame and
/// tail come from; the new TMD is written at its `frame_off + 0xC`.
pub fn splice_record_tmd(decoded: &[u8], new_tmd: &[u8], pool_block: &[u8]) -> Result<Vec<u8>> {
    let u32at = |o: usize| -> Result<u32> {
        legaia_bytes::u32_le(decoded, o).ok_or_else(|| anyhow::anyhow!("short record at +{o:#x}"))
    };
    let frame_off = u32at(0)? as usize;
    let swing_a = u32at(4)? as usize;
    let swing_b = u32at(8)? as usize;
    let body_end = u32at(0xC)? as usize;
    let attach_obj_count = i16::from_le_bytes(
        decoded
            .get(0x10..0x12)
            .ok_or_else(|| anyhow::anyhow!("short record"))?
            .try_into()
            .unwrap(),
    ) as usize;
    if body_end > decoded.len() || frame_off < 0x14 || frame_off >= body_end {
        bail!("section record header out of range");
    }
    let tmd_off = frame_off + 0xC;
    let old_data_size = u32at(frame_off + 8)? as usize;
    let tmd = legaia_tmd::parse(&decoded[tmd_off..]).context("section TMD")?;

    // Old TMD byte extent (up to the swing region / body_end).
    let old_last = tmd
        .objects
        .last()
        .ok_or_else(|| anyhow::anyhow!("section TMD has no objects"))?;
    let old_tmd_len = legaia_tmd::HEADER_SIZE + old_last.header.normal_top as usize;
    let old_tail_start = tmd_off + old_tmd_len;
    if old_tail_start > body_end {
        bail!("section TMD runs past body_end");
    }

    // Assemble: [0x00..tmd_off verbatim][new TMD][tail shifted][pool].
    let mut out = Vec::with_capacity(decoded.len());
    out.extend_from_slice(&decoded[..tmd_off]);
    out.extend_from_slice(new_tmd);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    let delta = out.len() as i64 - old_tail_start as i64;
    out.extend_from_slice(&decoded[old_tail_start..body_end]);
    let new_body_end = out.len();
    out.extend_from_slice(pool_block);

    // Header fixups.
    let put_u32 =
        |out: &mut [u8], o: usize, v: u32| out[o..o + 4].copy_from_slice(&v.to_le_bytes());
    if swing_a != 0 {
        put_u32(&mut out, 4, (swing_a as i64 + delta) as u32);
    }
    if swing_b != 0 {
        put_u32(&mut out, 8, (swing_b as i64 + delta) as u32);
    }
    put_u32(&mut out, 0xC, new_body_end as u32);
    out[0x12] = 1;
    out[0x13] = 0;
    for i in 0..attach_obj_count {
        let off = u32at(0x14 + i * 4)? as usize;
        if off >= old_tail_start && off < body_end {
            put_u32(&mut out, 0x14 + i * 4, (off as i64 + delta) as u32);
        } else if off >= body_end {
            bail!("attach-object record beyond body_end");
        }
    }
    // Frame data_size shifts with the TMD size change.
    let new_data_size = (old_data_size as i64 + (new_tmd.len() as i64 - old_tmd_len as i64)) as u32;
    put_u32(&mut out, frame_off + 8, new_data_size);
    Ok(out)
}
