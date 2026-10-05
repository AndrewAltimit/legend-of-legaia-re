//! `SceneHost` BGM/VAB byte access, dialog panel open/clear, and BGM event routing.
//!
//! Extracted verbatim from `scene/host.rs` as an additional `impl SceneHost` block.

use super::*;

impl SceneHost {
    /// The SEQ bytes at the scene block's `block_start + 6 + id` for a
    /// scene-local id (`< 2000`) - the index `FUN_800243F0` stores into
    /// `_DAT_8007BAB8` for change detection. It is **not** what retail
    /// loads: the resolver overwrites the load index with the `music_01`
    /// fallback ([`super::SCENE_LOCAL_BGM_FALLBACK_ID`]), and
    /// [`Self::route_bgm_events`] plays that. Kept for the audio-trace
    /// oracle's scene-local sweep. Returns `None` when no scene is loaded or
    /// no SEQ-bearing entry maps to the id.
    ///
    /// REF: FUN_800243F0 (the change-detection index only; the resolver's
    /// port is [`Self::music_bank_entry_bytes`])
    pub fn bgm_seq_bytes(&self, bgm_id: u16) -> Result<Option<Arc<Vec<u8>>>> {
        let Some(assets) = self.assets.as_ref() else {
            return Ok(None);
        };
        let Some(entry_idx) = assets.bgm_seq_entry(bgm_id) else {
            return Ok(None);
        };
        let bytes = self.index.entry_bytes(entry_idx)?;
        let offset = assets.bgm_seq_offset(bgm_id).unwrap_or(0);
        if offset == 0 {
            Ok(Some(bytes))
        } else if offset < bytes.len() {
            // Slice past the chunk-header wrapper so the returned bytes
            // start at the `pQES` magic. Allocates a fresh Arc - the
            // caller usually parses once and caches the resulting Seq.
            Ok(Some(Arc::new(bytes[offset..].to_vec())))
        } else {
            Ok(None)
        }
    }

    /// Raw `music_01` bank entry bytes for a **global-pool** BGM id
    /// (`>= 2000`): the whole `[VAB][SEQ]` pair the director uploads + plays
    /// itself (via [`BgmDirector::start_owned_vab`]). Global ids are
    /// `2000 + sound-test slot`, resolved to an extraction PROT entry through
    /// the piecewise bank map ([`crate::music_labels::prot_entry_for_bgm_id`] -
    /// the bank is not a single linear run). Returns `None` for scene-local ids, ids past
    /// the bank, or when the entry can't be read. This is the global half of
    /// the retail `FUN_800243F0` resolver that [`Self::bgm_seq_bytes`] left
    /// unmodeled - every real music cue (field, battle, minigame) is a global
    /// track, so this is the path most BGM actually takes.
    ///
    /// The ending theme [`crate::mode_entry_init::FIELD_BGM_TWO_PART_ID`] is
    /// the one id retail does not play from its bank slot: the field
    /// initialiser stages its score and instruments from two other entries on
    /// sound slot 10 ([`crate::mode_entry_init::field_bgm_plan`]). Its bytes
    /// here are that pair composed into one stream
    /// ([`crate::mode_entry_init::two_part_bgm_stream`]), so both play hosts
    /// stage it through the same owned-VAB path as every other track.
    ///
    /// This is the half of `FUN_800243F0` that runs in play: the op-`0x35`
    /// start arm of [`Self::route_bgm_events`] asks it for every track, after
    /// applying the scene-local overwrite
    /// ([`super::SCENE_LOCAL_BGM_FALLBACK_ID`], the `0x800245A4..0x800245BC`
    /// arm), and the hosts' battle / minigame BGM restores ask it directly.
    ///
    /// PORT: FUN_800243F0 (the BGM-id -> PROT-slot resolution; the retail
    /// double-buffered async load poller around it is host-replaced by the
    /// engine-audio Sequencer + this synchronous byte access)
    pub fn music_bank_entry_bytes(&self, bgm_id: u16) -> Result<Option<Arc<Vec<u8>>>> {
        use crate::mode_entry_init::{field_bgm_plan, two_part_bgm_stream};
        // No latch and no playing slot: re-starts are the director's to
        // suppress, as for every other track.
        if let Some([seq_raw, vab_raw]) =
            field_bgm_plan(u32::from(bgm_id), 0, u32::MAX, false).two_part_streams
        {
            let read = |raw: u32| {
                legaia_asset::boot_overlay::raw_to_extraction(raw)
                    .and_then(|e| self.index.entry_bytes(e).ok())
            };
            let (Some(seq), Some(vab)) = (read(seq_raw), read(vab_raw)) else {
                return Ok(None);
            };
            return Ok(two_part_bgm_stream(&seq, &vab).map(Arc::new));
        }
        let Some(entry) = crate::music_labels::prot_entry_for_bgm_id(bgm_id) else {
            return Ok(None);
        };
        Ok(self.index.entry_bytes(entry).ok())
    }

