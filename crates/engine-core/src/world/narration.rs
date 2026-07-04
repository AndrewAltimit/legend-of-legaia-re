//! Name entry, cutscene narration, prologue handoff, cutscene timelines, field channels, and inline-dialogue driving.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Record the active scene label. Engines call this from the scene-load
    /// path (typically right before `install_encounter_for_scene`) so
    /// downstream consumers (HUD, diagnostics, save snapshots) can surface
    /// the current scene without re-walking the [`crate::scene::SceneHost`].
    pub fn set_active_scene_label(&mut self, label: impl Into<String>) {
        self.active_scene_label = label.into();
    }

    /// Display name for a party slot - the name-entry result if one was
    /// committed, otherwise the template default seeded at
    /// [`Self::seed_starting_party`]. Empty string when the slot is unknown.
    pub fn party_name(&self, slot: usize) -> &str {
        self.party_names.get(slot).map(String::as_str).unwrap_or("")
    }

    /// Open the name-entry overlay for `slot`, seeded with the slot's current
    /// display name (e.g. the template `Vahn`). Mirrors the opening `town01`
    /// script's lead-character naming prompt. The host drives it each frame
    /// with [`Self::step_name_entry`] and renders from [`Self::name_entry`].
    pub fn open_name_entry(&mut self, slot: usize) {
        let initial = self.party_name(slot).to_string();
        self.name_entry = Some(crate::name_entry::NameEntry::new(slot, &initial));
    }

    /// `true` while the name-entry overlay is active.
    pub fn name_entry_active(&self) -> bool {
        self.name_entry.is_some()
    }

    /// Advance the active name-entry overlay by one input frame. On commit
    /// (the player confirms "Is this name okay?") the entered name is written
    /// into [`Self::party_names`] for the entry's slot, the session is closed,
    /// and `true` is returned so the host can resume the field script.
    /// Returns `false` while the overlay stays open (or when none is active).
    pub fn step_name_entry(&mut self, input: crate::name_entry::NameEntryInput) -> bool {
        let Some(entry) = self.name_entry.as_mut() else {
            return false;
        };
        entry.step(input);
        if entry.state == crate::name_entry::NameEntryState::Done {
            let slot = entry.char_index;
            let name = entry.committed_name();
            if self.party_names.len() <= slot {
                self.party_names.resize(slot + 1, String::new());
            }
            self.party_names[slot] = name;
            self.name_entry = None;
            true
        } else {
            false
        }
    }

    /// Install the opening-cutscene narration presenter with `pages` (the
    /// inline subtitle pages decoded from the scene MAN's cutscene-timeline
    /// script; see [`crate::man_field_scripts::collect_partition_narration`]).
    /// A presenter with no pages installs nothing - a scene that carries no
    /// inline narration simply never shows one. The host renders the active
    /// page from [`Self::cutscene_narration`]; [`Self::tick`] advances its
    /// per-page timer.
    pub fn open_cutscene_narration(&mut self, pages: Vec<String>) {
        if pages.is_empty() {
            return;
        }
        self.cutscene_narration = Some(crate::cutscene_narration::CutsceneNarration::new(pages));
    }

    /// `true` while the opening-cutscene narration is on screen (not yet
    /// stepped past its last page). Hosts gate the prologue hand-off on this:
    /// the narration plays first, the Rim Elm hand-off follows.
    pub fn cutscene_narration_active(&self) -> bool {
        self.cutscene_narration
            .as_ref()
            .is_some_and(|n| !n.is_complete())
    }

    /// The full-scene colour grade the current scene renders through, or
    /// `None` for the natural-colour default. The opening prologue cutscene
    /// (`opdeene`, "It was the Seru.") returns
    /// [`crate::fade::ColorGrade::PROLOGUE_SEPIA`] so the whole 3D scene draws
    /// in warm gold monochrome while the narration text stays white; every
    /// other scene (incl. the Rim Elm hand-off `town01`) is ungraded. Hosts
    /// stage this into the renderer each frame (e.g. `set_color_grade`).
    ///
    /// This mirrors retail keying the dim-ambient + gold far-colour grade on
    /// the cutscene scene and clearing it for the interactive field - see
    /// [`crate::fade::ColorGrade`] for the traced GTE mechanism.
    pub fn scene_color_grade(&self) -> Option<crate::fade::ColorGrade> {
        if self.active_scene_label == legaia_asset::new_game::OPENING_CUTSCENE_SCENE {
            Some(crate::fade::ColorGrade::PROLOGUE_SEPIA)
        } else {
            None
        }
    }

    /// Skip the active narration to its next page (a confirm press). Clears
    /// the presenter once it advances past the last page. Returns `true` while
    /// narration is still on screen, `false` once it completes (so the host
    /// lets the confirm fall through to [`Self::take_prologue_handoff`]).
    pub fn skip_cutscene_narration(&mut self) -> bool {
        let Some(narration) = self.cutscene_narration.as_mut() else {
            return false;
        };
        let still_active = narration.skip_page();
        if !still_active {
            self.cutscene_narration = None;
        }
        still_active
    }

    /// Arm the prologue cutscene -> Rim Elm handoff.
    ///
    /// In retail the opening cutscene scene `opdeene` runs a scripted
    /// timeline (a field-VM record in the MAN's third record partition)
    /// that ends with `GFLAG_SET 26` - field-VM op `0x2E` with operand
    /// `0x1A`, which sets bit 26 (`0x0400_0000`) of the scratchpad flag
    /// word `_DAT_1F800394` (the engine's [`Self::story_flags`]) right
    /// after staging the closing camera + actor moves. Once that bit is
    /// set, the per-frame field controller `FUN_801D1344` waits for the
    /// player's confirm press and then issues a name-based scene-change
    /// packet to `town01` (see [`Self::take_prologue_handoff`]).
    ///
    /// The engine doesn't yet replay that cutscene timeline (only record
    /// 0 of the scene runs), so callers arm the bit explicitly when they
    /// enter `opdeene` live. This sets exactly the flag the retail
    /// `GFLAG_SET 26` would, so the downstream gate stays faithful.
    // REF: FUN_801D1344
    pub fn arm_prologue_handoff(&mut self) {
        self.story_flags |= PROLOGUE_HANDOFF_FLAG;
    }

    /// Arm the prologue -> Rim Elm hand-off **only when** the scene's MAN
    /// cutscene timeline actually issues the `GFLAG_SET 26` write the retail
    /// hand-off gate waits on.
    ///
    /// This is the data-driven companion to [`Self::arm_prologue_handoff`]:
    /// instead of blindly raising the bit on scene entry, the engine walks
    /// the scene MAN's partition-2 records (the cutscene timelines) for a
    /// `GFLAG_SET` of [`PROLOGUE_HANDOFF_BIT`] via
    /// [`crate::man_field_scripts::walk_partition_gflag_sites`] and arms only
    /// when it is present - so a cutscene scene that never issues that write
    /// can never produce a false hand-off. Returns `true` when it armed.
    ///
    /// The engine doesn't yet tick `opdeene`'s partition-2 cutscene records
    /// frame-by-frame (the camera + actor `MoveTo`s that precede the flag
    /// write), so this confirms the arming op exists in the real disc
    /// bytecode and sets exactly the bit the executed `GFLAG_SET` would.
    /// Pairs with [`Self::take_prologue_handoff`] for the confirm-press gate.
    // REF: FUN_801D1344
    pub fn arm_prologue_handoff_from_man(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> bool {
        let armed = crate::man_field_scripts::walk_partition_gflag_sites(man_file, man, 2)
            .iter()
            .any(|s| s.set && s.bit as u32 == PROLOGUE_HANDOFF_BIT);
        if armed {
            self.arm_prologue_handoff();
        }
        armed
    }

    /// Poll the prologue cutscene -> Rim Elm handoff gate.
    ///
    /// Faithful port of the one-shot block in `FUN_801D1344`:
    ///
    /// ```c
    /// if (_DAT_8007b868 == 0 && (_DAT_1f800394 & 0x4000000) && (_DAT_8007b850 & 0x100)) {
    ///     ... fade; town01 entry coords (0xec0, 0x2dc0); ...
    ///     _DAT_1f800394 &= 0xfbffffff;            // fire-once: clear bit 26
    ///     func_0x8001fd44(s_town01_801ce82c, 3);  // name-based scene change
    /// }
    /// ```
    ///
    /// Returns the handoff target scene ([`legaia_asset::new_game::OPENING_SCENE`]
    /// = `town01`) once - when the active scene is the prologue cutscene
    /// ([`legaia_asset::new_game::OPENING_CUTSCENE_SCENE`] = `opdeene`),
    /// the trigger bit is set ([`Self::arm_prologue_handoff`]), and the
    /// caller reports a confirm-button press this frame. Clears the bit so
    /// it fires once, exactly as retail clears `0x4000000`. Returns `None`
    /// otherwise. The host issues the actual scene change (the engine's
    /// equivalent of the scene-change packet) on a `Some`.
    // REF: FUN_801D1344
    // REF: FUN_8001FD44
    pub fn take_prologue_handoff(&mut self, confirm: bool) -> Option<&'static str> {
        // The opening narration plays first: while its subtitle pages are on
        // screen the confirm press skips pages (see
        // [`Self::skip_cutscene_narration`]) and never reaches this gate, so
        // the hand-off can only fire once the narration has finished.
        if confirm
            && !self.cutscene_narration_active()
            && self.story_flags & PROLOGUE_HANDOFF_FLAG != 0
            && self.active_scene_label == legaia_asset::new_game::OPENING_CUTSCENE_SCENE
        {
            self.story_flags &= !PROLOGUE_HANDOFF_FLAG;
            // Mark the upcoming `town01` entry as the new-game opening so it
            // installs the opening cutscene timeline (which opens name entry at
            // its pinned op-`0x49`); a normal `town01` visit never sets this.
            self.entering_town01_opening = true;
            Some(legaia_asset::new_game::OPENING_SCENE)
        } else {
            None
        }
    }

    /// Load the opening-cutscene timeline record from the scene MAN as a
    /// spawned field-VM context, so its camera path + actor moves play and the
    /// closing `GFLAG_SET 26` fires by execution.
    ///
    /// Finds the partition-2 (cutscene-timeline) record that issues the
    /// [`PROLOGUE_HANDOFF_BIT`] `GFLAG_SET` via
    /// [`crate::man_field_scripts::walk_partition_gflag_sites`], resolves its
    /// named-record span with
    /// [`crate::man_field_scripts::partition_record_span`] (the partition-2
    /// header decode), and slices the record body from its `script_start` so
    /// relative jumps wrap against the record base (retail
    /// `buffer_base = script_start`). The spawned context begins at the
    /// record's first-opcode offset (`pc0`).
    ///
    /// Returns `true` when a timeline was installed. Returns `false` (no
    /// matching record, or span resolution failed) so the caller can fall back
    /// to the static hand-off arm ([`Self::arm_prologue_handoff_from_man`]).
    // REF: FUN_8003BDE0
    // REF: FUN_801D1344
    pub fn load_cutscene_timeline_from_man(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> bool {
        let Some(record_idx) =
            crate::man_field_scripts::walk_partition_gflag_sites(man_file, man, 2)
                .into_iter()
                .find(|s| s.set && s.bit as u32 == PROLOGUE_HANDOFF_BIT)
                .map(|s| s.record)
        else {
            return false;
        };
        if !self.install_cutscene_timeline_record(man_file, man, 2, record_idx, false) {
            return false;
        }
        // opdeene's terminal `GFLAG_SET 26` arms the `town01` hand-off; mark the
        // timeline so its completion / frame-cap safety net does so.
        if let Some(tl) = self.cutscene_timeline.take() {
            self.cutscene_timeline = Some(tl.arming_prologue_handoff());
        }
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
    // REF: FUN_8003BDE0
    pub fn install_town01_opening_timeline(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) -> bool {
        if !self.install_cutscene_timeline_record(
            man_file,
            man,
            2,
            Self::TOWN01_OPENING_TIMELINE_RECORD,
            false,
        ) {
            return false;
        }
        self.prologue_naming_pending = true;
        self.prologue_naming_armed = false;
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
    /// base), NOP-fills the inline narration spans (they are data the separate
    /// [`crate::cutscene_narration::CutsceneNarration`] presenter consumes, not
    /// field-VM opcodes - overwriting each with the 1-byte NOP `0x21` is
    /// offset-preserving so the camera / move / flag ops keep their offsets),
    /// and installs the timeline with `trace` controlling op-stream recording.
    ///
    /// Returns `true` when a timeline was installed; `false` when the span can't
    /// be resolved.
    // REF: FUN_8003BDE0
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
        let mut body = body.to_vec();
        for block in legaia_asset::cutscene_text::parse_narration(&body) {
            let (start, end) = block.byte_span();
            let end = end.min(body.len());
            if start < end {
                for b in &mut body[start..end] {
                    *b = 0x21;
                }
            }
        }
        let mut tl = crate::cutscene_timeline::CutsceneTimeline::new(body, pc0);
        if trace {
            tl = tl.with_trace();
        }
        self.cutscene_timeline = Some(tl);
        // Spawn the per-actor channels (one per partition-1 placement,
        // retail `FUN_8003AEB0`'s spawn loop) so the timeline's cross-context
        // pokes land on real per-actor contexts - the vignette mechanism.
        self.field_channels = crate::field_channels::spawn_channels(man_file, man);
        self.field_channels_man = Some(std::sync::Arc::new(man.to_vec()));
        self.field_npc_anim_cues.clear();
        true
    }

    /// `true` while the opening-cutscene timeline is still executing (installed
    /// and not yet complete). Diagnostics / tests read this; the hand-off gate
    /// itself keys off the scratchpad flag the timeline sets, not this.
    pub fn cutscene_timeline_active(&self) -> bool {
        self.cutscene_timeline
            .as_ref()
            .is_some_and(|t| !t.is_done())
    }

    /// Step the opening-cutscene timeline one frame.
    ///
    /// Runs the spawned cutscene context ([`crate::cutscene_timeline`]) through
    /// the field VM until it yields, waits, or completes - mirroring retail's
    /// run-until-`YIELD`-per-frame dispatch. Camera Configure (`0x45`) and
    /// actor MoveTo (`0x23`) ops emit the same [`crate::field_events::FieldEvent`]s
    /// the runtime camera folds in; the closing `GFLAG_SET 26` writes the
    /// hand-off bit through the same host path the main field VM uses, so the
    /// `town01` hand-off arms by execution.
    ///
    /// Bounded two ways so real disc bytecode can never hang the tick or stall
    /// the prologue:
    /// - a per-frame step budget caps a non-yielding loop;
    /// - a frame cap forces completion if the timeline never reaches its
    ///   closing op (e.g. it hits an op this port cannot advance past); for the
    ///   `opdeene` prologue ([`crate::cutscene_timeline::CutsceneTimeline::arms_prologue_handoff`])
    ///   the hand-off is then armed statically as a safety net.
    ///
    /// The `town01` opening timeline parks on op-`0x49` STATE_RESUME to open the
    /// name-entry overlay (via the op-49 host hooks); while that overlay is up
    /// the timeline is frozen (no step, no frame-cap progress) so the cutscene
    /// stays suspended exactly as retail's STATE_RESUME does.
    ///
    /// No-op when no timeline is installed or it has already completed.
    // REF: FUN_8003BDE0
    pub fn step_cutscene_timeline(&mut self) {
        let Some(mut tl) = self.cutscene_timeline.take() else {
            return;
        };
        if tl.done {
            self.cutscene_timeline = Some(tl);
            return;
        }
        // Freeze the timeline while the name-entry overlay it spawned is open:
        // its op-`0x49` STATE_RESUME is suspended until the player commits a
        // name, so neither the VM nor the frame cap advances meanwhile.
        if self.name_entry_active() {
            self.cutscene_timeline = Some(tl);
            return;
        }
        tl.frames = tl.frames.saturating_add(1);
        self.in_cutscene_timeline = true;
        let mut channels = std::mem::take(&mut self.field_channels);
        let channel_pre_pos: Vec<(u16, u16)> = channels
            .iter()
            .map(|c| (c.ctx.world_x, c.ctx.world_z))
            .collect();
        {
            let mut host = FieldHostImpl { world: self };
            let mut budget = CUTSCENE_TIMELINE_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = tl.pc;
                let opcode_byte = tl.bytecode.get(pc).copied().unwrap_or(0);
                // Cross-context dispatch (`0x80`-bit ops): resolve the target
                // byte to a spawned per-actor channel (`ctx[+0x50] == target`,
                // retail `FUN_8003C83C`) and run the op against THAT context -
                // this is how the timeline cues the vignette actors. `0xF8`
                // (player anchor) / `0xFB` (system) / unmatched ids keep the
                // timeline's own context, the prior approximation.
                let target = vm::field::peek_extended(&tl.bytecode, pc).and_then(|t| {
                    crate::field_channels::resolve_target(&channels, t).map(|ci| (t, ci))
                });
                let result = if let Some((_, ci)) = target {
                    host.world.executing_channel = Some(channels[ci].placement_index as u8);
                    // The timeline is the acquirer: it halt-acquired these
                    // channels earlier (the `4C 85` freeze sweep) and now
                    // drives them beat by beat. A poke from the owner is the
                    // resume signal, so clear the target's halt bit before the
                    // op runs - otherwise the dispatcher prelude parks the
                    // caller on its own frozen actor and the camera beats
                    // after the sweep never play.
                    channels[ci].ctx.flags &= !0x400;
                    let r = vm::field::step_with_caller(
                        &mut host,
                        &mut channels[ci].ctx,
                        &mut tl.ctx,
                        false,
                        &tl.bytecode,
                        pc,
                    );
                    host.world.executing_channel = None;
                    r
                } else {
                    vm::field::step(&mut host, &mut tl.ctx, &tl.bytecode, pc)
                };
                let (mut next_pc, kind, mut stop) = match result {
                    FieldStepResult::Advance { next_pc } => (
                        next_pc,
                        crate::cutscene_timeline::TraceResult::Advance,
                        false,
                    ),
                    FieldStepResult::Yield { resume_pc } => (
                        resume_pc,
                        crate::cutscene_timeline::TraceResult::Yield,
                        true,
                    ),
                    // WAIT_FRAMES and conditional holds return `Halt` at the
                    // same PC: end the frame and resume there next tick.
                    FieldStepResult::Halt { final_pc } => {
                        (final_pc, crate::cutscene_timeline::TraceResult::Halt, true)
                    }
                    // An op this port can't advance past: stop and let the
                    // safety net below arm the hand-off.
                    FieldStepResult::Pending { pc, .. } => {
                        (pc, crate::cutscene_timeline::TraceResult::Pending, true)
                    }
                    FieldStepResult::Unknown { pc, .. } => {
                        (pc, crate::cutscene_timeline::TraceResult::Unknown, true)
                    }
                };
                // Step past the timeline's conditional-wait parks. Retail Halts
                // at PC on a handshake the engine doesn't fully model - a flag
                // a spawned sub-context sets - so advancing by the op's encoded
                // width (these flag-tests read one operand byte,
                // `header_size + 1`) keeps the timeline flowing toward its
                // camera / move / STATE_RESUME ops. The handshake ops are the
                // flag-tests `0x2D` (LFLAG), `0x30` (GFLAG), `0x33` (CFLAG) and
                // the `0x4C` nibble-C `script_alloc` / globals-gate - all
                // 2-byte (3 extended), so a fixed step-past is correct-width
                // for them, INCLUDING their cross-context (`0x80`-bit) forms
                // (e.g. `B3 <id> <bit>` = the timeline waiting on a vignette
                // channel's completion flag). Other cross-context ops (the
                // `4C`/`23` action pokes) are NOT stepped past - they run
                // against the target and advance by their real width. Two
                // parks are kept: `0x4A` WAIT_FRAMES (a real timed wait that
                // plays out via the wait accumulator) and `0x49` STATE_RESUME
                // (the name-entry suspend, driven by the op-49 host hooks).
                let op = opcode_byte & 0x7F;
                let is_flag_test_handshake =
                    matches!(op, 0x2D | 0x30 | 0x33) || (op == 0x4C && target.is_none());
                if matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                    && next_pc == pc
                    && op != 0x4A
                    && op != 0x49
                    && (target.is_none() || is_flag_test_handshake)
                {
                    let header_size = if opcode_byte & 0x80 != 0 { 2 } else { 1 };
                    next_pc = pc + header_size + 1;
                    stop = false;
                }
                if tl.trace_enabled {
                    tl.trace.push(crate::cutscene_timeline::TraceEntry {
                        pc,
                        opcode_byte,
                        opcode: opcode_byte & 0x7F,
                        next_pc,
                        result: kind,
                    });
                }
                tl.pc = next_pc;
                if matches!(
                    kind,
                    crate::cutscene_timeline::TraceResult::Pending
                        | crate::cutscene_timeline::TraceResult::Unknown
                ) {
                    tl.done = true;
                }
                if stop {
                    break;
                }
            }
        }
        // Timeline pokes that moved a channel context (cross-context MoveTo)
        // write through to the field NPC render/probe state.
        for (c, pre) in channels.iter().zip(channel_pre_pos) {
            if (c.ctx.world_x, c.ctx.world_z) != pre {
                self.field_npc_positions.insert(
                    c.placement_index as u8,
                    (c.ctx.world_x as i16, c.ctx.world_z as i16),
                );
            }
        }
        self.field_channels = channels;
        self.in_cutscene_timeline = false;
        // Frame cap: real disc bytecode must never hang the tick. The opdeene
        // prologue gets the generous cap - its record arms the hand-off bit
        // at its TOP (`GFLAG_SET 26` at body `+0x17`) and then STAGES the
        // vignettes for the narration's duration, so "bit armed" must not
        // complete it (that early-out silently dropped the whole vignette
        // choreography - camera beats, actor-channel pokes - after two ops).
        let cap = if tl.arms_prologue_handoff {
            PROLOGUE_TIMELINE_MAX_FRAMES
        } else {
            CUTSCENE_TIMELINE_MAX_FRAMES
        };
        if tl.frames >= cap {
            tl.done = true;
        }
        if tl.arms_prologue_handoff {
            // Safety net: if the record terminated without executing its
            // `GFLAG_SET 26`, arm the hand-off statically so the prologue
            // can't stall.
            if tl.done && self.story_flags & PROLOGUE_HANDOFF_FLAG == 0 {
                self.arm_prologue_handoff();
            }
            self.cutscene_timeline = Some(tl);
        } else if tl.done {
            // town01 opening timeline finished (or capped): drop it so the view
            // reverts from the cutscene camera to normal field gameplay.
            self.cutscene_timeline = None;
            // The opening choreography `MoveTo`s the townsfolk to the off-map
            // hide box to clear the establishing shot. Nothing reloads the
            // scene between the cutscene and free-roam, so those position
            // overrides must be dropped here or the field render draws each
            // hidden NPC off-screen (the "town NPCs vanish after New Game"
            // symptom, only on the prologue path that installs this timeline).
            self.restore_hidden_field_npcs();
        } else {
            self.cutscene_timeline = Some(tl);
        }
    }

    /// Un-park every field NPC a cutscene left at the off-map hide box
    /// ([`crate::world::FIELD_OFFMAP_HIDE_XZ`]), dropping its
    /// [`Self::field_npc_positions`] / [`Self::field_npc_headings`] overrides so
    /// the field render falls back to the NPC's MAN spawn tile.
    ///
    /// The `town01` opening cutscene hides the townsfolk at that box for its
    /// establishing shot; since no scene reload sits between the cutscene and
    /// free-roam, the overrides have to be cleared explicitly when the timeline
    /// completes (retail restores the town when control returns). No-op when
    /// nothing is parked there.
    fn restore_hidden_field_npcs(&mut self) {
        let hide = crate::world::FIELD_OFFMAP_HIDE_XZ;
        let restored: Vec<u8> = self
            .field_npc_positions
            .iter()
            .filter(|&(_, &(x, z))| x == hide && z == hide)
            .map(|(&slot, _)| slot)
            .collect();
        for slot in restored {
            self.field_npc_positions.remove(&slot);
            self.field_npc_headings.remove(&slot);
        }
    }

    /// Step every live per-actor field-VM channel one frame slice.
    ///
    /// Mirrors the retail per-actor script ticker (`FUN_80039B7C`, the
    /// `+0x9C == 0` branch): each context runs ops until it yields, parks on
    /// a conditional hold, **or executes a `0x21` NOP** - the NOP is the
    /// per-frame pacing point in retail (`if (bVar1 == 0x21) break`), which is
    /// why placement idle loops are written `21 21 26 FE FF`. A parked channel
    /// (flag-test `Halt`) retries the same PC next frame; a cross-context poke
    /// (an extended op naming another channel's script id) resolves the target
    /// context and runs against it, parking the caller if the target is
    /// halted - the retail synchronisation primitive between the cutscene
    /// timeline and its vignette actors.
    ///
    /// Channels are cutscene-scoped: they spawn with a cutscene timeline
    /// ([`Self::install_cutscene_timeline_record`]) and drop when it
    /// completes, so normal field NPC behaviour (waypoint motion) is
    /// untouched outside cutscenes.
    ///
    /// After stepping, each channel whose context position changed writes
    /// through to [`Self::field_npc_positions`] so the field render / probes
    /// follow the scripted move.
    // PORT: FUN_80039B7C (per-actor frame-slice loop; NOP break + halt park)
    // REF: FUN_8003C83C (cross-context target resolve)
    pub fn step_field_channels(&mut self) {
        if self.field_channels.is_empty() {
            return;
        }
        if !self.cutscene_timeline_active() {
            self.field_channels.clear();
            self.field_channels_man = None;
            return;
        }
        let Some(man) = self.field_channels_man.clone() else {
            return;
        };
        let mut channels = std::mem::take(&mut self.field_channels);
        let pre_pos: Vec<(u16, u16)> = channels
            .iter()
            .map(|c| (c.ctx.world_x, c.ctx.world_z))
            .collect();
        for i in 0..channels.len() {
            if channels[i].done {
                continue;
            }
            if man.len() <= channels[i].record_offset {
                channels[i].done = true;
                continue;
            }
            // A halted channel is SUSPENDED - the halt bit persists until an
            // explicit un-halt (the op-0x32 CFLAG_CLR bit-10 carve-out, the
            // one cross-context op allowed against a halted target). This is
            // the timeline's freeze/unfreeze choreography primitive: a
            // `4C 85` halt-acquire suspends the actor, `B3 <id> 0A` verifies
            // the suspension, `B2 <id> 0A` resumes it for its next beat.
            // (Autonomous frame pacing uses the `0x21` NOP break instead of
            // yields, so suspension never races normal idling.)
            if channels[i].ctx.is_halted() {
                continue;
            }
            let mut budget = FIELD_CHANNEL_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = channels[i].pc;
                let record_offset = channels[i].record_offset;
                let bc = &man[record_offset..];
                let Some(&opcode_byte) = bc.get(pc) else {
                    channels[i].done = true;
                    break;
                };
                // Cross-context poke: resolve the extended target to another
                // channel and run the op against that context.
                let target = vm::field::peek_extended(bc, pc).and_then(|t| {
                    let ci = crate::field_channels::resolve_target(&channels, t)?;
                    (ci != i).then_some(ci)
                });
                self.executing_channel = Some(match target {
                    Some(ci) => channels[ci].placement_index as u8,
                    None => channels[i].placement_index as u8,
                });
                let result = {
                    let mut host = FieldHostImpl { world: self };
                    match target {
                        Some(ci) => {
                            // Two disjoint &mut contexts out of the vec.
                            let (lo, hi) = channels.split_at_mut(i.max(ci));
                            let (target_ctx, caller_ctx) = if ci < i {
                                (&mut lo[ci].ctx, &mut hi[0].ctx)
                            } else {
                                (&mut hi[0].ctx, &mut lo[i].ctx)
                            };
                            let bc = &man[record_offset..];
                            vm::field::step_with_caller(
                                &mut host, target_ctx, caller_ctx, false, bc, pc,
                            )
                        }
                        None => {
                            let bc = &man[record_offset..];
                            vm::field::step(&mut host, &mut channels[i].ctx, bc, pc)
                        }
                    }
                };
                self.executing_channel = None;
                match result {
                    FieldStepResult::Advance { next_pc } => {
                        let stalled = next_pc == pc;
                        channels[i].pc = next_pc;
                        // Retail frame-slice pacing: a NOP ends the slice
                        // after executing (FUN_80039B7C `bVar1 == 0x21`), and
                        // a non-advancing PC ends it defensively.
                        if opcode_byte & 0x7F == 0x21 || stalled {
                            break;
                        }
                    }
                    FieldStepResult::Yield { resume_pc } => {
                        channels[i].pc = resume_pc;
                        break;
                    }
                    FieldStepResult::Halt { final_pc } => {
                        // Parked (conditional hold / halted cross-target):
                        // retry the same PC next frame.
                        channels[i].pc = final_pc;
                        break;
                    }
                    FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                        // An op this port can't advance past inside a channel
                        // script: retire the channel (it stays resolvable as a
                        // cross-context target).
                        channels[i].pc = pc;
                        channels[i].done = true;
                        break;
                    }
                }
            }
        }
        // Write scripted moves through to the field NPC render/probe state.
        for (c, pre) in channels.iter().zip(pre_pos) {
            if (c.ctx.world_x, c.ctx.world_z) != pre {
                self.field_npc_positions.insert(
                    c.placement_index as u8,
                    (c.ctx.world_x as i16, c.ctx.world_z as i16),
                );
            }
        }
        self.field_channels = channels;
    }

    /// Begin running an inline interaction script through the field VM (the
    /// faithful dialogue path - see [`crate::inline_dialogue`]). `inline` is the
    /// actor's interaction-script bytes (e.g. [`DialogRequest::inline`]), which
    /// begin at the first `0x1F` text segment. Replaces any running script.
    pub fn start_inline_dialogue(&mut self, inline: Vec<u8>) {
        self.inline_dialogue = Some(crate::inline_dialogue::InlineDialogue::from_inline(inline));
    }

    /// Start the inline-script runner on a full interaction record, executing the
    /// prologue from `entry_pc` (the record's `script_pc0`) before the first text
    /// segment at `first_segment`. The prologue's `SysFlag.Test`/`JmpRel` chain
    /// selects which segment the box opens at per story state; if it can't reach a
    /// segment the runner falls back to `first_segment`. See
    /// [`crate::inline_dialogue::InlineDialogue::with_prologue`].
    pub fn start_inline_dialogue_with_prologue(
        &mut self,
        body: Vec<u8>,
        entry_pc: usize,
        first_segment: usize,
    ) {
        self.inline_dialogue = Some(crate::inline_dialogue::InlineDialogue::with_prologue(
            std::sync::Arc::new(body),
            entry_pc,
            first_segment,
        ));
    }

    /// Advance the running inline interaction script one tick. Between text
    /// boxes the field VM executes the control bytecode (prologue story-flag
    /// tests, `SET`/`CLEAR` flag ops, scene changes) through the World host; at
    /// each `0x1F` segment it opens / ticks a dialog box. `confirm` dismisses the
    /// current box, or commits a menu choice - applying that option's relative
    /// jump (`FUN_80038050`) and handing the branch to the VM so its side
    /// effects run before the reply. `up`/`down` move a menu cursor. No-op when
    /// no inline dialogue is running.
    // PORT: FUN_80039B7C
    // REF: FUN_80038050 (the option-jump apply is delegated to OwnedDialogPanel::confirm_menu)
    // REF: FUN_8003CF7C (the inline fast-forward loop below subsumes retail's
    //      run-to-next-text helper: tick the field VM until `byte & 0x7F < 0x20`)
    pub fn step_inline_dialogue(&mut self, confirm: bool, up: bool, down: bool) {
        use crate::inline_dialogue::INLINE_DIALOGUE_STEP_BUDGET;
        let Some(mut id) = self.inline_dialogue.take() else {
            return;
        };
        if id.done {
            self.inline_dialogue = Some(id);
            return;
        }

        // A box is open: tick the typewriter + route input.
        if let Some(panel) = id.panel.as_mut() {
            if panel.menu_active() {
                if up {
                    panel.move_picker_cursor(-1);
                }
                if down {
                    panel.move_picker_cursor(1);
                }
            }
            panel.tick();
            if confirm {
                if panel.menu_active() {
                    // Commit the choice: apply the option's relative jump and
                    // resume the VM at the branch handler (its flag-sets /
                    // scene-change run before the reply box).
                    let choice = panel.picker_cursor();
                    let target = panel.picker().and_then(|pk| pk.jump_target(choice));
                    id.last_choice = Some(choice);
                    match target {
                        Some(t) => id.pc = t,
                        None => id.done = true,
                    }
                    id.panel = None;
                } else if panel.is_waiting_for_input() || panel.is_done() {
                    // Plain box dismissed: resume the VM just past this segment.
                    id.pc = panel.pc;
                    id.panel = None;
                }
            }
            self.inline_dialogue = Some(id);
            return;
        }

        // No box open: step the VM until the next text segment or an end.
        // Expose the record's NPC slot so the host's `0x4C 0x51` NPC-run hook
        // can route the prologue's walk ops to the interacted actor.
        self.stepping_inline_npc = id.npc_slot;
        let mut host = FieldHostImpl { world: self };
        let mut budget = INLINE_DIALOGUE_STEP_BUDGET;
        while budget > 0 {
            budget -= 1;
            let b = id.bytecode.get(id.pc).copied().unwrap_or(0);
            // Retail SM transition test: a byte with `& 0x7F < 0x20` is a text
            // lead (`0x1F`) or a terminator (`0x00..0x1E`), not an opcode.
            if b & 0x7F < 0x20 {
                if b == 0x1F {
                    // Reached a text segment. A prologue (if any) selected it, so
                    // retire the fallback and open the box here.
                    id.fallback_segment_pc = None;
                    id.panel = Some(crate::dialog::OwnedDialogPanel::at_segment(
                        std::sync::Arc::clone(&id.bytecode),
                        id.pc,
                    ));
                    break;
                }
                // A non-`0x1F` terminator before any box opened: if a prologue
                // fallback is pending, resume at the first segment (so the box
                // still shows); otherwise the conversation ends.
                if let Some(fb) = id.fallback_segment_pc.take() {
                    id.pc = fb;
                    continue;
                }
                id.done = true;
                break;
            }
            match vm::field::step(&mut host, &mut id.ctx, &id.bytecode, id.pc) {
                FieldStepResult::Advance { next_pc } => id.pc = next_pc,
                FieldStepResult::Yield { resume_pc } => id.pc = resume_pc,
                // A wait/hold, an unhandled op, or an end: stop. (Unlike the
                // cutscene timeline the runner does not force-advance past a
                // Halt - an inline interaction script that can't proceed ends.)
                // While a prologue is still running (no box opened yet), a halt
                // means the prologue can't proceed - fall back to the first
                // segment so the dialogue is never worse than the truncated path.
                FieldStepResult::Halt { .. }
                | FieldStepResult::Pending { .. }
                | FieldStepResult::Unknown { .. } => {
                    if let Some(fb) = id.fallback_segment_pc.take() {
                        id.pc = fb;
                        continue;
                    }
                    id.done = true;
                    break;
                }
            }
        }
        self.stepping_inline_npc = None;
        self.inline_dialogue = Some(id);
    }

    /// Live-loop bridge for the inline-script runner: when [`Self::use_vm_dialogue`]
    /// is set, this starts the runner the frame a field dialogue opens (from
    /// [`Self::current_dialog`]'s inline buffer), steps it from the current pad
    /// edges (Cross/Circle = confirm, Up/Down = menu cursor), and tears it down
    /// (clearing `current_dialog`) when the conversation ends. No-op when the
    /// flag is off, so the default simplified path is untouched.
    pub fn drive_inline_dialogue(&mut self) {
        if !self.use_vm_dialogue {
            return;
        }
        // Start the runner the frame a dialogue request appears. When the opened
        // NPC carries a prologue record, run it from the entry PC so the
        // interaction prologue (segment selection) executes; otherwise start at
        // the first segment from the request's inline buffer.
        if self.inline_dialogue.is_none() {
            if let Some(prologue) = self.active_inline_prologue.take() {
                let mut runner = crate::inline_dialogue::InlineDialogue::with_prologue(
                    std::sync::Arc::new(prologue.body),
                    prologue.entry_pc,
                    prologue.first_segment,
                );
                runner.npc_slot = self.active_inline_slot.take();
                self.inline_dialogue = Some(runner);
            } else if let Some(req) = self.current_dialog.as_ref() {
                if !req.inline.is_empty() {
                    let slot = self.active_inline_slot.take();
                    self.start_inline_dialogue(req.inline.clone());
                    if let Some(runner) = self.inline_dialogue.as_mut() {
                        runner.npc_slot = slot;
                    }
                } else {
                    return;
                }
            } else {
                return;
            }
        }
        let confirm = self.input.just_pressed(input::PadButton::Cross)
            || self.input.just_pressed(input::PadButton::Circle);
        let up = self.input.just_pressed(input::PadButton::Up);
        let down = self.input.just_pressed(input::PadButton::Down);
        self.step_inline_dialogue(confirm, up, down);
        if self.inline_dialogue.as_ref().is_some_and(|d| d.is_done()) {
            self.inline_dialogue = None;
            self.current_dialog = None;
            self.pending_field_events
                .push(crate::field_events::FieldEvent::DialogDismissed);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::world::{FIELD_OFFMAP_HIDE_XZ, World};

    #[test]
    fn restore_hidden_field_npcs_unparks_only_the_hide_box() {
        let mut w = World::default();
        // Two townsfolk parked off-map by the opening cutscene, one NPC left at
        // a real tile (e.g. a mid-scene walker), plus stale headings for all.
        let hide = FIELD_OFFMAP_HIDE_XZ;
        w.field_npc_positions.insert(1, (hide, hide));
        w.field_npc_positions.insert(2, (hide, hide));
        w.field_npc_positions.insert(3, (2880, 5440));
        w.field_npc_headings.insert(1, 0x800);
        w.field_npc_headings.insert(2, 0x000);
        w.field_npc_headings.insert(3, 0x400);

        w.restore_hidden_field_npcs();

        // The hide-box NPCs lose their overrides (render falls back to the MAN
        // spawn); the on-tile NPC and its heading are untouched.
        assert!(!w.field_npc_positions.contains_key(&1));
        assert!(!w.field_npc_positions.contains_key(&2));
        assert!(!w.field_npc_headings.contains_key(&1));
        assert!(!w.field_npc_headings.contains_key(&2));
        assert_eq!(w.field_npc_positions.get(&3), Some(&(2880, 5440)));
        assert_eq!(w.field_npc_headings.get(&3), Some(&0x400));
    }

    #[test]
    fn restore_hidden_field_npcs_noop_when_none_parked() {
        let mut w = World::default();
        w.field_npc_positions.insert(5, (1000, 2000));
        w.restore_hidden_field_npcs();
        assert_eq!(w.field_npc_positions.get(&5), Some(&(1000, 2000)));
    }

    #[test]
    fn scene_color_grade_only_on_the_prologue_cutscene() {
        let mut w = World::new();
        // No scene / arbitrary field scene -> ungraded natural colour.
        assert!(w.scene_color_grade().is_none());
        w.set_active_scene_label("town01");
        assert!(
            w.scene_color_grade().is_none(),
            "the Rim Elm hand-off renders in full colour"
        );
        // The opdeene prologue cutscene renders through the warm sepia grade.
        w.set_active_scene_label(legaia_asset::new_game::OPENING_CUTSCENE_SCENE);
        assert_eq!(
            w.scene_color_grade(),
            Some(crate::fade::ColorGrade::PROLOGUE_SEPIA)
        );
    }
}
