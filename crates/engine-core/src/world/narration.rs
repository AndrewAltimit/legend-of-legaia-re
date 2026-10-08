//! Name entry, cutscene narration, prologue handoff, cutscene timelines, field channels, and inline-dialogue driving.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

mod dialogue_drive;
mod field_channels;
mod record_install;
mod spawned_slice;

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
    pub(super) fn step_player_glide(
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