    /// The tile board's quit-prompt lines (title, then the two rows), read
    /// off the field overlay's image (extraction entry `0897`) the way the
    /// render tail reads them out of its own data segment. `None` when the
    /// entry cannot be read. See [`crate::tile_board::prompt_strings`].
    pub fn tile_board_prompt_lines(&self) -> Option<[Vec<u8>; 3]> {
        let bytes = self.index.entry_bytes(897).ok()?;
        crate::tile_board::prompt_strings(&bytes)
    }

    /// The developer EVENT FLAG page's list projection, read off the field
    /// overlay's image (extraction entry `0897`) at `DAT_801F2E94`. Empty
    /// when the entry cannot be read. See [`crate::dev_menu::flag_list_tags`].
    pub fn dev_flag_list_tags(&self) -> Vec<u8> {
        self.index
            .entry_bytes(crate::incense_notice::FIELD_OVERLAY_PROT_INDEX)
            .map(|b| crate::dev_menu::flag_list_tags(&b))
            .unwrap_or_default()
    }

    /// The Incense wear-off notice's line while it is up: the field
    /// overlay's string at `0x801CF1A4` (extraction entry `0897`) with its
    /// `0xC2` item-name escape expanded through the live item names. `None`
    /// while no notice is shown or the entry cannot be read. See
    /// [`crate::incense_notice`].
    pub fn incense_notice_line(&self) -> Option<Vec<u8>> {
        if !self.world.incense_notice_shown() {
            return None;
        }
        let bytes = self
            .index
            .entry_bytes(crate::incense_notice::FIELD_OVERLAY_PROT_INDEX)
            .ok()?;
        let text = self.world.menu.text.as_ref();
        crate::incense_notice::notice_line(&bytes, |id| {
            text.and_then(|t| t.item_name(id)).map(str::to_string)
        })
    }

    /// First VAB-bearing entry in the scene, with the byte offset of the
    /// `pBAV` magic inside it. Mirrors the asset chain's "load the scene's
    /// bank before the first sound plays" pre-pass. Returns `None` when no
    /// VAB-tagged entry is in the scene.
    ///
    /// **The offset is not optional.** A `vab_entries` member is a
    /// [`SceneVabStream`](legaia_asset::scene_vab_stream) - a DATA_FIELD
    /// chunk stream whose chunk 0 carries the VAB's header part - so the
    /// entry begins with a 4-byte chunk header and the bank begins at `+4`
    /// (`docs/formats/vab.md`). No retail PROT entry begins with the magic
    /// itself, so a caller that parses this buffer at offset 0 gets an error,
    /// not a bank. Returning the pair makes that unmissable; the raw entry is
    /// still what comes back, because
    /// [`legaia_engine_audio::VabBank::upload`] resolves both the caller's
    /// base convention and the real VAG-body origin off this same buffer.
    pub fn scene_vab_bytes(&self) -> Result<Option<(Arc<Vec<u8>>, usize)>> {
        let Some(assets) = self.assets.as_ref() else {
            return Ok(None);
        };
        let Some(&entry_idx) = assets.vab_entries.first() else {
            return Ok(None);
        };
        let bytes = self.index.entry_bytes(entry_idx)?;
        let vab_off = legaia_asset::scene_vab_stream::detect(&bytes)
            .map(|s| s.vab_range().start)
            .with_context(|| {
                format!("PROT entry {entry_idx} is classed VAB-bearing but is not a VAB stream")
            })?;
        Ok(Some((bytes, vab_off)))
    }

