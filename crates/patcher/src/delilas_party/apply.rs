//! The party-swap patch driver (`apply_delilas_party_with`) and the per-slot element retarget.
//! Split out of `delilas_party.rs`.

use super::*;

/// [`apply_delilas_party`] with [`DelilasPartyOptions`] explicit.
pub fn apply_delilas_party_with(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    arts_voice: crate::delilas_voice_fx::ArtsVoiceMode,
    move_mode: DelilasMoveMode,
    cast_route: CastRoutePolicy,
    options: DelilasPartyOptions,
) -> Result<DelilasPartyReport> {
    let mut report = DelilasPartyReport::default();
    let archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .context("read monster archive")?;
    // Retail player files for all three heroes, captured before the
    // model loop patches them: the signature-art anim retarget must run
    // against the same retail rest/mesh statistics the win-pose
    // conversion uses.
    let mut retail_players: Vec<Vec<u8>> = Vec::with_capacity(3);
    for (entry, _, _, who, _) in mapping.pairs() {
        retail_players.push(
            patcher
                .read_entry_footprint(entry)
                .with_context(|| format!("read retail {who} player file"))?,
        );
    }
    // readef.DAT before any pass rewrites its ME slots: the enemy-side
    // anim mirror retargets the heroes' own clips (incl. the base-ME
    // victory flourish) into the swapped monster blocks.
    let retail_readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .context("read retail readef.DAT")?;

    // Baseline pass before any write: every target block's name must be
    // its retail sibling name (fresh) or its mapped character's (already
    // applied).
    let mut fresh = Vec::new();
    for (entry, _, _, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let name = monster_archive::record(&archive, id)?
            .map(|r| r.name)
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: empty slot"))?;
        if name == who {
            continue; // this pairing is already applied
        }
        if name != sibling.retail_block_name() {
            bail!(
                "monster id {id} is named {name:?} - neither retail \
                 ({:?}) nor swapped ({who:?}); refusing to touch an \
                 unrecognized build",
                sibling.retail_block_name()
            );
        }
        fresh.push(entry);
    }

    for (entry, rig, template_slot, who, sibling) in mapping.pairs() {
        if !fresh.contains(&entry) {
            continue;
        }
        let id = sibling.monster_id();
        let player_file = patcher
            .read_entry_footprint(entry)
            .with_context(|| format!("read player file PROT {entry}"))?;

        // Enemy side: the sibling's block wears the character's model
        // and name.
        let swapped = party_swap::swap_into_block(&player_file, rig, &archive, id)
            .with_context(|| format!("{who} -> monster {id}"))?;
        let mut block = swapped.block;
        rename_block(&mut block, who).with_context(|| format!("rename monster {id} to {who:?}"))?;
        let slot = monster_archive::encode_slot(&block)
            .with_context(|| format!("re-encode monster {id}"))?;
        patcher.patch_monster_slot(id, &slot)?;

        // Player side: the character wears the sibling's model.
        let entry_len = patcher
            .read_entry(entry)
            .with_context(|| format!("read PROT entry {entry}"))?
            .len();
        let keep_hammer = options.keep_che_hammer && id == Sibling::Che.monster_id();
        let playerized = playerize::playerize_player_file_with(
            &player_file,
            entry_len,
            rig,
            &archive,
            id,
            template_slot,
            Some(&patcher.read_entry_footprint(READEF_ENTRY)?),
            keep_hammer,
        )
        .with_context(|| format!("{who} <- monster {id}"))?;
        if keep_hammer {
            report
                .notes
                .push(format!("{who}: Che's welded hammer kept on the mesh"));
        }
        patcher.patch_prot_entry(entry, 0, &playerized.file)?;

        // Win poses: the character's eight base "ME" victory streams
        // (readef.DAT slot 3*char+2) rebuild from the sibling's own
        // victory clip, retargeted onto the player rig - the swapped
        // character celebrates like the Delilas they depict. Non-fatal:
        // a failed rebuild leaves the retail pose (with a note).
        match winpose::victory_clip(&archive, id).and_then(|clip| {
            let readef = patcher
                .read_entry_footprint(READEF_ENTRY)
                .context("read readef.DAT")?;
            let slot_idx = winpose::base_slot_index(template_slot);
            let off = slot_idx * winpose::READEF_SLOT;
            let slot = readef
                .get(off..off + winpose::READEF_SLOT)
                .ok_or_else(|| anyhow::anyhow!("readef slot {slot_idx} out of range"))?;
            let rebuilt = winpose::rebuild_base_slot(
                slot,
                &clip,
                rig,
                &player_file,
                &archive,
                id,
                party_swap::playerize::kept_welded_hand(id, keep_hammer),
            )?;
            patcher.patch_prot_entry(READEF_ENTRY, off as u64, &rebuilt)?;
            Ok(clip.action_id)
        }) {
            Ok(_) => report.notes.push(format!(
                "{who}: victory poses <- {}'s own victory clip",
                sibling.display_name()
            )),
            Err(e) => report
                .notes
                .push(format!("{who}: victory poses stay retail ({e:#})")),
        }

        // New-game template name (fixed 10-byte NUL-padded field; only
        // affects new games - existing saves keep their stored names).
        let scus = patcher
            .read_named_file(crate::steal::SCUS_NAME)
            .ok_or_else(|| anyhow::anyhow!("SCUS_942.54 not found"))?;
        let tmpl_off = new_game::party_template_file_offset(&scus)
            .ok_or_else(|| anyhow::anyhow!("starting-party template not found in SCUS"))?
            as u64;
        let name_off = tmpl_off + (template_slot * new_game::RECORD_STRIDE) as u64 + 16;
        let mut field = vec![0u8; new_game::NAME_LEN];
        field[..sibling.display_name().len()].copy_from_slice(sibling.display_name().as_bytes());
        patcher
            .patch_named_file(crate::steal::SCUS_NAME, name_off, &field)
            .with_context(|| format!("write template name for slot {template_slot}"))?;

        report.changed = true;
        for w in swapped.warnings.iter().chain(playerized.warnings.iter()) {
            report
                .notes
                .push(format!("{who} <-> {}: {w}", sibling.display_name()));
        }
    }

    // Field forms: rebuild PROT 0874 so the party walks around as the
    // mapped siblings too (built from the same monster models as the
    // battle side, so both forms match). Runs only alongside a fresh
    // apply - an already-swapped 0874 must not re-convert.
    if report.changed {
        // The element each slot now fights in. First of the post-model
        // passes because it is pure identity - it decides what every
        // attack of that slot deals and takes, not just the signature
        // art's, and nothing below reads it.
        report
            .notes
            .extend(retarget_character_elements(patcher, mapping, &archive)?);

        let field_entry = fieldize::PROT_ENTRY_INDEX;
        let prot_0874 = patcher
            .read_entry_footprint(field_entry)
            .context("read PROT 0874")?;
        let entry_len = patcher
            .read_entry(field_entry)
            .context("PROT 0874 length")?
            .len();
        let field_mapping = [
            mapping.vahn.monster_id(),
            mapping.noa.monster_id(),
            mapping.gala.monster_id(),
        ];
        // Preferred source: the siblings' own field NPC meshes (nilboa
        // duel scene) - retail-authored chibi geometry that fits the §0
        // budget at full detail. The battle-model conversion is the
        // fallback (it survives only via heavy decimation).
        let npc_pack = patcher.read_entry_footprint(fieldize::NPC_PACK_ENTRY)?;
        let npc_bundle = patcher.read_entry_footprint(fieldize::NPC_BUNDLE_ENTRY)?;
        let fieldized = fieldize::fieldize_pack_npc(
            &prot_0874,
            entry_len,
            &npc_pack,
            &npc_bundle,
            field_mapping,
        )
        .or_else(|npc_err| {
            report.notes.push(format!(
                "field: NPC-mesh source unavailable ({npc_err:#}); using battle-model conversion"
            ));
            fieldize::fieldize_pack(&prot_0874, entry_len, &archive, field_mapping)
        })
        .context("rebuild field forms (PROT 0874)")?;
        patcher.patch_prot_entry(field_entry, 0, &fieldized.entry)?;
        for w in &fieldized.warnings {
            report.notes.push(format!("field: {w}"));
        }

        // The nilboa duel scene's own Delilas NPC meshes become the
        // mapped heroes (PROT 0639 members 106/107/108 + the 0638 head
        // TIMs), so the ravine no longer shows two Delilas sets. Uses
        // the pre-fieldize PROT 0874 capture above - load-bearing: the
        // rewritten 0874 carries siblings, not heroes.
        let nivora = crate::nivora_field::apply_nivora_field(patcher, mapping, &prot_0874)
            .context("nilboa field mirror")?;
        report.notes.extend(nivora.notes);

        // The remaining Delilas event appearances (map stone, floating
        // castle, past Conkram) mirror the same way - their sibling
        // meshes live inside each scene's bundle instead of a separate
        // pack. Same pre-fieldize PROT 0874 dependency.
        let events = crate::nivora_field::apply_event_field(patcher, mapping, &prot_0874)
            .context("event-scene field mirrors")?;
        report.notes.extend(events.notes);

        // Save metadata wears the swap too: the save-select face and
        // the PSX card-block icon come off the portrait sheet (tile =
        // party id / card slot), and the boot load screen reads its own
        // standalone copies of tiles 0..2. Exchange each hero tile with
        // the mapped sibling's tile - the party's saves show the
        // siblings, and the sheet slots the siblings held now show the
        // heroes (they are this world's Delilas family).
        for (hero_tile, who, sibling) in [
            (0usize, "Vahn", mapping.vahn),
            (1, "Noa", mapping.noa),
            (2, "Gala", mapping.gala),
        ] {
            crate::save_icon::swap_slot_portraits(patcher, hero_tile, sibling.portrait_tile())
                .with_context(|| format!("save portrait {who} <-> {}", sibling.display_name()))?;
            report.notes.push(format!(
                "save portraits: {who}'s face tile now shows {} (and {}'s shows {who})",
                sibling.display_name(),
                sibling.display_name()
            ));
        }

        // Dialog follows the swap: any line that names a sibling now
        // names the hero who took that sibling's place in the duels.
        // "Delilas" itself stays - in this world Vahn, Noa and Gala ARE
        // the Delilas family, so "Gi Delilas: ..." reads e.g. "Noa
        // Delilas: ...". Word-boundary matches only ("Che" never
        // rewrites a "Chest"). Runs last: a scene MAN whose renamed
        // dialog no longer fits its compressed footprint is grown by
        // whole sectors, which relays the disc.
        {
            let renames = [
                (mapping.vahn.display_name(), "Vahn"),
                (mapping.noa.display_name(), "Noa"),
                (mapping.gala.display_name(), "Gala"),
            ];
            let mut pack =
                crate::translation::export_pack(patcher).context("export dialog corpus")?;
            let mut lines = 0usize;
            for entry in pack
                .sections
                .scene_dialog
                .iter_mut()
                .chain(pack.sections.inline_text.iter_mut())
            {
                let mut text = entry.source.clone();
                let mut hit = false;
                for (from, to) in renames {
                    if let Some(replaced) = replace_word(&text, from, to) {
                        text = replaced;
                        hit = true;
                    }
                }
                if hit {
                    // Fit the raw carriers' fixed budgets: hero names run
                    // longer than sibling names, so a line can overflow
                    // by a byte or two. Contract "I am " -> "I'm ", then
                    // drop the " Delilas" surname after a hero name -
                    // each only when the full line doesn't fit.
                    let fits = |t: &str| {
                        crate::translation::markup::encode(
                            t,
                            crate::translation::markup::Target::Segment,
                        )
                        .map(|b| b.len() <= entry.budget)
                        .unwrap_or(false)
                    };
                    // Candidate ladder, least destructive first: drop
                    // " Delilas" after hero names one occurrence at a
                    // time (the speaker prefix goes first, an
                    // in-sentence "I am Vahn Delilas!" survives as long
                    // as it fits), interleaving the "I am " -> "I'm "
                    // contraction, and take the first candidate that
                    // fits the budget.
                    if !fits(&text) {
                        let drop_n = |t: &str, n: usize| {
                            let mut out = t.to_string();
                            for _ in 0..n {
                                let hit = ["Vahn", "Noa", "Gala"]
                                    .iter()
                                    .filter_map(|h| {
                                        out.find(&format!("{h} Delilas")).map(|p| (p, h.len()))
                                    })
                                    .min();
                                match hit {
                                    Some((pos, hlen)) => out.replace_range(
                                        pos + hlen..pos + hlen + " Delilas".len(),
                                        "",
                                    ),
                                    None => break,
                                }
                            }
                            out
                        };
                        'ladder: for n in 0..=3usize {
                            for contract in [false, true] {
                                let mut cand = drop_n(&text, n);
                                if contract {
                                    cand = cand.replace("I am ", "I'm ");
                                }
                                if fits(&cand) {
                                    text = cand;
                                    break 'ladder;
                                }
                            }
                        }
                    }
                    entry.translation = text;
                    lines += 1;
                }
            }
            let dialog = crate::translation::import_pack_relayout(patcher, &pack)
                .context("rename sibling dialog mentions")?;
            report.notes.push(format!(
                "dialog: {} of {lines} sibling-name line(s) now name the heroes{}",
                dialog.applied + dialog.already_applied,
                if dialog.relayout_entries > 0 {
                    format!(
                        " ({} scene(s) grown by {} sector(s))",
                        dialog.relayout_entries, dialog.relayout_sectors_added
                    )
                } else {
                    String::new()
                }
            ));
            for (key, msg) in &dialog.issues {
                report.notes.push(format!("dialog: {key}: {msg}"));
            }
        }

        // Battle-voice passes, in dependency order: every XA mute first,
        // then the XA + victory-clip fills (which SOURCE the siblings'
        // grunts from monster.snd), and the duel-bank splice LAST -
        // the splice overwrites the sibling banks with the heroes'
        // samples, so a fill that runs after it reads Vahn's voice back
        // out of Lu's bank and hands the "sibling" slots to the wrong
        // speaker.

        // The Plasma Strike bed's intro remaster runs FIRST: it edits
        // the same XA20 channel the special-cue capture below excerpts,
        // so ordering the boost ahead of the capture makes the spliced
        // fanfare open audibly too (the pass-order law).
        match crate::delilas_xa_voice::boost_cast_bed_intro(patcher) {
            Ok(true) => report
                .notes
                .push("cast bed: Plasma Strike music advanced to open with the walk".into()),
            Ok(false) => {}
            Err(e) => report
                .notes
                .push(format!("cast bed: intro remaster skipped ({e:#})")),
        }

        // Sibling XA victory lines - captured off the still-retail
        // reels BEFORE any mute below wipes them (XA21 mutes whole).
        let victory_lines = crate::delilas_xa_voice::capture_victory_lines(patcher, mapping);

        // Retail arts shouts, same read-before-mute law: the `adjusted`
        // arts-voice mode re-voices this audio toward the siblings.
        let hero_shouts = if arts_voice == crate::delilas_voice_fx::ArtsVoiceMode::Adjusted {
            crate::delilas_xa_voice::capture_hero_shouts(patcher)
        } else {
            crate::delilas_xa_voice::HeroShoutCapture {
                banks: Default::default(),
                fanfare: Default::default(),
                staged2: Default::default(),
            }
        };

        // The arts XA shout banks (XA2/XA4/XA6 - the character's VOICE
        // on arts, item use and other callouts) have no sibling
        // counterpart to splice (the Delilas only grunt), so hearing
        // Vahn shout out of Gi's body is worse than silence: mute the
        // swapped characters' banks. The cue still fires (routing
        // untouched); the sectors decode to silence, and the spliced
        // SPU grunts remain the audible voice.
        // `original` arts-voice mode keeps the retail shouts: skip the
        // mute entirely (the fill below leaves the banks untouched too).
        if arts_voice != crate::delilas_voice_fx::ArtsVoiceMode::Original {
            for (slot, file) in ["XA/XA2.XA", "XA/XA4.XA", "XA/XA6.XA"].iter().enumerate() {
                let who = ["Vahn", "Noa", "Gala"][slot];
                let n = patcher
                    .silence_xa_file(file)
                    .with_context(|| format!("mute {who} XA shout bank"))?;
                report
                    .notes
                    .push(format!("{who}: XA shout bank muted ({n} sectors)"));
            }
        }

        // The SECOND voice cue: `XA30.XA` carries the party's normal-move
        // grunt, one channel per character (Vahn 0, Noa 4, Gala 6 - see
        // docs/subsystems/battle-action.md "Battle voice cues"). The
        // battle-action input handler fires it on every ordinary swing -
        // and a tactical art IS a chain of swings, so with only XA2/4/6
        // muted the loudest Vahn line in an art still played. Mute the
        // three hero channels; every other channel in the bank survives.
        let n = patcher
            .silence_xa_channels("XA/XA30.XA", &[0, 4, 6])
            .context("mute party XA30 grunt channels")?;
        report.notes.push(format!(
            "party: XA30 grunt channels 0/4/6 muted ({n} sectors)"
        ));

        // The victory barks: the battle-event bark jukebox (the sound
        // command byte `gp+0x9F4` dispatch in `FUN_8004E568`) resolves
        // its char-keyed victory ids into `XA21.XA` - Vahn picks
        // randomly between ids 0x1A2/0x1A3 (channels 2/3), with 0x1A4 /
        // 0x1A6 / 0x1A7 (channels 4/6/7) as the sibling arms. The whole
        // file is short bark reels (7-22 s per channel); it mutes whole.
        // (`XA12.XA` is NOT touched: its only captured battle fire went
        // through the NON-voice jingle path - id 0x0B, whole-channel dur
        // - i.e. results music, not a hero line.)
        let n = patcher
            .silence_xa_file("XA/XA21.XA")
            .context("mute battle bark bank XA21")?;
        report
            .notes
            .push(format!("party: XA21 victory-bark bank muted ({n} sectors)"));

        // The jukebox's two outlying arms point INTO the music files:
        // id 0x19F = XA20 channel 7, id 0x1AF = XA22 channel 7 - the
        // close-call ("barely won") victory barks. Both channels are
        // short bark reels (12-17 s) interleaved beside 27-274 s music
        // channels; only channel 7 mutes, the music is untouched.
        for file in ["XA/XA20.XA", "XA/XA22.XA"] {
            let n = patcher
                .silence_xa_channels(file, &[7])
                .with_context(|| format!("mute {file} bark channel 7"))?;
            report
                .notes
                .push(format!("party: {file} bark channel 7 muted ({n} sectors)"));
        }

        // The FOURTH voice tier: the staged-event id space (id >= 0x100
        // through `FUN_8004FCC8`; the anim materialiser `FUN_8004AD80`
        // picks the id from an inline char-keyed table - Vahn 0x101,
        // Noa 0x111, Gala 0x121). Two 8-channel banks per hero:
        // Vahn = XA1 + XA27, Noa = XA3 + XA28, Gala = XA5 + XA29.
        //
        // These are NOT bare voice lines: XA1/3/5 are the Hyper / Super
        // / Miracle **fanfare** banks and XA27/28/29 the Seru-magic
        // fanfare streams - stereo cue beds carrying the hero's voice
        // over a jingle. They follow `arts_voice` for the same reason
        // the shout banks do, and `Original` leaves them alone entirely:
        // a Hyper Art fires no shout from the XA2/4/6 pool, so muting
        // its fanfare is the whole difference between a cue and silence.
        if arts_voice != crate::delilas_voice_fx::ArtsVoiceMode::Original {
            for (who, file) in [
                ("Vahn", "XA/XA1.XA"),
                ("Vahn", "XA/XA27.XA"),
                ("Noa", "XA/XA3.XA"),
                ("Noa", "XA/XA28.XA"),
                ("Gala", "XA/XA5.XA"),
                ("Gala", "XA/XA29.XA"),
            ] {
                let n = patcher
                    .silence_xa_file(file)
                    .with_context(|| format!("mute {who} staged-event bank {file}"))?;
                report
                    .notes
                    .push(format!("{who}: {file} fanfare bank muted ({n} sectors)"));
            }
        }

        // Then give the silenced slots the siblings' REAL voices: their
        // monster.snd grunts, XA-encoded over the muted channels. Must
        // run after EVERY mute above (a later whole-file mute would
        // erase the fill) and before the duel-bank splice below (which
        // replaces the sibling banks' samples with the heroes').
        let notes = crate::delilas_xa_voice::fill_hero_xa_voices(
            patcher,
            mapping,
            &victory_lines,
            arts_voice,
            &hero_shouts,
        )
        .context("fill hero XA voice slots with sibling grunts")?;
        report.notes.extend(notes);

        // The FIFTH voice tier, and the one every XA sweep is blind to:
        // the ordinary victory pose's voice is an SPU sample streamed
        // from `monster.snd`'s own sector TOC (`FUN_8003e104`; pose
        // action -> clip byte via the SCUS tables at 0x800788A0 /
        // 0x80078867). Replace the heroes' clip bands with the mapped
        // siblings' own victory lines, re-pitched to their recorded
        // rates - verbatim SPU-ADPCM, same file.
        let notes =
            crate::delilas_xa_voice::fill_hero_victory_clips(patcher, mapping, &victory_lines)
                .context("fill hero victory-voice clips in monster.snd")?;
        report.notes.extend(notes);

        // Battle voices: the party grunts like the mapped siblings.
        // LAST of the voice passes - this swaps the heroes' samples
        // INTO the sibling banks, so any pass sourcing "the sibling's
        // voice" from monster.snd after this point reads the wrong
        // speaker.
        let notes = crate::delilas_voice::splice_party_voices(patcher, mapping)
            .context("splice party battle voices")?;
        report.notes.extend(notes);

        // The signature-special reskin, once per hero slot (name +
        // combo + the sibling's own clip as the staged animation + the
        // fanfare duration to cover the soundtrack the fills above
        // wrote).
        // The transplanted-burst cave holds exactly ONE record (88 bytes
        // between prototype ids 37 and 39 - the battle overlay is packed
        // to the byte), so the first hero slot claims it and the other
        // two keep the borrowed cast projectile.
        let mut cave_taken = false;
        for (_, rig, slot, who, sibling) in mapping.pairs() {
            let ctx = SignatureCtx {
                slot,
                sibling,
                rig,
                retail_player: &retail_players[slot],
                archive: &archive,
                natural_wrist_hand: party_swap::playerize::kept_welded_hand(
                    sibling.monster_id(),
                    options.keep_che_hammer && sibling == Sibling::Che,
                ),
            };
            let notes = reskin_signature_art(patcher, &ctx, &mut cave_taken)
                .with_context(|| format!("reskin the {who}-slot signature art"))?;
            report.notes.extend(notes);

            // The rest of the kit, when the caller asked for it. Runs
            // last per slot: it carries the signature stream the reskin
            // just authored into the archive it re-authors, so it has
            // to see that pass's output.
            if move_mode == DelilasMoveMode::Delilas {
                match apply_delilas_moveset(patcher, &ctx) {
                    Ok(notes) => report.notes.extend(notes),
                    Err(e) => report
                        .notes
                        .push(format!("{who} moves: stay the host's ({e:#})")),
                }
            }

            // Always last per slot: the charge-loop aliasing guard reads
            // the archive as it will ship, whichever move mode built it.
            match clamp_charge_loop_windows(patcher, slot, who) {
                Ok(notes) => report.notes.extend(notes),
                Err(e) => report
                    .notes
                    .push(format!("{who} charge-loop guard skipped ({e:#})")),
            }
        }

        // The cast route: the mapped sibling's signature plays the RETAIL
        // enemy cast module (camera track, effect barrage, multi-hit
        // build-up) instead of the art-side approximation - Blazing
        // Slash (spell 0x79, PROT 958), Megaton Press (0x7A, 959) and
        // Plasma Strike (0x7B, 960), each module carrying its own
        // damage-retarget + wipe-skip + staged-walk fold edit set in
        // `crate::delilas_cast`. Claims the SCUS injection gap, so it
        // composes with neither --shiny-seru nor --show-super-arts - on
        // a conflict the note says so and the art-side signature stays.
        let routes: Vec<crate::delilas_cast::CastRoute> = mapping
            .pairs()
            .iter()
            .filter_map(|&(_, _, slot, _, sibling)| {
                host_art(slot).map(|art| crate::delilas_cast::CastRoute {
                    char_index: slot as u8,
                    art_constant: art.action_constant,
                    spell_id: signature_spell_id(sibling),
                })
            })
            .collect();
        if !routes.is_empty() && cast_route == CastRoutePolicy::ArenaTaken {
            report.notes.push(
                "cast route: art-side signature kept (shiny-seru / show-super-arts / \
                 arts-ap own the SCUS injection arena this run; no cast edits applied)"
                    .to_string(),
            );
        } else if !routes.is_empty() {
            // The CASTER's own body animation: author real staged rows
            // (the sibling's wind-up + payoff on player rows 0x0A/0x0B,
            // Block re-homed to row 0x06 across every player file) so
            // each module's folded stage walk delivers real clips. When
            // the rewrite cannot land, fall back to the probe-proven
            // Che-only shape: pin 959's stages to the empty row 0x0A
            // (caster holds a pose; the enemy-side Megaton also loses
            // its smash stage) and keep Gi/Lu on the art-side reskin.
            let module_name = |spell: u8| match spell {
                0x79 => "Blazing Slash (958)",
                0x7A => "Megaton Press (959)",
                _ => "Plasma Strike (960)",
            };
            match author_staged_cast_rows(patcher, mapping, &retail_players, &archive, &options) {
                Ok(authored) => {
                    report.notes.extend(authored.notes);
                    let gi = authored.gi_unfold.as_deref();
                    let lu = authored.lu_unfold.as_deref();
                    let installed = crate::delilas_cast::patch_module_959(patcher, false)
                        .and_then(|_| crate::delilas_cast::patch_module_958(patcher, gi))
                        .and_then(|_| crate::delilas_cast::patch_module_960(patcher, lu))
                        .and_then(|_| crate::delilas_cast::install_cast_hook(patcher, &routes))
                        .and_then(|_| crate::delilas_cast::install_stage_caves(patcher, gi, lu))
                        .and_then(|_| crate::delilas_cast::install_delilas_arena(patcher))
                        .and_then(|_| crate::delilas_cast::install_strike_morph(patcher))
                        .and_then(|_| crate::delilas_cast::install_cast_label_gate(patcher))
                        .and_then(|_| crate::delilas_cast::install_chain_admission_tier(patcher));
                    match installed {
                        Ok(_) => {
                            report.notes.push(
                                "cast label: the special's spell name replaces the arts banner \
                                 (state-0x28 label runs for every Magic cast)"
                                    .into(),
                            );
                            report.notes.push(
                                "chain admission: Hyper tier 10 -> 5 per arrow, so an \
                                 art-then-special chain clears the matcher's AP gate \
                                 from ~40 AP (the special itself admits at 25)"
                                    .into(),
                            );
                            for r in &routes {
                                let walk = match r.spell_id {
                                    0x79 if gi.is_some() => " (un-folded retail walk)",
                                    0x7B if lu.is_some() => " (un-folded retail walk)",
                                    0x7A => "",
                                    _ => " (folded walk)",
                                };
                                report.notes.push(format!(
                                    "cast route: slot {} signature runs the retail {} module{}",
                                    r.char_index,
                                    module_name(r.spell_id),
                                    walk
                                ));
                            }
                        }
                        Err(e) => report
                            .notes
                            .push(format!("cast route: art-side signature kept ({e:#})")),
                    }
                }
                Err(e) => {
                    report.notes.push(format!(
                        "cast route: caster rows stay pinned to the held pose ({e:#})"
                    ));
                    let che_routes: Vec<crate::delilas_cast::CastRoute> = routes
                        .iter()
                        .filter(|r| r.spell_id == 0x7A)
                        .copied()
                        .collect();
                    let installed = crate::delilas_cast::patch_module_959(patcher, true)
                        .and_then(|_| crate::delilas_cast::install_cast_hook(patcher, &che_routes))
                        .and_then(|_| crate::delilas_cast::install_delilas_arena(patcher))
                        .and_then(|_| crate::delilas_cast::install_strike_morph(patcher))
                        .and_then(|_| crate::delilas_cast::install_cast_label_gate(patcher))
                        .and_then(|_| crate::delilas_cast::install_chain_admission_tier(patcher));
                    match installed {
                        Ok(_) => {
                            report.notes.push(
                                "cast label: the special's spell name replaces the arts banner \
                                 (state-0x28 label runs for every Magic cast)"
                                    .into(),
                            );
                            report.notes.push(
                                "chain admission: Hyper tier 10 -> 5 per arrow, so an \
                                 art-then-special chain clears the matcher's AP gate \
                                 from ~40 AP (the special itself admits at 25)"
                                    .into(),
                            );
                            for r in &che_routes {
                                report.notes.push(format!(
                                    "cast route: slot {} signature runs the retail {} module \
                                     (pinned pose); other slots keep the art-side signature",
                                    r.char_index,
                                    module_name(r.spell_id)
                                ));
                            }
                        }
                        Err(e) => report
                            .notes
                            .push(format!("cast route: art-side signature kept ({e:#})")),
                    }
                }
            }
        }

        // Enemy-side anim mirror, LAST: the swapped duel blocks fight
        // with the mapped hero's own clips (idle / walk / reactions /
        // swings, plus the hero's 50-AP Hyper across the cast module's
        // staged entries). Runs after every pass that touches the
        // monster slots, the player files or readef; all its inputs are
        // the pre-patch captures above.
        let retail = crate::enemy_anim_mirror::RetailSources {
            archive: &archive,
            players: [&retail_players[0], &retail_players[1], &retail_players[2]],
            readef: &retail_readef,
        };
        report
            .notes
            .extend(crate::enemy_anim_mirror::apply_enemy_anim_mirror(
                patcher, mapping, &retail,
            )?);
    }
    Ok(report)
}

