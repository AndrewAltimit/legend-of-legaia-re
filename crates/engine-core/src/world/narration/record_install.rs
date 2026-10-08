//! Installing P2 records: spawned / gated / helper records and the cutscene
//! timeline records (`town01`'s opening timeline among them).
//! Split out of `narration.rs`; no logic change.

use super::*;

impl World {
    /// Install a field-VM op-`0x44` SPAWN_RECORD request: re-base the GLOBAL
    /// record index into partition 2 (`global - N0 - N1`, retail
    /// `FUN_8003BDE0`), check the record's C1/C2 story-flag gates, and install
    /// it as a cutscene timeline. Returns `true` when a timeline installed.
    ///
    /// This is how the opening chain's `opstati` / `opurud` legs launch their
    /// prologue records (`44 21` / `44 32` in their P1[0] entry scripts).
    // REF: FUN_8003BDE0
    pub fn install_spawned_record(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        global_index: u8,
    ) -> bool {
        let n0 = man_file.header.partition_counts[0].max(0) as usize;
        let n1 = man_file.header.partition_counts[1].max(0) as usize;
        let Some(record_idx) = (global_index as usize).checked_sub(n0 + n1) else {
            return false;
        };
        self.install_gated_p2_record(man_file, man, record_idx)
    }

    /// Install partition-2 record `record_idx` as a cutscene timeline after
    /// checking its C1/C2 story-flag gates (the `FUN_8003BDE0` dispatch
    /// body). Returns `true` when a timeline installed. Shared by the
    /// op-`0x44` spawn ([`Self::install_spawned_record`]) and the walk-on
    /// tile trigger.
    // PORT: FUN_8003BDE0 (record resolve + name/C0 skip + C1-any/C2-all gate
    // eval + context install). Retail also stores ctx[+0x50] =
    // hdr[+0x22] + hdr[+0x24] + record index (0x8003C050..0x8003C094): the
    // MAN header's partition counts, so +0x50 is the record's **global**
    // index across partitions - not a seat-position seed from coords, as
    // this note used to say. The port keeps the record index on the
    // timeline instead of a context field.
    pub fn install_gated_p2_record(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        record_idx: usize,
    ) -> bool {
        match crate::man_field_scripts::partition2_record_gates(man_file, man, record_idx) {
            Some((c1, c2)) => {
                if !self.p2_record_gates_pass(&c1, &c2) {
                    return false;
                }
            }
            None => return false,
        }
        let installed = self.install_cutscene_timeline_record(man_file, man, 2, record_idx, false);
        if installed {
            // `FUN_8003BDE0` ends a successful spawn with the motion-pause
            // kick (`jal 0x8003C9AC` at `0x8003C0D4`).
            self.kick_field_npc_motion_pause();
        }
        installed
    }

    /// Install a field-VM op-`0x44` SPAWN_RECORD request as a **concurrent
    /// helper context** ([`crate::world::FieldVmState::helper_contexts`]): re-base the GLOBAL record
    /// index into partition 2 (`global - N0 - N1`, retail `FUN_8003BDE0`),
    /// check the record's C1/C2 story-flag gates, and push the record as an
    /// independent spawned context. Returns `true` when a context installed.
    ///
    /// The non-cutscene-class counterpart to [`Self::install_spawned_record`]:
    /// retail runs every spawned record as an independent field-VM context and
    /// only cutscene-class records seize the camera, so an ordinary scene's
    /// mid-play helper spawn goes here - it executes its script (flag writes,
    /// channel pokes, moves) without the modal-timeline attributes. It still
    /// holds the pad while it runs
    /// ([`Self::script_context_engages_player`]).
    // REF: FUN_8003BDE0
    pub fn install_spawned_helper_record(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        global_index: u8,
    ) -> bool {
        let n0 = man_file.header.partition_counts[0].max(0) as usize;
        let n1 = man_file.header.partition_counts[1].max(0) as usize;
        let Some(record_idx) = (global_index as usize).checked_sub(n0 + n1) else {
            return false;
        };
        self.install_helper_record(man_file, man, record_idx)
    }

