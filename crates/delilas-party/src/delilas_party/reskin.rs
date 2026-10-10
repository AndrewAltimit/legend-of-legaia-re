//! Re-skinning a signature art onto the host slot, and the attack-camera arm retiming / swap.
//! Split out of `delilas_party.rs`.

use super::*;

/// Reskin one hero slot's Hyper art as the sibling mapped onto it.
///
/// Four coordinated edits: a same-length name swap in the SCUS
/// arts-name table (menu + battle banner), a fresh 5-input combo
/// written to both copies retail keeps in sync (the SCUS display glyphs
/// and the player-file record0 matcher), the sibling's own monster clip
/// retargeted onto the player rig into the host art's "ME" stream (host
/// rate byte halved so a clip resampled into the host's shorter stream
/// keeps its authored pace), and the fanfare duration table extended so
/// the sibling's soundtrack - where one was captured - plays to
/// completion.
///
/// Must run while record0 still holds the VANILLA combo bytes (the
/// playerize rebuild keeps record0 verbatim, so ordering after it is
/// fine).
pub(super) fn reskin_signature_art(
    patcher: &mut DiscPatcher,
    ctx: &SignatureCtx<'_>,
    cave_taken: &mut bool,
) -> Result<Vec<String>> {
    use legaia_art::arts_table;
    use legaia_art::queue::Command;
    let &SignatureCtx {
        slot,
        sibling,
        rig,
        retail_player,
        archive,
        natural_wrist_hand,
    } = ctx;
    let who = ["Vahn", "Noa", "Gala"][slot];
    let character = slot_character(slot);
    let Some(art) = host_art(slot) else {
        return Ok(vec![format!(
            "{}'s signature art: no {who}-slot host art wired yet (skipped)",
            sibling.display_name()
        )]);
    };
    let mut notes = Vec::new();

    let scus = patcher
        .read_named_file(crate::arts::SCUS_NAME)
        .context("read SCUS for the art rename")?;
    let old = art.retail_name;
    let new = signature_name(sibling);
    let edits =
        crate::arts::ArtsEdits::locate(patcher.image()).context("locate arts-name table")?;
    let target = edits
        .records()
        .iter()
        .find(|r| r.character == character && r.index == art.index && !r.is_miracle)
        .cloned()
        .with_context(|| {
            format!(
                "{who} art index {} ({}) not found",
                art.index,
                String::from_utf8_lossy(old)
            )
        })?;

    // 1. Name: written through the record's own `+0xC` pointer, never by
    // searching the image for the old text. The strings nest - searching
    // for "Hurricane" finds the "Hurricane Kick" that contains it - so a
    // text-driven renamer is one table row away from corrupting a
    // neighbour. The field is NUL-padded, so a shorter name is written
    // with the tail cleared to the old length.
    let field = legaia_art::arts_table::name_field(&scus, target.record_file_offset)
        .with_context(|| format!("locate the {who} art's name field"))?;
    let current = scus
        .get(field.file_offset..field.file_offset + field.len)
        .unwrap_or_default();
    if current != old {
        bail!(
            "{who} art index {} reads {:?}, expected {:?} - the host-art table is stale",
            art.index,
            String::from_utf8_lossy(current),
            String::from_utf8_lossy(old)
        );
    }
    if new.len() > old.len() || old.len() >= field.budget {
        bail!(
            "{:?} ({} B) does not fit the {who} art's {}-byte name field",
            String::from_utf8_lossy(new),
            new.len(),
            field.budget
        );
    }
    let mut name_bytes = new.to_vec();
    name_bytes.resize(old.len(), 0);
    patcher
        .patch_named_file(
            crate::arts::SCUS_NAME,
            field.file_offset as u64,
            &name_bytes,
        )
        .context("write art name")?;
    notes.push(format!(
        "art renamed: {} -> {}",
        String::from_utf8_lossy(old),
        String::from_utf8_lossy(new)
    ));

    // 2. Combo: fresh 5-input sequence, checked unique among the
    // character's own arts.
    let new_combo: Vec<Command> = art.combo.to_vec();
    for r in edits.records() {
        if r.character == character && r.cmd_ptr != target.cmd_ptr && r.commands == new_combo {
            bail!(
                "the {who}-slot combo collides with {who} art index {}",
                r.index
            );
        }
    }
    let layout = arts_table::combo_string_layout(&scus, target.cmd_ptr)
        .with_context(|| format!("decode {} combo layout", String::from_utf8_lossy(old)))?;
    let plan = vec![crate::arts::ComboEdit {
        cmd_ptr: target.cmd_ptr,
        direction_slots: layout.direction_slots.clone(),
        old_directions: layout.directions.clone(),
        new_directions: new_combo.clone(),
    }];
    // Matcher first (player record0), then the display glyphs. The
    // host bank record is found by its VANILLA combo bytes, and its rate
    // byte (entry +0x78) is re-timed in the SAME record0 rewrite so the
    // host's shorter stream, now carrying the sibling's longer clip,
    // still runs for the clip's authored duration.
    use legaia_asset::battle_char_assembly;
    let char_edits = edits.player_edits(&plan, character);
    let index = crate::arts::player_entry_index(character);
    let entry = patcher
        .read_entry(index)
        .with_context(|| format!("read player file PROT {index}"))?;
    let rec0 = battle_char_assembly::decode_record0(&entry)
        .with_context(|| format!("decode {who} record0"))?;
    let bank = battle_char_assembly::art_animation_bank(&rec0)
        .with_context(|| format!("{who} art bank"))?;
    let host = char_edits
        .first()
        .and_then(|(vanilla, _)| {
            bank.iter()
                .find(|r| !r.uses_base_archive() && r.combo == *vanilla)
        })
        .cloned();

    // `patch_player_record0_full` reports success when ANY of its edits
    // changed, so a combo needle that matches nothing is dropped in
    // silence while the offset edits still write - the art would then
    // display the new combo in the menu and still answer to the old one
    // in battle. Prove the needle exists first.
    for (vanilla, _) in &char_edits {
        let hits = rec0
            .windows(vanilla.len() + 1)
            .filter(|w| &w[..vanilla.len()] == vanilla.as_slice() && w[vanilla.len()] == 0)
            .count();
        if hits == 0 {
            bail!(
                "{who} record0 carries no {} combo to rewrite - the matcher \
                 would keep answering to the retail input",
                combo_str(&target.commands)
            );
        }
    }

    // The sibling's signature clip, and the readef shape it has to fit -
    // both needed before the record0 write, because the re-timed rate is
    // a function of the two.
    let source_id = sibling.monster_id();
    let chain_entries = signature_clip_chain(sibling);
    let sibling_clips = monster_archive::animations(archive, source_id)
        .with_context(|| format!("read monster {source_id} animations"))?
        .unwrap_or_default();
    let chain: Vec<&monster_archive::MonsterAnimation> = chain_entries
        .iter()
        .filter_map(|&i| sibling_clips.get(i))
        .collect();
    // The payoff stage - what the art record's hit timing has to line up
    // with, and the shape check's subject.
    let clip = chain.last().map(|c| (*c).clone());
    let readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .context("read readef.DAT")?;
    let me_slot_idx = battle_char_assembly::art_me_slot(slot, false);
    let me_off = me_slot_idx * winpose::READEF_SLOT;
    // 3. The sibling's own choreography, retargeted onto the player rig
    // into the host art's "ME" stream. Rebuilt BEFORE the record0 write,
    // because the frame count it lands on is what every frame-indexed
    // field of the art record has to be rescaled against. Non-fatal: a
    // failed rebuild leaves the host animation (with a note).
    let rebuilt = (|| -> Result<winpose::RebuiltArtSlot> {
        let h = host
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("host art bank record not found by vanilla combo"))?;
        if chain.len() != chain_entries.len() {
            bail!("monster {source_id} is missing a stage of {chain_entries:?}");
        }
        for (i, c) in chain_entries.iter().zip(&chain) {
            if c.part_count != party_swap::CANONICAL_PARTS {
                bail!(
                    "monster {source_id} entry {i} has {} parts, expected {}",
                    c.part_count,
                    party_swap::CANONICAL_PARTS
                );
            }
        }
        let me = readef
            .get(me_off..me_off + winpose::READEF_SLOT)
            .ok_or_else(|| anyhow::anyhow!("readef art slot {me_slot_idx} out of range"))?;
        winpose::rebuild_art_slot_entry(
            me,
            h.stream_source as usize,
            &chain,
            rig,
            retail_player,
            archive,
            source_id,
            natural_wrist_hand,
        )
    })();
    let rebuilt = match rebuilt {
        Ok(r) => {
            patcher
                .patch_prot_entry(READEF_ENTRY, me_off as u64, &r.bytes)
                .context("write the retargeted art stream")?;
            let dropped = chain_entries.len() - r.stages;
            notes.push(format!(
                "{} animation: {}'s own {}-stage chain {:?}{}, {} frames \
                 (host stream carried {})",
                String::from_utf8_lossy(new),
                sibling.display_name(),
                r.stages,
                &chain_entries[dropped..],
                if dropped > 0 {
                    format!(" ({dropped} wind-up stage(s) dropped - slot too tight)")
                } else {
                    String::new()
                },
                r.frames,
                r.retail_frames
            ));
            Some(r)
        }
        Err(e) => {
            notes.push(format!(
                "{} animation stays the host's ({e:#})",
                String::from_utf8_lossy(new)
            ));
            None
        }
    };

    // Everything the art record says about WHEN things happen is an index
    // into the stream that just changed under it, so each frame-indexed
    // field is rescaled by the same ratio.
    let mut offset_edits = Vec::new();
    if let (Some(h), Some(c), Some(r)) = (&host, &clip, &rebuilt) {
        // The chain is written at the rate its stages were stretched to,
        // except on the retail-shape fallback, where it has to be
        // re-timed the old way.
        let rate = if r.frames == r.retail_frames && r.stages == 1 {
            winpose::retimed_rate(r.frames, c.frame_count, c.rate)
        } else {
            r.rate
        };
        offset_edits.push((h.entry_offset + 0x78, rate));
        notes.push(format!(
            "{} pace: {} frames at rate {}",
            String::from_utf8_lossy(new),
            r.frames,
            rate
        ));
        // One clock for the whole move: the frames the sibling's chain
        // actually connects on drive both the damage and the burst.
        let contacts = chain_contacts(&chain, r);
        let (hits, why) = retimed_hit_frames(h, r, &contacts);
        offset_edits.extend(hits);
        notes.push(format!("{} hits: {why}", String::from_utf8_lossy(new)));

        // The real enemy-side burst, when the cave is still free: one of
        // the signature cast module's own effect records, transplanted
        // into a spare prototype slot so the art's one-byte effect id can
        // name it. Non-fatal - without it the borrowed cast projectile
        // stands.
        let burst = match crate::delilas_effects::plan(patcher, sibling, *cave_taken) {
            Ok(Some(p)) => {
                let note = crate::delilas_effects::apply(patcher, &p)
                    .context("install the transplanted burst record")?;
                *cave_taken = true;
                notes.push(format!("{} burst: {note}", String::from_utf8_lossy(new)));
                Some(p.effect_id)
            }
            Ok(None) => None,
            Err(e) => {
                notes.push(format!(
                    "{} burst: not transplanted ({e:#})",
                    String::from_utf8_lossy(new)
                ));
                None
            }
        };

        let (fx, why) = effect_script_edits(h, c, &sibling_clips, r, burst, &contacts);
        offset_edits.extend(fx);
        notes.push(format!("{} effects: {why}", String::from_utf8_lossy(new)));

        // Drop the mid-clip loop hold. Entry +0x84 seeds a hold counter
        // and +0x85/+0x86 bound the window it replays: Vahn's Burning
        // Flare holds frames 9-10 five times, which is its multi-hit
        // flurry and is nonsense over someone else's choreography (and
        // meaningless anyway once the frame count moves).
        //
        // Safe because +0x84 is a hold, not the rate the sibling doc
        // comment on `ArtAnimRecord::rate_alt` reads it as. Census over
        // all three player files: it is 0 on every playable art record
        // but five, including records whose clips run at rate 3, 4 and
        // 7 - a rate field of 0 would freeze them. The rate is +0x78,
        // and each of the five holds bounds a window strictly inside
        // its own clip. 0xFF stays the base-archive marker; the hosts
        // are 5 / 0 / 0, so none of them is one.
        for k in [0x84usize, 0x85, 0x86] {
            offset_edits.push((h.entry_offset + k, 0));
        }

        // Drop the host's impact-effect class. Entry `+0x7A` is a 1..5
        // selector that `FUN_801EC3E4` copies into `actor[+0x21F]` (and
        // whose config row it copies into `actor[+0x04]`), and it drives
        // TWO renderers that both sit OUTSIDE the art's 8-record effect
        // script - which is why rewriting that script does not silence
        // them:
        //
        //   - `FUN_8004998C` streams an element spark along the swing
        //     path at random cadence, `efect.dat` sprite 0x0B for
        //     selector 1 and 0x10 for selector 2;
        //   - `FUN_80049348` draws afterimage copies of the mesh tinted
        //     from a per-CHARACTER table (`0x80076908 + (char-1)*4`),
        //     fading per copy.
        //
        // Vahn's Burning Flare is the only host art of the three that
        // sets it (`1`; Vulture Blade and Explosive Fist are both `0`),
        // so a sibling in Vahn's slot wore his fire sparks and his
        // afterimage tint through every rewrite the swap makes. That is
        // the "Vahn's fire took over" report.
        //
        // Zeroed rather than re-pointed at the sibling's own element:
        // selector 2 would give Lu the lightning-class spark, but it
        // also switches the afterimages on, and those take their colour
        // from the character table, not the art - so it would trade the
        // host's sparks for the host's ghosts. Removing what is wrong is
        // measured; adding what is right needs a frame capture first.
        offset_edits.push((h.entry_offset + IMPACT_CLASS_OFFSET, 0));
    }
    // The battle idle rides the SAME record0 write - it is the only
    // record0 edit that adds bytes, so batching it means one LZS re-fit
    // instead of two that each have to clear the footprint alone.
    match winpose::rebuild_idle_stream(retail_player, rig, archive, source_id, natural_wrist_hand) {
        Ok(idle) => {
            offset_edits.extend(
                idle.bytes
                    .iter()
                    .enumerate()
                    .map(|(i, &b)| (idle.offset + i, b)),
            );
            notes.push(format!(
                "{who} idle: {}'s own combat stance over {} frames, cycling {:.2}x its authored speed",
                sibling.display_name(),
                idle.frames,
                idle.pace
            ));
        }
        Err(e) => notes.push(format!("{who} idle: stays the host's ({e:#})")),
    }

    if !char_edits.is_empty() || !offset_edits.is_empty() {
        // `None` here is indistinguishable from "nothing needed
        // changing", and the combo needle was proven present above, so
        // at this point it can only mean the recompressed block missed
        // its LZS footprint. Failing loudly matters more than usual:
        // a silent skip would leave the art displaying its new combo in
        // the menu while still answering to the old one in battle.
        let (lzs_off, recompressed) =
            crate::arts::patch_player_record0_full(&entry, &char_edits, &offset_edits).ok_or_else(
                || {
                    anyhow::anyhow!(
                        "{who}'s record0 will not fit its LZS footprint with the \
                         signature-art and idle edits applied"
                    )
                },
            )?;
        patcher
            .patch_prot_entry(index, lzs_off as u64, &recompressed)
            .context("write player record0 combo matcher + anim rate")?;
    }
    for (off, bytes) in edits.glyph_patches(&plan) {
        patcher
            .patch_named_file(crate::arts::SCUS_NAME, off, &bytes)
            .context("write combo display glyph")?;
    }
    notes.push(format!(
        "{} combo: {} (was {})",
        String::from_utf8_lossy(new),
        combo_str(&new_combo),
        combo_str(&target.commands)
    ));

    // 3a2 (retired). The sibling's spell-table row (0x79/0x7A/0x7B) once
    // took the host art's retail name so the Nivora duel's mirrored-hero
    // CAST announced the hero art. Two later changes inverted the
    // ownership: the enemy-side signature is now a physical attack
    // (`delilas_signature_attack` rewrites the AI picker's cast arm in
    // place, so no enemy ever casts these ids), and the state-0x28
    // spell-name label is un-gated for player Magic casts
    // (`delilas_cast::install_cast_label_gate`), which reads exactly this
    // row when the converted signature fires. The retail bytes - the
    // sibling special's own name - are what that banner must show, so
    // the row is left retail.

    // 3b. The swing camera. Not a retarget - a re-time. See
    // [`retime_camera_arm`] for why the arm the art already dispatches to
    // is the right one to edit and the wrong one to replace.
    if let Some(r) = &rebuilt {
        match retime_camera_arm(patcher, slot, &art, r) {
            Ok(why) => notes.push(format!("{} camera: {why}", String::from_utf8_lossy(new))),
            Err(e) => notes.push(format!(
                "{} camera: left retail ({e:#})",
                String::from_utf8_lossy(new)
            )),
        }
    }

    // 4. Fanfare duration: the art's pair channels are SILENCED (the
    // cast bed carries the special's audio - see `delilas_xa_voice`),
    // so the duration rows shrink to a token 0.1 s: the entry is
    // CENTISECONDS of channel audio (measured against retail across 24
    // ids; `dur = entry * 0.6` is a 60 Hz tick budget, not the
    // 75-sectors/s physical span an earlier reading assumed). A silent
    // fire that held the retail 3-7 s span would occupy the guarded XA
    // system and swallow any shout fired inside it; 0.1 s releases it
    // immediately. The table is indexed by jingle id - 0x100; the rows
    // are the art's `base_id` pair, NOT a fixed {4, 7}.
    if let Some(fanfare) = signature_fanfare(slot) {
        let toff = legaia_art::hyper_fanfare::dur_table_file_offset(&scus)
            .ok_or_else(|| anyhow::anyhow!("fanfare duration table not found in SCUS"))?;
        let entry_val = 10u16.to_le_bytes();
        let base = (fanfare.base_id - 0x100) as usize;
        for n in [base, base + 3] {
            patcher
                .patch_named_file(crate::arts::SCUS_NAME, (toff + n * 2) as u64, &entry_val)
                .context("write fanfare duration")?;
        }
        notes.push(format!(
            "{} fanfare pair silenced; duration rows -> 0.1 s",
            String::from_utf8_lossy(new)
        ));
    }
    Ok(notes)
}

