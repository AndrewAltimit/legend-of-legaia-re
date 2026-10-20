//! Walk-touch contact: the per-step touch check, arrival brackets, sealed-
//! arrival walk-off and the post-ambient touch.

use super::*;

impl World {
    /// The locomotion's per-step **walk-touch dispatch**: when the player's
    /// body stands inside a walk-touch placement's static contact box
    /// (±[`FIELD_PROP_BOX_HALF`]), post that placement's event - no button
    /// press, the same dispatch path the button-gated interact uses
    /// ([`Self::trigger_field_interact`]) plus the decoded script effect:
    ///
    /// - [`WalkTouchEvent::Warp`] → arm the **mode-24 minigame door-warp**
    ///   (the effect of the record's `0x3E` op, staged exactly as the
    ///   field-VM arm stages it - see [`legaia_engine_mode::minigame_entry`]);
    /// - [`WalkTouchEvent::PlayerMoveTo`] → snap the player to the decoded
    ///   world coords (the record's cross-context `0x23` into the player
    ///   channel) and surface a [`FieldEvent::MoveTo`].
    ///
    /// Retail posts the touch event (`FUN_801d5b5c`) on every contact step,
    /// gated by the player's `+0x10 & 0x80000` engaged flag until the dialog
    /// SM teardown clears it; the engine latches one post per contact
    /// ([`crate::world::FieldPropState::active_walk_touch`]) instead. The full post kernel (engaged
    /// flag, facing save/restore, touch counters) is not modelled.
    ///
    /// REF: FUN_801d5b5c, FUN_801cfc40
    pub(in crate::world::field_movement) fn check_field_walk_touch(&mut self) {
        if self.props.walk_touch.is_empty() {
            self.props.active_walk_touch = None;
            return;
        }
        let Some(slot) = self.player_actor_slot else {
            return;
        };
        let slot = slot as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return;
        }
        if !self.props.arrival_exempt.is_empty() && self.dialog.inline.is_none() {
            self.walk_off_sealed_arrival(slot);
        }
        let (px, pz) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        // Contact fires from the SAME probe points that block movement
        // (retail: `FUN_801cfe4c`'s three `FUN_801cfc40` calls both refuse
        // the step and link/post the touched actor) - so a **solid** door
        // object still fires its walk-touch while the player stands pressed
        // against its box, 64+ units short of the centre. The stand-inside
        // test is kept as well (a landing seated inside a box, nav drivers).
        let mut points: Vec<(i32, i32)> = vec![(px as i32, pz as i32)];
        for dir in Self::dirs_of_bits(self.locomotion.last_move_dir_bits) {
            for &(dx, dz) in &FIELD_ACTOR_PROBES[dir] {
                points.push((px.saturating_add(dx) as i32, pz.saturating_sub(dz) as i32));
            }
        }
        // The arrival bracket lifts once the player is off the platform: no
        // probe point in any direction reaches the partner's contact box.
        // Not while the ride's own record is still running it: a lift parks
        // the player on tile (0, 0) and tours the floors before the arrival
        // `MoveTo`, and retail's `B2` comes only after the walk-off.
        if !self.props.arrival_exempt.is_empty() && self.dialog.inline.is_none() {
            let reach: Vec<(i32, i32)> =
                std::iter::once((px as i32, pz as i32))
                    .chain(FIELD_ACTOR_PROBES.iter().flatten().map(|&(dx, dz)| {
                        (px.saturating_add(dx) as i32, pz.saturating_sub(dz) as i32)
                    }))
                    .collect();
            self.props.arrival_exempt.retain(|&(_, (wx, wz))| {
                reach.iter().any(|&(qx, qz)| {
                    (qx - wx as i32).abs() < FIELD_PROP_BOX_HALF
                        && (qz - wz as i32).abs() < FIELD_PROP_BOX_HALF
                })
            });
        }
        let exempt = &self.props.arrival_exempt;
        // A door whose own record has set its `+0x10 & 3` (`31 00` as it
        // swings open, a ride's `B1 <partner> 00`) is out of the touch probes
        // until the record clears it again, as `FUN_801CF754` /
        // `FUN_801CF9F4` skip `flags & 3` actors.
        let script_exempt = |s: &u8| {
            self.props
                .walk_touch_records
                .get(s)
                .and_then(|&r| self.walk_touch_door_prop(r))
                .is_some_and(|a| self.props.bank.props[&a].collision_exempt())
        };
        let hit = self
            .props
            .walk_touch
            .iter()
            .filter(|(s, _)| !exempt.iter().any(|(e, _)| e == *s))
            .filter(|(s, _)| !script_exempt(s))
            .find(|(_, ((wx, wz), _))| {
                points.iter().any(|&(qx, qz)| {
                    (qx - *wx as i32).abs() < FIELD_PROP_BOX_HALF
                        && (qz - *wz as i32).abs() < FIELD_PROP_BOX_HALF
                })
            })
            .map(|(&s, &(_, event))| (s, event));
        let Some((touch_slot, event)) = hit else {
            self.props.active_walk_touch = None;
            return;
        };
        if self.props.active_walk_touch == Some(touch_slot) {
            return; // still inside the same contact - already posted
        }
        self.props.active_walk_touch = Some(touch_slot);
        // A door record is a field-VM script, not a constant: its opening
        // `SysFlag.Test` chain picks which arm runs (teleport into the
        // interior vs. spawn the story beat). Retail resumes the record on
        // contact, so the arm is chosen against the *live* flags - re-resolve
        // here rather than reusing the load-time structural decode. Falls back
        // to that decode only when the record can't be re-walked: a record
        // walked to its idle loop with no teleport on the taken arm (the
        // `tower` rapid elevator while its switch `0x1C6` is off) moves
        // nobody - its static first `MoveTo` belongs to the other arm.
        // REF: FUN_801d5b5c (contact resumes the object's script)
        let resolved = self
            .props
            .walk_touch_records
            .get(&touch_slot)
            .copied()
            .and_then(|record| {
                let man = self.field_vm.channels_man.clone()?;
                let man_file = legaia_scene_formats::man_section::parse(&man).ok()?;
                let flags = self.flags.system_flags.clone();
                let test = |idx: u16| -> bool {
                    let byte = usize::from(idx >> 3);
                    byte < flags.len() && flags[byte] & (0x80u8 >> (idx & 7)) != 0
                };
                legaia_engine_field::man_field_scripts::resolve_walk_touch_arm(
                    &man_file, &man, record, &test,
                )
            })
            .unwrap_or(Some(event));
        // Post through the same dispatch path the button-gated interact uses.
        self.trigger_field_interact(0, touch_slot);
        let Some(event) = resolved else {
            return;
        };
        // A door bound to a placed object with a prop runs its own record on
        // contact (`FUN_801d5b5c` resumes the object's script, which the prop
        // runner executes): the door swings open, the record walks the player
        // in, closes it, and its own `A3 F8 x z` makes the jump. Applying the
        // decoded `MoveTo` here as well put the player on the far side before
        // the door had moved, so the swing played out of view and the
        // partner door's opening, run by the same record afterwards, never
        // showed (`tower`'s floor doors). Post the touch to the prop instead.
        // REF: FUN_801d5b5c
        if matches!(event, WalkTouchEvent::PlayerMoveTo { .. })
            && let Some(anchor) = self
                .props
                .walk_touch_records
                .get(&touch_slot)
                .and_then(|&r| self.walk_touch_door_prop(r))
        {
            // An interact-gated object (`+0x10 & 0x40020000`, contact result
            // bit `1`) blocks but posts nothing on contact: only the confirm
            // press runs it (`nilboa` P0[6], the Shield Control Panel lever,
            // whose `A3 F8` is the cutscene after its Yes/No).
            // REF: FUN_801CFC40, FUN_801CF9F4
            if !self.props.bank.props[&anchor].interact_gated()
                && self.props.pending_touch.is_none()
            {
                self.props.pending_touch = Some(anchor);
            }
            return;
        }
        match event {
            // A `Warp` event is, by construction, **only** ever a mode-24
            // minigame sub-id: `is_genuine_warp` gates the decoded `0x3E`'s
            // `op0` to `100..=106`, and every carrier of that id space is a
            // venue cabinet (see [`legaia_engine_mode::minigame_entry`]).
            //
            // Body contact must NOT execute it. Retail's contact kernel
            // `FUN_801d5b5c` **resumes the placement's script**; it does not
            // shortcut to that script's terminal warp. A koin1 cabinet's
            // script is a coin compare into a confirm dialogue, and only the
            // taken arm reaches the `0x3E` - which is why brushing a slot
            // machine in retail costs nothing and opens nothing.
            //
            // Executing the *decoded* effect here instead skipped the compare
            // and the confirm, and made the whole casino floor a trap: the
            // contact box is +/-`FIELD_PROP_BOX_HALF` around the placement
            // and koin1's casino NPCs stand inside their neighbouring
            // cabinets' boxes, so walking up to an NPC to talk entered a
            // minigame. No shipped host has a player-reachable exit from one
            // (the browser play page does not even draw them), so the entry
            // read as a freeze: field stopped, BGM kept playing.
            //
            // The entry stays on the button probe
            // ([`Self::field_interact_probe_slot`]) - retail's actual
            // trigger - which runs the record and lets its own `0x3E` arm
            // ([`crate::world::vm_hosts`]) do the warp when the script gets
            // there. Contact still posts the interact above, which is the
            // script resume `FUN_801d5b5c` does model.
            WalkTouchEvent::Warp { .. } => {}
            WalkTouchEvent::PlayerMoveTo {
                world_x,
                world_z,
                facing,
            } => {
                // The retail op-0x23 player arm rewrites X/Z (`+0x14`/`+0x18`)
                // and re-seats the actor on the floor; the paired op-0x38
                // cross-context CAM_CFG writes the arrival heading (`+0x26`).
                // A door's interior is a *sub-area of the same collision grid*
                // at its own elevation, so the floor must be resampled at the
                // landing - otherwise the player keeps the doorstep's outdoor
                // height until the next locomotion frame nudges it.
                let y = self.sample_field_floor_height(world_x as i32, world_z as i32) as i16;
                if let Some(p) = self.player_actor_slot
                    && let Some(actor) = self.actors.get_mut(p as usize)
                {
                    actor.move_state.world_x = world_x;
                    actor.move_state.world_z = world_z;
                    actor.move_state.world_y = y;
                    if let Some(heading) = facing {
                        actor.move_state.render_26 = heading;
                    }
                }
                // The player arm's camera re-centre, which the record's `0x23`
                // (and the `4C 51` form) makes whenever its target actor is
                // the player context: `FUN_80017EC8(op[0] & 0x7F, op[1] &
                // 0x7F, ..)` at `0x801DEC9C` (`0x801E1A58` for `4C 51`). It
                // re-latches the region box on the landing tile and re-plans
                // the windowed static-object list - without it a door out of
                // a house keeps the interior's box, and under retail
                // windowing every window-owned placement outside it (geremi's
                // street slabs) stays hidden. The operand tile is `(world -
                // 0x40) >> 7` of the decoded seat, as in the VM's own arm.
                // REF: FUN_80017EC8
                self.recentre_field_window(
                    (i32::from(world_x) - 0x40) >> 7,
                    (i32::from(world_z) - 0x40) >> 7,
                );
                self.pending_field_events.push(FieldEvent::MoveTo {
                    world_x: world_x as u16,
                    world_z: world_z as u16,
                    is_player: true,
                });
                self.arm_arrival_bracket(touch_slot);
            }
            // Boss-stager contact: the `trigger_field_interact` call above
            // already ran the placement's record ([`crate::world::World::
            // run_boss_stager_record`]); the event carries no extra effect.
            WalkTouchEvent::StagerBeat => {}
            // The record's taken arm is an op-`0x44` SPAWN_RECORD: queue the
            // referenced record so `SceneHost::tick` installs it as a spawned
            // field-VM context (the same drain the in-script op-`0x44` uses).
            // This is the arm a story-gated door takes once its flag is set -
            // the in-house beat, not a bare reposition.
            WalkTouchEvent::SpawnRecord { flat_index } => {
                if let Ok(idx) = u8::try_from(flat_index) {
                    self.field_vm.pending_record_spawns.push(idx);
                }
            }
        }
    }

    /// Arm the arrival bracket for a teleport the walk-touch bind `slot`'s
    /// record just made: every bind whose record the teleporting record
    /// names in a cross-context `B1 <obj> 00`
    /// ([`legaia_engine_field::man_field_scripts::record_exempted_objects`]) is skipped by
    /// the touch dispatch and the collision probe until the player is off
    /// its contact box. The engine moves the player by the record's
    /// `MoveTo` alone (the ride's `A2 F8` walk-off clip is not run), so the
    /// player steps off the platform themselves; the bracket keeps that step
    /// from re-firing the partner, as retail's exemption does across the
    /// scripted walk-off.
    ///
    /// REF: FUN_801CF754, FUN_801CF9F4, FUN_801d5b5c
    pub(in crate::world::field_movement) fn arm_arrival_bracket(&mut self, slot: u8) {
        let Some(&record) = self.props.walk_touch_records.get(&slot) else {
            return;
        };
        let Some(man) = self.field_vm.channels_man.clone() else {
            return;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
            return;
        };
        let targets = legaia_engine_field::man_field_scripts::record_exempted_objects(
            &man_file, &man, record,
        );
        // A ride that sets the player down inside another door's contact
        // box walks the player out of it before letting go (`balden`'s
        // elevator cars, P0[7] / P0[14]: `CC F8 51` runs the player to the
        // partner car, then the compass leg `C1 F8 00 43` - under the
        // `A2 F8 01` / `A2 F8 02` walk clips - carries it out through the
        // partner's door, with no `B1` bracket). A landing still on the
        // partner's centre would post its touch on the first step in any
        // direction and ride straight back; the partner is exempt until the
        // player has stepped off it, as a bracketed one is.
        let landing = self
            .player_actor_slot
            .and_then(|p| self.actors.get(p as usize))
            .map(|a| {
                (
                    i32::from(a.move_state.world_x),
                    i32::from(a.move_state.world_z),
                )
            });
        let partners: Vec<(u8, (i16, i16))> = self
            .props
            .walk_touch_records
            .iter()
            .filter(|&(&s, _)| s != slot)
            .filter_map(|(&s, &r)| self.props.walk_touch.get(&s).map(|&(pos, _)| (s, r, pos)))
            .filter(|&(_, r, (wx, wz))| {
                targets.iter().any(|&t| usize::from(t) == r)
                    || landing.is_some_and(|(lx, lz)| {
                        (lx - i32::from(wx)).abs() < FIELD_PROP_BOX_HALF
                            && (lz - i32::from(wz)).abs() < FIELD_PROP_BOX_HALF
                    })
            })
            .map(|(s, _, pos)| (s, pos))
            .collect();
        for p in partners {
            if !self.props.arrival_exempt.contains(&p) {
                self.props.arrival_exempt.push(p);
            }
        }
    }

    /// The ride's walk-off, for an arrival the walls seal. A lift record's
    /// tail turns the player (`B8 F8 80 00`) and plays the walk-off clips
    /// (`A2 F8 01`, `A2 F8 02`) across the bracketed platform before its
    /// `B2`; the engine runs neither clip as motion, so a landing set in the
    /// wall niche behind a platform - `tower`'s rapid lift down arrives at
    /// (1856, 9280), walled on every side but its own lift - would hold the
    /// player for good. When every direction from the player reads wall and
    /// the ride's own record has finished, carry the player straight through
    /// the nearest bracketed platform to the first open spot past its reach,
    /// which is where the scripted walk-off leaves it.
    ///
    /// REF: FUN_801DE840 (cross-context `0x22` ExecMove into the player)
    pub(in crate::world::field_movement) fn walk_off_sealed_arrival(&mut self, slot: usize) {
        let (px, pz) = {
            let ms = &self.actors[slot].move_state;
            (ms.world_x, ms.world_z)
        };
        let Some(&(_, platform)) = self
            .props
            .arrival_exempt
            .iter()
            .min_by_key(|&&(_, (x, z))| {
                (i32::from(x) - i32::from(px)).abs() + (i32::from(z) - i32::from(pz)).abs()
            })
        else {
            return;
        };
        let Some((x, z)) = self.sealed_arrival_walk_off((px, pz), platform) else {
            return;
        };
        let y = self.sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
        let ms = &mut self.actors[slot].move_state;
        ms.world_x = x;
        ms.world_z = z;
        ms.world_y = y;
    }

    /// Where [`Self::walk_off_sealed_arrival`] carries a player standing at
    /// `from` across the bracketed platform centred on `platform`: `None`
    /// when `from` has an open side (the player steps off by itself) or no
    /// open spot lies within 384 units past the platform's centre.
    pub fn sealed_arrival_walk_off(
        &self,
        from: (i16, i16),
        platform: (i16, i16),
    ) -> Option<(i16, i16)> {
        let (px, pz) = from;
        if !(0..4).all(|d| self.field_dir_blocked(px, pz, d)) {
            return None;
        }
        let (dx, dz) = (
            i32::from(platform.0) - i32::from(px),
            i32::from(platform.1) - i32::from(pz),
        );
        let len = dx.abs().max(dz.abs());
        if len == 0 {
            return None;
        }
        // Past the platform's centre, in 16-unit steps, to the first spot
        // the walls leave open on some side.
        (1..=24).find_map(|k| {
            let t = len + 16 * k;
            let x = i16::try_from(i32::from(px) + dx * t / len).ok()?;
            let z = i16::try_from(i32::from(pz) + dz * t / len).ok()?;
            (!self.field_tile_is_wall(x, z) && (0..4).any(|d| !self.field_dir_blocked(x, z, d)))
                .then_some((x, z))
        })
    }

    /// The bank prop a walk-touch door's flat `record` binds, when the
    /// record is a runnable prop script (a placed object with a bank entry):
    /// the prop the contact resumes.
    pub(in crate::world::field_movement) fn walk_touch_door_prop(
        &self,
        record: usize,
    ) -> Option<(u8, u8)> {
        self.props
            .bank
            .props
            .iter()
            .find(|(_, p)| p.record == record)
            .map(|(&a, _)| a)
    }

    /// Whether a prop collider is under the arrival bracket
    /// ([`crate::world::FieldPropState::arrival_exempt`]): its static
    /// contact centre is a bracketed bind's.
    pub(crate) fn collider_arrival_exempt(&self, c: &FieldPropCollider) -> bool {
        !c.moving_box
            && self
                .props
                .arrival_exempt
                .iter()
                .any(|&(_, (x, z))| (i32::from(x), i32::from(z)) == c.center)
    }

    /// Post a player-NPC contact into the motion VM's one-slot touch mailbox
    /// (`DAT_80073F1C`), the sibling of [`Self::check_field_walk_touch`].
    ///
    /// Retail's producer is the actor box test `FUN_801CFC40`, which the
    /// per-axis collision `FUN_801CFE4C` calls three times per step: an
    /// overlap both refuses the step and tail-calls `FUN_8003D038` with the
    /// touched actor's `+0x50`. The consumer is the ambient motion VM's
    /// `0x05` wait arm (`FUN_80038158` at `0x8003882C`), which ends the wait
    /// on the frame the post names its own actor - so bumping into a town
    /// NPC breaks it out of its idle pause instead of leaving it parked.
    ///
    /// The `0x801C6470` arena the guard reads is assembled from the live
    /// channels' op-`0x17` records rather than from
    /// [`crate::world::FieldNpcState::default_moves`] (the static harvest): retail reads
    /// the arena, and a stream that has not run its `0x17` yet still holds
    /// the [`DEFAULT_MOVE_UNSET`](legaia_engine_motion_vm::ambient_motion::DEFAULT_MOVE_UNSET)
    /// sentinel there, which suppresses the post.
    ///
    /// REF: FUN_801cfc40, FUN_8003d038, FUN_80038158
    pub(crate) fn post_ambient_motion_touch(&mut self) {
        if self.npcs.ambient.is_empty() {
            return;
        }
        let Some((px, pz)) = self.player_field_position() else {
            return;
        };
        // The same probe fan the NPC collision test walks
        // (`Self::field_npc_dir_blocked`), plus the stand-inside point.
        let mut points: Vec<(i32, i32)> = vec![(px as i32, pz as i32)];
        for dir in Self::dirs_of_bits(self.locomotion.last_move_dir_bits) {
            for &(dx, dz) in &FIELD_ACTOR_PROBES[dir] {
                points.push((px.saturating_add(dx) as i32, pz.saturating_sub(dz) as i32));
            }
        }
        let hit = self.npcs.ambient.keys().copied().find(|slot| {
            let Some(&(ax, az)) = self.npcs.positions.get(slot) else {
                return false;
            };
            points.iter().any(|&(qx, qz)| {
                (qx - ax as i32).abs() < FIELD_NPC_BOX_HALF
                    && (qz - az as i32).abs() < FIELD_NPC_BOX_HALF
            })
        });
        let Some(slot) = hit else { return };
        let stride = legaia_engine_motion_vm::motion_vm::BIND_RECORD_STRIDE;
        let slots = usize::from(*self.npcs.ambient.keys().next_back().unwrap_or(&0)) + 1;
        let mut arena =
            vec![legaia_engine_motion_vm::ambient_motion::DEFAULT_MOVE_UNSET; slots * stride];
        for (&s, chan) in &self.npcs.ambient {
            arena[usize::from(s) * stride] = chan.vm.default_move[0];
        }
        let Some(posted) =
            legaia_engine_motion_vm::motion_vm::post_touch(&arena, usize::from(slot))
        else {
            return; // suppressed by the record's class byte
        };
        if let Some(chan) = self.npcs.ambient.get_mut(&slot) {
            chan.vm.pending_touch = Some(posted);
        }
    }
}
