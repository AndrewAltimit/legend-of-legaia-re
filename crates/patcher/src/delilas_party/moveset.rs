//! Delilas move mode: the whole art kit re-animated from the sibling's own clips.
//! Split out of `delilas_party.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Delilas move mode: the whole art kit re-animated from the sibling's
// own clips.
// ---------------------------------------------------------------------------

/// Bank row the arts matcher starts each scan at, and the Miracle Art's
/// own record.
///
/// `FUN_801EED1C` seeds its row cursor `li s3, 0xb` (`0x801EF2EC`) and
/// abandons the whole scan when the bank's record count is `<= 0x0B`
/// (`0x801EF2F4`-`0x801EF2FC`), so rows below 11 are never matched. Row
/// 11 is additionally the Miracle Art: the substitution path branches to
/// the wholesale queue overwrite from `0x801F64F4` only while the
/// rows-visited counter is still zero (`0x801EF4D8`-`0x801EF4E0`), i.e.
/// only on this first row - and reading the disc confirms it, row 11
/// carrying `RDLULURDL` / `LURDULUDR` / `RRDUDUDLL`, the three Miracle
/// combos the SCUS arts table flags.
pub(super) const MIRACLE_BANK_ROW: usize = 0x0B;

/// Queue action constant of bank row `row`: the matcher writes
/// `s3 + 0x10` (`0x801EF63C` single-record arts, `0x801EF610` the
/// multi-record form), so the two spaces differ by a constant.
pub(super) const ART_CONSTANT_BASE: usize = 0x10;

/// VA of the per-character **innate art cap** the learn-on-use gate
/// reads (`FUN_801EFBFC`, `0x801EFD0C`-`0x801EFD18`): an art id is only
/// self-taught when `cap < id`. Reads `[3, 5, 3]` on the USA disc, which
/// is exactly each character's Hyper-Art block - those are granted by a
/// script instead (the `+0x74E` insert at `0x80041FB4` in SCUS
/// `FUN_800402F4`), so blanking their combos would list an art that can
/// never fire.
pub(super) const INNATE_ART_CAP_VA: u32 = 0x801F_686C;

/// Record-image span of an art record's zero-terminated combo. `+0x0A`
/// is the stream index and `+0x0B..+0x0D` size the matcher's row stride,
/// so blanking must stop short of them.
pub(super) const COMBO_FIELD: std::ops::Range<usize> = 0..9;

/// Record-image span of the inline art name (`+0x10`, NUL-padded, ends
/// where the embedded action entry begins).
pub(super) const NAME_FIELD: std::ops::Range<usize> = 0x10..0x24;

/// Bank rows whose art constant appears in one of the character's Super
/// Art `find` patterns.
///
/// A Super is not entered as a combo - `FUN_801EF9E4` walks the
/// **finished** action queue at `actor[+0x1DF]` and tail-matches it
/// against the resident trigger table, so a Super can only fire if every
/// regular art of its `find` string reached that queue, and the only
/// writer that puts an art constant there is the combo matcher. Blanking
/// one of these rows would silently cost the Super.
pub(super) fn super_critical_rows(
    character: legaia_art::queue::Character,
) -> std::collections::BTreeSet<usize> {
    legaia_art::super_art::SUPER_ARTS
        .iter()
        .filter(|s| s.character == character)
        .flat_map(|s| s.art_sequence())
        .filter_map(|c| (c as usize).checked_sub(ART_CONSTANT_BASE))
        .collect()
}

/// The innate cap byte for a party slot, read off the battle overlay.
pub(super) fn innate_art_cap(overlay: &[u8], slot: usize) -> Result<u8> {
    let off = (INNATE_ART_CAP_VA - BATTLE_OVERLAY_BASE) as usize + slot;
    overlay
        .get(off)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("innate art cap for slot {slot} is past the overlay"))
}