/// Give each hero slot the mapped sibling's own **element**.
///
/// The battle overlay's per-character element table
/// (`0x801F5480`, one byte per 1-based character id;
/// `legaia_asset::element_affinity::CHARACTER_ELEMENTS_FILE_OFFSET`) is the
/// only per-character element on the disc, and retail seeds it Vahn = fire,
/// Noa = wind, Gala = thunder. Two routines index it, both with
/// `DAT_8007BD10[actor] - 1` (the slot's active member id), and both use the
/// result as a row/column of the affinity matrix `0x801F53E8`:
/// `FUN_801DD864` (`0x801dd8ac` / `0x801dd900`) and the hit kernel
/// `FUN_801EC3E4` (`0x801ecf38` attacker, `0x801ecf94` defender). So the
/// table decides what element every one of that slot's attacks *deals* and
/// what it *takes* - and until this runs, a swapped party fights in the
/// hero's element: Lu's Plasma Strike lands as fire out of Vahn's slot and
/// Che's Megaton Press as thunder out of Gala's.
///
/// The replacement is not a choice - each sibling's monster record already
/// carries their own element at `+0x1D` (the same byte `FUN_801EC3E4` reads
/// for an enemy attacker at `0x801ecf68`), and the three read Gi = fire,
/// Che = earth, Lu = thunder. Taken from the archive image captured
/// **before** the model loop, so a re-skinned block cannot feed it back.
///
/// This is the whole character, not just the signature art: retail has no
/// per-art element, so a Ra-Seru cast and a basic swing scale through the
/// same byte.
pub(super) fn retarget_character_elements(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    archive: &[u8],
) -> Result<Vec<String>> {
    use legaia_asset::element_affinity as ea;
    let mut notes = Vec::new();
    for (_, _, slot, who, sibling) in mapping.pairs() {
        let id = sibling.monster_id();
        let element = monster_archive::record(archive, id)
            .with_context(|| format!("read monster {id} for its element"))?
            .ok_or_else(|| anyhow::anyhow!("monster id {id}: empty slot"))?
            .element;
        let name = ea::Element::from_id(element)
            .map(|e| e.name())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} carries element id {element}, outside the {}-element space",
                    sibling.display_name(),
                    ea::ELEMENT_COUNT
                )
            })?;
        // The table is 1-based on character id and the party slots are
        // characters 1..=3, so the slot index IS the table index.
        let off = ea::CHARACTER_ELEMENTS_FILE_OFFSET + slot;
        patcher
            .patch_prot_entry(BATTLE_OVERLAY_ENTRY, off as u64, &[element])
            .with_context(|| format!("write the {who}-slot element"))?;
        notes.push(format!(
            "{who} element: {} ({}'s own)",
            name,
            sibling.display_name()
        ));
    }
    Ok(notes)
}