    /// If the world has a pending dialog request and no panel is currently
    /// running, build an [`crate::dialog::OwnedDialogPanel`] resolved through
    /// the scene's MES container and return it. The caller drives the
    /// panel per-frame; when [`crate::dialog::OwnedDialogPanel::is_done`]
    /// reports true, the caller calls [`SceneHost::clear_dialog`] to
    /// release the field-VM script.
    ///
    /// Returns `None` when no dialog is pending or the scene has no MES
    /// container. The resolved request is left on the world; calling
    /// [`SceneHost::clear_dialog`] cleans it up when the user dismisses
    /// the box.
    pub fn open_pending_dialog(&mut self) -> Option<crate::dialog::OwnedDialogPanel> {
        let req = self.world.dialog.current.as_ref()?;
        // Placement-NPC / event dialogue carries its text inline (the field-VM
        // `0x3F` op's buffer); its `text_id` is a box-config id, not an MES
        // index, so it never resolves through the scene MES. Prefer the inline
        // text when present, falling back to the MES `text_id` lookup (used by
        // the message-table dialogue paths).
        if !req.inline.is_empty()
            && let Some(mut panel) =
                crate::dialog::OwnedDialogPanel::from_inline_dialog(&req.inline)
        {
            // The same name / number escape resolution the VM-dialogue
            // panels take (`World::dialog_substitutions`).
            panel.substitutions = self.world.dialog_substitutions(&req.inline);
            return Some(panel.opening_menu_at_wait());
        }
        let mes = self.assets.as_ref()?.mes.as_ref()?;
        crate::dialog::OwnedDialogPanel::from_scene_mes(mes, req.text_id)
    }

    /// Clear the world's pending dialog request. Call after the user
    /// dismisses the box (the field VM resumes the next frame).
    pub fn clear_dialog(&mut self) {
        self.world.dialog.current = None;
    }