/// Which bank rows keep a working combo under [`DelilasMoveMode::Delilas`].
///
/// Four reasons a row survives, and only the first is about taste:
///
/// - the **signature host** row, which now carries the sibling's special;
/// - row 11, the **Miracle Art** - its combo is the only thing that
///   reaches the wholesale queue overwrite;
/// - every row a **Super Art** trigger names ([`super_critical_rows`]);
/// - every row whose art id is at or below the **innate cap**, because
///   those are script-granted and blanking them would leave a listed art
///   that can never be performed.
///
/// Everything else is blanked, and blanking hides it for free: an art is
/// only listed once `FUN_801EFBFC` has inserted it at char record
/// `+0x185` on a successful performance, and a blanked combo can never
/// be performed. The load-bearing reason is the `combo_len == 1` guard at
/// `0x801EF424`, which abandons a one-input match outright - a blanked
/// combo is zero-terminated at byte 0, so it can only ever complete at
/// length 1. That guard is retail's own mechanism for the same job: the
/// Super and Miracle **finisher** rows all carry a single-`D` combo and
/// are unreachable for exactly this reason. The weaker argument - that
/// the `token - 0x0B` compare at `0x801EF3EC` needs a `0x0B` queue token
/// and `0x0B` is `BlockAnim`, not an input - is a second line of defence
/// and was not proved exhaustively over every queue writer.
pub(super) fn retained_bank_rows(
    character: legaia_art::queue::Character,
    cap: u8,
    host_row: usize,
    bank_len: usize,
) -> std::collections::BTreeSet<usize> {
    let mut keep = super_critical_rows(character);
    keep.insert(MIRACLE_BANK_ROW);
    keep.insert(host_row);
    for row in MIRACLE_BANK_ROW..bank_len {
        if (row - MIRACLE_BANK_ROW) <= cap as usize {
            keep.insert(row);
        }
    }
    keep.retain(|&r| r < bank_len);
    keep
}

/// Menu labels for the sibling's swing clips, in the archive order
/// [`legaia_asset::party_swap::moveset::swing_entries`] returns.
///
/// [`LABEL_MAX`] bytes at most, for every sibling. The SCUS arts-name
/// field is rewritten in place over the retail string plus its measured
/// NUL padding, the tightest field any retained art carries is Vahn's
/// "Cyclone", and the mapping is a free permutation - so a label sized
/// against the slot its sibling usually lands in would silently keep the
/// retail name under a rearranged party.
/// Longest menu label that fits every retained art's name field on the
/// USA disc. Vahn's "Cyclone" is the binding one: seven string bytes and
/// one byte of NUL padding, and a replacement needs one of those for its
/// own terminator.
pub(super) const LABEL_MAX: usize = 7;

pub(super) fn swing_labels(sibling: Sibling) -> &'static [&'static str] {
    match sibling {
        Sibling::Gi => &["Gi Cut", "Gi Chop", "Gi Ram", "Gi Rush"],
        Sibling::Che => &["Che Ram", "Che Jab", "Che Hit", "Che Arm"],
        Sibling::Lu => &["Lu Bolt", "Lu Zap", "Lu Jolt", "Lu Volt"],
    }
}