/// Bytes of one effect-script record, and where the eight of them start
/// inside an action entry (`[frame_gate, effect_id, i16 x, i16 y, i16 z]`,
/// walked by `FUN_801DEA50`).
pub(super) const FX_RECORD: usize = 8;
pub(super) const FX_BASE: usize = 0x14;
pub(super) const FX_RECORDS: usize = 8;

/// Action-entry offset of the impact-effect class byte: a 1..5 selector
/// (`0` = none) read by `FUN_801EC3E4`, which stores it at
/// `actor[+0x21F]` and its config row at `actor[+0x04]`. Both of the
/// renderers it drives - the swing-path element spark in `FUN_8004998C`
/// and the tinted afterimages in `FUN_80049348` - draw independently of
/// the art's effect script.
pub(super) const IMPACT_CLASS_OFFSET: usize = 0x7A;

/// Spell id of a sibling's signature cast.
///
/// `FUN_801E9FD4`'s `0xA2`/`0xA3`/`0xA4` arms fire on the round counter
/// (`% 3 == 2`) and write `actor[+0x1DF] = monster_id - 0x29`, so Gi's
/// `162` becomes `0x79`, Che's `163` `0x7A` and Lu's `164` `0x7B`. The
/// subtraction is a literal in the raw battle overlay at file `0x1CFFC`
/// (`0x2442FFD7` = `addiu v0,v0,-0x29`).
pub(super) fn signature_spell_id(sibling: Sibling) -> u8 {
    (sibling.monster_id() - 0x29) as u8
}
/// PROT 0898 file offset of each character's attack-camera jump table
/// (`0x801CEA88` / `0x801CEAD0` / `0x801CEB20` less the overlay base) and
/// how many art constants it admits - the `sltiu` bounds at `0x801D72E0`
/// / `0x801D76C4` / `0x801D7B24`. Reading past them walks into the next
/// character's table.
pub(super) const CAMERA_TABLES: [(usize, usize); 3] = [(0x270, 17), (0x2B8, 20), (0x308, 17)];
/// Load base the overlay's own addresses are printed against.
pub(super) const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;
/// The shared epilogue every unused table slot points at. Not an arm.
pub(super) const CAMERA_EPILOGUE: u32 = 0x801D_828C;
/// `actor[+0x22C][+0x68]`, the animation cursor in sixteenths of a
/// keyframe - the displacement an arm loads it from.
pub(super) const CURSOR_DISP: u32 = 0x0068;