    /// Drain the world's pending BGM events through `director`, resolving
    /// each `Bgm{text_id, sub_op}` into the right director hook. Mirrors
    /// the field-VM op `0x35` sub-op table - the arm table at `0x801CEE00`
    /// in the field overlay, indexed `sub - 1`: `1` = start (resolve SEQ
    /// bytes), `2` = pause (`0x801E0138`: set pause bit 1,
    /// `FUN_800266E0`), `3` = pause (`0x801E015C`: set pause bit 1,
    /// `FUN_80026740`), `4` = resume (`0x801E0180`: clear pause bit 1,
    /// `FUN_80026478` re-attaches the slot), `8` = re-attach + volume
    /// re-apply (`FUN_80019898`), `9` = start behind a load barrier, `10` =
    /// the unhalt-pause swap-commit ([`BgmDirector::unhalt_pause`]), and the
    /// engine's own [`super::BGM_SUB_OP_ENGINE_STOP`] = stop. Other sub-ops
    /// are passed through as no-ops (the host already surfaced them on the
    /// world's event queue for richer engines to consume).
    ///
    /// # Sub-ops 3 and 4 are a pause and a resume
    ///
    /// This router used to read `3` as a resume and `4` as a stop, the
    /// legacy labels. The arm bodies say otherwise: `3` sets the pause bit
    /// and stops the bound sequence's play flag (`FUN_80026740` ->
    /// `FUN_8006275C` raises channel flag `0x2`, the key-off pause), and `4`
    /// clears the pause bit and re-attaches the slot. So the most common
    /// control word in the disc-wide census after the start / commit pair -
    /// sub-op `4` - silenced the score in the port where retail brought it
    /// back. What the port does not reproduce is where the re-attach resumes:
    /// `FUN_80026478` plays through `FUN_800628F0` mode `1`, which rewinds the
    /// sequence to its start (`+0x4` into the cursor words) before it sets the
    /// play flag, and the port's gate resumes from the playhead.
    ///
    /// # Sub-op 9 is a start, not a queue
    ///
    /// Sub-op 9 is the op a cutscene changes music with part-way through a
    /// scene, and its retail arm (field overlay `0x801E0224`) is a load
    /// barrier followed by sub-op 1's own track select:
    ///
    /// ```text
    /// 801e022c  lw a0,-0x4548(v0)     ; a0 = *0x8007BAB8  (resolved PROT index)
    /// 801e0230  lw v0,-0x4564(v1)     ; v0 = *0x8007BA9C  (index actually loaded)
    /// 801e0238  bne a0,v0,0x801dee4c  ; not settled -> `move s8,s4`, i.e. re-run this PC
    /// 801e0240  jal 0x8003ce9c        ; read the u16 operand
    /// 801e0254  sw v0,-0x4538(a1)     ; *0x8007BAC8 = id   <-- sub-op 1's store, verbatim
    /// ```
    ///
    /// So the barrier stalls the *script* until the previously requested
    /// asset has landed, and then the track is selected exactly as sub-op 1
    /// selects it. This host resolves BGM bytes synchronously
    /// ([`SceneHost::music_bank_entry_bytes`] is a plain PROT read), so
    /// nothing is ever in flight and the barrier is satisfied on arrival -
    /// which leaves sub-op 9 as a plain start.
    ///
    /// Reading it instead as "queue for the next scene entry" is silent in a
    /// corpus sweep (a scene's *entry* music uses sub-op 1) and audible only
    /// inside a cutscene: the score never plays where it belongs and then
    /// starts over whatever scene the player walks into next.
    ///
    /// Returns the number of events that the director acted on. Call once
    /// per frame after [`SceneHost::tick`].
    pub fn route_bgm_events(&mut self, director: &mut dyn BgmDirector) -> Result<usize> {
        let mut acted = 0usize;
        let mut leftover = Vec::new();
        for ev in self.world.drain_field_events() {
            match ev {
                crate::field_events::FieldEvent::Bgm { text_id, sub_op } => match sub_op {
                    // 1 = start; 9 = start behind a load barrier this host
                    // never has to wait on (see the doc comment above).
                    1 | 9 => {
                        // Both arms store the id into `_DAT_8007BAC8` before
                        // anything resolves it, the park sentinel included.
                        self.bgm_track_word = Some(text_id);
                        // Every track brings its own VAB. A scene-local id
                        // loads retail's fallback track, not a scene bank
                        // (`SCENE_LOCAL_BGM_FALLBACK_ID`); the id itself is
                        // kept so the director's same-track suppression
                        // compares what retail's `_DAT_8007BAB8` does.
                        let bank_id = if text_id < super::GLOBAL_BGM_BASE {
                            super::SCENE_LOCAL_BGM_FALLBACK_ID
                        } else {
                            text_id
                        };
                        if let Some(entry) = self.music_bank_entry_bytes(bank_id)? {
                            director.start_owned_vab(text_id, &entry);
                            acted += 1;
                        }
                    }
                    // 2 / 3 both set pause bit 1 (`0x801E0138`,
                    // `0x801E015C`); 4 clears it and re-attaches the slot
                    // (`0x801E0180`).
                    2 | 3 => {
                        director.pause();
                        acted += 1;
                    }
                    4 => {
                        director.resume();
                        acted += 1;
                    }
                    super::BGM_SUB_OP_ENGINE_STOP => {
                        director.stop();
                        acted += 1;
                    }
                    8 => {
                        // FUN_80019898: re-attach the BGM sound source and
                        // re-apply the field volume global (DAT_8007B6EC).
                        director.reattach_volume(super::bgm_reattach_volume(self.bgm_volume_raw));
                        acted += 1;
                    }
                    // Sub-op 0xA - the unhalt-pause swap-commit (retail arm
                    // 0x801E0264: wait on _DAT_8007B750 bit 3, release the
                    // paused slot via FUN_800266E0 + FUN_80026520, set the
                    // release-ack bit 4, clear the pause bit 1). The wait is
                    // on the resolver's load-settle bit, which this
                    // synchronous host satisfies on arrival - same reasoning
                    // as sub-op 9's barrier above - so the commit routes
                    // immediately.
                    // PORT: FUN_800266E0 (the detach half of the commit)
                    // PORT: FUN_80026520 (the close half; both are subsumed by
                    // the director's release of its paused source)
                    10 => {
                        director.unhalt_pause();
                        acted += 1;
                    }
                    _ => {
                        // Other sub-ops (5/6/7/11) are control words -
                        // surface them back on the queue for richer engines.
                        leftover.push(crate::field_events::FieldEvent::Bgm { text_id, sub_op });
                    }
                },
                other => leftover.push(other),
            }
        }
        // Restore non-BGM (and unhandled-BGM) events so engine layers that
        // also consume them aren't shorted by this routing pass.
        self.world.pending_field_events.extend(leftover);
        Ok(acted)
    }
}