/// Clamp the base-archive loop windows to the stream the loop actually
/// addresses at runtime - the "Spirit streak" guard.
///
/// The Spirit charge loop is the base-archive record `0x11`: loop count
/// `+0x84 = 0xFF` over window `[+0x85, +0x86)`, stream source `0`. At
/// commit the runtime materializes the stream by decoding entry
/// `stream_source` out of whichever readef archive is RESIDENT in the
/// side-band streaming buffer (`FUN_8002b28c(_DAT_8007BD74, ..)`), and it
/// routinely commits with the MAIN archive resident - measured live on
/// retail and on a patched disc alike. The loop window then addresses rows
/// of the MAIN archive's entry `stream_source` up to `+0x86 - 1`,
/// regardless of that stream's real frame count. A row past the decoded
/// body reads virgin materialize scratch - all zeros - and an all-zero
/// pose row collapses every part onto the model origin, which the charge
/// close-up camera sits on: the GTE near-projection smears the hand /
/// fused-weapon prims across the screen. Retail dodges it only when its
/// scratch happens to hold sane stale rows there.
///
/// The guard clamps `+0x86` to the aliased main entry's frame count (and
/// `+0x85` under it), so no phantom row is ever addressed; the decoder's
/// own `frame == +0x86 - 1` arm then routes the interpolation partner to
/// `+0x85`, so the last in-window frame never reads one-past-the-end
/// either. The frames clamped away are duplicates of the held charge
/// pose, so the charge looks identical when the correct base-archive
/// stream is resident.
pub(super) fn clamp_charge_loop_windows(
    patcher: &mut DiscPatcher,
    slot: usize,
    who: &str,
) -> Result<Vec<String>> {
    use legaia_asset::battle_char_assembly;
    use legaia_asset::party_swap::moveset;

    let character = slot_character(slot);
    let index = crate::arts::player_entry_index(character);
    let entry = patcher
        .read_entry(index)
        .with_context(|| format!("read player file PROT {index}"))?;
    let rec0 = battle_char_assembly::decode_record0(&entry)
        .with_context(|| format!("decode {who} record0"))?;
    let bank = battle_char_assembly::art_animation_bank(&rec0)
        .with_context(|| format!("{who} art bank"))?;
    let readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .context("read readef.DAT")?;
    let me_off = battle_char_assembly::art_me_slot(slot, false) * winpose::READEF_SLOT;
    let me = readef
        .get(me_off..me_off + winpose::READEF_SLOT)
        .ok_or_else(|| anyhow::anyhow!("readef art slot for {who} out of range"))?;
    let main_frames = moveset::entry_frames(me).context("read the main stream frame counts")?;

    let mut edits: Vec<(usize, u8)> = Vec::new();
    let mut notes = Vec::new();
    for rec in &bank {
        // The Spirit charge loop only. The other base records share the
        // aliasing exposure in principle, but none has been observed to
        // materialize mis-resident, and clamping them would degrade their
        // real loop holds; the charge's clamped-away frames are duplicates
        // of the held pose, so it alone is free to guard.
        if !rec.uses_base_archive() || rec.anim_id != 0x11 {
            continue;
        }
        let Some(&aliased) = main_frames.get(rec.stream_source as usize) else {
            continue;
        };
        let cap = aliased.min(u8::MAX as usize) as u8;
        let e = rec.entry_offset;
        let (Some(&lo), Some(&hi)) = (rec0.get(e + 0x85), rec0.get(e + 0x86)) else {
            continue;
        };
        if rec.rate_alt == 0 || hi == 0 || cap < 2 || hi <= cap {
            continue;
        }
        let new_lo = lo.min(cap - 1);
        edits.push((e + 0x85, new_lo));
        edits.push((e + 0x86, cap));
        notes.push(format!(
            "{who} anim {:#04x}: charge-loop window [{lo}, {hi}) clamped to \
             [{new_lo}, {cap}) - the aliased main stream {} carries {aliased} rows",
            rec.anim_id, rec.stream_source
        ));
    }
    if edits.is_empty() {
        return Ok(notes);
    }
    let (lzs_off, recompressed) = crate::arts::patch_player_record0_full(&entry, &[], &edits)
        .ok_or_else(|| {
            anyhow::anyhow!("{who}'s record0 will not fit with the charge-loop guard applied")
        })?;
    patcher
        .patch_prot_entry(index, lzs_off as u64, &recompressed)
        .context("write the charge-loop guarded art bank")?;
    Ok(notes)
}