/// Re-time the attack camera to the length of the swing it now films.
///
/// Each arm is a cascade of `slti` tests on the animation cursor
/// (`actor[+0x22C][+0x68]`, sixteenths of a keyframe), which is how a
/// swing gets several framings instead of one: Gala's Explosive Fist arm
/// changes shot at keyframes 4, 7 and 10, Noa's Vulture Blade arm at 14.
/// Those thresholds are literals sized for the retail clip - the highest
/// in the whole dispatcher is keyframe 17 - and
/// the signature chains run 75 to 100 frames - so the camera finishes its
/// whole choreography inside the wind-up and then holds one shot for the
/// rest of the move. Scaling each threshold by the length ratio spreads
/// the same shots across the same *fractions* of the new swing.
///
/// The arm is edited, never replaced. Retargeting a table slot to a
/// better-choreographed arm is possible and was tried, but every arm in
/// the overlay is already live in some character's table, so a retarget
/// can only alias an arm that another art still uses - and the re-time
/// would then follow the alias into that art and mistune it. Editing the
/// arm the art already dispatches to keeps the blast radius at exactly
/// one (character, art constant) pair, which this checks rather than
/// assumes: the arm must be reachable from exactly one live table slot
/// across all three tables.
///
/// An arm with no cursor test (Vahn's Burning Flare arm reads only the
/// `ctx[+0x26E]` ramp) has no choreography to mistime and is left alone.
pub(super) fn retime_camera_arm(
    patcher: &mut DiscPatcher,
    slot: usize,
    art: &HostArt,
    rebuilt: &crate::party_swap::winpose::RebuiltArtSlot,
) -> Result<String> {
    let (frames, retail) = (rebuilt.frames, rebuilt.retail_frames);
    if retail == 0 || (frames == retail && rebuilt.stages == 1) {
        return Ok("unchanged (the swing is its retail shape)".into());
    }
    let (base, len) = *CAMERA_TABLES
        .get(slot)
        .ok_or_else(|| anyhow::anyhow!("no camera table for party slot {slot}"))?;
    let row = (art.action_constant as usize)
        .checked_sub(0x1A)
        .filter(|&r| r < len)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "art constant {:#04X} is outside the table",
                art.action_constant
            )
        })?;
    let mut swapped = String::new();
    if let Some(want) = art.camera_swap {
        swapped = swap_camera_arms(patcher, base + row * 4, want)?;
    }
    let overlay = patcher
        .read_entry(BATTLE_OVERLAY_ENTRY)
        .context("read the battle-action overlay")?;
    let word = |off: usize| -> Option<u32> {
        overlay
            .get(off..off + 4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
    };
    let arm = word(base + row * 4)
        .ok_or_else(|| anyhow::anyhow!("camera table row {row} out of range"))?;
    if arm == 0 || arm == CAMERA_EPILOGUE {
        return Ok("the art dispatches straight to the shared epilogue".into());
    }

    // Every live arm across all three tables: the exclusivity check, and
    // the address list that bounds this arm's body.
    let mut live: Vec<u32> = Vec::new();
    let mut uses = 0usize;
    for &(b, n) in &CAMERA_TABLES {
        for r in 0..n {
            let Some(w) = word(b + r * 4) else { continue };
            if w == arm {
                uses += 1;
            }
            if w != 0 && w != CAMERA_EPILOGUE {
                live.push(w);
            }
        }
    }
    if uses != 1 {
        bail!(
            "arm {arm:#010X} is reached from {uses} table slots, so re-timing it would mistune another art"
        );
    }
    live.sort_unstable();
    live.dedup();
    // The arms are laid out in dispatch order, so the next one up is this
    // one's end. The epilogue closes the last arm.
    let end_va = live
        .iter()
        .copied()
        .find(|&a| a > arm)
        .unwrap_or(CAMERA_EPILOGUE);
    let (start, end) = (
        (arm - BATTLE_OVERLAY_BASE) as usize,
        (end_va - BATTLE_OVERLAY_BASE) as usize,
    );
    if end <= start || end > overlay.len() {
        bail!("arm {arm:#010X} spans {start:#X}..{end:#X}, outside the overlay");
    }

    // Which register holds the cursor, and every `slti` against it.
    // Linear clobber tracking would be wrong here: the arms are branch
    // cascades, and the path that reaches a later test skips the block
    // that reuses the register, so the register is live on the path even
    // though a straight-line read says otherwise. The test is instead
    // shape-based - a `slti` (the dispatcher's own bounds checks are
    // `sltiu`, a different opcode) against a register some `lh`/`lhu`
    // loaded from `+0x68`, with a threshold in the keyframe range.
    let mut cursor_regs = [false; 32];
    let mut sites: Vec<(usize, u32)> = Vec::new();
    for off in (start..end).step_by(4) {
        let Some(w) = word(off) else { continue };
        let (op, rs, rt, imm) = (w >> 26, (w >> 21) & 0x1F, (w >> 16) & 0x1F, w & 0xFFFF);
        match op {
            // lh / lhu rt, 0x68(rs)
            0x21 | 0x25 if imm == CURSOR_DISP => cursor_regs[rt as usize] = true,
            // slti rt, rs, imm
            0x0A if cursor_regs[rs as usize] && (0x10..=0x400).contains(&imm) => {
                sites.push((off, imm))
            }
            _ => {}
        }
    }
    let Some(&last_shot) = sites.iter().map(|(_, i)| i).max() else {
        return Ok(format!(
            "{swapped}arm {arm:#010X} has no cursor-gated shot change to re-time"
        ));
    };
    // Anchor the LAST shot change on the frame the payoff stage begins,
    // and scale the earlier ones by the same factor so their spacing is
    // preserved. Anchoring beats scaling by the raw length ratio: the
    // final framing is the one that films the strike, so it should start
    // when the strike does, and a chain can be its host's length while
    // still opening with a wind-up the retail thresholds know nothing
    // about - Lu's two-stage strike is 58 frames either way, so a length
    // ratio of 1 would leave her final shot in the wind-up. The ratio is
    // the fallback for a stream with no wind-up to clear.
    let (num, den) = if rebuilt.payoff_start > 0 && last_shot > 0 {
        (rebuilt.payoff_start * 16, last_shot as usize)
    } else {
        (frames, retail)
    };
    let edits: Vec<(usize, u16, u16)> = sites
        .iter()
        .filter_map(|&(off, imm)| {
            let scaled = ((imm as usize * num + den / 2) / den).min(0x7FFF) as u16;
            (scaled != imm as u16).then_some((off, imm as u16, scaled))
        })
        .collect();
    if edits.is_empty() {
        return Ok(format!(
            "{swapped}arm {arm:#010X} is already timed for this swing"
        ));
    }
    let shots: Vec<String> = edits
        .iter()
        .map(|(_, o, n)| format!("kf {}->{}", o / 16, n / 16))
        .collect();
    for (off, _, scaled) in &edits {
        // The immediate is the instruction word's low halfword, and the
        // word is stored little-endian, so it is the two bytes AT the
        // instruction - not two bytes into it.
        patcher
            .patch_prot_entry(BATTLE_OVERLAY_ENTRY, *off as u64, &scaled.to_le_bytes()[..])
            .context("write a re-timed camera threshold")?;
    }
    Ok(format!(
        "{swapped}arm {arm:#010X} re-timed over {frames} frames (host had {retail}), \
         final shot on the strike at kf {}: {}",
        rebuilt.payoff_start,
        shots.join(", ")
    ))
}