    /// Install partition-2 record `record_idx` as a concurrent helper context
    /// after checking its C1/C2 story-flag gates (the `FUN_8003BDE0` dispatch
    /// body - the same gate walk as [`Self::install_gated_p2_record`]).
    /// Returns `true` when a context installed; `false` on a failed gate, an
    /// unresolvable span, or a full context table
    /// ([`crate::world::SPAWNED_CONTEXT_SLOTS`]).
    ///
    /// Unlike [`Self::install_cutscene_timeline_record`] this does NOT
    /// re-spawn the per-actor channels (an ordinary scene's channels are
    /// seeded at entry by [`Self::seed_field_channels`] and must keep their
    /// state) and does not parse inline narration blocks (the crawl roller is
    /// a modal-timeline presentation).
    // REF: FUN_8003BDE0
    pub fn install_helper_record(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        record_idx: usize,
    ) -> bool {
        if self.field_vm.helper_contexts.len() >= crate::world::SPAWNED_CONTEXT_SLOTS {
            return false;
        }
        match crate::man_field_scripts::partition2_record_gates(man_file, man, record_idx) {
            Some((c1, c2)) => {
                if !self.p2_record_gates_pass(&c1, &c2) {
                    return false;
                }
            }
            None => return false,
        }
        let Some((script_start, pc0, body_len)) =
            crate::man_field_scripts::partition_record_span(man_file, man, 2, record_idx)
        else {
            return false;
        };
        let Some(body) = man.get(script_start..script_start + body_len) else {
            return false;
        };
        self.field_vm
            .helper_contexts
            .push(crate::cutscene_timeline::CutsceneTimeline::new(
                body.to_vec(),
                pc0,
            ));
        // The same spawn tail as `Self::install_gated_p2_record`: the kick at
        // `0x8003C0D4`.
        self.kick_field_npc_motion_pause();
        true
    }

    /// Partition-2 record index of `town01`'s opening cutscene timeline (the
    /// establishing camera sweep + Vahn's walk-out + the name-entry handoff).
    /// A stable disc invariant; the record carries the name-entry STATE_RESUME
    /// pinned at body offset `0x02c6` (see `town01_opening_timeline_trace.rs`).
    pub const TOWN01_OPENING_TIMELINE_RECORD: usize = 3;