/// Rebuild one hero slot's whole art kit around the mapped sibling.
///
/// Runs after [`reskin_signature_art`], and depends on it: the signature
/// stream it built is carried into the new archive byte-identical, so
/// every frame-indexed field that pass tuned stays valid.
///
/// Four coordinated edits, all same-size:
///
/// 1. the main `"ME"` slot is re-authored from the sibling's motions
///    ([`legaia_asset::party_swap::moveset`]) - the retail streams are
///    dropped, which is the only way Noa's slot (2446 free bytes) has
///    room for anything new;
/// 2. every art record that reads that archive is repointed at one of
///    the new streams and re-timed to its rate, with the record's
///    frame-indexed hit list and effect-script gates rescaled from the
///    stream it used to read;
/// 3. the host's impact-effect class and mid-clip loop hold are cleared
///    (both are keyed to choreography that no longer exists), and every
///    inline record name becomes the label of the clip it now plays -
///    which is also what makes the block fit, since a handful of
///    repeated strings compress far better than 22 distinct ones;
/// 4. the arts outside [`retained_bank_rows`] have their combos blanked.
pub(super) fn apply_delilas_moveset(
    patcher: &mut DiscPatcher,
    ctx: &SignatureCtx<'_>,
) -> Result<Vec<String>> {
    use legaia_asset::battle_char_assembly;
    use legaia_asset::party_swap::moveset;

    let &SignatureCtx {
        slot,
        sibling,
        rig,
        retail_player,
        archive,
        natural_wrist_hand,
        ..
    } = ctx;
    let who = ["Vahn", "Noa", "Gala"][slot];
    let character = slot_character(slot);
    let mut notes = Vec::new();

    let index = crate::arts::player_entry_index(character);
    let entry = patcher
        .read_entry(index)
        .with_context(|| format!("read player file PROT {index}"))?;
    let rec0 = battle_char_assembly::decode_record0(&entry)
        .with_context(|| format!("decode {who} record0"))?;
    let bank = battle_char_assembly::art_animation_bank(&rec0)
        .with_context(|| format!("{who} art bank"))?;

    // The signature row, found by the combo the reskin just wrote.
    let host_combo: Vec<u8> = host_art(slot)
        .ok_or_else(|| anyhow::anyhow!("no {who}-slot host art"))?
        .combo
        .iter()
        .map(|c| c.as_byte())
        .collect();
    let host = bank
        .iter()
        .find(|r| !r.uses_base_archive() && r.combo == host_combo)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{who}'s signature combo is not in the art bank - the reskin did not land"
            )
        })?;
    let host_row = host.index;
    let signature_stream = host.stream_source as usize;

    let overlay = patcher
        .read_entry(BATTLE_OVERLAY_ENTRY)
        .context("read the battle-action overlay")?;
    let cap = innate_art_cap(&overlay, slot)?;
    let keep = retained_bank_rows(character, cap, host_row, bank.len());

    // Re-author the stream archive from the sibling's own clips.
    let source_id = sibling.monster_id();
    let anims = moveset::sibling_animations(archive, source_id)?;
    let chain = signature_clip_chain(sibling);
    let swings = moveset::swing_entries(&anims, chain);
    let approach = moveset::approach_entry(&anims);
    let readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .context("read readef.DAT")?;
    let me_off = battle_char_assembly::art_me_slot(slot, false) * winpose::READEF_SLOT;
    let me = readef
        .get(me_off..me_off + winpose::READEF_SLOT)
        .ok_or_else(|| anyhow::anyhow!("readef art slot for {who} out of range"))?;
    let old_frames = moveset::entry_frames(me).context("read the retail stream frame counts")?;
    let rebuilt = moveset::rebuild_moveset_archive(
        me,
        signature_stream,
        &anims,
        approach,
        &swings,
        rig,
        retail_player,
        archive,
        source_id,
        natural_wrist_hand,
    )
    .with_context(|| {
        format!(
            "rebuild {who}'s art streams from {}",
            sibling.display_name()
        )
    })?;

    // Repoint / re-time / rename every record that reads that archive.
    // Nothing is written until BOTH halves are known to fit: a rebuilt
    // archive whose records still hold their retail stream indices would
    // send most of them past the end of it.
    let labels = swing_labels(sibling);
    let mut offset_edits: Vec<(usize, u8)> = Vec::new();
    let mut assignments: Vec<(usize, usize, &'static str)> = Vec::new();
    let mut blanked = Vec::new();
    let mut nth = 0usize;
    for rec in &bank {
        if rec.uses_base_archive() {
            continue;
        }
        let record_off = rec.entry_offset - battle_char_assembly::ART_ENTRY_OFFSET;
        if rec.index == host_row {
            // The signature keeps the stream the reskin authored; only
            // its index moved.
            offset_edits.push((record_off + 0x0A, rebuilt.signature as u8));
            continue;
        }
        if rec.index < MIRACLE_BANK_ROW {
            // The combo starters: the sibling's locomotion clip, which is
            // what a step-in before a chain wants.
            let stream = &rebuilt.streams[rebuilt.approach];
            offset_edits.push((record_off + 0x0A, rebuilt.approach as u8));
            offset_edits.push((rec.entry_offset + 0x78, stream.rate));
            continue;
        }
        let swing = rebuilt.swing_for(nth);
        let label = labels[(nth % rebuilt.swings.len()).min(labels.len() - 1)];
        nth += 1;
        let stream = &rebuilt.streams[swing];
        let from = old_frames
            .get(rec.stream_source as usize)
            .copied()
            .unwrap_or(0);
        offset_edits.push((record_off + 0x0A, swing as u8));
        offset_edits.push((rec.entry_offset + 0x78, stream.rate));
        // The host's element spark / afterimage tint, and the mid-clip
        // loop hold - both keyed to choreography that is gone.
        offset_edits.push((rec.entry_offset + IMPACT_CLASS_OFFSET, 0));
        for k in [0x84usize, 0x85, 0x86] {
            offset_edits.push((rec.entry_offset + k, 0));
        }
        // Frame-indexed fields, rescaled from the stream the record used
        // to read onto the one it reads now.
        for i in 0..4 {
            let f = rec.effect_script.get(0x10 + i).copied().unwrap_or(0);
            if f != 0 {
                offset_edits.push((
                    rec.entry_offset + 0x10 + i,
                    rescale_frame(f, from, stream.frames),
                ));
            }
        }
        for i in 0..FX_RECORDS {
            let at = FX_BASE + i * FX_RECORD;
            let gate = rec.effect_script.get(at).copied().unwrap_or(0);
            if gate != 0 {
                offset_edits.push((
                    rec.entry_offset + at,
                    rescale_frame(gate, from, stream.frames),
                ));
            }
        }
        // The inline name: the clip's label, NUL-padded over the retail
        // string. Repetition here is what buys the LZS margin.
        let mut field = label.as_bytes().to_vec();
        field.resize(NAME_FIELD.len(), 0);
        offset_edits.extend(
            field
                .iter()
                .enumerate()
                .map(|(i, &b)| (record_off + NAME_FIELD.start + i, b)),
        );
        // A retail row whose combo is a single direction is already
        // unmatchable (`0x801EF424` rejects a one-input match outright)
        // - those are the Super and Miracle finisher rows. Blanking them
        // too is free and compresses, but only a real multi-input art
        // counts as one this mode hid.
        let is_art = rec.combo.len() >= 2;
        if !keep.contains(&rec.index) {
            offset_edits.extend(COMBO_FIELD.map(|i| (record_off + i, 0u8)));
            if is_art {
                blanked.push(rec.index);
            }
        } else if is_art {
            assignments.push((rec.index, swing, label));
        }
    }

    let (lzs_off, recompressed) =
        crate::arts::patch_player_record0_full(&entry, &[], &offset_edits).ok_or_else(|| {
            anyhow::anyhow!(
                "{who}'s record0 will not fit its LZS footprint with the Delilas \
                 moveset applied"
            )
        })?;
    let region = crate::arts::record0_lzs_region(&entry)
        .ok_or_else(|| anyhow::anyhow!("{who} record0 LZS region"))?;

    // Both halves fit - commit them together.
    patcher
        .patch_prot_entry(READEF_ENTRY, me_off as u64, &rebuilt.bytes)
        .context("write the rebuilt art-stream archive")?;
    patcher
        .patch_prot_entry(index, lzs_off as u64, &recompressed)
        .context("write the repointed art bank")?;
    notes.push(format!(
        "{who} moves: {} stream(s) rebuilt from {}'s own clips - the signature, \
         the approach and {} swing(s) - {} B of {} used",
        rebuilt.streams.len(),
        sibling.display_name(),
        rebuilt.swings.len(),
        rebuilt.used,
        winpose::READEF_SLOT
    ));
    notes.push(format!(
        "{who} record0: {} B of {} used ({} B spare)",
        recompressed.len(),
        region.avail,
        region.avail - recompressed.len()
    ));

    // The menu side: each surviving art is named after the clip it plays.
    match rename_retained_arts(patcher, character, &assignments) {
        Ok((n, 0)) => notes.push(format!("{who} art names: {n} renamed after their clip")),
        Ok((n, skipped)) => notes.push(format!(
            "{who} art names: {n} renamed after their clip, {skipped} too tight \
             for a {LABEL_MAX}-byte label and left retail"
        )),
        Err(e) => notes.push(format!("{who} art names: left retail ({e:#})")),
    }
    notes.push(format!(
        "{who} arts: {} performable (the signature, the Miracle, {} Super component(s) \
         and the innate block below cap {cap}), {} blanked out of the matcher",
        assignments.len() + 1,
        super_critical_rows(character).len(),
        blanked.len()
    ));
    let mut per_clip: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (_, _, label) in &assignments {
        *per_clip.entry(label).or_default() += 1;
    }
    notes.push(format!(
        "{who} clips: {}",
        per_clip
            .iter()
            .map(|(l, n)| format!("{l} x{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(notes)
}

/// Rewrite the SCUS arts-name string of each retained art to the label of
/// the sibling clip it plays. Same-size, in place, through each record's
/// own name pointer; a label that will not fit its field is skipped.
pub(super) fn rename_retained_arts(
    patcher: &mut DiscPatcher,
    character: legaia_art::queue::Character,
    assignments: &[(usize, usize, &'static str)],
) -> Result<(usize, usize)> {
    let scus = patcher
        .read_named_file(crate::arts::SCUS_NAME)
        .ok_or_else(|| anyhow::anyhow!("SCUS_942.54 not found"))?;
    let records = legaia_art::arts_table::raw_records_from_scus(&scus)
        .ok_or_else(|| anyhow::anyhow!("arts-name table not parseable"))?;
    let mut written = std::collections::BTreeSet::new();
    let mut n = 0usize;
    let mut skipped = 0usize;
    for &(row, _, label) in assignments {
        let id = row - MIRACLE_BANK_ROW;
        let Some(rec) = records
            .iter()
            .find(|r| r.character == character && !r.is_miracle && r.index as usize == id)
        else {
            continue;
        };
        let Some(field) = legaia_art::arts_table::name_field(&scus, rec.record_file_offset) else {
            continue;
        };
        // The field is the string plus its measured NUL padding, and a
        // replacement needs one byte of that for its own terminator - so
        // a label may be longer than the retail name it covers. A name
        // string can also be shared between records; the first
        // assignment in bank order wins, so the write stays deterministic.
        if !written.insert(field.file_offset) {
            continue;
        }
        if label.len() + 1 > field.budget {
            skipped += 1;
            continue;
        }
        let mut bytes = label.as_bytes().to_vec();
        bytes.resize(field.budget, 0);
        patcher
            .patch_named_file(crate::arts::SCUS_NAME, field.file_offset as u64, &bytes)
            .context("write a retained art's name")?;
        n += 1;
    }
    Ok((n, skipped))
}
