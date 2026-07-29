//! `SceneHost` BGM/VAB byte access, dialog panel open/clear, and BGM event routing.
//!
//! Extracted verbatim from `scene/host.rs` as an additional `impl SceneHost` block.

use super::*;

impl SceneHost {
    /// Resolve a BGM id to the raw SEQ bytes the runtime would pass to its
    /// sequencer. Mirrors `FUN_800243F0` (the BGM resolver): scene-local ids
    /// (`< 2000`) live at `block_start + 6 + id`; global-pool ids
    /// (`>= 2000`) are not modeled. Returns `None` when no scene is loaded
    /// or no SEQ-bearing entry maps to the id.
    ///
    /// Engines parse the returned bytes with [`legaia_seq::Seq::parse`] and
    /// attach to [`legaia_engine_audio::Sequencer::new`] alongside the
    /// scene's VAB bank.
    // PORT: FUN_800243F0 (the BGM-id -> PROT-slot resolution; the retail
    // double-buffered async load poller around it is host-replaced by the
    // engine-audio Sequencer + this synchronous byte access)
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
    pub fn music_bank_entry_bytes(&self, bgm_id: u16) -> Result<Option<Arc<Vec<u8>>>> {
        let Some(entry) = crate::music_labels::prot_entry_for_bgm_id(bgm_id) else {
            return Ok(None);
        };
        Ok(self.index.entry_bytes(entry).ok())
    }

    /// First VAB-bearing entry in the scene, ready for parsing as a sound
    /// bank. Mirrors the asset chain's "load the scene's bank before the
    /// first sound plays" pre-pass. Returns `None` when no VAB-tagged
    /// entries are in the scene.
    pub fn scene_vab_bytes(&self) -> Result<Option<Arc<Vec<u8>>>> {
        let Some(assets) = self.assets.as_ref() else {
            return Ok(None);
        };
        let Some(&entry_idx) = assets.vab_entries.first() else {
            return Ok(None);
        };
        let bytes = self.index.entry_bytes(entry_idx)?;
        Ok(Some(bytes))
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
        let req = self.world.current_dialog.as_ref()?;
        // Placement-NPC / event dialogue carries its text inline (the field-VM
        // `0x3F` op's buffer); its `text_id` is a box-config id, not an MES
        // index, so it never resolves through the scene MES. Prefer the inline
        // text when present, falling back to the MES `text_id` lookup (used by
        // the message-table dialogue paths).
        if !req.inline.is_empty()
            && let Some(panel) = crate::dialog::OwnedDialogPanel::from_inline_dialog(&req.inline)
        {
            return Some(panel);
        }
        let mes = self.assets.as_ref()?.mes.as_ref()?;
        crate::dialog::OwnedDialogPanel::from_scene_mes(mes, req.text_id)
    }

    /// Clear the world's pending dialog request. Call after the user
    /// dismisses the box (the field VM resumes the next frame).
    pub fn clear_dialog(&mut self) {
        self.world.current_dialog = None;
    }

    /// Drain the world's pending BGM events through `director`, resolving
    /// each `Bgm{text_id, sub_op}` into the right director hook. Mirrors
    /// the field-VM op `0x35` sub-op table: `1` = start (resolve SEQ
    /// bytes), `2` = pause, `3` = resume, `4` = stop, `8` = re-attach +
    /// volume re-apply (`FUN_80019898`), `9` = start behind a load barrier,
    /// `10` = the unhalt-pause swap-commit
    /// ([`BgmDirector::unhalt_pause`]). Other sub-ops are passed through
    /// as no-ops (the host already surfaced them on the world's event
    /// queue for richer engines to consume).
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
                        if let Some(bytes) = self.bgm_seq_bytes(text_id)? {
                            director.start(text_id, &bytes);
                            acted += 1;
                        } else if let Some(entry) = self.music_bank_entry_bytes(text_id)? {
                            // Global-pool track: it brings its own VAB.
                            director.start_owned_vab(text_id, &entry);
                            acted += 1;
                        }
                    }
                    2 => {
                        director.pause();
                        acted += 1;
                    }
                    3 => {
                        director.resume();
                        acted += 1;
                    }
                    4 => {
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
