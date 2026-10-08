//! Name entry, cutscene narration, prologue handoff, cutscene timelines, field channels, and inline-dialogue driving.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

mod dialogue_drive;
mod field_channels;

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
        self.party
            .party_names
            .get(slot)
            .map(String::as_str)
            .unwrap_or("")
    }

    /// Open the name-entry overlay for `slot`, seeded with the slot's current
    /// display name (e.g. the template `Vahn`). Mirrors the opening `town01`
    /// script's lead-character naming prompt. The host drives it each frame
    /// with [`Self::step_name_entry`] and renders from [`crate::world::PartyState::name_entry`].
    ///
    /// The screen also holds the frame-step floor at `1` while it is open and
    /// hands the scene's floor back on close
    /// ([`crate::name_entry::NameEntry::saved_frame_step_floor`]).
    pub fn open_name_entry(&mut self, slot: usize) {
        let initial = self.party_name(slot).to_string();
        let mut entry = crate::name_entry::NameEntry::new(slot, &initial);
        entry.saved_frame_step_floor = Some(self.clock.frame_step_floor);
        self.set_frame_step_floor(1);
        self.party.name_entry = Some(entry);
    }

    /// `true` while the name-entry overlay is active.
    pub fn name_entry_active(&self) -> bool {
        self.party.name_entry.is_some()
    }

    /// Advance the active name-entry overlay by one input frame. On commit
    /// (the player confirms "Is this name okay?") the entered name is written
    /// into [`crate::world::PartyState::party_names`] for the entry's slot, the session is closed,
    /// and `true` is returned so the host can resume the field script.
    /// Returns `false` while the overlay stays open (or when none is active).
    pub fn step_name_entry(&mut self, input: crate::name_entry::NameEntryInput) -> bool {
        let Some(entry) = self.party.name_entry.as_mut() else {
            return false;
        };
        entry.step(input);
        if entry.state == crate::name_entry::NameEntryState::Done {
            let slot = entry.char_index;
            let name = entry.committed_name();
            if self.party.party_names.len() <= slot {
                self.party.party_names.resize(slot + 1, String::new());
            }
            self.party.party_names[slot] = name.clone();
            // Stamp the record too, so the committed name survives a
            // save/load round trip - retail's carrier is the record's
            // `+0x2A7`, not a side table.
            if let Some(rec) = self.party.roster.members.get_mut(slot) {
                rec.set_name(&name);
            }
            if let Some(floor) = entry.saved_frame_step_floor {
                self.set_frame_step_floor(floor);
            }
            self.party.name_entry = None;
            true
        } else {
            false
        }
    }

    /// One frame of the modal naming prompt, from a pad **edge** word (bits
    /// newly pressed this frame, the [`Self::set_pad`] layout).
    ///
    /// While the prompt is open the field is frozen (the op-`0x49` gate holds
    /// the script, and the timeline waits on [`Self::name_entry_active`]), so
    /// a frame is: route the edge into the entry SM, and advance the frame
    /// counter the caret blinks off. Returns `true` when the prompt owned the
    /// frame - the caller then skips the rest of its world tick - and `false`
    /// when no prompt is open.
    ///
    /// This is the one routing every host shares: the native window's
    /// per-tick arm and [`crate::scene::SceneHost`]-driving sessions
    /// (`BootSession::tick`, and so every headless driver) both call it. The
    /// browser page runs the same pair on its own two clocks - one edge per
    /// display frame into [`Self::step_name_entry`], the frame counter per
    /// sim step.
    pub fn step_name_entry_frame(&mut self, edge: u16) -> bool {
        if !self.name_entry_active() {
            return false;
        }
        self.step_name_entry(crate::name_entry::NameEntryInput::from_pad_edge(edge));
        self.name_entry_display_frames(1);
        true
    }

    /// `steps` display frames pass under the naming prompt.
    ///
    /// The prompt is not a mode of its own: op `0x49` sub-3 hands off to an
    /// actor (`func_0x80020de0(0x8007065c, ...)` on the field list), and the
    /// field frame loop keeps running around it - only the opening's script
    /// is parked. So the camera mover (`FUN_801DC0BC`) keeps stepping, and the
    /// 16-frame glide the opening stages one op before the `49 03`
    /// (`town01` P2[3] `+0x2B1`: pitch `292`, yaw `-510`, eye
    /// `(-600, 8, 3840)`) lands under the prompt - the shot every retail
    /// capture of the screen holds, with Vahn standing in the upper left. The
    /// engine's two glide clocks both run on
    /// [`crate::world::FrameClock::display_frames`], so the prompt advances
    /// it along with the caret's [`Self::frame`]; the hosts then run their
    /// camera half as on any frame.
    // REF: FUN_801DC0BC
    pub fn name_entry_display_frames(&mut self, steps: u32) {
        self.frame = self.frame.wrapping_add(u64::from(steps));
        self.clock.display_frames += u64::from(steps);
        let glide = &mut self.camera.state.glide_frames;
        *glide = (*glide - steps as i32).max(0);
    }

    /// Install the opening-cutscene narration presenter with `pages` (the
    /// inline subtitle pages decoded from the scene MAN's cutscene-timeline
    /// script; see [`crate::man_field_scripts::collect_partition_narration`]).
    /// A presenter with no pages installs nothing - a scene that carries no
    /// inline narration simply never shows one. The host renders the active
    /// page from [`crate::world::CutsceneState::narration`]; [`Self::tick`] advances its
    /// per-page timer.
    pub fn open_cutscene_narration(&mut self, pages: Vec<String>) {
        if pages.is_empty() {
            return;
        }
        // The crawl geometry is the config block the scene's seed op left
        // (`CutsceneState::narration_seed`), at the world's own game-tick
        // cadence: the opening scenes' prescripts raise the frame-step floor
        // `DAT_8007B9D8` to 3 through move-VM ext sub-op `0x2F` (opdeene's
        // record 16), which is what the capture reads - so no per-roller
        // override is needed.
        self.cutscene.narration = Some(crate::cutscene_narration::CutsceneNarration::with_seed(
            pages,
            self.cutscene.narration_seed,
            u16::from(self.clock.frame_step),
        ));
        // Monotonic "which crawl block is showing" counter. Because a
        // non-blocking crawl lets the next block open the very tick the prior
        // one scrolls out (continuous crawl, no blank frame), a rising-edge
        // `active && !was_active` observer can miss a block; observers count
        // this instead. Never reset within a scene's opening.
        self.cutscene.narration_seq = self.cutscene.narration_seq.wrapping_add(1);
    }

    /// `true` while the opening-cutscene narration is on screen (not yet
    /// stepped past its last page). Hosts gate the prologue hand-off on this:
    /// the narration plays first, the Rim Elm hand-off follows.
    pub fn cutscene_narration_active(&self) -> bool {
        self.cutscene
            .narration
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

    /// The op `0x4C 0x12` global tint currently in force
    /// (`DAT_8007BCB8/B9/BA`, normalized; `None` = neutral).
    ///
    /// It is **not** a frame multiply. A disc-wide reference scan finds one
    /// reader, the fog particle update `FUN_8003F3FC` (`0x8003F558` /
    /// `0x8003F588` / `0x8003F5B8`); every other site is the op's own ramp or
    /// a reset. `retona_field_card_boot` holds the word at `27` mid-arrival
    /// over a full-brightness frame. So the hosts stage it into no colour
    /// grade: it reaches the fog sheets (`World::fog_render_step`) and the
    /// non-retail volumetric fog, which follows it so the two fade together.
    ///
    /// The op `0x34` sub-0 screen effect is deliberately NOT composed in:
    /// the retail cold-boot capture holds the lit villager tableau across
    /// the whole span where the opening timeline's
    /// `34 01 00 00 00 28 00` → `34 05 FF FF FF 5A 00` pair would black a
    /// full-screen fade, which falsifies the "op 0x34 = screen fade"
    /// reading. That op spawns a colour tween whose per-frame
    /// `FUN_80024EE4` push is its own layer - read it off
    /// [`crate::world::World::screen_tint_pushes`].
    pub fn scene_screen_tint(&self) -> Option<[f32; 3]> {
        self.presentation.tint.as_ref().map(|t| t.factor())
    }

    /// Skip the active narration to its next page (a confirm press). Clears
    /// the presenter once it advances past the last page. Returns `true` while
    /// narration is still on screen, `false` once it completes (so the host
    /// lets the confirm fall through to [`Self::take_prologue_handoff`]).
    pub fn skip_cutscene_narration(&mut self) -> bool {
        let Some(narration) = self.cutscene.narration.as_mut() else {
            return false;
        };
        let still_active = narration.skip_page();
        if !still_active {
            self.cutscene.narration = None;
        }
        still_active
    }

    /// Arm the prologue cutscene -> Rim Elm handoff.
    ///
    /// In retail the opening cutscene scene `opdeene` runs a scripted
    /// timeline (a field-VM record in the MAN's third record partition)
    /// that ends with `GFLAG_SET 26` - field-VM op `0x2E` with operand
    /// `0x1A`, which sets bit 26 (`0x0400_0000`) of the scratchpad flag
    /// word `_DAT_1F800394` (the engine's [`crate::world::StoryFlagState::story_flags`]) right
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
        self.flags.story_flags |= PROLOGUE_HANDOFF_FLAG;
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
    /// ([`crate::world::CutsceneState::opening_chain_active`], set at the `opdeene` entry and carried
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
    /// Abandon the opening cutscene chain wholesale, without arming the
    /// `town01` opening the intro-skip arms: the narration roller, the title
    /// card, the running timeline, any scene change it had queued, and the
    /// chain flag itself. Returns whether a chain was live.
    ///
    /// What a **user-initiated** direct scene entry owes - the browser play
    /// page's scene picker - as distinct from the chain's own hand-offs: a
    /// picked scene must be that scene's free-roam, not the next leg of an
    /// opening the picker interrupted. With the flag still set, the picked
    /// scene's arrival tile spawned its trigger record as a chain leg
    /// (`spawn_arrival_trigger_record`) and `town01` installed its opening
    /// sweep, so the "interrupted" cutscene simply continued elsewhere.
    /// Entering `opdeene` re-arms the chain as ever.
    pub fn abandon_opening_chain(&mut self) -> bool {
        let was_live = self.cutscene.opening_chain_active
            || self.cutscene_timeline_active()
            || self.cutscene.narration.is_some();
        self.cutscene.narration = None;
        self.cutscene.card = None;
        self.cutscene.timeline = None;
        self.pending_named_scene_transition = None;
        self.scene_transition_hold = None;
        self.cutscene.opening_chain_active = false;
        self.cutscene.entering_town01_opening = false;
        was_live
    }

    // REF: FUN_801D1344
    // REF: FUN_8001FD44
    pub fn take_prologue_handoff(&mut self, confirm: bool) -> Option<&'static str> {
        if confirm
            && self.flags.story_flags & PROLOGUE_HANDOFF_FLAG != 0
            && self.cutscene.opening_chain_active
        {
            self.flags.story_flags &= !PROLOGUE_HANDOFF_FLAG;
            // Tear down whatever leg of the opening is mid-flight - the skip
            // abandons the remaining narration + choreography wholesale.
            self.abandon_opening_chain();
            // Mark the upcoming `town01` entry as the new-game opening so it
            // installs the opening cutscene timeline (which opens name entry at
            // its pinned op-`0x49`); a normal `town01` visit never sets this.
            self.cutscene.entering_town01_opening = true;
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
        if let Some(tl) = self.cutscene.timeline.take() {
            self.cutscene.timeline = Some(tl.arming_prologue_handoff());
        }
        true
    }

    /// `true` when story flag `flag` is set in the partition-2 gate bitmap
    /// (retail `DAT_80085758`). That base is the **system-flag bank** the
    /// field VM's `0x50`/`0x60`/`0x70` SET/CLEAR/TEST opcodes operate on
    /// ([`crate::world::StoryFlagState::system_flags`], same `byte = flag >> 3`,
    /// `bit = 0x80 >> (flag & 7)` addressing - `FUN_8003BDE0`'s test), so the
    /// gate check and the VM writes share one store. It also sits at offset
    /// `0x158` of the `0x80085600..0x80085800` save-bitmap window
    /// ([`crate::world::StoryFlagState::story_flag_bits`]); the save/load paths sync that overlap so
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

    /// `true` while the opening-cutscene timeline is still executing (installed
    /// and not yet complete). Diagnostics / tests read this; the hand-off gate
    /// itself keys off the scratchpad flag the timeline sets, not this.
    pub fn cutscene_timeline_active(&self) -> bool {
        self.cutscene
            .timeline
            .as_ref()
            .is_some_and(|t| !t.is_done())
    }

    /// `true` while a spawned field-VM context holds the player - the
    /// engine's reading of retail's engaged bit `+0x10 & 0x80000` as the
    /// script runner raises it.
    ///
    /// Retail has one rule for every script context, modal or not. A record
    /// `FUN_8003BDE0` spawns gets `+0x10 |= 0x100` and a script pointer
    /// (`0x8003C088..0x8003C0AC`), so the per-actor tick `FUN_8003BC08` steps
    /// it through `FUN_80039B7C` every frame (`jal` at `0x8003BD34`). That
    /// runner counts the frame into `*(0x801C6EA4)+0xA` and raises the
    /// player's `0x80000` on **every** frame it steps a context
    /// (`0x80039DB8..0x80039DD4`), and clears it only when the count drains
    /// on the context's closing raw `0x21` (`0x80039EE8..0x80039F14`, which
    /// also drops the context's `0x100`). The field tick `FUN_801D1344` skips
    /// the pad controller `FUN_801D01B0` while the bit is up (`0x801D1694`).
    /// So a concurrent helper record refuses the pad - walking, talking and
    /// the menu button - from its first slice to its end, exactly like the
    /// modal timeline; "modal" only decides the camera and the chain's beat
    /// sequencing. A helper counts from its first slice: one installed this
    /// tick has raised nothing yet, and one that runs to its end inside a
    /// slice is dropped before the next pad read, as retail's same-frame
    /// raise-and-clear leaves the bit down.
    ///
    /// REF: FUN_80039B7C (the raise and the clear), FUN_8003BC08 (`0x8003BD34`),
    /// FUN_8003BDE0 (the `0x100` install), FUN_801D1344 (`0x801D1694`)
    pub fn script_context_engages_player(&self) -> bool {
        // A parked scene change holds the player too: the door record that
        // issued it is parked (`26 FF FF` / `21`) for the whole countdown and
        // keeps raising the engaged bit. So does one requested but not yet
        // held: the port drops a record that ran its `0x3F` before the host
        // drains the request into the hold, and in that gap the pad must not
        // open a talk with whoever stands by (`taiku` P2[15] ends beside the
        // NPC whose talk then rode the scene change into `map03`).
        self.scene_transition_hold.is_some()
            || self.pending_named_scene_transition.is_some()
            || self.pending_scene_transition.is_some()
            || self.cutscene_timeline_active()
            || self
                .field_vm
                .helper_contexts
                .iter()
                .any(|tl| tl.stepped && !tl.is_done())
    }

    /// `true` while a dialogue engagement owns the pad and the player.
    ///
    /// The engine has **two** dialogue channels and either one can be live on
    /// its own. [`crate::world::DialogState::current`] is the simplified request the probe /
    /// world map open; [`crate::world::DialogState::inline`] is the faithful field-VM
    /// runner ([`crate::inline_dialogue`]), which the ordinary NPC-talk path
    /// runs and which can hold a box open with no `current_dialog` at all -
    /// an interaction record whose prologue selects its segment never sets
    /// one. Testing only the first therefore leaves the pad live under the
    /// commonest conversation in the game.
    ///
    /// This is the engine's stand-in for retail's single answer to the same
    /// question: the player actor's engaged bit `+0x10 & 0x80000`, raised by
    /// the touch post `FUN_801D5B5C` and cleared by the dialog SM's teardown.
    /// `FUN_801D01B0` branches on it at its very first test (`0x801D01F0`),
    /// past the action-button accept, past the menu-open accept and past every
    /// movement leg - so while it is set retail neither walks the player nor
    /// opens the pause menu.
    ///
    /// Call this rather than testing either field directly: a site that tests
    /// one and not the other is the exact asymmetry this predicate exists to
    /// make unrepresentable.
    ///
    /// REF: FUN_801D01B0 (`0x801D01F0`, the engaged-bit branch), FUN_801D5B5C
    pub fn dialogue_owns_input(&self) -> bool {
        self.dialog.current.is_some()
            || self.dialog.inline.is_some()
            // A spawned helper record parked on its text segment shows the
            // one shared box ([`Self::script_dialog_panel`]).
            || self
                .field_vm
                .helper_contexts
                .iter()
                .any(|tl| tl.dialog.is_some())
    }

    /// The dialog box a script context is showing: the modal timeline's when
    /// it has one, else the first concurrent helper's. Retail has one shared
    /// box, so hosts draw exactly this one
    /// ([`Self::drive_script_dialog`] routes the pad the same way).
    ///
    /// `None` while a battle runs. The box is drawn by the field overlay's
    /// pager (`FUN_801D84D0`, PROT 0897 at slot A `0x801CE818`), and the
    /// battle overlay (PROT 0898) is loaded over the same slot, so a context
    /// still parked on a text page when its fight starts keeps its park - the
    /// field contexts are frozen, not torn down - but nothing draws its box
    /// on the battle frame.
    // REF: FUN_801D84D0
    pub fn script_dialog_panel(&self) -> Option<&crate::dialog::OwnedDialogPanel> {
        if self.mode == SceneMode::Battle {
            return None;
        }
        self.cutscene
            .timeline
            .as_ref()
            .and_then(|tl| tl.dialog.as_ref())
            .or_else(|| {
                // The helper that claimed the box first; the others wait.
                self.field_vm
                    .helper_contexts
                    .iter()
                    .filter(|tl| !tl.done)
                    .filter_map(|tl| tl.dialog.as_ref().map(|d| (tl.dialog_claim, d)))
                    .min_by_key(|(claim, _)| *claim)
                    .map(|(_, d)| d)
            })
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
    /// Step the modal cutscene timeline and every concurrent helper context
    /// **once per retail display frame**.
    ///
    /// Retail's script contexts live on actors that the per-frame actor-list
    /// walk (`FUN_8002519C`) dispatches every frame - every context gets one
    /// run-until-yield slice per frame, with no budget and no round-robin.
    /// The rate that matters is therefore the display-frame rate, and every
    /// duration a cutscene record can express is counted in display frames:
    /// op-`0x4A` `WaitFrames` accumulates `DAT_1F800393` (the frame-skip
    /// factor = the logic tick's `dt` in display frames) into `ctx[+0x54]`
    /// per visit, and the camera mover accumulates the same `dt` into its
    /// progress - so a logic tick running once per `dt` display frames credits
    /// exactly one display frame of wait per display frame either way.
    ///
    /// The engine's sim clock runs at 100 Hz, so stepping the timeline once
    /// per sim tick drained every `WaitFrames` 1.67x too fast. The narration
    /// roller was already corrected onto the retail-frame sub-clock
    /// ([`crate::world::FrameClock::display_frame_step`]); the timeline is paced off
    /// the same sub-clock here so wait-dominated legs keep retail wall-time
    /// too. (Measured against a headless retail capture of the New Game
    /// opening chain: before, the roller-bound `opdeene` leg matched retail
    /// wall-time to ~1% while the wait-bound `opstati` / `opurud` legs ran
    /// ~15% / ~30% short.)
    // REF: FUN_8002519C
    // REF: FUN_801DC0BC
    pub fn step_spawned_record_contexts(&mut self) {
        if self.clock.display_frame_step != 1 {
            return;
        }
        let glide = &mut self.camera.state.glide_frames;
        *glide = (*glide - 1).max(0);
        self.step_cutscene_timeline();
        self.step_helper_contexts();
    }

    // REF: FUN_8003BDE0
    pub fn step_cutscene_timeline(&mut self) {
        let Some(mut tl) = self.cutscene.timeline.take() else {
            return;
        };
        if tl.done {
            self.cutscene.timeline = Some(tl);
            return;
        }
        // Freeze the timeline while the name-entry overlay it spawned is open:
        // its op-`0x49` STATE_RESUME is suspended until the player commits a
        // name, so neither the VM nor the frame cap advances meanwhile.
        if self.name_entry_active() {
            self.cutscene.timeline = Some(tl);
            return;
        }
        // Parked at an inline dialog box (a `0x1F` glyph segment the record's
        // own flow reached - e.g. the Mei walk-on beat's conversation). Tick
        // the typewriter and route pad input exactly as the inline-script
        // runner does ([`Self::step_inline_dialogue`]): Up/Down move a picker
        // cursor, confirm commits a choice (applying its relative jump) or
        // dismisses a finished box, resuming the timeline past the segment.
        // The park freezes the frame cap - a dialog waits on the player.
        if tl.dialog.is_some() {
            self.drive_script_dialog(&mut tl, true);
            if tl.dialog.is_some() && !tl.done {
                self.cutscene.timeline = Some(tl);
                return;
            }
            if tl.done {
                let restore = tl.restore_hidden_on_complete;
                self.release_interaction_context(&tl);
                self.restore_owed_player_scale(&tl.bytecode, tl.pc, &tl.visited);
                self.cutscene.timeline = None;
                if restore {
                    self.restore_hidden_field_npcs();
                }
                return;
            }
        }
        // Held AT an inline narration block's op because a PRIOR roller is
        // still scrolling (`narration_pending_open`) - two rollers never
        // stack. Wait for the active roller to drain; retail's `FUN_80037174`
        // clears the parent's halt bit when every page has scrolled off.
        if tl.narration_pc.is_some() {
            if self.cutscene_narration_active() {
                self.cutscene.timeline = Some(tl);
                return;
            }
            // The prior roller drained: clear the hold and leave the PC AT
            // the block op so the loop below re-enters and opens it now that
            // nothing stacks.
            tl.narration_pc = None;
            tl.narration_pending_open = false;
        }
        self.field_vm.halted_elsewhere = self
            .field_vm
            .helper_contexts
            .iter()
            .flat_map(|h| h.halted_targets())
            .collect();
        if self.run_spawned_record_slice(&mut tl, true) {
            self.finish_cutscene_timeline_frame(tl);
        } else {
            // Still parked on the channel-completion handshake: keep the
            // timeline installed and re-test next tick.
            self.cutscene.timeline = Some(tl);
        }
    }

    /// Tick a script context's owned dialog panel one frame and, when
    /// `routed`, hand it the pad: Up/Down move a picker cursor, confirm
    /// commits a choice (applying its relative jump), turns a page or
    /// dismisses a finished box, resuming the context past the segment.
    /// Shared by the modal timeline and the concurrent helper contexts:
    /// retail's runner `FUN_80039B7C` hands any engaged context's text
    /// segment to the one shared dialog box and parks the context on it
    /// (its `+0x9C == 2` arm) until the box closes.
    // REF: FUN_80039B7C
    fn drive_script_dialog(
        &mut self,
        tl: &mut crate::cutscene_timeline::CutsceneTimeline,
        routed: bool,
    ) {
        let Some(panel) = tl.dialog.as_mut() else {
            return;
        };
        let pressed = |w: &Self, b: crate::input::PadButton| routed && w.input.just_pressed(b);
        let confirm = pressed(self, crate::input::PadButton::Cross)
            || pressed(self, crate::input::PadButton::Circle);
        if panel.menu_active() {
            if pressed(self, crate::input::PadButton::Up) {
                panel.move_picker_cursor(-1);
            }
            if pressed(self, crate::input::PadButton::Down) {
                panel.move_picker_cursor(1);
            }
        }
        // One-frame rule (see `step_inline_dialogue`): a menu that opened
        // on this tick is shown before any confirm can commit it.
        let menu_was_open = panel.menu_active();
        panel.tick_at_auto(self.clock.frame_step, &mut self.dialog.auto_press);
        // The pager's automatic press (`_DAT_80073F00`, op `4C 89`) is a
        // confirm the player did not make.
        let confirm = confirm || panel.take_auto_press();
        if confirm {
            if panel.menu_active() && (!menu_was_open || !panel.picker_takes_input()) {
                // Opened this frame, or still sliding in (the pager reads the
                // choice only once the slide rests): nothing to commit yet.
            } else if panel.menu_active() {
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
            } else if panel.is_done() {
                tl.pc = panel.pc;
                tl.dialog = None;
            } else if panel.is_waiting_for_input() {
                // Multi-page conversation: turn the page in place (the
                // timeline stays parked on the segment until the last
                // page is dismissed).
                panel.advance_page();
                if panel.is_done() {
                    tl.pc = panel.pc;
                    tl.dialog = None;
                }
            } else {
                // Still typing or scrolling: the pager's skip latch
                // completes the page (`crate::dialog_window`).
                panel.confirm_while_typing();
            }
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
    /// - `modal` sets [`crate::world::CutsceneState::in_timeline`] while the VM steps (the
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
    /// One vsync of a player compass-walk leg (`B7 F8` / `C1 F8`): the walk
    /// kernel's spend against the player actor. `true` while the leg still
    /// has units left (and the park timeout has not run out).
    // REF: FUN_8003774C (the 0x37 / 0x41 arm)
    fn step_player_glide(
        &mut self,
        glide: &mut crate::cutscene_timeline::TimelinePlayerGlide,
    ) -> bool {
        glide.frames += 1;
        let done = match self.player_actor_slot {
            Some(p) if (p as usize) < self.actors.len() => {
                let ms = &self.actors[p as usize].move_state;
                glide.state.world_x = ms.world_x;
                glide.state.world_z = ms.world_z;
                let done = vm::motion_vm::compass_walk(
                    &mut glide.state,
                    glide.body0,
                    glide.body1,
                    glide.rate,
                );
                let (nx, nz) = (glide.state.world_x, glide.state.world_z);
                let y = self.sample_field_floor_height(i32::from(nx), i32::from(nz)) as i16;
                let ms = &mut self.actors[p as usize].move_state;
                ms.world_x = nx;
                ms.world_z = nz;
                ms.world_y = y;
                done
            }
            // No player actor to move: nothing plays the leg out.
            _ => true,
        };
        !done && glide.frames < WALK_PARK_TIMEOUT
    }

    /// One vsync of an NPC compass-walk leg (`B7 <id>` / `C1 <id>`): the
    /// walk kernel's spend against the placement's live position. `true`
    /// while the leg still has units left (and the park timeout has not run
    /// out).
    // REF: FUN_8003774C (the 0x37 / 0x41 arm)
    fn step_npc_glide(&mut self, glide: &mut crate::cutscene_timeline::TimelineNpcGlide) -> bool {
        glide.frames += 1;
        let Some(&(x, z)) = self.npcs.positions.get(&glide.slot) else {
            return false;
        };
        glide.state.world_x = x;
        glide.state.world_z = z;
        let done =
            vm::motion_vm::compass_walk(&mut glide.state, glide.body0, glide.body1, glide.rate);
        self.npcs
            .positions
            .insert(glide.slot, (glide.state.world_x, glide.state.world_z));
        !done && glide.frames < WALK_PARK_TIMEOUT
    }

    fn run_spawned_record_slice(
        &mut self,
        tl: &mut crate::cutscene_timeline::CutsceneTimeline,
        modal: bool,
    ) -> bool {
        tl.frames = tl.frames.saturating_add(1);
        tl.stepped = true;
        // The player's poked scene-bank clip plays one engine tick per
        // slice, parked or not - retail's clip tick runs every frame.
        tl.player_clip_ticks = tl.player_clip_ticks.saturating_sub(1);
        self.cutscene.in_timeline = modal;
        self.field_vm.in_spawned_record_slice = true;
        let mut channels = std::mem::take(&mut self.field_vm.channels);
        // Host hooks resolve cross-context ids against the channel set while
        // one of these is executing; the live vector is moved out for the
        // borrow, so they read this copy (`World::channel_view`).
        self.field_vm.stepping_view = channels.clone();
        let channel_pre_pos: Vec<(u16, u16)> = channels
            .iter()
            .map(|c| (c.ctx.world_x, c.ctx.world_z))
            .collect();
        // Cross-context channel handshake (`B3 <id> <bit>` = CFLAG_TST
        // against a spawned per-actor channel): the timeline PARKED here on a
        // prior tick. Retail's op-`0x33` arm holds the PC while the target's
        // bit is SET and advances once it is clear (`0x801DEE44`), so re-test
        // it before stepping - rather than the pre-handshake behaviour of
        // advancing past the wait by instruction width.
        if let Some(mut wait) = tl.channel_wait.take() {
            let flag_set = crate::field_channels::resolve_target(&channels, wait.target_id)
                .map(|ci| channels[ci].ctx.flags & (1u32 << (wait.bit & 0x1F)) != 0);
            match flag_set {
                // Still waiting (the channel's bit is still up) and within the
                // park budget: hold the PC on the flag-test op another tick.
                Some(true) if wait.frames < CHANNEL_WAIT_PARK_TIMEOUT => {
                    wait.frames += 1;
                    tl.channel_wait = Some(wait);
                    self.field_vm.channels = channels;
                    self.field_vm.stepping_view.clear();
                    self.cutscene.in_timeline = false;
                    self.field_vm.in_spawned_record_slice = false;
                    return false;
                }
                // The channel dropped the flag (resume), the target is gone, or
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
        // Player-channel (`0xF8`) arc park: the timeline is holding at a
        // `C3 F8` op while the player's scripted arc flies (retail halts the
        // caller with the player and the arc's watcher releases both on
        // landing). When no arc could start, the countdown armed by a
        // preceding `A2 F8 <move_id>` stands in for the playout instead. Either
        // way, once it clears, step PAST the op by its encoded width so the
        // record flows on to its trailing ops (the door records' terminal
        // `0x3F`).
        // REF: FUN_8003BDE0
        if let Some(width) = tl.player_wait.take() {
            tl.player_move_frames = tl.player_move_frames.saturating_sub(1);
            if tl.player_move_frames > 0 || self.player_script_arc_live() {
                tl.player_wait = Some(width);
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                return false;
            }
            tl.pc += width;
        }
        // Cross-context walk-to-tile park (`C7 <id> <tx> <tz> <mode>` = op
        // 0x47 against an NPC channel / the player anchor): retail saves the
        // yield-op pointer into the TARGET actor's `+0x94` and the per-frame
        // walk kernel (`FUN_8003774C` case 0x47) moves it toward the tile at
        // `0x80 >> (2 + (mode & 7))` per frame, resuming the parked record on
        // arrival. The NPC leg glides through the motion VM
        // (`tick_field_npc_motions` removes the leg on arrival); the player
        // leg steps here directly. The town01 Mei walk-on beat is the pinned
        // case: `C7 46 11 1B 33` / `C7 46 11 1A 33` walk Mei from her seat at
        // the Vahn's-house door into the conversation frame, and
        // `C7 F8 12 1A 33` walks the player to the beat's camera focus -
        // dropping these left Mei OFFSCREEN (top corner of the shot) for the
        // whole conversation.
        // REF: FUN_8003774C (case 0x47), FUN_8003BC08 (0x400 walk-bit tick)
        if let Some(mut walk) = tl.walk_wait.take() {
            walk.frames += 1;
            let arrived = match walk.slot {
                Some(slot) => !self.npcs.motions.contains_key(&slot),
                None => self.step_player_walk_leg(walk.target, walk.speed),
            };
            if !arrived && walk.frames < WALK_PARK_TIMEOUT {
                tl.walk_wait = Some(walk);
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                // A walk park is real playout progress, not a hang: don't let
                // it accumulate toward the anti-hang frame cap (a long leg -
                // tower P2[2]'s `C7 F8 0D 45` covers ~7500 units at 4/tick -
                // would otherwise burn the cap mid-walk and the forced
                // completion would skip the record's trailing flag latches).
                tl.frames = tl.frames.saturating_sub(1);
                return false;
            }
            // Arrived (or safety timeout: snap to the target so the
            // choreography stays coherent) - resume past the yield.
            if !arrived {
                match walk.slot {
                    Some(slot) => {
                        self.npcs.motions.remove(&slot);
                        self.npcs.positions.insert(slot, walk.target);
                    }
                    None => {
                        let y = self.sample_field_floor_height(
                            i32::from(walk.target.0),
                            i32::from(walk.target.1),
                        ) as i16;
                        if let Some(p) = self.player_actor_slot
                            && let Some(actor) = self.actors.get_mut(p as usize)
                        {
                            actor.move_state.world_x = walk.target.0;
                            actor.move_state.world_z = walk.target.1;
                            actor.move_state.world_y = y;
                        }
                    }
                }
            }
            tl.pc = walk.resume_pc;
        }
        // Player compass walk (`B7 F8 <b0> <b1>` / `C1 F8 ..`, ops `0x37` /
        // `0x41` against the player anchor): the walk kernel translates the
        // player in place, one speed unit per vsync, while the record runs on
        // past the op; the record's next cross-context op on the player
        // waits until the leg is spent (see the halted-target refusal below).
        //
        // Retail's kernel runs after the script in the record's actor tick
        // (`FUN_8003BC08`: runner at `0x8003BD34`, kernel at `0x8003BD50`),
        // so the leg spends its first unit on the frame the op arms it, and
        // a leg that lands frees the player for the script only on the next
        // frame. `glide_hold` carries that frame.
        // REF: FUN_8003774C (the 0x37 / 0x41 arm), FUN_8003BC08
        let mut glide_hold = false;
        if let Some(mut glide) = tl.player_glide.take() {
            if self.step_player_glide(&mut glide) {
                tl.player_glide = Some(glide);
            }
            glide_hold = true;
        }
        // NPC compass walks (`B7 <id> ..` / `C1 <id> ..`): the same in-place
        // kernel against a placement. A leg that lands this frame keeps its
        // actor held for the script until the next one, like `glide_hold`.
        // REF: FUN_8003774C (the 0x37 / 0x41 arm), FUN_8003BC08
        let mut npc_glide_hold: Vec<u8> = Vec::new();
        for mut glide in std::mem::take(&mut tl.npc_glides) {
            npc_glide_hold.push(glide.slot);
            if self.step_npc_glide(&mut glide) {
                tl.npc_glides.push(glide);
            }
        }
        // NPC walk-to-tile legs (`C7 <id> ..`) run on the motion VM
        // (`tick_field_npc_motions` drops the leg on arrival); the actor stays
        // held for the script while its leg is live and on the landing frame.
        // REF: FUN_801DE840 (0x801DEFC0..0x801DF054)
        for slot in std::mem::take(&mut tl.npc_walks) {
            npc_glide_hold.push(slot);
            if self.npcs.motions.contains_key(&slot) {
                tl.npc_walks.push(slot);
            }
        }
        // NPC rotate legs (`B8 <id> <dir> <budget>`): the same in-place
        // `0x38` RotateToAngle kernel the player park runs, stepped here
        // while the record runs on; the actor stays held for the script
        // until the ramp snaps, and on the snap frame.
        // REF: FUN_801DE840 (0x801DEE90..0x801DEF24), FUN_8003774C (case 0x38)
        for mut fw in std::mem::take(&mut tl.npc_facings) {
            if let Some(slot) = fw.slot {
                npc_glide_hold.push(slot);
            }
            fw.frames += 1;
            let r = vm::motion_vm::step(
                &mut fw.state,
                vm::motion_vm::MotionTarget::default(),
                &fw.program,
            );
            if fw.state.yaw_written {
                self.set_timeline_facing(fw.slot, fw.state.yaw as i16);
            }
            if r != vm::motion_vm::StepResult::Done && fw.frames < WALK_PARK_TIMEOUT {
                tl.npc_facings.push(fw);
            }
        }
        // NPC face-at legs (`CC <id> 85|8E|8F ..`): the target's walk kernel
        // turns it toward the bind while the record runs on; the actor stays
        // held for the script until the leg's terminal frame, and on it.
        // REF: FUN_8003774C (the 0x4C arm), FUN_8003BC08
        for mut face in std::mem::take(&mut tl.npc_faces) {
            npc_glide_hold.push(face.slot);
            face.frames += 1;
            if !self.step_npc_face_leg(face.slot, &mut face.ramp) && face.frames < WALK_PARK_TIMEOUT
            {
                tl.npc_faces.push(face);
            }
        }
        // Player end-latch spin (`AD F8 08`): held while the scene-bank clip
        // the record poked onto the player is still playing; retail's clip
        // tick latches `+0x62 & 0x100` on its last frame and the spin falls
        // through on the next visit.
        // REF: FUN_800204F8 (0x800206E4..0x8002072C)
        if let Some(width) = tl.player_clip_wait.take() {
            if tl.player_clip_ticks > 0 {
                tl.player_clip_wait = Some(width);
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                tl.frames = tl.frames.saturating_sub(1);
                return false;
            }
            tl.pc += width;
        }
        // Cross-context rotate park (`B8 <id> <dir|flags> <budget|dir>` = op
        // 0x38 with a non-zero budget against an NPC channel): retail parks
        // the record and the `FUN_8003774C` 0x38 RotateToAngle leg ramps the
        // target actor's `+0x26` linearly over the operand budget - per-op
        // turn rates (`arc / budget`), raw pre-unwrap mid-ramp headings, and
        // an exact terminal snap onto the compass entry. Step the parked leg
        // once per tick, mirror the raw yaw into the render-heading map, and
        // resume the record past the yield when the ramp snaps.
        // REF: FUN_8003774C (case 0x38 interpreted in place)
        if let Some(mut fw) = tl.facing_wait.take() {
            fw.frames += 1;
            let r = vm::motion_vm::step(
                &mut fw.state,
                vm::motion_vm::MotionTarget::default(),
                &fw.program,
            );
            if fw.state.yaw_written {
                // Raw write-back (`yaw` may sit outside 0..0xFFF mid-ramp,
                // exactly as retail's `+0x26` does); render consumers mask.
                self.set_timeline_facing(fw.slot, fw.state.yaw as i16);
            }
            if r != vm::motion_vm::StepResult::Done && fw.frames < WALK_PARK_TIMEOUT {
                tl.facing_wait = Some(fw);
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                // Like the walk park: a rotate park is real playout progress,
                // not a hang - keep it off the anti-hang frame cap.
                tl.frames = tl.frames.saturating_sub(1);
                return false;
            }
            tl.pc = fw.resume_pc;
        }
        // Player face-at park (`CC F8 85|8E|8F <lo> <hi> <id>`): the walk
        // kernel's FaceTarget leg turns the player toward the named actor,
        // and the record resumes past the acquire on the leg's terminal
        // frame - see `CutsceneTimeline::player_face`.
        // REF: FUN_8003774C (the 0x4C arm)
        if let Some((mut ramp, resume_pc, frames)) = tl.player_face.take() {
            let done = self.step_player_face_leg(&mut ramp);
            if !done && frames < WALK_PARK_TIMEOUT {
                tl.player_face = Some((ramp, resume_pc, frames + 1));
                self.field_vm.channels = channels;
                self.field_vm.stepping_view.clear();
                self.cutscene.in_timeline = false;
                self.field_vm.in_spawned_record_slice = false;
                tl.frames = tl.frames.saturating_sub(1);
                return false;
            }
            tl.pc = resume_pc;
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
                // timeline, so the camera cuts / fades / waits authored after
                // the block play UNDER the scrolling text - for EVERY block,
                // the last included. (Pinned by the `map01` fly-in retail
                // capture: its last crawl's authored `4A` 600 + 330 tail runs
                // concurrent with the roller - the leg span only fits the
                // authored waits, leaving no room for a serialized roller.)
                // If a prior roller is still scrolling when a block is
                // reached, hold (don't stack rollers) until it drains, then
                // re-enter to open this one. Nothing holds the final pages:
                // the record's terminal SceneChange runs on under them.
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
                            let pages = site.pages.clone();
                            // The `CC F8 E8` geometry seed the field VM runs
                            // immediately before the block (retail stores it
                            // into `*0x801C6EA4 +0x4C..+0x50`; a block with no
                            // seed op of its own reads the one left there).
                            if let Some(seed) = host
                                .world
                                .cutscene
                                .narration_seed
                                .config_op_before(&tl.bytecode, pc)
                            {
                                host.world.cutscene.narration_seed = seed;
                            }
                            host.world.open_cutscene_narration(pages);
                            // Non-blocking: the roller scrolls on its own
                            // (`World::tick`); continue into the camera cuts.
                            tl.pc = site_end;
                            continue;
                        }
                        legaia_asset::cutscene_text::NarrationKind::Card => {
                            let blank = site.pages.iter().all(|p| p.trim().is_empty());
                            host.world.cutscene.card = if blank {
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
                        // Modal timeline or concurrent helper alike: the
                        // runner `FUN_80039B7C` parks ANY engaged context on
                        // its text segment and hands it to the shared dialog
                        // box (`+0x9C = 2`). A helper used to complete at its
                        // first segment instead, which dropped the rest of
                        // every op-`0x44` record with a line of text in it -
                        // town01 `P2[25]` (the FMV hand-off to town0b) and
                        // town0e `P2[5]` (the ending's hop to edteien).
                        //
                        // Resolve the record's `0xC1`/`0xC2`/`0xC4` name
                        // escapes, exactly as the prop-interaction panel
                        // does, or a name renders as an empty string.
                        let _ = modal;
                        let mut panel = crate::dialog::OwnedDialogPanel::at_segment(
                            std::sync::Arc::clone(&tl.bytecode),
                            pc,
                        );
                        panel.substitutions = host.world.dialog_substitutions(&tl.bytecode);
                        tl.dialog = Some(panel);
                        host.world.field_vm.dialog_claims += 1;
                        tl.dialog_claim = host.world.field_vm.dialog_claims;
                        break;
                    }
                    tl.pc = pc + 1;
                    continue;
                }
                let opcode_byte = tl.bytecode.get(pc).copied().unwrap_or(0);
                // A terminal SceneChange (`0x3F`) does NOT wait for a roller
                // still scrolling: the op's arm only builds the scene-change
                // packet (`FUN_8001FD44`), and a per-vsync capture of the
                // zero-input `opdeene` leg has the record execute its `3F`
                // with the 8-page Seru-history roller (`FUN_80037174`) three
                // pages short of retiring; the departing scene tears the
                // roller down with everything else.
                // A crawl's roller holds the player's halt bit for its whole
                // run: the `CC F8 80 N` spawn is a halt-acquire on its target
                // (`ori v0,v0,0x400` into the player's `+0x10` at
                // `0x801E1F24`), and the roller clears its parent's `0x400`
                // as it retires the last page. So `B3 F8 0A` - the halt-bit
                // test on the player - is how a record waits for its crawl:
                // `opurud`'s record sits on one before its `3F` until the
                // last page retires (per-vsync capture), while `opdeene`'s
                // carries none and changes scene under its roller.
                // REF: FUN_801DE840 (0x801E1ECC..0x801E1F58), FUN_80037174
                if opcode_byte == 0xB3
                    && tl.bytecode.get(pc + 1) == Some(&0xF8)
                    && tl.bytecode.get(pc + 2) == Some(&0x0A)
                    && host.world.cutscene_narration_active()
                {
                    tl.frames = tl.frames.saturating_sub(1);
                    break;
                }
                // Halted-target refusal: a cross-context op aimed at an actor
                // ANOTHER context holds in a walk / rotate / glide park waits
                // at the op and retries next frame. The park holds the
                // target's halt bit `0x400` until the kernel lands it, and
                // the dispatcher's prologue returns with the PC still on the
                // op for any target carrying `0x400` while the scene word
                // `*(_DAT_801C6EA4) + 8` is zero, unless the caller is the
                // system context `0xFB`. A spawned record's context is never
                // that one: `FUN_8003BDE0` stamps its global record index into
                // `+0x50` (`0x8003C094`); the `0xFB` this context carries is
                // the engine's stand-in. Without the refusal two records
                // walked the player at once: `dolk2` P2[15]'s
                // `C7 F8 46 4C 33` pulled against P2[12]'s `C7 F8 48 53 23`,
                // and the tug-of-war left the party inside the wall at tile
                // (66, 87).
                // REF: FUN_801DE840 (0x801DE90C..0x801DE944), FUN_8003BDE0
                let halted_target = opcode_byte & 0x80 != 0
                    && !host.world.field_vm.halted_elsewhere.is_empty()
                    && match vm::field::peek_extended(&tl.bytecode, pc) {
                        Some(0xF8) => host.world.field_vm.halted_elsewhere.contains(&None),
                        Some(t) => crate::field_channels::resolve_target(&channels, t)
                            .filter(|&ci| !channels[ci].object_bind)
                            .is_some_and(|ci| {
                                host.world
                                    .field_vm
                                    .halted_elsewhere
                                    .contains(&Some(channels[ci].placement_index as u8))
                            }),
                        None => false,
                    };
                // The player walk this context armed itself holds the player's
                // `0x400` the same way: the `0x37` / `0x41` arm advances the
                // record past the op (`s7 = 3` at `0x801DEEFC` for a player
                // target) and the next cross-context op on the player waits
                // for the leg to land - `map01`'s credits record sits on its
                // `B8 F8 82 08` at `+0x97` while the `C1 F8 03 C4` leg before
                // it walks Vahn. `32 <id> 0A` (the halt clear) is exempt
                // (`0x801DE8E0..0x801DE904`).
                // REF: FUN_801DE840 (0x801DEE90..0x801DEF1C)
                let own_glide_target = opcode_byte & 0x80 != 0
                    && (tl.player_glide.is_some() || glide_hold)
                    && vm::field::peek_extended(&tl.bytecode, pc) == Some(0xF8)
                    && !(opcode_byte & 0x7F == 0x32 && tl.bytecode.get(pc + 2) == Some(&0x0A));
                // The NPC legs this context armed hold their actors the
                // same way (the `s7 = 3` advance is taken for every target).
                // That includes a walk-to-tile leg armed earlier in this same
                // slice (`tl.npc_walks`): `dolk2` P2[11] runs `C7 1F 46 5B 32`
                // then `B3 1F 0A` within one tick, and the halt-bit verify
                // must hold until Noa lands - stepping past it opened the
                // "Noa: Vahn..." box with Noa still walking through Vahn.
                let own_npc_glide_target = opcode_byte & 0x80 != 0
                    && (!tl.npc_glides.is_empty()
                        || !tl.npc_walks.is_empty()
                        || !tl.npc_faces.is_empty()
                        || !npc_glide_hold.is_empty())
                    && !(opcode_byte & 0x7F == 0x32 && tl.bytecode.get(pc + 2) == Some(&0x0A))
                    && vm::field::peek_extended(&tl.bytecode, pc)
                        .filter(|&t| t != 0xF8 && t != 0xFB)
                        .and_then(|t| crate::field_channels::resolve_target(&channels, t))
                        .filter(|&ci| !channels[ci].object_bind)
                        .is_some_and(|ci| {
                            let slot = channels[ci].placement_index as u8;
                            npc_glide_hold.contains(&slot)
                                || tl.npc_glides.iter().any(|g| g.slot == slot)
                                || tl.npc_walks.contains(&slot)
                                || tl.npc_faces.iter().any(|f| f.slot == slot)
                        });
                if halted_target || own_glide_target || own_npc_glide_target {
                    tl.frames = tl.frames.saturating_sub(1);
                    break;
                }
                // Cross-context dispatch (`0x80`-bit ops): resolve the target
                // byte to a spawned per-actor channel (`ctx[+0x50] == target`,
                // retail `FUN_8003C83C`) and run the op against THAT context -
                // this is how the timeline cues the vignette actors. `0xF8`
                // (player anchor) / `0xFB` (system) keep the timeline's own
                // context (the player pokes route through host hooks).
                //
                // Partition-0 object contexts resolve too: the scene entry
                // spawns an object-bind channel per `.MAP` object script
                // (retail `FUN_8003A55C` writes the gate-0 trigger's flat
                // record index into `actor[+0x50]`), so the Mei beat's `0x01`
                // pokes land on the Vahn's-house door context. An id that
                // STILL matches no channel is skipped by its decoded width
                // instead: running it against the timeline's own ctx
                // corrupted the timeline (a `B1 <id> 00` set the timeline's
                // OWN busy bit, and the `CC <id> A0` busy-wait then hijacked
                // the caller PC into the record header).
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
                // Cross-context walk-to-tile yield (`C7 <id|F8> <tx> <tz>
                // <mode>`): retail parks the record and the walk kernel
                // (`FUN_8003774C` case 0x47) moves the TARGET toward the tile
                // in place. Arm the walk + park; the pre-step gate resumes
                // past the op on arrival. The despawn form (tile 127,127)
                // seats instantly - walking to the off-map box is invisible.
                // REF: FUN_8003774C (case 0x47)
                if opcode_byte & 0x7F == 0x47
                    && opcode_byte & 0x80 != 0
                    && let (Some(&b0), Some(&b1), Some(&b2)) = (
                        tl.bytecode.get(pc + 2),
                        tl.bytecode.get(pc + 3),
                        tl.bytecode.get(pc + 4),
                    )
                {
                    let ext = vm::field::peek_extended(&tl.bytecode, pc);
                    let decode = |b: u8| -> i16 {
                        i16::from(b & 0x7F) * 0x80 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 }
                    };
                    let (tx, tz) = (decode(b0), decode(b1));
                    let speed = crate::world::field_npc_walk_step_speed(0x80, b2 & 7);
                    let parked_sentinel =
                        (b0 & 0x7F, b1 & 0x7F) == crate::man_field_scripts::PARKED_SENTINEL_TILE;
                    let walk_slot: Option<Option<u8>> = if ext == Some(0xF8) {
                        Some(None) // player anchor
                    } else if let Some((_, ci)) = target {
                        (!channels[ci].object_bind)
                            .then_some(Some(channels[ci].placement_index as u8))
                    } else {
                        None
                    };
                    if let Some(slot) = walk_slot {
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        if let Some(s) = slot {
                            if let Some((_, ci)) = target {
                                channels[ci].ctx.world_x = tx as u16;
                                channels[ci].ctx.world_z = tz as u16;
                            }
                            if parked_sentinel {
                                // Despawn: seat at the hide box, no playout.
                                host.world.npcs.positions.insert(
                                    s,
                                    (
                                        crate::world::FIELD_OFFMAP_HIDE_XZ,
                                        crate::world::FIELD_OFFMAP_HIDE_XZ,
                                    ),
                                );
                                tl.pc = pc + 5;
                                continue;
                            }
                            if host.world.start_field_npc_motion(s, tx, tz) {
                                if let Some(m) = host.world.npcs.motions.get_mut(&s) {
                                    m.state.speed = speed;
                                    // The mode byte's high nibble picks the
                                    // walk kernel's approach (`srl a1,a1,0x4`
                                    // at `0x80037BEC`).
                                    m.state.approach = b2 >> 4;
                                }
                                // The record runs on past an NPC walk (`li
                                // s7,4` in the delay slot at `0x801DF030`);
                                // only a player target parks the caller.
                                tl.npc_walks.retain(|&w| w != s);
                                tl.npc_walks.push(s);
                                tl.pc = pc + 5;
                                continue;
                            } else {
                                // No surfaced live position to glide from:
                                // seat directly (the pre-park fallback).
                                host.world.npcs.positions.insert(s, (tx, tz));
                                tl.pc = pc + 5;
                                continue;
                            }
                        } else if parked_sentinel {
                            tl.pc = pc + 5;
                            continue;
                        }
                        tl.walk_wait = Some(crate::cutscene_timeline::TimelineWalk {
                            slot,
                            target: (tx, tz),
                            resume_pc: pc + 5,
                            speed,
                            frames: 0,
                        });
                        break;
                    }
                }
                // Halt clear on an NPC (`B2 <id> 0A`, op 0x32 bit 10): the
                // actor tick runs the walk kernel only while `+0x10 & 0x400`
                // is up (`FUN_8003BC08`), so clearing the bit ends whatever
                // leg this context armed on it, where it stands. `opdeene`
                // cuts the two Seru's `C1` legs this way before their next
                // beat; leaving them running held every later op on both
                // actors for the rest of the legs.
                // REF: FUN_8003BC08, FUN_8003774C
                if opcode_byte == 0xB2
                    && tl.bytecode.get(pc + 2) == Some(&0x0A)
                    && let Some((_, ci)) = target
                    && !channels[ci].object_bind
                {
                    let slot = channels[ci].placement_index as u8;
                    tl.npc_glides.retain(|g| g.slot != slot);
                    tl.npc_facings.retain(|f| f.slot != Some(slot));
                    tl.npc_faces.retain(|f| f.slot != slot);
                    if tl.npc_walks.contains(&slot) {
                        tl.npc_walks.retain(|&w| w != slot);
                        host.world.npcs.motions.remove(&slot);
                    }
                    npc_glide_hold.retain(|&s| s != slot);
                }
                // Cross-context compass walk on an NPC (`B7 <id> <b0> <b1>` /
                // `C1 <id> ..` = op 0x37 / 0x41 against a placement channel):
                // retail's arm seats the op on the target's `+0x94`, raises
                // its `0x400` and advances the record past the op - the same
                // `s7 = 3` exit the player target takes - so arm the leg and
                // run on; the record's next op on this actor waits for it
                // (the refusal above). The leg replaces any walk the actor
                // had in flight: retail overwrites `+0x94`. Dropping it left
                // bylon's Maya at the top of the shrine stairs, out of frame,
                // for her whole first conversation.
                // REF: FUN_801DE840 (0x801DEE90..0x801DEF1C), FUN_8003774C (the 0x37 / 0x41 arm)
                if matches!(opcode_byte & 0x7F, 0x37 | 0x41)
                    && opcode_byte & 0x80 != 0
                    && let (Some(&body0), Some(&body1)) =
                        (tl.bytecode.get(pc + 2), tl.bytecode.get(pc + 3))
                    && let Some((_, ci)) = target
                    && !channels[ci].object_bind
                {
                    let slot = channels[ci].placement_index as u8;
                    if pc < tl.visited.len() {
                        tl.visited[pc] = true;
                    }
                    host.world.npcs.motions.remove(&slot);
                    // An actor nothing has moved yet stands where its context
                    // was seated (retail's kernel walks the live `+0x14` /
                    // `+0x18`, which the spawn wrote). Without the seat the
                    // leg had no start and was dropped: `opdeene`'s vignette
                    // actor `0x05` never took its `C1 05 00 C4` step.
                    host.world.npcs.positions.entry(slot).or_insert((
                        channels[ci].ctx.world_x as i16,
                        channels[ci].ctx.world_z as i16,
                    ));
                    let mut glide = crate::cutscene_timeline::TimelineNpcGlide {
                        slot,
                        state: vm::motion_vm::MotionState {
                            speed: 1,
                            ..Default::default()
                        },
                        body0,
                        body1,
                        rate: if opcode_byte & 0x7F == 0x37 {
                            0x80
                        } else {
                            0x40
                        },
                        frames: 0,
                    };
                    // Retail's kernel runs after the script in the same actor
                    // tick, so the leg spends its first unit on the arming
                    // frame (the player arm does the same).
                    tl.npc_glides.retain(|g| g.slot != slot);
                    if host.world.step_npc_glide(&mut glide) {
                        tl.npc_glides.push(glide);
                    }
                    npc_glide_hold.push(slot);
                    tl.pc = pc + 4;
                    continue;
                }
                // Cross-context facing op (`B8 <id> <op0> <op1>` = op 0x38
                // CAM_CFG against a spawned NPC channel).
                //
                // - Simple path (`op1 & 0x7F == 0`): retail copies the
                //   compass-LUT entry `0x80073F04 + (op0 & 0xF) * 2` straight
                //   into the target's `+0x26` - an instant scripted pose.
                // - Budget path: the halt-acquire arm parks the record and
                //   the op bytes run in place as the walk kernel's `0x38`
                //   RotateToAngle leg - a linear ramp at the op's own
                //   `arc / budget` rate with an exact terminal compass snap
                //   (the town01 Mei dinner beat authors seven of these at
                //   budgets 0x12..0x20). Arm the rotate park; the pre-step
                //   gate plays it out and resumes past the yield.
                // REF: FUN_801DE840 (case 0x38), FUN_8003774C (case 0x38)
                //
                // The player (`B8 F8 ..`, `FUN_8003C83C` resolving `0xF8` to
                // the player object) takes both paths the same way, on its
                // own heading - every story beat turns the hero with these.
                let facing_target = match target {
                    Some((_, ci)) if !channels[ci].object_bind => {
                        Some(Some(channels[ci].placement_index as u8))
                    }
                    None if vm::field::peek_extended(&tl.bytecode, pc) == Some(0xF8) => Some(None),
                    _ => None,
                };
                if opcode_byte & 0x7F == 0x38
                    && opcode_byte & 0x80 != 0
                    && let (Some(&op0), Some(&op1)) =
                        (tl.bytecode.get(pc + 2), tl.bytecode.get(pc + 3))
                    && let Some(slot) = facing_target
                {
                    if pc < tl.visited.len() {
                        tl.visited[pc] = true;
                    }
                    if op1 & 0x7F == 0 {
                        if let Some(h) =
                            crate::man_field_scripts::facing_index_to_engine_heading(op0 & 0xF)
                        {
                            host.world.set_timeline_facing(slot, h);
                        }
                        tl.pc = pc + 4;
                        continue;
                    }
                    // Seed from the target's live heading (retail reads the
                    // live `+0x26`); a never-posed NPC stands at the retail
                    // spawn default 0 = engine 0x800.
                    let cur = host.world.timeline_facing(slot).unwrap_or(0x800);
                    let leg = crate::cutscene_timeline::TimelineFacing {
                        slot,
                        state: vm::motion_vm::MotionState {
                            yaw: (cur as u16) & 0x0FFF,
                            // The timeline ticks once per retail display
                            // frame, so speed 1 maps the operand budget 1:1
                            // to parked ticks (retail consumes the same
                            // budget at `_DAT_1F800393` per actor tick).
                            speed: 1,
                            ..Default::default()
                        },
                        program: [0x38, op0, op1],
                        resume_pc: pc + 4,
                        frames: 0,
                    };
                    // Only a player target parks the caller; an NPC's turn
                    // plays out while the record runs on (`li s7,3` in the
                    // delay slot at `0x801DEEFC`).
                    if slot.is_some() {
                        tl.npc_facings.retain(|f| f.slot != slot);
                        tl.npc_facings.push(leg);
                        tl.pc = pc + 4;
                        continue;
                    }
                    tl.facing_wait = Some(leg);
                    break;
                }
                // Halt-acquire of an NPC (`CC <id> 85|8E|8F <lo> <hi> <bind>`
                // against a placement channel): the target turns to face the
                // bind over the op's budget while the record runs on past the
                // op (`CutsceneTimeline::npc_faces`). Run as a plain
                // halt-acquire on the channel context, it set a flag and
                // nothing turned: Noa and Gala faced wherever their last
                // walk left them through every "turns to Vahn" beat.
                // REF: FUN_801DE840 (0x801E2148..0x801E21DC)
                if let Some((_, ci)) = target
                    && !channels[ci].object_bind
                    && let Some((_, ramp)) =
                        crate::inline_dialogue::TalkFaceRamp::from_npc_acquire(&tl.bytecode, pc)
                {
                    if pc < tl.visited.len() {
                        tl.visited[pc] = true;
                    }
                    let slot = channels[ci].placement_index as u8;
                    host.world.npcs.positions.entry(slot).or_insert((
                        channels[ci].ctx.world_x as i16,
                        channels[ci].ctx.world_z as i16,
                    ));
                    // The kernel runs in the same actor tick that armed it, so
                    // the leg takes its first frame now.
                    let mut face = crate::cutscene_timeline::TimelineNpcFace {
                        slot,
                        ramp,
                        frames: 1,
                    };
                    tl.npc_faces.retain(|f| f.slot != slot);
                    tl.npc_facings.retain(|f| f.slot != Some(slot));
                    if !host.world.step_npc_face_leg(slot, &mut face.ramp) {
                        tl.npc_faces.push(face);
                    }
                    npc_glide_hold.push(slot);
                    tl.pc = pc + 6;
                    continue;
                }
                if vm::field::peek_extended(&tl.bytecode, pc) == Some(0xF8) {
                    let op = opcode_byte & 0x7F;
                    // Halt-acquire of the player (`CC F8 85|8E|8F`): the
                    // player turns to face the op's actor bind and the record
                    // parks until the turn's terminal frame
                    // (`CutsceneTimeline::player_face`). `jouine` `P2[5]`
                    // turns Vahn toward Cort this way before the evolved-Cort
                    // fight; stepped as a plain halt on the record's own
                    // context, the player kept facing the camera.
                    // REF: FUN_801DE840 (0x801E2148..0x801E21DC)
                    if let Some(mut ramp) =
                        crate::inline_dialogue::TalkFaceRamp::from_acquire(&tl.bytecode, pc)
                    {
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        let resume_pc = pc + 6;
                        if host.world.step_player_face_leg(&mut ramp) {
                            tl.pc = resume_pc;
                            continue;
                        }
                        tl.player_face = Some((ramp, resume_pc, 1));
                        break;
                    }
                    // Player seats (`A3 F8 x z` MOVE_TO, `CC F8 51 x z ..`
                    // NPC-run): `FUN_8003C83C` resolves `0xF8` to the player
                    // object, so the op runs with the PLAYER as its context
                    // and takes the player arm (`0x801DEC7C` compares the
                    // context pointer against `_DAT_8007C364`). Stepping
                    // them on the record's own context instead dropped the
                    // seat: `urudre1` `P2[1]` walks the player to (97,8) for
                    // the shot and closes with `A3 F8 60 0D`, and without it
                    // free roam resumed on a tile no direction leaves.
                    // REF: FUN_8003C83C, FUN_801DE840 (0x23 / 4C 51 arms)
                    if op == 0x23
                        || (op == 0x4C
                            && matches!(tl.bytecode.get(pc + 2), Some(&0x51) | Some(&0xE3)))
                    {
                        let mut player_ctx = legaia_engine_vm::field::FieldCtx {
                            script_id: 0xF8,
                            flags: 0x0100_0000,
                            ..Default::default()
                        };
                        let r = vm::field::step(&mut host, &mut player_ctx, &tl.bytecode, pc);
                        if let FieldStepResult::Advance { next_pc } = r {
                            if let Some(slot) = host.world.player_actor_slot
                                && let Some(actor) = host.world.actors.get(slot as usize)
                            {
                                let (x, z) = (actor.move_state.world_x, actor.move_state.world_z);
                                let y = host
                                    .world
                                    .sample_field_floor_height(i32::from(x), i32::from(z))
                                    as i16;
                                if let Some(a) = host.world.actors.get_mut(slot as usize) {
                                    a.move_state.world_y = y;
                                }
                            }
                            if pc < tl.visited.len() {
                                tl.visited[pc] = true;
                            }
                            tl.pc = next_pc;
                            continue;
                        }
                    }
                    if op == 0x22
                        && let Some(&move_id) = tl.bytecode.get(pc + 2)
                    {
                        host.world
                            .pending_field_events
                            .push(FieldEvent::ExecMove { move_id });
                        // Retail's player arm of op 0x22: the move id becomes
                        // the clip base and is picked + bound at once.
                        let pick = host.world.field_player_script_clip(move_id);
                        // The end latch a following `AD F8 08` waits on lands
                        // when a scene-bank clip has played its frames; a
                        // party-bank clip (the locomotion loops) is not timed.
                        tl.player_clip_ticks = match pick.bound() {
                            Some((vm::field_player_clip::ClipBank::Scene, record)) => host
                                .world
                                .locomotion
                                .scene_clip_ticks
                                .get(usize::from(record))
                                .copied()
                                .unwrap_or(0),
                            _ => 0,
                        };
                        // Cue the scripted player clip: the windowed host
                        // resolves scene-ANM record `move_id - 1` and plays
                        // it once over idle/walk (live-pinned: the town01
                        // post-naming `A2 F8 30`/`31` land the retail anim
                        // pointer on scene records 47/48 for one playthrough
                        // each). Only a pick that binds the scene bank plays
                        // a scene record: with the party-bank bit up the id
                        // strides into the leader's own bank, which the
                        // settle's slot pick already plays.
                        if let Some(id) = host.world.player_move_cue(&pick) {
                            host.world.locomotion.player_move_cues.push(id);
                        }
                        tl.player_move_frames = CHANNEL_WAIT_PARK_TIMEOUT;
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        tl.pc = pc + 3;
                        continue;
                    }
                    // Compass walk on the player (`B7 F8 b0 b1` / `C1 F8
                    // b0 b1`): arm the leg and run on; the walk kernel plays
                    // it while the record's next op on the player waits.
                    //
                    // Modal timelines and concurrent helpers alike: both run
                    // under the player's engaged bit, so the pad is refused
                    // while the script walks the player
                    // (`World::script_context_engages_player`; `korout`'s
                    // first-visit walk is the helper case).
                    // REF: FUN_8003774C (the 0x37 / 0x41 arm)
                    if matches!(op, 0x37 | 0x41)
                        && let (Some(&body0), Some(&body1)) =
                            (tl.bytecode.get(pc + 2), tl.bytecode.get(pc + 3))
                    {
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        let mut glide = crate::cutscene_timeline::TimelinePlayerGlide {
                            state: vm::motion_vm::MotionState {
                                speed: 1,
                                ..Default::default()
                            },
                            body0,
                            body1,
                            rate: if op == 0x37 { 0x80 } else { 0x40 },
                            resume_pc: pc + 4,
                            frames: 0,
                        };
                        if host.world.step_player_glide(&mut glide) {
                            tl.player_glide = Some(glide);
                        }
                        glide_hold = true;
                        tl.pc = pc + 4;
                        continue;
                    }
                    // End-latch spin on the player (`AD F8 08`): park while
                    // the poked scene-bank clip is still playing.
                    if op == 0x2D && tl.bytecode.get(pc + 2) == Some(&8) && tl.player_clip_ticks > 0
                    {
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        tl.player_clip_wait = Some(3);
                        break;
                    }
                    if op == 0x43
                        && let Some(&sub) = tl.bytecode.get(pc + 2)
                        && matches!(sub, 0 | 1 | 0xA | 0xB)
                    {
                        // Encoded width: extended header (2) + sub-0/1
                        // operand (7) or sub-A/B operand (9) - the VM's own
                        // stride (`overlay_0897` `0x801DF5B8` `addiu s8,s8,8`
                        // plus the `sub >= 0xA` `+2` at `0x801DF534`, over an
                        // `s8` the prologue already advanced past the extended
                        // channel byte).
                        let width = if sub == 0xA || sub == 0xB { 11 } else { 9 };
                        if pc < tl.visited.len() {
                            tl.visited[pc] = true;
                        }
                        // The halt is the arc's: retail arcs the player
                        // (`FUN_801D25EC`, `0x801DF5AC`) and its watcher
                        // releases the halted caller on landing
                        // (`FUN_801D5D60`), so the park lasts exactly the
                        // clip. The move countdown is the fallback only when
                        // no arc could start.
                        // REF: FUN_801d25ec
                        if let Some(req) = tl
                            .bytecode
                            .get(pc + 2..)
                            .and_then(vm::field_ledge_hop_arc::ScriptArcRequest::decode)
                            && host.world.start_field_script_arc(
                                crate::world::ScriptActorRef::Player,
                                &req,
                                None,
                            )
                        {
                            tl.player_move_frames = 0;
                            tl.player_wait = Some(width);
                            break;
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
                    // Object-bind channels are poke targets, but their
                    // `placement_index` is a flat record index - never
                    // attribute placement-keyed side effects (anim cues,
                    // seat write-throughs) to them.
                    host.world.field_vm.executing_channel =
                        (!channels[ci].object_bind).then_some(channels[ci].placement_index as u8);
                    host.world.field_vm.executing_object = channels[ci]
                        .object_bind
                        .then_some(channels[ci].ctx.script_id);
                    // The timeline is the acquirer: it halt-acquired these
                    // channels earlier (the `4C 85` freeze sweep) and now
                    // drives them beat by beat. A poke from the owner is the
                    // resume signal, so clear the target's halt bit before the
                    // op runs - otherwise the dispatcher prelude parks the
                    // caller on its own frozen actor and the camera beats
                    // after the sweep never play.
                    channels[ci].ctx.flags &= !0x400;
                    let before = (channels[ci].ctx.world_x, channels[ci].ctx.world_z);
                    let r = vm::field::step_with_caller(
                        &mut host,
                        &mut channels[ci].ctx,
                        &mut tl.ctx,
                        false,
                        &tl.bytecode,
                        pc,
                    );
                    host.world.field_vm.executing_channel = None;
                    host.world.field_vm.executing_object = None;
                    // A poke that moved the actor (`A3 <id>` seat, `CC <id> 37`
                    // copy-from-player, ...) lands on retail's `+0x14`/`+0x18`
                    // at once; surface it now rather than at the slice's end,
                    // so a walk later in the same slice starts from it.
                    let c = &channels[ci];
                    let after = (c.ctx.world_x, c.ctx.world_z);
                    if !c.object_bind
                        && after != before
                        && let Ok(slot) = u8::try_from(c.placement_index)
                    {
                        host.world
                            .npcs
                            .positions
                            .insert(slot, (after.0 as i16, after.1 as i16));
                        host.world.npcs.motions.remove(&slot);
                    }
                    r
                } else {
                    field_step_routed(&mut host, &mut tl.ctx, &tl.bytecode, pc)
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
                // Cross-context channel wait (`B3 <id> <bit>`, CFLAG_TST against
                // a spawned channel): PARK the timeline while the awaited
                // channel's bit is set (`0x801DEE44`), rather than
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
                // `4C CD` halts only while the camera mover's glide is in
                // flight (the VM advances it otherwise), so its park is a
                // timed wait to hold, not a handshake to step past.
                let glide_wait = op == 0x4C && tl.bytecode.get(pc + 1) == Some(&0xCD);
                // NPC end-latch spin (`AD <id> 08`): the record re-tests the
                // poked actor's `+0x62 & 0x100` every frame until its clip
                // tick latches it - the clip's remaining length for a
                // clamped clip, the time to the next wrap for a looping one.
                // Held only where the world owns that actor's clip cursor
                // (the `0x22` poke binds one); a held clip, which never
                // latches, falls back to the step-past after
                // [`NPC_CLIP_SPIN_TIMEOUT`] frames.
                // REF: FUN_800204F8 (0x800206E4..0x8002072C), FUN_801DE840 (op 0x2D)
                let npc_clip_spin = op == 0x2D
                    && opcode_byte & 0x80 != 0
                    && tl.bytecode.get(pc + 2) == Some(&8)
                    && matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                    && next_pc == pc
                    && target.is_some_and(|(_, ci)| {
                        !channels[ci].object_bind
                            && host
                                .world
                                .npc_clip_cursor_bound(channels[ci].placement_index as u8)
                    });
                if npc_clip_spin && tl.npc_clip_spin_frames < NPC_CLIP_SPIN_TIMEOUT {
                    tl.npc_clip_spin_frames += 1;
                    tl.frames = tl.frames.saturating_sub(1);
                    break;
                }
                tl.npc_clip_spin_frames = 0;
                let is_flag_test_handshake = matches!(op, 0x2D | 0x30 | 0x33)
                    || (op == 0x4C && target.is_none() && !glide_wait);
                if matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                    && next_pc == pc
                    && op != 0x4A
                    && op != 0x49
                    && !glide_wait
                    && (target.is_none() || is_flag_test_handshake)
                {
                    // By the op's own width: the flag tests are two bytes
                    // (three extended), but a `0x4C` park is not always -
                    // `4C D2 <ch>` is three, and stepping it by two read its
                    // channel byte as the next opcode (`rayman` `P2[19]`'s
                    // `4C D2 53 .. 4C D2 59` run then lost the `44 7A` that
                    // re-seats the village after the quake).
                    let header_size = if opcode_byte & 0x80 != 0 { 2 } else { 1 };
                    next_pc = legaia_asset::field_disasm::decode(&tl.bytecode, pc)
                        .map_or(pc + header_size + 1, |insn| pc + insn.size);
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
                // One backward jump is NOT a wrap: a loop that polls the held
                // pad (`42 01 <button>`) is the record waiting for the player.
                // `edlast`'s ending record closes that way - `4A 08 00` then
                // `42 01 08` / `42 01 09` (Circle / Cross held) and a `26`
                // back to the wait - and retail sits in it until the press;
                // reading it as a wrap dropped the record and handed the
                // player the pad in the ending's last scene.
                if matches!(kind, crate::cutscene_timeline::TraceResult::Advance)
                    && next_pc <= pc
                    && tl.visited.get(next_pc).copied().unwrap_or(false)
                    && !loop_polls_held_pad(&tl.bytecode, next_pc, pc)
                {
                    // There used to be a carve-out here for a `45 C0 <s16>`
                    // "camera-apply loop-back", on the reading that retail's
                    // sub-`0xC0` arm jumps to the operand `s16`. It does not:
                    // the arm is a four-byte fall-through and the `s16` is the
                    // apply trigger (`docs/subsystems/script-vm.md`,
                    // "0x45 CAMERA arm widths"). With the VM's arm corrected
                    // the op can no longer produce a backward `Advance` at all,
                    // so the carve-out was rescuing a loop the port invented.
                    // REF: FUN_801dab90
                    tl.done = true;
                    stop = true;
                }
                // A touch-resumed placement context ends its interaction at
                // the first raw `0x21` it executes (`FUN_80039B7C`: the loop
                // exits on `0x21` at `0x80039E20` and `0x80039E68..0x80039E7C`
                // clears the engaged bit). The Rim Elm bee beat (`town0c` /
                // `town0b` `P1[21]`) is `50 00` (the scripted-loss latch),
                // `3E FF 03`, `21`, then a jump back to its flag dispatch: the
                // `21` is what stops the fight re-firing until the next touch.
                if tl.interaction_slot.is_some()
                    && opcode_byte == 0x21
                    && matches!(kind, crate::cutscene_timeline::TraceResult::Advance)
                {
                    tl.done = true;
                    stop = true;
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
                    // An authored `0x4A WAIT_FRAMES` hold is real playout, not
                    // a hang - the same carve-out the walk / rotate / narration
                    // parks above take. The op is bounded by construction
                    // (`ctx.wait_accum` grows by `frame_delta` every tick until
                    // it reaches the operand, then the op advances), so it
                    // cannot spin, and a record that spends its time in one is
                    // playing, not stuck. Counting those ticks made the
                    // anti-hang cap a *record-length* cap instead: `urudre3`
                    // P2[0] and `jouine` P2[16] need ~5400 and ~4900 stepping
                    // frames to reach their exits and were cut at 1200, so the
                    // forced completion dropped both records before their tail
                    // and those rooms read as one-way.
                    // A `4C CD` camera-glide wait is the same kind of hold:
                    // bounded by the glide's own frame count, which the
                    // mover spends one display frame at a time.
                    if ((opcode_byte & 0x7F) == 0x4A || glide_wait)
                        && matches!(kind, crate::cutscene_timeline::TraceResult::Halt)
                        && next_pc == pc
                    {
                        tl.frames = tl.frames.saturating_sub(1);
                    }
                    // Same carve-out for an op-`0x43` halt-acquire park
                    // (sub-0/1/A/B). Retail's arm raises the context's halt bit
                    // and hands the actor its walk target
                    // (`FUN_801D25EC`); the context then sits out however many
                    // frames the actor's leg takes. That is authored playout,
                    // and it cannot spin here - the port's arm advances the PC
                    // past the op, so the parks a record can spend are bounded
                    // by its own instruction count. Counting them turned the
                    // anti-hang cap back into a length cap on exactly the
                    // record the cap's own note names: `jouine` `P2[16]`
                    // stopped three bytes short of its `4C E2 08` FMV tail.
                    if (opcode_byte & 0x7F) == 0x43
                        && matches!(kind, crate::cutscene_timeline::TraceResult::Yield)
                        && tl
                            .bytecode
                            .get(pc + if opcode_byte & 0x80 != 0 { 2 } else { 1 })
                            .is_some_and(|s| matches!(s, 0 | 1 | 0xA | 0xB))
                    {
                        tl.frames = tl.frames.saturating_sub(1);
                    }
                    break;
                }
            }
        }
        // Timeline pokes that moved a channel context (cross-context MoveTo)
        // write through to the field NPC render/probe state. Object-bind
        // channels never write through: their `placement_index` is a FLAT
        // record index, not a placement slot, and the NPC surfaces are
        // placement-keyed.
        for (c, pre) in channels.iter().zip(channel_pre_pos) {
            if !c.object_bind && (c.ctx.world_x, c.ctx.world_z) != pre {
                self.npcs.positions.insert(
                    c.placement_index as u8,
                    (c.ctx.world_x as i16, c.ctx.world_z as i16),
                );
            }
        }
        self.field_vm.channels = channels;
        self.field_vm.stepping_view.clear();
        self.cutscene.in_timeline = false;
        self.field_vm.in_spawned_record_slice = false;
        true
    }

    /// Write a timeline rotate's heading onto its target: a placement's
    /// render heading, or the player's `render_26` for `None`.
    fn set_timeline_facing(&mut self, slot: Option<u8>, heading: i16) {
        match slot {
            Some(slot) => {
                self.npcs.headings.insert(slot, heading);
            }
            None => {
                if let Some(a) = self
                    .player_actor_slot
                    .and_then(|s| self.actors.get_mut(usize::from(s)))
                {
                    a.move_state.render_26 = heading;
                }
            }
        }
    }

    /// The live heading a timeline rotate starts from - see
    /// [`Self::set_timeline_facing`].
    fn timeline_facing(&self, slot: Option<u8>) -> Option<i16> {
        match slot {
            Some(slot) => self.npcs.headings.get(&slot).copied(),
            None => self
                .player_actor_slot
                .and_then(|s| self.actors.get(usize::from(s)))
                .map(|a| a.move_state.render_26),
        }
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
        let cap = if tl.arms_prologue_handoff || self.cutscene.opening_chain_active {
            PROLOGUE_TIMELINE_MAX_FRAMES
        } else {
            CUTSCENE_TIMELINE_MAX_FRAMES
        };
        if tl.frames >= cap {
            tl.done = true;
        }
        // A record that ends while an NPC turn it armed is still ramping:
        // retail's turn belongs to the NPC's own walk kernel and finishes
        // without the record, so land it on its compass entry here rather
        // than leave the actor frozen mid-ramp.
        if tl.done {
            for mut face in std::mem::take(&mut tl.npc_faces) {
                for _ in 0..WALK_PARK_TIMEOUT {
                    if self.step_npc_face_leg(face.slot, &mut face.ramp) {
                        break;
                    }
                }
            }
            for fw in std::mem::take(&mut tl.npc_facings) {
                if let Some(h) =
                    crate::man_field_scripts::facing_index_to_engine_heading(fw.program[1] & 0xF)
                {
                    self.set_timeline_facing(fw.slot, h);
                }
            }
        }
        if tl.arms_prologue_handoff {
            // Safety net: if the record terminated without executing its
            // `GFLAG_SET 26`, arm the hand-off statically so the prologue
            // can't stall.
            if tl.done && self.flags.story_flags & PROLOGUE_HANDOFF_FLAG == 0 {
                self.arm_prologue_handoff();
            }
            self.cutscene.timeline = Some(tl);
        } else if tl.done {
            // Timeline finished (or capped): drop it so the view reverts
            // from the cutscene camera to normal field gameplay.
            let restore = tl.restore_hidden_on_complete;
            self.release_interaction_context(&tl);
            self.restore_owed_player_scale(&tl.bytecode, tl.pc, &tl.visited);
            self.cutscene.timeline = None;
            // The town01 OPENING choreography `MoveTo`s the townsfolk to the
            // off-map hide box to clear the establishing shot. Nothing
            // reloads the scene between the cutscene and free-roam, so that
            // install marks the timeline restore-on-complete or the field
            // render draws each hidden NPC off-screen (the "town NPCs vanish
            // after New Game" symptom). A mid-scene walk-on beat does NOT
            // restore: its hide-box seats are story choreography (the Mei
            // beat walks her out and despawns her - retail keeps her hidden
            // until the next scene entry re-runs her spawn prologue).
            if restore {
                self.restore_hidden_field_npcs();
            }
        } else {
            self.cutscene.timeline = Some(tl);
        }
    }

    /// Hand a finished touch-resumed timeline's PC back to its placement
    /// channel ([`crate::cutscene_timeline::CutsceneTimeline::interaction_slot`]):
    /// the two are one retail context, so the next touch resumes where this
    /// interaction's `0x21` left it. No-op for a spawned-record timeline.
    // REF: FUN_80039B7C
    fn release_interaction_context(&mut self, tl: &crate::cutscene_timeline::CutsceneTimeline) {
        let Some(slot) = tl.interaction_slot else {
            return;
        };
        if let Some(c) = self
            .field_vm
            .channels
            .iter_mut()
            .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
        {
            c.pc = tl.pc;
            // The interaction's `0x21` disengages the context
            // (`0x80039E68..0x80039EE4` clears `+0x10 & 0x100`).
            c.ctx.flags &= !0x100;
        }
    }

    /// Step every concurrent helper context ([`crate::world::FieldVmState::helper_contexts`]) one
    /// frame slice - the per-frame sweep over the mid-play spawned records,
    /// running each through the shared [`Self::run_spawned_record_slice`]
    /// core (`modal = false`). Helper contexts execute alongside the modal
    /// cutscene timeline and the per-actor channels without seizing the
    /// camera, but hold the pad while they run
    /// ([`Self::script_context_engages_player`]); a context that completes (wrapped its
    /// choreography, ran off its bytecode, or hit the plain
    /// [`CUTSCENE_TIMELINE_MAX_FRAMES`] cap) is dropped from the table.
    // REF: FUN_8003BDE0
    pub fn step_helper_contexts(&mut self) {
        if self.field_vm.helper_contexts.is_empty() {
            return;
        }
        let mut contexts = std::mem::take(&mut self.field_vm.helper_contexts);
        // One dialog box, and it has one owner: the modal timeline's box when
        // it shows one, else the first helper holding a box. A helper that
        // reached its text while the box is taken waits for it without
        // typing, paging or taking the pager's automatic press - retail's
        // runner leaves such a context at `+0x9C = 1` and claims the box only
        // once its state word `0x801F2734` reads free (`1` / `4` / `7`,
        // `0x80039F9C..0x80039FD8`). Contexts that are not on text keep
        // their slice every frame: the runner gates on the context's own
        // `+0x9C`, never on the box.
        // REF: FUN_80039B7C
        let timeline_box = self
            .cutscene
            .timeline
            .as_ref()
            .is_some_and(|t| t.dialog.is_some());
        let mut owner = if timeline_box {
            None
        } else {
            contexts
                .iter()
                .filter(|tl| !tl.done && tl.dialog.is_some())
                .map(|tl| tl.dialog_claim)
                .min()
        };
        let timeline_halted: Vec<Option<u8>> = self
            .cutscene
            .timeline
            .iter()
            .flat_map(|t| t.halted_targets())
            .collect();
        for i in 0..contexts.len() {
            if contexts[i].done {
                continue;
            }
            // Recomputed per context: an earlier sibling may have just armed
            // or landed its leg this frame.
            let mut halted = timeline_halted.clone();
            halted.extend(
                contexts
                    .iter()
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .flat_map(|(_, h)| h.halted_targets()),
            );
            self.field_vm.halted_elsewhere = halted;
            let tl = &mut contexts[i];
            // Parked on its text segment: the box, not the slice, advances
            // it; the park holds the frame cap like the modal timeline's.
            if tl.dialog.is_some() {
                if owner != Some(tl.dialog_claim) {
                    continue;
                }
                owner = None;
                self.drive_script_dialog(tl, true);
                if tl.dialog.is_some() || tl.done {
                    continue;
                }
            }
            if self.run_spawned_record_slice(tl, false) && tl.frames >= CUTSCENE_TIMELINE_MAX_FRAMES
            {
                tl.done = true;
            }
        }
        self.field_vm.halted_elsewhere.clear();
        let dropped = contexts.iter().any(|tl| tl.done);
        for tl in contexts.iter().filter(|tl| tl.done) {
            self.restore_owed_player_scale(&tl.bytecode, tl.pc, &tl.visited);
        }
        contexts.retain(|tl| !tl.done);
        self.field_vm.helper_contexts = contexts;
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
            && self.field_vm.helper_contexts.is_empty()
            && matches!(self.mode, crate::world::SceneMode::Field)
            // A record that ended by leaving the scene (`chitei2`'s rescue
            // beat jumps the party into the drain pipe, then `0x3F`) parks
            // the hand-off behind the streaming actor; the departing scene's
            // last frames must not show the party yanked back to its spawn.
            && self.scene_transition_hold.is_none()
            && self.pending_named_scene_transition.is_none()
            && let Some((sx, sz)) = self.props.resolved_cold_spawn
            && let Some(slot) = self.player_actor_slot
            && let Some(actor) = self.actors.get(slot as usize)
            && self.field_walk_component_size(actor.move_state.world_x, actor.move_state.world_z)
                == 0
            // ... and only when the collision grid really boxes them in. Floor
            // a placed object provides has no walk-visible bit, but it is open
            // collision the player walks off: `chitei2`'s collapse beat leaves
            // the party on the escape platform (partition-0 record 31's mesh)
            // to run down its stairs onto the corridor floor, and yanking them to the cold spawn skipped
            // the boulder beat that platform leads to. Nor is open ground
            // with no floor bit at all: `taiku`'s post-boss cutscene leaves the
            // party on the collapse escape route, and the rescue carried them
            // back to the scene's spawn, off the route the escape timer runs.
            && self.field_collision_boxed_in(
                actor.move_state.world_x,
                actor.move_state.world_z,
                STRANDED_COLLISION_REACH,
            )
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
    /// [`crate::world::FieldNpcState::positions`] / [`crate::world::FieldNpcState::headings`] overrides so
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
            .npcs
            .positions
            .iter()
            .filter(|&(_, &(x, z))| x == hide && z == hide)
            .map(|(&slot, _)| slot)
            .collect();
        for slot in restored {
            // A slot the spawn-prologue pre-run ALREADY parked at scene entry
            // ([`Self::pre_run_field_channel_prologues`]) is story-hidden, not
            // cutscene-hidden: restore it to its entry state (which may itself
            // be the hide box) rather than resurrecting it at the raw MAN
            // spawn tile - the exact ghost the prologue park exists to
            // prevent.
            match self.npcs.entry_positions.get(&slot) {
                Some(&entry_pos) => {
                    self.npcs.positions.insert(slot, entry_pos);
                }
                None => {
                    self.npcs.positions.remove(&slot);
                    self.npcs.headings.remove(&slot);
                }
            }
        }
    }
}

/// Apply `FUN_80038050`'s verdict on the byte after a dismissed box to the
/// runner: a parking byte ends the talk with the cursor left on it (recorded
/// in [`crate::inline_dialogue::InlineDialogue::parked_pc`]); `0x21` and the
/// continuing bytes are left to the VM loop, which already ends on a raw
/// `0x21` and runs the rest. A prop-bound run (door / cupboard record) keeps
/// running through its tail: the parking rule is pinned on NPC talks only.
///
/// REF: FUN_80039B7C (`0x80039C84..0x80039D60`, the talk end after a box),
/// FUN_80038050
fn end_talk_at_post_box_byte(id: &mut crate::inline_dialogue::InlineDialogue) {
    if id.prop_anchor.is_some() {
        return;
    }
    if let crate::inline_dialogue::TalkDispatch::EndParked(pc) =
        crate::inline_dialogue::talk_dispatch(&id.bytecode, id.pc)
    {
        id.parked_pc = Some(pc);
        id.done = true;
    }
}

/// Whether the loop body `from..=to` (a backward jump's target up to the jump
/// itself) carries an op-`0x42` mode-1 test - the held-pad poll. Walked on
/// decoded op boundaries, so a `0x42` byte inside another op's operands does
/// not count.
///
/// Such a loop is a record waiting on the player (retail's `0x42` mode-1 arm
/// compares the packed held pad `_DAT_8007B850`; see `script-vm.md`), not a
/// wrapped choreography, so the timeline's natural-termination rule must not
/// end it.
fn loop_polls_held_pad(bytecode: &[u8], from: usize, to: usize) -> bool {
    let mut pc = from;
    while pc <= to {
        let Ok(insn) = legaia_asset::field_disasm::decode(bytecode, pc) else {
            return false;
        };
        if insn.size == 0 {
            return false;
        }
        if matches!(
            insn.info,
            legaia_asset::field_disasm::InsnInfo::CondJmp { mode: 1, .. }
        ) {
            return true;
        }
        pc += insn.size;
    }
    false
}

#[cfg(test)]
mod tests;
