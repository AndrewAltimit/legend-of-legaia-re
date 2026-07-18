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
        // Per-scene crawl geometry / speed (capture-pinned; see
        // `RollerParams::for_scene`).
        let params = crate::cutscene_narration::RollerParams::for_scene(&self.active_scene_label);
        self.cutscene_narration = Some(crate::cutscene_narration::CutsceneNarration::with_params(
            pages, params,
        ));
        // Monotonic "which crawl block is showing" counter. Because a
        // non-blocking crawl lets the next block open the very tick the prior
        // one scrolls out (continuous crawl, no blank frame), a rising-edge
        // `active && !was_active` observer can miss a block; observers count
        // this instead. Never reset within a scene's opening.
        self.cutscene_narration_seq = self.cutscene_narration_seq.wrapping_add(1);
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
    /// scenes (`opdeene` / `opstati` / `opurud`) return
    /// [`crate::fade::ColorGrade::PROLOGUE_SEPIA`] so the whole 3D scene draws
    /// through the warm gold multiply tint while the narration text stays
    /// white - the
    /// retail cold-boot capture shows the grade persisting across all three
    /// legs and dropping for the full-colour `map01` fly-in + `town01`. Hosts
    /// stage this into the renderer each frame (e.g. `set_color_grade`).
    ///
    /// This mirrors retail keying the dim-ambient + gold far-colour grade on
    /// the cutscene scenes and clearing it for the interactive field - see
    /// [`crate::fade::ColorGrade`] for the traced GTE mechanism.
    pub fn scene_color_grade(&self) -> Option<crate::fade::ColorGrade> {
        if matches!(
            self.active_scene_label.as_str(),
            "opdeene" | "opstati" | "opurud"
        ) {
            Some(crate::fade::ColorGrade::PROLOGUE_SEPIA)
        } else {
            None
        }
    }

    /// The per-render-node depth-cue pull the current scene renders through,
    /// or `None` for the identity default. Keyed on the same prologue scene
    /// gate as [`Self::scene_color_grade`]: retail stages a gold DPCS far
    /// colour + depth-graded `IR0` per render node across the opening's
    /// narration beats (crushing far scenery toward gold) and neutral values
    /// on the interactive field, where the cue is the identity. Hosts stage
    /// this each frame (`set_depth_cue_ramp` / `clear_depth_cue_ramp`) - see
    /// [`crate::fade::DepthCueRamp`] for the traced mechanism.
    pub fn scene_depth_cue(&self) -> Option<crate::fade::DepthCueRamp> {
        self.scene_color_grade()
            .map(|_| crate::fade::DepthCueRamp::PROLOGUE_GOLD)
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
    /// Returns the skip target scene ([`legaia_asset::new_game::OPENING_SCENE`]
    /// = `town01`) once - when the opening cutscene chain is playing
    /// ([`Self::opening_chain_active`], set at the `opdeene` entry and carried
    /// through its `opstati` / `opurud` legs), the trigger bit is set
    /// ([`Self::arm_prologue_handoff`] - `opdeene`'s timeline raises it near
    /// its top, so the skip is available almost immediately), and the caller
    /// reports a confirm-button press this frame. This is the retail
    /// intro-SKIP: the packet fires mid-narration too (the roller is
    /// timer-driven, not confirm-paced). Clears the bit so it fires once,
    /// exactly as retail clears `0x4000000`, and tears down the playing
    /// narration / timeline. Returns `None` otherwise. The host issues the
    /// actual scene change (the engine's equivalent of the scene-change
    /// packet) on a `Some`.
    // REF: FUN_801D1344
    // REF: FUN_8001FD44
    pub fn take_prologue_handoff(&mut self, confirm: bool) -> Option<&'static str> {
        if confirm && self.story_flags & PROLOGUE_HANDOFF_FLAG != 0 && self.opening_chain_active {
            self.story_flags &= !PROLOGUE_HANDOFF_FLAG;
            // Tear down whatever leg of the opening is mid-flight - the skip
            // abandons the remaining narration + choreography wholesale.
            self.cutscene_narration = None;
            self.cutscene_card = None;
            self.cutscene_timeline = None;
            self.pending_named_scene_transition = None;
            self.opening_chain_active = false;
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

    /// `true` when story flag `flag` is set in the partition-2 gate bitmap
    /// (retail `DAT_80085758`). That base is the **system-flag bank** the
    /// field VM's `0x50`/`0x60`/`0x70` SET/CLEAR/TEST opcodes operate on
    /// ([`Self::system_flags`], same `byte = flag >> 3`,
    /// `bit = 0x80 >> (flag & 7)` addressing - `FUN_8003BDE0`'s test), so the
    /// gate check and the VM writes share one store. It also sits at offset
    /// `0x158` of the `0x80085600..0x80085800` save-bitmap window
    /// ([`Self::story_flag_bits`]); the save/load paths sync that overlap so
    /// gate state persists (see [`Self::save_full`] / [`Self::load_full`]).
    // REF: FUN_8003BDE0
    pub fn p2_gate_flag_set(&self, flag: u16) -> bool {
        self.system_flag_test(flag)
    }

    /// Evaluate a partition-2 record's C1 / C2 story-flag gates: C1 blocks
    /// the spawn if ANY listed flag is set (the one-shot mechanism); C2
    /// requires ALL listed flags set. Empty lists pass.
    // REF: FUN_8003BDE0
    pub fn p2_record_gates_pass(&self, c1: &[u16], c2: &[u16]) -> bool {
        !c1.iter().any(|&f| self.p2_gate_flag_set(f))
            && c2.iter().all(|&f| self.p2_gate_flag_set(f))
    }

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
    // eval + context install; the retail ctx[+0x50] seat-position seed from
    // the header +0x22/+0x24 coords is carried by the walk-on trigger path's
    // spawn tile instead of a context field)
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
        self.install_cutscene_timeline_record(man_file, man, 2, record_idx, false)
    }

    /// Install a field-VM op-`0x44` SPAWN_RECORD request as a **concurrent
    /// helper context** ([`Self::helper_contexts`]): re-base the GLOBAL record
    /// index into partition 2 (`global - N0 - N1`, retail `FUN_8003BDE0`),
    /// check the record's C1/C2 story-flag gates, and push the record as an
    /// independent spawned context. Returns `true` when a context installed.
    ///
    /// The non-cutscene-class counterpart to [`Self::install_spawned_record`]:
    /// retail runs every spawned record as an independent field-VM context and
    /// only cutscene-class records seize the camera / lock locomotion, so an
    /// ordinary scene's mid-play helper spawn goes here - it executes its
    /// script (flag writes, channel pokes, moves) without the modal-timeline
    /// attributes.
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
        if self.helper_contexts.len() >= crate::world::SPAWNED_CONTEXT_SLOTS {
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
        self.helper_contexts
            .push(crate::cutscene_timeline::CutsceneTimeline::new(
                body.to_vec(),
                pc0,
            ));
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
        // Parked at an inline dialog box (a `0x1F` glyph segment the record's
        // own flow reached - e.g. the Mei walk-on beat's conversation). Tick
        // the typewriter and route pad input exactly as the inline-script
        // runner does ([`Self::step_inline_dialogue`]): Up/Down move a picker
        // cursor, confirm commits a choice (applying its relative jump) or
        // dismisses a finished box, resuming the timeline past the segment.
        // The park freezes the frame cap - a dialog waits on the player.
        if let Some(panel) = tl.dialog.as_mut() {
            let confirm = self.input.just_pressed(crate::input::PadButton::Cross)
                || self.input.just_pressed(crate::input::PadButton::Circle);
            if panel.menu_active() {
                if self.input.just_pressed(crate::input::PadButton::Up) {
                    panel.move_picker_cursor(-1);
                }
                if self.input.just_pressed(crate::input::PadButton::Down) {
                    panel.move_picker_cursor(1);
                }
            }
            panel.tick();
            if confirm {
                if panel.menu_active() {
                    // NB: unlike the inline runner's picker commit, the wrap
                    // map is NOT cleared here. A cutscene record's picker
                    // picks a branch of one linear scene (the Mei beat's
                    // mid-conversation choice); clearing the map let the
                    // record replay already-played choreography before
                    // re-wrapping. Re-emission menus live in interaction
                    // records (the inline runner), not timeline records.
                    let choice = panel.picker_cursor();
                    let target = panel.picker().and_then(|pk| pk.jump_target(choice));
                    match target {
                        Some(t) => tl.pc = t,
                        None => tl.done = true,
                    }
                    tl.dialog = None;
                } else if panel.is_waiting_for_input() || panel.is_done() {
                    tl.pc = panel.pc;
                    tl.dialog = None;
                }
            }
            if tl.dialog.is_some() && !tl.done {
                self.cutscene_timeline = Some(tl);
                return;
            }
            if tl.done {
                self.cutscene_timeline = None;
                self.restore_hidden_field_npcs();
                return;
            }
        }
        // Held at an inline narration block. Two hold shapes share
        // `narration_pc` (see [`crate::cutscene_timeline::CutsceneTimeline`]):
        // the LAST crawl block (blocking) waiting for its own roller to scroll
        // out before advancing, and any block reached while a PRIOR roller is
        // still scrolling (`narration_pending_open`) held so a second roller
        // does not stack. In both cases we wait for the active roller to
        // drain; retail's `FUN_80037174` clears the parent's halt bit when
        // every page has scrolled off.
        if let Some(block_pc) = tl.narration_pc {
            if self.cutscene_narration_active() {
                self.cutscene_timeline = Some(tl);
                return;
            }
            if tl.narration_pending_open {
                // Was waiting for a prior roller to drain before opening this
                // block: clear the hold and leave the PC AT the block op so the
                // loop below re-enters and opens it now that nothing stacks.
                tl.narration_pc = None;
                tl.narration_pending_open = false;
            } else {
                // This (last, blocking) block's own roller finished: advance
                // the PC past the block into the timeline's terminal ops.
                if let Some(site) = tl.narration_blocks.iter().find(|b| b.op_offset == block_pc) {
                    tl.pc = site.end;
                }
                tl.narration_pc = None;
            }
        }
        if self.run_spawned_record_slice(&mut tl, true) {
            self.finish_cutscene_timeline_frame(tl);
        } else {
            // Still parked on the channel-completion handshake: keep the
            // timeline installed and re-test next tick.
            self.cutscene_timeline = Some(tl);
        }
    }

    /// Run one frame slice of a spawned partition-2 record context through
    /// the field VM - the shared core behind the modal cutscene timeline
    /// ([`Self::step_cutscene_timeline`], `modal = true`) and the concurrent
    /// helper contexts ([`Self::step_helper_contexts`], `modal = false`).
    ///
    /// Both shapes get the full retail context semantics: run-until-yield
    /// under a step budget, cross-context (`0x80`-bit) pokes resolved onto
    /// the spawned per-actor channels, the channel-completion handshake park
    /// (`B3 <id> <bit>`), the flag-test step-past rules, and the
    /// backward-wrap completion detection. Only `modal` differences apply:
    /// - `modal` sets [`Self::in_cutscene_timeline`] while the VM steps (the
    ///   op-49 name-entry / narration-draw host-hook scoping); a helper
    ///   context runs with ordinary field-VM host semantics.
    /// - a `0x1F` inline-dialog segment parks a modal timeline on an owned
    ///   dialog panel (input-routed by the caller); a helper context has no
    ///   modal input routing, so it completes at the segment instead.
    /// - inline narration blocks only exist on modal timelines (helper
    ///   installs don't parse them), so those branches are modal-only in
    ///   practice.
    ///
    /// Returns `false` when the context is still PARKED on the
    /// channel-completion handshake (nothing else ran this frame); `true`
    /// when a slice ran (the caller then applies its frame cap / teardown).
    // REF: FUN_8003BDE0
    fn run_spawned_record_slice(
        &mut self,
        tl: &mut crate::cutscene_timeline::CutsceneTimeline,
        modal: bool,
    ) -> bool {
        tl.frames = tl.frames.saturating_add(1);
        self.in_cutscene_timeline = modal;
        let mut channels = std::mem::take(&mut self.field_channels);
        let channel_pre_pos: Vec<(u16, u16)> = channels
            .iter()
            .map(|c| (c.ctx.world_x, c.ctx.world_z))
            .collect();
        // Cross-context channel-completion handshake (`B3 <id> <bit>` =
        // CFLAG_TST against a spawned per-actor channel): the timeline PARKED
        // here on a prior tick. Retail's halt-acquire / state-resume protocol
        // resumes past the flag-test only once the poked channel raises the
        // completion bit, so re-test it before stepping - rather than the
        // pre-handshake behaviour of advancing past the wait by instruction
        // width.
        if let Some(mut wait) = tl.channel_wait.take() {
            let flag_set = crate::field_channels::resolve_target(&channels, wait.target_id)
                .map(|ci| channels[ci].ctx.flags & (1u32 << (wait.bit & 0x1F)) != 0);
            match flag_set {
                // Still waiting (the channel has not signalled) and within the
                // park budget: hold the PC on the flag-test op another tick.
                Some(false) if wait.frames < CHANNEL_WAIT_PARK_TIMEOUT => {
                    wait.frames += 1;
                    tl.channel_wait = Some(wait);
                    self.field_channels = channels;
                    self.in_cutscene_timeline = false;
                    return false;
                }
                // The channel raised the flag (resume), the target is gone, or
                // the park timed out: step past the flag-test op by its encoded
                // width and let the timeline flow. (An extended flag-test is
                // `header 2 + 1 operand` = 3 bytes.)
                _ => {
                    let header_size = if tl.bytecode.get(tl.pc).copied().unwrap_or(0) & 0x80 != 0 {
                        2
                    } else {
                        1
                    };
                    tl.pc += header_size + 1;
                }
            }
        }
        // Player-channel (`0xF8`) halt-acquire park: the timeline is holding
        // at a `C3 F8` op for the player-anchor move armed by a preceding
        // `A2 F8 <move_id>` to play out (retail's halt-acquire / state-resume
        // handshake against the live player object). The armed countdown
        // stands in for the playout - the engine's player pokes complete
        // synchronously - so drain it one frame per tick; when it hits zero,
        // step PAST the halt-acquire by its encoded width so the record flows
        // on to its trailing ops (the door records' terminal `0x3F`).
        // REF: FUN_8003BDE0
        if let Some(width) = tl.player_wait.take() {
            tl.player_move_frames = tl.player_move_frames.saturating_sub(1);
            if tl.player_move_frames > 0 {
                tl.player_wait = Some(width);
                self.field_channels = channels;
                self.in_cutscene_timeline = false;
                return false;
            }
            tl.pc += width;
        }
        {
            let mut host = FieldHostImpl { world: self };
            let mut budget = CUTSCENE_TIMELINE_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = tl.pc;
                // Arrived at an inline narration block.
                //
                // Crawl (`op0 0x80`): retail spawns the roller as a CHILD
                // context (`FUN_80037174`) and keeps executing THIS parent
                // timeline, so the camera cuts / fades / waits authored between
                // the crawl blocks play UNDER the scrolling text. Mirror that:
                // open the roller and let the PC continue (non-blocking) for
                // every block except the LAST, which blocks so the timeline
                // does not reach its terminal scene-transition / hand-off
                // before the final pages scroll out. If a prior roller is still
                // scrolling when a block is reached, hold (don't stack rollers)
                // until it drains, then re-enter to open this one.
                //
                // Title card (`op0 0x89`): the pages show simultaneously
                // while the parent CONTINUES; a card whose pages are blank
                // clears the overlay. Skip past the block either way.
                if let Some(site) = tl.narration_blocks.iter().find(|b| b.op_offset == pc) {
                    match site.kind {
                        legaia_asset::cutscene_text::NarrationKind::Crawl => {
                            if host.world.cutscene_narration_active() {
                                tl.narration_pc = Some(pc);
                                tl.narration_pending_open = true;
                                break;
                            }
                            let site_end = site.end;
                            let site_off = site.op_offset;
                            let pages = site.pages.clone();
                            let is_last_block =
                                tl.narration_blocks.iter().all(|b| b.op_offset <= site_off);
                            host.world.open_cutscene_narration(pages);
                            if is_last_block {
                                // Blocking: park until these pages scroll out.
                                tl.narration_pc = Some(pc);
                                tl.narration_pending_open = false;
                                break;
                            }
                            // Non-blocking: the roller scrolls on its own
                            // (`World::tick`); continue into the camera cuts.
                            tl.pc = site_end;
                            continue;
                        }
                        legaia_asset::cutscene_text::NarrationKind::Card => {
                            let blank = site.pages.iter().all(|p| p.trim().is_empty());
                            host.world.cutscene_card = if blank {
                                None
                            } else {
                                Some(site.pages.clone())
                            };
                            tl.pc = site.end;
                            continue;
                        }
                    }
                }
                // Retail dialog-SM transition test (`FUN_80039B7C`): an
                // in-bounds byte with `& 0x7F < 0x20` is a text-segment lead
                // (`0x1F`) or a terminator (`0x00..0x1E`), not an opcode. A
                // `0x1F` opens an inline dialog box over the record bytes and
                // parks the timeline at the segment (resumed by the pre-step
                // gate when the player dismisses it). A stray terminator the
                // flow lands on is consumed (skipped) - timeline records
                // continue with choreography ops after their conversation, so
                // ending here would drop the record's closing flag-sets.
                // Running OFF the record end falls through to the VM step
                // instead (its `Unknown` completes the timeline).
                if let Some(&text_byte) = tl.bytecode.get(pc)
                    && text_byte & 0x7F < 0x20
                {
                    if text_byte == 0x1F {
                        if modal {
                            tl.dialog = Some(crate::dialog::OwnedDialogPanel::at_segment(
                                std::sync::Arc::clone(&tl.bytecode),
                                pc,
                            ));
                        } else {
                            // A concurrent helper context has no modal input
                            // routing to page an inline dialog; complete the
                            // context at the segment instead of parking on it
                            // forever.
                            tl.done = true;
                        }
                        break;
                    }
                    tl.pc = pc + 1;
                    continue;
                }
                let opcode_byte = tl.bytecode.get(pc).copied().unwrap_or(0);
                // Cross-context dispatch (`0x80`-bit ops): resolve the target
                // byte to a spawned per-actor channel (`ctx[+0x50] == target`,
                // retail `FUN_8003C83C`) and run the op against THAT context -
                // this is how the timeline cues the vignette actors. `0xF8`
                // (player anchor) / `0xFB` (system) keep the timeline's own
                // context (the player pokes route through host hooks).
                //
                // An UNRESOLVED id (a context the engine does not spawn -
                // partition-0 object contexts, e.g. the Mei beat's `0x01`) is
                // skipped by its decoded width instead: running it against the
                // timeline's own ctx corrupted the timeline (a `B1 01 00` set
                // the timeline's OWN busy bit, and the `CC 01 A0` busy-wait
                // then hijacked the caller PC into the record header).
                let target = vm::field::peek_extended(&tl.bytecode, pc).and_then(|t| {
                    crate::field_channels::resolve_target(&channels, t).map(|ci| (t, ci))
                });
                if target.is_none()
                    && let Some(t) = vm::field::peek_extended(&tl.bytecode, pc)
                    && t != 0xF8
                    && t != 0xFB
                    && let Ok(insn) = legaia_asset::field_disasm::decode(&tl.bytecode, pc)
                {
                    if pc < tl.visited.len() {
                        tl.visited[pc] = true;
                    }
                    tl.pc = pc + insn.size;
                    continue;
                }
                // Player-anchor channel (`0xF8`) ExecMove / halt-acquire
                // completion model. Retail resolves `0xF8` to the live player
                // object (`_DAT_8007C364`, the `FUN_8003C83C` special-target
                // arm) - not a spawned channel - so `resolve_target` keeps its
                // `None` contract, and the two ops the door-cutscene records
                // drive the player with are modelled here instead of falling
                // through to the timeline's own ctx:
                //
                // - `A2 F8 <move_id>` (op 0x22 ExecMove): retail pokes the
                //   move-table clip onto the player and lets it play out over
                //   the following frames. Emit the same `ExecMove` field
                //   event and arm a short completion countdown standing in
                //   for the playout.
                // - `C3 F8 <sub> …` (op 0x43 sub-0/1/A/B halt-acquire):
                //   retail halts the caller and state-resumes it at the
                //   operand s16 once the player move completes. That resume
                //   PC points BACKWARD into the poke loop (jou `P2[5]`:
                //   `C3 F8 00 5E E2 50` at `+0x60` resumes at `+0x50`), so
                //   taking the VM's yield here spins the timeline until the
                //   frame cap kills it WITHOUT the trailing `0x3F` scene
                //   change. Instead PARK at the op until the armed countdown
                //   drains (the pre-step gate above), then step PAST it by
                //   encoded width - the completion side of the handshake. A
                //   halt-acquire with no move in flight completes at once.
                //   (The op-0x38 halt-acquire variant resumes FORWARD at its
                //   post-instruction PC, so its yield is already
                //   completion-shaped and needs no special case.)
                // REF: FUN_8003C83C
                // REF: FUN_8003BDE0
                if vm::field::peek_extended(&tl.bytecode, pc) == Some(0xF8) {
                    let op = opcode_byte & 0x7F;
                    if op == 0x22
                        && let Some(&move_id) = tl.bytecode.get(pc + 2)
                    {
                        host.world
                            .pending_field_events
                            .push(FieldEvent::ExecMove { move_id });
                        tl.player_move_frames = CHANNEL_WAIT_PARK_TIMEOUT;
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        tl.pc = pc + 3;
                        continue;
                    }
                    if op == 0x43
                        && let Some(&sub) = tl.bytecode.get(pc + 2)
                        && matches!(sub, 0 | 1 | 0xA | 0xB)
                    {
                        // Encoded width: extended header (2) + sub-0/1
                        // operand (4) or sub-A/B operand (8) - the VM's own
                        // predicate-failure stride.
                        let width = if sub == 0xA || sub == 0xB { 10 } else { 6 };
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        if tl.player_move_frames == 0 {
                            tl.pc = pc + width;
                            continue;
                        }
                        tl.player_wait = Some(width);
                        break;
                    }
                }
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
                // Step past the timeline's conditional-wait parks that are NOT
                // the modelled channel handshake. Retail Halts at PC on these -
                // a flag a spawned sub-context sets - so advancing by the op's
                // encoded width (these flag-tests read one operand byte,
                // `header_size + 1`) keeps the timeline flowing toward its
                // camera / move / STATE_RESUME ops. The step-past ops are the
                // flag-tests `0x2D` (LFLAG), `0x30` (GFLAG) and the `0x4C`
                // nibble-C `script_alloc` / globals-gate - all 2-byte (3
                // extended), so a fixed step-past is correct-width for them.
                // The cross-context CFLAG_TST `0x33` (`B3 <id> <bit>` = the
                // timeline waiting on a vignette channel's completion flag) is
                // now PARKED instead (handled just above): it holds the PC until
                // the channel raises the bit - the halt-acquire / state-resume
                // handshake - and only the `B3 <id> 0A` halt-bit *verify* form
                // (bit 10) still steps past here. A bare (non-cross-context)
                // `0x33` also steps past. Other cross-context ops (the `4C`/`23`
                // action pokes) are NOT stepped past - they run against the
                // target and advance by their real width. Two parks are kept:
                // `0x4A` WAIT_FRAMES (a real timed wait that plays out via the
                // wait accumulator) and `0x49` STATE_RESUME (the name-entry
                // suspend, driven by the op-49 host hooks).
                let op = opcode_byte & 0x7F;
                // Cross-context `4C A0` busy-wait (`CC <ch> A0 <bit> <s16>`):
                // "while the poked channel's ctx-flag bit is still set, jump".
                // Retail's channel clears its own busy bit as its move plays
                // out frame by frame; the timeline's channel pokes complete
                // synchronously, so the busy branch must always fall through -
                // and the s16 target is meaningless in the caller record's pc
                // space (taking it here derailed the Mei beat into its own
                // header + dialog text). Force the skip path (6-byte width).
                if op == 0x4C
                    && target.is_some()
                    && tl.bytecode.get(pc + 2).is_some_and(|b| b >> 4 == 0xA)
                {
                    next_pc = pc + 2 + 4;
                    stop = false;
                }
                // Cross-context channel-completion wait (`B3 <id> <bit>`,
                // CFLAG_TST against a spawned channel): PARK the timeline until
                // the awaited channel raises the completion bit, rather than
                // stepping past by width. The park persists across ticks and is
                // resolved by the pre-step gate above. Bit 10 (0x400, the
                // halt/busy bit the acquire sweep toggles) is a suspension
                // *verify*, not a completion wait, so it falls through to the
                // width step-past below.
                if op == 0x33
                    && matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                    && next_pc == pc
                    && let Some((tid, _)) = target
                {
                    let bit = tl.bytecode.get(pc + 2).copied().unwrap_or(0) & 0x1F;
                    if bit != 10 {
                        tl.channel_wait = Some(crate::cutscene_timeline::ChannelWait {
                            target_id: tid,
                            bit,
                            frames: 0,
                        });
                        // Leave PC on the op; the pre-step gate resolves the park.
                        break;
                    }
                }
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
                // Natural termination: the record's choreography **wrapped**.
                // On-disc partition-2 records have no end opcode - they finish
                // by parking in a tight `Nop`+`JmpRel`-to-self spin (the fog /
                // flag-reset ambients) or by looping back to their top as a
                // resident actor-driver (the Mei beat's op-`0x45` APPLY jump
                // back to its conversation loop). Retail leaves both spinning
                // as *parallel* contexts, invisible to the player; the modal
                // timeline completes instead so control returns. The signal is
                // an `Advance` jumping backward onto an already-executed PC -
                // real waits `Halt` at their own PC and never trip this.
                if pc < tl.visited.len() {
                    tl.visited[pc] = true;
                }
                if matches!(kind, crate::cutscene_timeline::TraceResult::Advance)
                    && next_pc <= pc
                    && tl.visited.get(next_pc).copied().unwrap_or(false)
                {
                    // Camera-apply loop-back (`45 C0 <s16>`): retail's
                    // sub-`0xC0` arm applies the camera solve and jumps to the
                    // operand s16 - in the Drake-castle door records that is a
                    // camera-tracking repeat over the walk-through poke loop,
                    // and the record's door-state tail (`54 BE`-family latches
                    // + the `60 0F` mutex release) lives AFTER the op. Since
                    // the engine's choreography completes synchronously, break
                    // the loop ONCE per site: fall through past the op (plain
                    // width 4 / extended 5) so the tail executes. A second
                    // arrival wraps as usual - the resident-loop completion
                    // shape (the town01 Mei beat) still terminates.
                    // REF: FUN_801dab90
                    let header_size = if opcode_byte & 0x80 != 0 { 2 } else { 1 };
                    let is_camera_apply = (opcode_byte & 0x7F) == 0x45
                        && tl
                            .bytecode
                            .get(pc + header_size)
                            .is_some_and(|b| b & 0xC0 == 0xC0);
                    if is_camera_apply && !tl.camera_loop_broken.contains(&pc) {
                        tl.camera_loop_broken.push(pc);
                        next_pc = pc + header_size + 3;
                    } else {
                        tl.done = true;
                        stop = true;
                    }
                }
                if tl.trace_enabled {
                    if std::env::var_os("LEGAIA_DIAG_TIMELINE").is_some()
                        && !(matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                            && next_pc == pc)
                    {
                        eprintln!(
                            "DIAG timeline: frame {} pc {pc:#06x} op {opcode_byte:#04x} \
                             ({:#04x}) -> {next_pc:#06x} {kind:?} bytes {:02x?}",
                            tl.frames,
                            opcode_byte & 0x7F,
                            &tl.bytecode[pc..(pc + 12).min(tl.bytecode.len())]
                        );
                    }
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
        true
    }

    /// Post-slice bookkeeping for the **modal** cutscene timeline: apply the
    /// frame cap, arm the prologue hand-off safety net, and drop or
    /// re-install the timeline. Split from [`Self::step_cutscene_timeline`]
    /// so the frame slice itself ([`Self::run_spawned_record_slice`]) is
    /// shared with the concurrent helper contexts, which apply their own
    /// (plain) cap in [`Self::step_helper_contexts`] instead.
    fn finish_cutscene_timeline_frame(
        &mut self,
        mut tl: crate::cutscene_timeline::CutsceneTimeline,
    ) {
        // Frame cap: real disc bytecode must never hang the tick. The opdeene
        // prologue gets the generous cap - its record arms the hand-off bit
        // at its TOP (`GFLAG_SET 26` at body `+0x17`) and then STAGES the
        // vignettes for the narration's duration, so "bit armed" must not
        // complete it (that early-out silently dropped the whole vignette
        // choreography - camera beats, actor-channel pokes - after two ops).
        // Every opening-chain leg gets the generous cap: the `opstati` /
        // `opurud` records stage their Mist vignettes with multi-hundred-frame
        // `WaitFrames` between narration crawls (opurud alone waits ~2000
        // frames of choreography), so the tight anti-hang cap would cut the
        // chain mid-scene. `town01`'s opening (chain flag already cleared)
        // keeps the tight cap.
        let cap = if tl.arms_prologue_handoff || self.opening_chain_active {
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

    /// Step every concurrent helper context ([`Self::helper_contexts`]) one
    /// frame slice - the per-frame sweep over the mid-play spawned records,
    /// running each through the shared [`Self::run_spawned_record_slice`]
    /// core (`modal = false`). Helper contexts execute alongside the modal
    /// cutscene timeline and the per-actor channels without seizing the
    /// camera or locking locomotion; a context that completes (wrapped its
    /// choreography, ran off its bytecode, or hit the plain
    /// [`CUTSCENE_TIMELINE_MAX_FRAMES`] cap) is dropped from the table.
    // REF: FUN_8003BDE0
    pub fn step_helper_contexts(&mut self) {
        if self.helper_contexts.is_empty() {
            return;
        }
        let mut contexts = std::mem::take(&mut self.helper_contexts);
        for tl in contexts.iter_mut() {
            if tl.done {
                continue;
            }
            if self.run_spawned_record_slice(tl, false) && tl.frames >= CUTSCENE_TIMELINE_MAX_FRAMES
            {
                tl.done = true;
            }
        }
        let dropped = contexts.iter().any(|tl| tl.done);
        contexts.retain(|tl| !tl.done);
        self.helper_contexts = contexts;
        // Stranded-player rescue: a spawned record can `MoveTo` the PLAYER as
        // part of its choreography (izumi's first-visit record parks the
        // party at the spring pocket, a spot the base collision grid walls
        // in; retail plays the whole modal cutscene there and leaves the
        // scene through the record's closing `0x3F`). If the engine's
        // concurrent rendition ends - completed or frame-capped - with the
        // player left standing inside a wall / off the floor (walk component
        // size 0), re-seat them at the scene's resolved cold spawn so a
        // partially-executed record can never strand the player where no
        // direction unblocks.
        if dropped
            && self.helper_contexts.is_empty()
            && matches!(self.mode, crate::world::SceneMode::Field)
            && let Some((sx, sz)) = self.resolved_cold_spawn
            && let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get(slot as usize)
            && self.field_walk_component_size(actor.move_state.world_x, actor.move_state.world_z)
                == 0
            // ... and only when the resolved spawn itself is on open floor: a
            // scene with no walkability data at all (a cutscene shell) reads
            // component 0 everywhere, and yanking the player there would be
            // wrong.
            && self.field_walk_component_size(sx, sz) > 0
        {
            let y = self.sample_field_floor_height(sx as i32, sz as i32) as i16;
            if let Some(actor) = self.actors.get_mut(slot as usize) {
                log::info!(
                    "field: spawned record left the player at ({},{}) inside a wall; \
                     re-seating at the resolved cold spawn ({sx},{sz})",
                    actor.move_state.world_x,
                    actor.move_state.world_z
                );
                actor.move_state.world_x = sx;
                actor.move_state.world_z = sz;
                actor.move_state.world_y = y;
            }
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
    /// Channels are seeded two ways. A cutscene timeline spawns the full set
    /// ([`Self::install_cutscene_timeline_record`]) so its cross-context pokes
    /// land on real per-actor contexts (the opening prologue's vignettes). On
    /// **ordinary free-roam scene entry** the scene loader seeds the same set
    /// ([`Self::seed_field_channels`], the non-cutscene half of retail's
    /// `FUN_8003AEB0` spawn loop) so each placement's own init opcodes run -
    /// scripted initial facings, idle/`WAIT`-loop cadence, local-flag setup.
    ///
    /// After stepping, each channel whose context position changed writes
    /// through to [`Self::field_npc_positions`] so the field render / probes
    /// follow the scripted move. On free-roam the engine's waypoint patroller
    /// ([`Self::tick_field_npc_motions`]) owns any placement carrying a route,
    /// so a channel's move (and the heading derived from it) is only surfaced
    /// for placements the patroller does NOT drive - the two never fight over a
    /// slot's position. During a cutscene the patroller stands down, so the
    /// channel owns every slot's write-through exactly as before.
    // PORT: FUN_80039B7C (per-actor frame-slice loop; NOP break + halt park)
    // REF: FUN_8003C83C (cross-context target resolve)
    pub fn step_field_channels(&mut self) {
        if self.field_channels.is_empty() {
            return;
        }
        // Free-roam channels only STEP when the engine's NPC-animation switch is
        // on - the same master gate the waypoint patroller
        // ([`Self::tick_field_npc_motions`]) honours. With it off, the seeded
        // channels stay resident but idle, so ordinary free-roam behaves exactly
        // as it did before per-actor channels existed (no scripted repositioning,
        // no MoveTo `FieldEvent`s) until liveliness is enabled. A cutscene
        // timeline always drives its channels regardless of the switch.
        if !self.cutscene_timeline_active() && !self.animate_field_npcs {
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
        // On free-roam the waypoint patroller owns routed placements (it runs
        // right after this and would overwrite the slot anyway), so a channel's
        // move is surfaced only for placements it does not drive - "keep the
        // existing locomotion where the channel doesn't override it". During a
        // cutscene the patroller stands down, so the channel owns every slot.
        let patroller_active = !self.cutscene_timeline_active();
        for (c, pre) in channels.iter().zip(pre_pos) {
            let (nx, nz) = (c.ctx.world_x, c.ctx.world_z);
            if (nx, nz) == pre {
                continue;
            }
            let slot = c.placement_index as u8;
            if patroller_active {
                if self.field_npc_routes.contains_key(&slot) {
                    continue;
                }
                // Surface a facing from the scripted move so a never-walked NPC
                // its placement script repositions no longer renders unrotated:
                // the same 12-bit atan2 heading (`0` = Z+) the patroller writes
                // from its own step deltas (see `tick_field_npc_motions`).
                let (dx, dz) = (nx as f32 - pre.0 as f32, nz as f32 - pre.1 as f32);
                if dx != 0.0 || dz != 0.0 {
                    let heading = ((dx.atan2(dz) / std::f32::consts::TAU * 4096.0).round() as i32
                        & 0x0FFF) as i16;
                    self.field_npc_headings.insert(slot, heading);
                }
            }
            self.field_npc_positions
                .insert(slot, (nx as i16, nz as i16));
        }
        self.field_channels = channels;
    }

    /// Seed the per-actor field-VM channels for **ordinary free-roam** scene
    /// entry: one context per MAN partition-1 placement, exactly as a cutscene
    /// install seeds them, but without a timeline driving cross-context pokes.
    /// The scene loader calls this after the placement-derived carrier / NPC
    /// install so each placement's own init opcodes run through
    /// [`Self::step_field_channels`] - scripted facings, idle/`WAIT` cadence,
    /// local-flag setup - the non-cutscene half of retail's `FUN_8003AEB0`
    /// spawn loop.
    ///
    /// Cutscene scenes (`opdeene` and friends) re-seed the set through
    /// [`Self::install_cutscene_timeline_record`] afterwards, which simply
    /// replaces this set, so the two paths compose. A scene with no placements
    /// seeds an empty set and [`Self::step_field_channels`] no-ops.
    // PORT: FUN_8003AEB0 (the per-record spawn loop, free-roam entry)
    // REF: FUN_8003A1E4 (per-record context spawn)
    pub fn seed_field_channels(
        &mut self,
        man_file: &legaia_asset::man_section::ManFile,
        man: &[u8],
    ) {
        self.field_channels = crate::field_channels::spawn_channels(man_file, man);
        self.field_channels_man = if self.field_channels.is_empty() {
            None
        } else {
            Some(std::sync::Arc::new(man.to_vec()))
        };
        self.field_npc_anim_cues.clear();
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
    // PORT: FUN_8003CF7C (the inline fast-forward loop below is retail's
    //      run-to-next-text helper: tick the field VM until `byte & 0x7F <
    //      0x20`, with the raw-`0x21` execute-then-stop and the stalled-PC
    //      stop mapped to the loop's end paths)
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
                    // scene-change run before the reply box). A user choice is
                    // progress - clear the wrap map so menu records that
                    // re-emit their menu by jumping back still cycle.
                    let choice = panel.picker_cursor();
                    let target = panel.picker().and_then(|pk| pk.jump_target(choice));
                    id.last_choice = Some(choice);
                    id.visited.iter_mut().for_each(|v| *v = false);
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
            if id.pc < id.visited.len() {
                id.visited[id.pc] = true;
            }
            match vm::field::step(&mut host, &mut id.ctx, &id.bytecode, id.pc) {
                // A backward Advance onto an already-executed PC is the
                // record's resident loop-back to its top selector - the end
                // of ONE conversation pass (retail parks there until the next
                // talk). End the conversation like a Halt would; the wrap map
                // is cleared on picker commits so menu re-emission survives.
                FieldStepResult::Advance { next_pc }
                    if next_pc <= id.pc && id.visited.get(next_pc).copied().unwrap_or(false) =>
                {
                    if let Some(fb) = id.fallback_segment_pc.take() {
                        id.pc = fb;
                        continue;
                    }
                    id.done = true;
                    break;
                }
                FieldStepResult::Advance { next_pc } => {
                    id.pc = next_pc;
                    // Retail's run-to-next-text helper breaks after executing
                    // a raw `0x21` byte and returns it (`FUN_8003CF7C`
                    // `if (bVar1 == 0x21) break`); the dialog SM reads that
                    // as conversation end. Raw compare only - an extended
                    // `0xA1` NOP runs through like any other op.
                    if b == 0x21 {
                        if let Some(fb) = id.fallback_segment_pc.take() {
                            id.pc = fb;
                            continue;
                        }
                        id.done = true;
                        break;
                    }
                }
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
        // A prop-bound record run (door / cupboard) is stepped by its own
        // driver ([`Self::step_prop_interaction`]) with prop-actor bridging;
        // stepping it here too would double-run its VM slices.
        if self
            .inline_dialogue
            .as_ref()
            .is_some_and(|id| id.prop_anchor.is_some())
        {
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

    #[test]
    fn scene_depth_cue_tracks_the_prologue_grade_gate() {
        let mut w = World::new();
        // Interactive scenes stage NO depth-cue ramp: the renderer's ramp-off
        // path is the pre-ramp identity, so town01 pixels are unchanged.
        assert!(w.scene_depth_cue().is_none());
        w.set_active_scene_label("town01");
        assert!(
            w.scene_depth_cue().is_none(),
            "interactive field renders without the far-colour pull"
        );
        // All three prologue legs pull toward the gold far colour.
        for scene in ["opdeene", "opstati", "opurud"] {
            w.set_active_scene_label(scene);
            assert_eq!(
                w.scene_depth_cue(),
                Some(crate::fade::DepthCueRamp::PROLOGUE_GOLD),
                "{scene} stages the prologue depth-cue ramp"
            );
        }
    }

    /// Build a minimal timeline that reaches a cross-context CFLAG_TST
    /// (`B3 05 03` = op 0x33 extended, target channel id 5, completion bit 3),
    /// then a WAIT_FRAMES so the timeline stays installed once it resumes past
    /// the flag-test. A single spawned channel (script id 5) provides the
    /// cross-context target whose flag the timeline waits on.
    fn timeline_with_channel_wait(channel_flag_bit3: bool) -> World {
        use crate::cutscene_timeline::CutsceneTimeline;
        use crate::field_channels::FieldChannel;
        use legaia_engine_vm::field::FieldCtx;

        let mut w = World::new();
        // `B3 05 03` (3 bytes) then `4A FF 7F` (WAIT_FRAMES target 0x7FFF).
        let bc = vec![0xB3, 0x05, 0x03, 0x4A, 0xFF, 0x7F];
        w.cutscene_timeline = Some(CutsceneTimeline::new(bc, 0));
        let mut ctx = FieldCtx {
            script_id: 5,
            ..FieldCtx::default()
        };
        if channel_flag_bit3 {
            ctx.flags |= 1 << 3;
        }
        w.field_channels = vec![FieldChannel {
            placement_index: 5,
            ctx,
            record_offset: 0,
            pc: 0,
            done: false,
        }];
        w
    }

    #[test]
    fn cutscene_timeline_parks_on_channel_wait_until_flag_set() {
        // The timeline reaches `B3 05 03` with the channel's bit 3 clear: it
        // PARKS on the cross-context CFLAG_TST rather than stepping past.
        let mut w = timeline_with_channel_wait(false);
        w.step_cutscene_timeline();
        {
            let tl = w
                .cutscene_timeline
                .as_ref()
                .expect("timeline still installed");
            assert!(tl.channel_wait.is_some(), "parks on the channel handshake");
            assert_eq!(tl.pc, 0, "PC held on the flag-test op while parked");
            assert!(!tl.is_done());
        }
        // Repeated ticks with the flag still clear keep it parked (well within
        // the park timeout).
        for _ in 0..5 {
            w.step_cutscene_timeline();
        }
        {
            let tl = w.cutscene_timeline.as_ref().unwrap();
            assert!(
                tl.channel_wait.is_some(),
                "stays parked while the flag is clear"
            );
            assert_eq!(tl.pc, 0);
        }
        // The awaited channel raises the completion flag: the very next step
        // resolves the park and resumes PAST the 3-byte flag-test op.
        w.field_channels[0].ctx.flags |= 1 << 3;
        w.step_cutscene_timeline();
        let tl = w
            .cutscene_timeline
            .as_ref()
            .expect("timeline still installed");
        assert!(
            tl.channel_wait.is_none(),
            "resumes only after the completion flag is set"
        );
        assert_eq!(
            tl.pc, 3,
            "PC advanced past the CFLAG_TST op onto WAIT_FRAMES"
        );
    }

    #[test]
    fn cutscene_timeline_channel_wait_times_out_to_step_past() {
        // Safety net: a channel that never raises the flag must not stall the
        // timeline forever - after the park timeout it falls back to the
        // by-width step-past (the pre-handshake behaviour).
        let mut w = timeline_with_channel_wait(false);
        // First step parks; then it stays parked for CHANNEL_WAIT_PARK_TIMEOUT
        // frames, then steps past on the frame the budget is exhausted.
        let cap = crate::world::CHANNEL_WAIT_PARK_TIMEOUT;
        let mut resumed = false;
        for _ in 0..(cap + 4) {
            w.step_cutscene_timeline();
            if w.cutscene_timeline
                .as_ref()
                .is_some_and(|tl| tl.channel_wait.is_none() && tl.pc == 3)
            {
                resumed = true;
                break;
            }
        }
        assert!(
            resumed,
            "the park times out and the timeline steps past the flag-test"
        );
    }

    /// Build a timeline whose record drives the **player-anchor channel**
    /// (`0xF8`) with the jou castle-door shape: `A2 F8 06` ExecMove, the
    /// `C3 F8 00 …` halt-acquire whose operand s16 resumes BACKWARD (offset
    /// 0 here), the retail filler bytes, then the trailing `0x3F` scene
    /// change to `jouina` and the record's terminal backward-jump park.
    fn timeline_with_player_channel_door() -> World {
        use crate::cutscene_timeline::CutsceneTimeline;
        let mut w = World::new();
        let mut bc = vec![
            0xA2, 0xF8, 0x06, // ExecMove move_id=6 against the player anchor
            0xC3, 0xF8, 0x00, 0x5E, 0xE2,
            0x00, // halt-acquire sub-0, resume s16 = 0 (backward)
            0x00, 0x1E, 0x00, // filler region (consumed by the terminator skip)
        ];
        // `0x3F` SceneChange -> "jouina", entry (0x84, 0x14), dir 0.
        bc.extend_from_slice(&[
            0x3F, 0x8F, 0x02, 0x06, b'j', b'o', b'u', b'i', b'n', b'a', 0x84, 0x14, 0x00,
        ]);
        bc.extend_from_slice(&[0x21, 0x26, 0xFE, 0xFF]); // Nop + JmpRel-to-self park
        w.cutscene_timeline = Some(CutsceneTimeline::new(bc, 0));
        w
    }

    #[test]
    fn cutscene_timeline_player_channel_door_reaches_scene_change() {
        // The player-channel completion model: the `A2 F8` ExecMove arms the
        // in-flight countdown (emitting the move event), the `C3 F8`
        // halt-acquire PARKS instead of taking its backward resume yield,
        // and once the countdown drains the record runs its trailing `0x3F`.
        // Regression shape: the pre-model stepper took the backward yield
        // and spun `pc 0 -> 3` until the frame cap, never firing the scene
        // change.
        let mut w = timeline_with_player_channel_door();
        w.step_cutscene_timeline();
        {
            let tl = w
                .cutscene_timeline
                .as_ref()
                .expect("timeline still installed");
            assert!(
                tl.player_wait.is_some(),
                "parks at the player-channel halt-acquire"
            );
            assert_eq!(tl.pc, 3, "PC held on the halt-acquire op while parked");
            assert_eq!(
                tl.player_move_frames,
                crate::world::CHANNEL_WAIT_PARK_TIMEOUT,
                "the ExecMove armed the in-flight countdown"
            );
        }
        assert!(
            w.pending_field_events
                .iter()
                .any(|e| matches!(e, crate::field_events::FieldEvent::ExecMove { move_id: 6 })),
            "the player-channel ExecMove emits the move event"
        );
        // The park drains over the countdown, then the trailing `0x3F` fires
        // and the record's backward-jump park completes the timeline - well
        // inside the frame cap.
        let cap = crate::world::CHANNEL_WAIT_PARK_TIMEOUT;
        let mut ticks = 0;
        while w.cutscene_timeline.is_some() && ticks < cap + 8 {
            w.step_cutscene_timeline();
            ticks += 1;
        }
        assert_eq!(
            w.pending_named_scene_transition
                .as_ref()
                .map(|(n, ..)| n.as_str()),
            Some("jouina"),
            "the trailing 0x3F scene change fired"
        );
        assert!(
            w.cutscene_timeline.is_none(),
            "the timeline completed without hitting the frame cap"
        );
        assert!(
            ticks <= cap + 4,
            "completion took {ticks} ticks - the park drained, not the frame cap"
        );
    }

    #[test]
    fn cutscene_timeline_player_halt_acquire_without_move_steps_past() {
        // A player-channel halt-acquire with NO move in flight completes
        // immediately: no park, PC steps past by the encoded width onto the
        // next op.
        use crate::cutscene_timeline::CutsceneTimeline;
        let mut w = World::new();
        let bc = vec![
            0xC3, 0xF8, 0x00, 0x5E, 0xE2, 0x00, // halt-acquire sub-0
            0x00, // filler (terminator skip)
            0x4A, 0xFF, 0x7F, // WAIT_FRAMES target 0x7FFF (keeps it installed)
        ];
        w.cutscene_timeline = Some(CutsceneTimeline::new(bc, 0));
        w.step_cutscene_timeline();
        let tl = w
            .cutscene_timeline
            .as_ref()
            .expect("timeline still installed");
        assert!(tl.player_wait.is_none(), "no park without a move in flight");
        assert_eq!(
            tl.pc, 7,
            "stepped past the 6-byte halt-acquire (+ filler) onto WAIT_FRAMES"
        );
    }

    /// Build a minimal MAN whose partition 2 carries the given record
    /// bodies (each already in the named-record shape from [`p2_record`]).
    /// `n0` / `n1` fill the partition-0/1 counts so the op-`0x44` global
    /// re-base (`global - N0 - N1`) is exercised.
    fn man_with_p2_records(
        records: &[Vec<u8>],
        n0: i16,
        n1: i16,
    ) -> (legaia_asset::man_section::ManFile, Vec<u8>) {
        use legaia_asset::man_section::{ManFile, ManHeader, SectionRef};
        let data_region_offset = 0x40usize;
        let mut man = vec![0u8; data_region_offset];
        let mut offsets = Vec::new();
        for body in records {
            offsets.push((man.len() - data_region_offset) as u32);
            man.extend_from_slice(body);
        }
        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [n0, n1, records.len() as i16],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![], offsets],
            data_region_offset,
            sections: std::array::from_fn(|_| SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        (man_file, man)
    }

    /// A partition-2 named-record body: 1-char name, empty C0, the given C1
    /// story-flag OR-gate, empty C2, then `script`.
    fn p2_record(c1: &[u16], script: &[u8]) -> Vec<u8> {
        let mut r = vec![1u8, 0x41, 0x00]; // name_len=1 + 2 SJIS name bytes
        r.push(0); // C0 empty
        r.push(c1.len() as u8);
        for f in c1 {
            r.extend_from_slice(&f.to_le_bytes());
        }
        r.push(0); // C2 empty
        r.extend_from_slice(script);
        r
    }

    #[test]
    fn helper_record_installs_and_executes_without_modal_attributes() {
        // A mid-play spawned record (GFLAG_SET 26 then run-off-end) installs
        // as a concurrent helper context: it executes its script by the next
        // frame slice and never touches the modal timeline slot.
        let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x2E, 0x1A])], 0, 0);
        let mut w = World::new();
        assert!(w.install_helper_record(&mf, &man, 0));
        assert_eq!(w.helper_contexts.len(), 1);
        assert!(
            !w.cutscene_timeline_active(),
            "a helper spawn never installs the modal cutscene timeline"
        );
        w.step_helper_contexts();
        assert_ne!(
            w.story_flags & crate::world::PROLOGUE_HANDOFF_FLAG,
            0,
            "the helper record's GFLAG_SET executed"
        );
        assert!(
            w.helper_contexts.is_empty(),
            "a completed helper context is dropped from the table"
        );
    }

    #[test]
    fn helper_record_honors_c1_one_shot_gate() {
        // C1 blocks the spawn when ANY listed flag is set (the one-shot
        // latch) - the same FUN_8003BDE0 gate walk as the modal install.
        let (mf, man) = man_with_p2_records(&[p2_record(&[0x0193], &[0x21])], 0, 0);
        let mut w = World::new();
        assert!(w.install_helper_record(&mf, &man, 0), "clear flag: spawns");
        w.helper_contexts.clear();
        w.system_flag_set(0x0193);
        assert!(
            !w.install_helper_record(&mf, &man, 0),
            "latched C1 flag blocks the spawn"
        );
        assert!(w.helper_contexts.is_empty());
    }

    #[test]
    fn helper_spawn_while_timeline_active_is_not_dropped() {
        // A second spawned record while a modal timeline (or another helper)
        // executes must not be dropped: both helper contexts coexist with the
        // active timeline in the bounded context table.
        use crate::cutscene_timeline::CutsceneTimeline;
        let long_wait = &[0x4A, 0xFF, 0x7F]; // WAIT_FRAMES 0x7FFF: stays live
        let (mf, man) = man_with_p2_records(
            &[p2_record(&[], long_wait), p2_record(&[], long_wait)],
            0,
            0,
        );
        let mut w = World::new();
        w.cutscene_timeline = Some(CutsceneTimeline::new(long_wait.to_vec(), 0));
        assert!(w.cutscene_timeline_active());
        assert!(w.install_helper_record(&mf, &man, 0));
        assert!(w.install_helper_record(&mf, &man, 1));
        assert_eq!(
            w.helper_contexts.len(),
            2,
            "concurrent spawns coexist with the modal timeline"
        );
        w.step_helper_contexts();
        assert_eq!(
            w.helper_contexts.len(),
            2,
            "waiting helper contexts stay installed across a frame"
        );
        assert!(w.cutscene_timeline_active(), "the modal slot is untouched");
    }

    #[test]
    fn helper_context_table_is_bounded() {
        let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x4A, 0xFF, 0x7F])], 0, 0);
        let mut w = World::new();
        for _ in 0..crate::world::SPAWNED_CONTEXT_SLOTS {
            assert!(w.install_helper_record(&mf, &man, 0));
        }
        assert!(
            !w.install_helper_record(&mf, &man, 0),
            "a full context table refuses further spawns"
        );
        assert_eq!(w.helper_contexts.len(), crate::world::SPAWNED_CONTEXT_SLOTS);
    }

    #[test]
    fn spawned_helper_record_rebases_global_index() {
        // op-0x44 carries a GLOBAL record index; the install re-bases it into
        // partition 2 (`global - N0 - N1`, retail FUN_8003BDE0).
        let (mf, man) = man_with_p2_records(&[p2_record(&[], &[0x2E, 0x1A])], 3, 4);
        let mut w = World::new();
        assert!(
            !w.install_spawned_helper_record(&mf, &man, 2),
            "a global index below N0+N1 cannot re-base"
        );
        assert!(w.install_spawned_helper_record(&mf, &man, 7));
        assert_eq!(w.helper_contexts.len(), 1);
    }
}