    /// Install `town01`'s opening cutscene timeline (the establishing shot +
    /// Vahn's scripted walk-out + the name-entry handoff) as a spawned field-VM
    /// context, and arm the name-entry handoff so the timeline's pinned op-`0x49`
    /// STATE_RESUME opens the *"Select your name."* overlay (rather than the
    /// host opening it blindly at the scene hand-off).
    ///
    /// Unlike [`Self::load_cutscene_timeline_from_man`] this does NOT arm a
    /// prologue scene hand-off - `town01` is the destination, and the record's
    /// terminal is the name-entry suspend, not a scene change. Returns `true`
    /// when installed.
    ///
    /// The record's C1/C2 header gates are honored (the `FUN_8003BDE0`
    /// dispatch walk): `P2[3]` lists its own SET, system flag `0x225` (549) -
    /// the record's opening `52 25` script bytes latch it, so the opening is
    /// a self-disabling one-shot exactly like the rikuroa post-victory
    /// record. A world whose flag bank already carries `0x225` (a replay /
    /// loaded save) refuses the install.
    // REF: FUN_8003BDE0
    pub fn install_town01_opening_timeline(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> bool {
        match crate::man_field_scripts::partition2_record_gates(
            man_file,
            man,
            Self::TOWN01_OPENING_TIMELINE_RECORD,
        ) {
            Some((c1, c2)) => {
                if !self.p2_record_gates_pass(&c1, &c2) {
                    return false;
                }
            }
            None => return false,
        }
        if !self.install_cutscene_timeline_record(
            man_file,
            man,
            2,
            Self::TOWN01_OPENING_TIMELINE_RECORD,
            false,
        ) {
            return false;
        }
        // The opening hides the town for its establishing shot; free-roam
        // follows with no scene reload, so completion must drop the hide-box
        // overrides (see `CutsceneTimeline::restore_hidden_on_complete`).
        if let Some(tl) = self.cutscene.timeline.as_mut() {
            tl.restore_hidden_on_complete = true;
        }
        self.cutscene.prologue_naming_pending = true;
        self.cutscene.prologue_naming_armed = false;
        self.cutscene.naming_owner =
            Some(crate::field_submode_screen::Op49ParkOwner::CutsceneTimeline);
        self.cutscene.naming_slot = 0;
        true
    }

    /// Install a specific partition / record as a spawned cutscene-timeline
    /// context. The general core behind [`Self::load_cutscene_timeline_from_man`]
    /// (which locates `opdeene`'s `GFLAG_SET 26` record first) and the
    /// town-opening op-stream trace harness (which installs `town01`'s opening
    /// timeline record by index).
    ///
    /// Resolves the record's `(script_start, pc0, body_len)` span, slices the
    /// body from `script_start` (so relative jumps wrap against the record
    /// base), parses the inline narration blocks into
    /// [`crate::cutscene_timeline::NarrationSite`]s (the stepper suspends the
    /// timeline at each block while the
    /// [`crate::cutscene_narration::CutsceneNarration`] presenter plays its
    /// pages - the retail caption-child suspend), and installs the timeline
    /// with `trace` controlling op-stream recording.
    ///
    /// Returns `true` when a timeline was installed; `false` when the span can't
    /// be resolved.
    // REF: FUN_8003BDE0
    // REF: FUN_8003C764
    pub fn install_cutscene_timeline_record(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
        partition: usize,
        record_idx: usize,
        trace: bool,
    ) -> bool {
        let Some((script_start, pc0, body_len)) =
            crate::man_field_scripts::partition_record_span(man_file, man, partition, record_idx)
        else {
            return false;
        };
        let Some(body) = man.get(script_start..script_start + body_len) else {
            return false;
        };
        let body = body.to_vec();
        let narration_blocks: Vec<crate::cutscene_timeline::NarrationSite> =
            legaia_asset::cutscene_text::parse_narration(&body)
                .into_iter()
                .filter(|b| b.count_matches() && !b.pages.is_empty())
                .map(|b| {
                    let (start, end) = b.byte_span();
                    debug_assert_eq!(start, b.op_offset);
                    crate::cutscene_timeline::NarrationSite {
                        op_offset: b.op_offset,
                        end: end.min(body.len()),
                        pages: b.pages.into_iter().map(|p| p.text).collect(),
                        kind: b.kind,
                    }
                })
                .collect();
        let mut tl = crate::cutscene_timeline::CutsceneTimeline::new(body, pc0);
        tl.narration_blocks = narration_blocks;
        if trace || std::env::var_os("LEGAIA_DIAG_TIMELINE").is_some() {
            tl = tl.with_trace();
        }
        self.cutscene.timeline = Some(tl);
        // Spawn the per-actor channels (one per partition-1 placement,
        // retail `FUN_8003AEB0`'s spawn loop) so the timeline's cross-context
        // pokes land on real per-actor contexts - the vignette mechanism -
        // UNLESS this scene's channels are already seeded over the SAME MAN.
        // Retail's walk-on record dispatch (`FUN_8003BDE0`) creates ONE new
        // context and leaves the placement contexts (spawned once at scene
        // entry by `FUN_8003AEB0`) untouched, so a mid-scene beat must keep
        // their state: halt bits, entry-pre-run positions, parked PCs.
        // Respawning here discarded that state and re-ran every spawn
        // prologue mid-scene with the beat's OWN flag writes visible - the
        // town01 Mei beat's opening `SET 550` re-routed her prologue to its
        // post-beat seat, fighting the beat's own door-tile seat poke.
        let same_man = self
            .field_vm
            .channels_man
            .as_deref()
            .is_some_and(|m| m.as_slice() == man);
        if !same_man || self.field_vm.channels.is_empty() {
            self.field_vm.channels = crate::field_channels::spawn_channels(man_file, man);
            let binds = std::mem::take(&mut self.field_vm.object_channel_binds);
            let mut obj = crate::field_channels::spawn_object_channels(man_file, man, &binds);
            self.seat_object_channel_rots(&mut obj);
            self.field_vm.channels.extend(obj);
            self.field_vm.object_channel_binds = binds;
            self.field_vm.channels_man = Some(std::sync::Arc::new(man.to_vec()));
            self.npcs.clip_cursors.clear();
            self.npcs.clip_rate_live.clear();
            self.npcs.clip_bones.clear();
        }
        self.npcs.anim_cues.clear();
        self.npcs.clip_current.clear();
        true
    }
}