/// Exchange a table slot's camera arm with another arm already live in
/// the dispatcher, giving every slot that held the wanted arm this
/// slot's own in return.
///
/// A plain retarget would leave the wanted arm reachable from two arts,
/// which is exactly the condition [`retime_camera_arm`] refuses to edit
/// under. The exchange keeps the set of live arms unchanged - only which
/// art dispatches to which - and leaves the wanted arm reachable from
/// this slot alone. Idempotent: a slot that already holds the wanted arm
/// is left alone, so a re-apply cannot swap the pair back.
pub(super) fn swap_camera_arms(
    patcher: &mut DiscPatcher,
    slot_off: usize,
    want: u32,
) -> Result<String> {
    let overlay = patcher
        .read_entry(BATTLE_OVERLAY_ENTRY)
        .context("read the battle-action overlay for the camera swap")?;
    let word = |off: usize| -> Option<u32> {
        overlay
            .get(off..off + 4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
    };
    let mine = word(slot_off).ok_or_else(|| anyhow::anyhow!("camera slot out of range"))?;
    if mine == want {
        return Ok(String::new()); // already swapped
    }
    let mut ceded = Vec::new();
    for &(b, n) in &CAMERA_TABLES {
        for r in 0..n {
            let off = b + r * 4;
            if off != slot_off && word(off) == Some(want) {
                ceded.push(format!("{:#04X}", 0x1A + r));
                patcher
                    .patch_prot_entry(BATTLE_OVERLAY_ENTRY, off as u64, &mine.to_le_bytes())
                    .context("cede the borrower's arm")?;
            }
        }
    }
    patcher
        .patch_prot_entry(BATTLE_OVERLAY_ENTRY, slot_off as u64, &want.to_le_bytes())
        .context("take the borrowed arm")?;
    Ok(format!(
        "took arm {want:#010X}, ceded {mine:#010X} to art(s) {}; ",
        ceded.join(", ")
    ))
}
