//! The summon-creature directors' writes outside the camera
//! ([`legaia_engine_cast::cast_module_camera`]'s whole-chain directors):
//! the colour words their arms drive on the creature and the victim,
//! PROT 0916's (Aluru) kill verdict and instant kill, and PROT 0931's (Jedo)
//! kill of the monster row.

use super::*;

/// The PROT entry of Jedo's module (`summon_jedo`).
const JEDO_ENTRY: u32 = 931;

impl World {
    /// Draw PROT 0916's arm-0 verdict once per cast and hold it on the
    /// module state ([`ModuleCamState::kill`]), the way retail parks it in
    /// `0x801F9010` before any later arm reads it. The skip condition is
    /// Nighto's: the scripted-fight byte and the victim record's `+0x20`,
    /// unless the victim carries `+0x16E` bit `4`.
    ///
    /// A kill trains the spell on the same pass: the arm adds `0xC` to the
    /// caster's XP word for the cast spell (`lw` / `addiu v1,v1,0xc` / `sw` on
    /// `+0x5D0` off `0x80084140` at `0x801F6E7C..0x801F6E88`, the slot the
    /// `+0x13D` search found, `0x20` when it found none) unless the
    /// special-battle word `_DAT_8007BAC0` is up (`0x801F6E38..0x801F6E40`).
    /// The kill waives the fold, so no finisher tail credits anything else;
    /// this is what the level check at `0x36` reads, and on an unlearned
    /// spell's first cast it raises the "magic level increased" banner.
    ///
    /// [`ModuleCamState::kill`]: legaia_engine_cast::cast_module_camera::ModuleCamState::kill
    ///
    /// REF: overlay_summon_aluru_0916_801f69f8 (`0x801F6D44..0x801F6E88`)
    pub(in crate::world::battle::cast_band) fn draw_aluru_verdict(
        &mut self,
        caster_slot: u8,
        victim_slot: u8,
        spell_id: u8,
    ) {
        use legaia_engine_cast::cast_module_camera as cam;
        if self.casting.module_cam.kill.is_some() {
            return;
        }
        let level = self.caster_magic_power_byte(caster_slot, spell_id);
        let skipped = self.battle_ctx.scripted_fight != 0
            && self
                .actors
                .get(victim_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| self.tables.monster_catalog.get(id))
                .is_some_and(|def| def.wide_texture_page != 0)
            && self.cast_view(victim_slot).flags
                & legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE
                == 0;
        let roll = (!skipped).then(|| self.next_rand());
        let kill = cam::aluru_kill_verdict(roll, level);
        self.casting.module_cam.kill = Some(kill);
        if kill {
            self.accrue_summon_spell_xp(caster_slot, spell_id, cam::ALURU_KILL_XP);
        }
    }

    /// The instant-kill arms' records and result lines, on the pass that
    /// leaves the arm. Each adds one to the caster's Monsters Defeated word
    /// (record `+0x9C`, indexed by `0x8007BD10[ctx[+0x13]]`) itself, because
    /// none of these victims plays its knockdown, so the death arm of
    /// `FUN_8004AD80` never credits them:
    ///
    /// | module | arm | credit | message bar |
    /// |---|---|---|---|
    /// | 0907 Nighto | 0 (the verdict draw, [`Self::nighto_verdict`]) | the kill leg (`0x801F6F0C`) | arm 13's pass (`0x801F7DDC`) for every outcome: the victim's name and " is confused." / " has been annihilated.", or "No effect." alone |
    /// | 0916 Aluru | `0x11`, the kill pass (`0x801F7E48`) | the kill | arm `0x13`'s pass to `0xFF` on the kill (`0x801F87BC`): the victim's name and its line `0x801F8FEC` |
    /// | 0918 Kemaro | `0x12`, a roll reaching the victim's HP (`0x801F8810`) | in the fold ([`Self::fold_cast`]) | - |
    /// | 0924 Ultimate Rave | 9, the kill path (`0x801F7580`) | the kill | - |
    /// | 0931 Jedo | 29 off a scripted fight (`0x801F864C`) | each monster seat with HP | - |
    ///
    /// Ultimate Rave is the one that also stages its victim's knockdown
    /// (`+0x1DA = +0x1F1` at `0x801F76A4`) on HP `0`, so its kill reaches the
    /// death arm as well and counts twice.
    ///
    /// REF: FUN_8004AD80 (the death arm's credit, `0x8004B184..0x8004B1F4`)
    /// PORT: overlay_summon_nighto_0907_801f69e8 (the `0x5B` raise at `0x801F7DDC`)
    /// PORT: overlay_summon_aluru_0916_801f69f8 (`0x801F7E48..0x801F7E70`, `0x801F87B8..0x801F87D0`)
    /// PORT: overlay_stager_ultimate_rave_0924_801f6a18 (`0x801F7544..0x801F758C`)
    /// PORT: overlay_summon_jedo_0931_801f6a58 (`0x801F8558..0x801F8658`)
    pub(in crate::world::battle::cast_band) fn apply_cast_kill_results(
        &mut self,
        entry: u32,
        phase_in: u8,
        passed: bool,
        caster_slot: u8,
        victim_slot: u8,
    ) {
        use legaia_engine_cast::cast_module_camera as cam;
        use legaia_engine_cast::seru_magic_ticks as sm;
        if !passed || caster_slot >= self.party.party_count {
            return;
        }
        let st = self.casting.module_cam;
        let victim = st.victim_slot.unwrap_or(victim_slot);
        let killed = st.kill == Some(true);
        match entry {
            907 if phase_in == sm::NIGHTO_FORK_ARM => {
                if let Some(credit) = self.casting.module_nighto_outcome.map(|o| o.arm0_credit()) {
                    self.raise_composed_message_bar(
                        entry,
                        credit.line,
                        credit.named.then_some(victim),
                    );
                }
            }
            cam::ALURU_ENTRY if killed && phase_in == cam::ALURU_BLAST_ARM => {
                self.credit_monster_defeated(caster_slot);
            }
            cam::ALURU_ENTRY if killed && phase_in == cam::ALURU_SPREAD_ARM => {
                self.raise_composed_message_bar(entry, cam::ALURU_KILL_LINE, Some(victim));
            }
            legaia_engine_cast::cast_module_ticks::ULTIMATE_RAVE_ENTRY
                if killed
                    && phase_in
                        == legaia_engine_cast::cast_module_ticks::ULTIMATE_RAVE_TITLE_ARM =>
            {
                self.credit_monster_defeated(caster_slot);
            }
            JEDO_ENTRY if self.battle_ctx.scripted_fight == 0 && phase_in == cam::JEDO_HIT_ARM => {
                for _ in self.living_monster_slots() {
                    self.credit_monster_defeated(caster_slot);
                }
            }
            _ => {}
        }
    }

    /// PROT 0916's part kills and tint stores.
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 4 (`0x801F71E0`) | pass | victim `+0x21C = 0`: the band's hide eases out |
    /// | 7 (`0x801F74C0..0x801F74D0`) | pass | victim `+0x04 = 0x20080200` |
    /// | 14 (`0x801F7994..0x801F79A8`) | pass | creature's running clip entry root speed `+0x0C = -16` |
    /// | 15 (`0x801F7B24..0x801F7B7C`) | pass | the arm-14 records ended; creature `+0x04 = 0`, `+0x21C = 0xFF` |
    /// | `0x11` (`0x801F7E8C`, `0x801F7E94`, `0x801F7CF4..0x801F7D08`) | pass | victim rate `1`, `+0x21C = 0xC8`, facing the creature's `+ 0x800` |
    /// | `0x12`, `0x13` (`0x801F81C0..0x801F81DC`) | every, while the victim's clip is `0` | the same rate and tint |
    /// | `0x12` on the damage path (`0x801F8680..0x801F868C`) | pass | victim rate `2`, impact `2`, `+0x21C = 0` |
    /// | `0xFF` (`0x801F88BC`) | every | a dead victim hidden |
    ///
    /// The word the `0xC8` state reads is the neutral `0x20080200` arm 7
    /// stored.
    ///
    /// PORT: overlay_summon_aluru_0916_801f69f8 (PROT 0916; the part kills of arm 15, the arm-14 root speed and the seat tints of arms 4, 7, 15, 0x11..0x13)
    fn apply_aluru_seat_writes(&mut self, phase_in: u8, passed: bool, victim: u8, seat_slot: u8) {
        use legaia_engine_cast::cast_module_camera as cam;
        const ALURU_VICTIM_SHOWN_ARM: u8 = 7;
        const TINT_FLAT: u8 = 0xC8;
        const NEUTRAL_WORD: u32 = 0x2008_0200;
        const HIDDEN: u8 = 0xFF;
        const ALURU_BACKOFF_ARM: u8 = 14;
        const ALURU_BACKOFF_SPEED: i16 = -16;
        // Arm 4's pass seats the creature (`jal 0x801F19EC`), then lights it
        // white and points its node at deformation row 1, a sweep that
        // re-forms it from the floor up as its `+0x1BA` drains
        // (`0x801F7120..0x801F71D8`). The host seats the creature's mesh
        // when it has bound it, which can be after the arm's pass, so the
        // stores land on the first pass of arms 4..6 the seat exists on:
        // ahead of them the installer's reset would undo them. Arm 6's exit
        // drops `+0x21C` (`0x801F7430`) and the tint ease takes the word back.
        let assembling = (phase_in == cam::ALURU_ASSEMBLY_ARM && passed)
            || (phase_in > cam::ALURU_ASSEMBLY_ARM && phase_in <= cam::ALURU_ASSEMBLED_ARM);
        if assembling
            && let Some(slot) = self.casting.summon_actor_slot
            && self
                .actors
                .get(usize::from(slot))
                .is_some_and(|a| a.battle.shatter_row != Some(cam::ALURU_SHATTER_ROW))
        {
            self.npcs.face_rows.insert(
                cam::ALURU_SHATTER_ROW,
                crate::world::FieldFaceRow {
                    offset: cam::ALURU_SHATTER_ROW_OFFSET,
                    flags: cam::ALURU_SHATTER_ROW_FLAGS,
                },
            );
            if let Some(a) = self.actors.get_mut(usize::from(slot)) {
                a.battle.render_color = cam::ALURU_ASSEMBLY_COLOUR;
                a.battle.render_blend = cam::ALURU_ASSEMBLY_BLEND;
                a.battle.render_flag = HIDDEN;
                a.battle.shatter_row = Some(cam::ALURU_SHATTER_ROW);
            }
        }
        // The victim the summon band hid comes back on the creature's way in:
        // arm 4's pass drops its `+0x21C` (`sb zero,0x21c(s3)` at
        // `0x801F71E0`, `s3` the caster's `+0x1DD` seat), so the tint ease
        // takes its zero word back up, and arm 7's pass stores the neutral word
        // (`0x801F74C0..0x801F74D0`). The engine's zero word under a lowered
        // state is its resting neutral, so the victim stands at once where
        // retail's eases in over sixteen vsyncs.
        if passed
            && (phase_in == cam::ALURU_ASSEMBLY_ARM || phase_in == ALURU_VICTIM_SHOWN_ARM)
            && let Some(a) = self.actors.get_mut(usize::from(victim))
        {
            a.battle.render_flag = 0;
            if phase_in == ALURU_VICTIM_SHOWN_ARM {
                a.battle.render_color = NEUTRAL_WORD;
            }
        }
        if phase_in == cam::ALURU_ASSEMBLED_ARM
            && passed
            && let Some(a) = self
                .casting
                .summon_actor_slot
                .and_then(|s| self.actors.get_mut(usize::from(s)))
        {
            a.battle.render_flag = 0;
            a.battle.shatter_row = None;
        }
        if phase_in == cam::ALURU_BLAST_ARM && passed && self.casting.module_cam.kill == Some(true)
        {
            self.npcs.face_rows.insert(
                cam::ALURU_SHATTER_ROW,
                crate::world::FieldFaceRow {
                    offset: cam::ALURU_KILL_SHATTER_ROW_OFFSET,
                    flags: cam::ALURU_SHATTER_ROW_FLAGS,
                },
            );
            if let Some(a) = self.actors.get_mut(usize::from(victim)) {
                a.battle.shatter_row = Some(cam::ALURU_SHATTER_ROW);
            }
        }
        // Arm 14's pass stores the root speed `-16` straight into the
        // creature's running clip entry (`lw v0,-0x6CA8(v0)` / `lw v1,0x50(v0)`
        // / `sh v0,0xc(v1)` at `0x801F7994..0x801F79A8`), beside its fade spawn
        // and the `scalar * 55` countdown. The disc's is `0`; a negative speed
        // steps every pass with no range test (`bltz` at `0x80047D3C`), so the
        // creature backs off along its facing from arm 15 on: a `--watch` of
        // its `+0x38` reads `-24` a step-`3` pass from `0x80047DC0`.
        if phase_in == ALURU_BACKOFF_ARM
            && passed
            && let Some(p) = self
                .casting
                .summon_actor_slot
                .and_then(|s| self.actors.get_mut(usize::from(s)))
                .and_then(|a| a.battle_animation.as_mut())
        {
            p.set_root_speed(ALURU_BACKOFF_SPEED);
        }
        if phase_in == cam::ALURU_RECORD_KILL_ARM && passed {
            if let Some(scene) = self.casting.active_summon.as_mut() {
                for part in &mut scene.parts {
                    if part
                        .tag
                        .is_some_and(|t| cam::ALURU_ARM14_RECORDS.contains(&t))
                    {
                        part.finished = true;
                    }
                }
            }
            if let Some(a) = self.actors.get_mut(usize::from(seat_slot)) {
                a.battle.render_color = 0;
                a.battle.render_flag = HIDDEN;
            }
        }
        // Arm `0xFF` hides the victim (`sb s0,0x21C(s3)`, `sw zero,0x4(s3)` at
        // `0x801F88BC`, `s0` the phase byte). A live victim's way back is not read
        // yet, so the port hides only the seat the kill path emptied.
        if phase_in == cam::ALURU_DONE_ARM
            && let Some(a) = self.actors.get_mut(usize::from(victim))
            && a.battle.hp == 0
        {
            a.battle.render_flag = HIDDEN;
            a.battle.render_color = 0;
        }
        let creature_facing = self
            .actors
            .get(usize::from(seat_slot))
            .map(|a| a.battle.facing_angle);
        // The damage path's way out of arm `0x12`, beside its roll
        // (`0x801F8680..0x801F868C`): rate `2`, impact `2`, the tint state off.
        if phase_in == cam::ALURU_HIT_ARM
            && passed
            && self.casting.module_cam.kill != Some(true)
            && let Some(a) = self.actors.get_mut(usize::from(victim))
        {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(2);
            a.battle.impact_state = 2;
            a.battle.render_flag = 0;
            return;
        }
        let blast = phase_in == cam::ALURU_BLAST_ARM && passed;
        let spread = matches!(phase_in, cam::ALURU_HIT_ARM | cam::ALURU_SPREAD_ARM);
        if let Some(a) = self.actors.get_mut(usize::from(victim))
            && (blast || (spread && a.battle.current_anim == 0))
        {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(1);
            a.battle.render_flag = TINT_FLAT;
            a.battle.render_color = NEUTRAL_WORD;
            if blast && let Some(f) = creature_facing {
                a.battle.facing_angle = f.wrapping_add(0x800) & 0xFFF;
            }
        }
    }

    /// The colour-word and HP stores of the summon-creature directors' arms
    /// on the pass the director just made for `phase_in` of module `entry`
    /// (`passed`: it left the arm).
    ///
    /// - The creature's `+0x1BA` as the director owns it
    ///   ([`ModuleCamState::creature_shatter`]): PROT 0916's arms 4..6
    ///   assemble it, PROT 0914's arm `0xE` breaks it up.
    /// - The victim's step ([`ModuleCamState::victim_shatter_step`]): PROT
    ///   0916's arm `0x13` on its kill path (`0x801F81AC..0x801F81BC`).
    /// - PROT 0916's arm `0x11`, kill path, on its passing tick: the victim's
    ///   HP to `0` (`sh zero,0x14C(s3)` at `0x801F7E14`), and the band's fold
    ///   owes nothing - the damage path is the only one that reaches the hit.
    /// - PROT 0917's arm `0xE` pass: the creature into the defeat fade
    ///   (`+0x21C = 2`, [`cam::BARRA_CREATURE_FADE_ARM`]).
    /// - PROT 0931 on a fight that is not scripted: arm 29 makes no damage
    ///   call (the fold is waived), arm 30 adds `delta << 2` to every living
    ///   monster's `+0x1BA` a pass (`0x801F86E0..0x801F874C`) and its exit
    ///   zeroes their HP (`0x801F8814`).
    ///
    /// [`ModuleCamState::creature_shatter`]: legaia_engine_cast::cast_module_camera::ModuleCamState::creature_shatter
    /// [`ModuleCamState::victim_shatter_step`]: legaia_engine_cast::cast_module_camera::ModuleCamState::victim_shatter_step
    pub(in crate::world::battle::cast_band) fn apply_summon_body_writes(
        &mut self,
        entry: u32,
        phase_in: u8,
        passed: bool,
        victim_slot: u8,
        seat_slot: u8,
    ) {
        use legaia_engine_cast::cast_module_camera as cam;
        let st = self.casting.module_cam;
        let victim = st.victim_slot.unwrap_or(victim_slot);
        if entry == cam::ALURU_ENTRY
            && phase_in == cam::ALURU_BLAST_ARM
            && passed
            && st.kill == Some(true)
        {
            let mut v = self.cast_view(victim);
            v.hp = 0;
            self.write_cast_view(victim, &v);
            self.casting.module_skips_fold = true;
        }
        if entry == cam::ALURU_ENTRY {
            self.apply_aluru_seat_writes(phase_in, passed, victim, seat_slot);
        }
        if st.victim_shatter_step != 0
            && let Some(a) = self.actors.get_mut(usize::from(victim))
        {
            a.battle.shatter = a.battle.shatter.wrapping_add(st.victim_shatter_step);
        }
        if let Some(w) = st.creature_shatter
            && let Some(a) = self.actors.get_mut(usize::from(seat_slot))
        {
            a.battle.shatter = w;
        }
        if entry == 917
            && phase_in == cam::BARRA_CREATURE_FADE_ARM
            && passed
            && let Some(a) = self.actors.get_mut(usize::from(seat_slot))
        {
            a.battle.render_flag = 2;
        }
        if entry == JEDO_ENTRY && self.battle_ctx.scripted_fight == 0 {
            if phase_in == cam::JEDO_HIT_ARM && passed {
                self.casting.module_skips_fold = true;
            }
            if phase_in == cam::JEDO_KILL_ARM {
                for slot in self.living_monster_slots() {
                    if let Some(a) = self.actors.get_mut(slot) {
                        a.battle.shatter = a
                            .battle
                            .shatter
                            .wrapping_add(cam::JEDO_SHATTER_STEP_PER_TICK);
                    }
                }
                if passed {
                    for slot in self.living_monster_slots() {
                        let mut v = self.cast_view(slot as u8);
                        v.hp = 0;
                        self.write_cast_view(slot as u8, &v);
                    }
                }
            }
        }
    }

    /// The non-lethal pre-hits five summon modules land before the
    /// band's fold, on the pass that makes them: each target's roll is the
    /// summon branch of `FUN_801DD0AC` with its finisher
    /// ([`Self::summon_module_roll`]), clamped to `HP - 1`
    /// ([`legaia_engine_cast::seru_magic_ticks::summon_pre_hit`]).
    ///
    /// - PROT 0915 arm 7's pass: the latched victim, the roll quartered and
    ///   floored at `1` (`0x801F7004..`); skipped for a victim with
    ///   `+0x16E` bit `4`.
    /// - PROT 0928 arm `0xF`: once, on the first pass its word falls below
    ///   `0x1C1` (`0x801F80C4..0x801F81EC`), every living targetable monster.
    /// - PROT 0932 arm `0x12` and PROT 0933 arm `0xC`, on their pass: every
    ///   living monster.
    /// - PROT 0908 arm 8's pass (`0x801F76CC..0x801F77F4`): the latched
    ///   victim, the roll quartered; skipped for a victim with `+0x16E` bit
    ///   `4`.
    ///
    /// Not carried: the per-seat tallies `0x801F6978` / `0x801F6980` the
    /// quartered roll is checked against. The first argument's `0x10` /
    /// `0x12` mode is read only on the kernel's monster branch. PROT 0929 arm
    /// 16's wave is [`Self::mule_wave_hit`], PROT 0934 arm `0x18`'s ladder
    /// `ozma_hit_ladder`.
    pub(in crate::world::battle::cast_band) fn apply_summon_pre_hits(
        &mut self,
        entry: u32,
        phase_in: u8,
        passed: bool,
        caster: u8,
        spell_id: u8,
    ) {
        use legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE;
        let st = self.casting.module_cam;
        let targets: Vec<u8> = match (entry, phase_in) {
            (915, 7) if passed => st
                .victim_slot
                .filter(|&v| self.cast_view(v).flags & FLAG_NON_TARGETABLE == 0)
                .into_iter()
                .collect(),
            (928, 0xF) if st.countdown.0 < 0x1C1 && st.wait != (0xF, 1) => {
                // Once: the arm hides the row after its sweep, so later
                // passes find nothing to hit.
                self.casting.module_cam.wait = (0xF, 1);
                self.living_monster_slots()
                    .into_iter()
                    .map(|s| s as u8)
                    .filter(|&s| self.cast_view(s).flags & FLAG_NON_TARGETABLE == 0)
                    .collect()
            }
            (932, 0x12) | (933, 0xC) if passed => self
                .living_monster_slots()
                .into_iter()
                .map(|s| s as u8)
                .collect(),
            (908, 8) if passed => st
                .victim_slot
                .filter(|&v| self.cast_view(v).flags & FLAG_NON_TARGETABLE == 0)
                .into_iter()
                .collect(),
            _ => return,
        };
        for t in targets {
            let Some(dmg) = self.summon_module_roll(caster, t, spell_id) else {
                continue;
            };
            let mut dmg = dmg.min(u32::from(u16::MAX)) as u16;
            if entry == 915 {
                dmg = (dmg >> 2).max(1);
            }
            // Arm 8's roll quartered (`srl s0,s0,0x2` at `0x801F7728`).
            if entry == 908 {
                dmg >>= 2;
            }
            let mut v = self.cast_view(t);
            if v.hp == 0 {
                continue;
            }
            // Each store `0` into the readout flag `0x8007B64C` the damage
            // kernel raised (PROT 0908 `0x801F76DC`, 0915 `0x801F7118`, 0928
            // `0x801F8134`, 0932 `0x801F7BC0`, 0933 `0x801F7CEC`): the popup goes
            // up with no combo cluster, which opens on the fold's hit.
            self.battle.combo_cluster_held = true;
            // `FUN_801F44A0(amount, seat)`: PROT 0915's after its clamp, the
            // others' ahead of it.
            if matches!(entry, 908 | 915 | 928 | 932 | 933) {
                let amount = if entry == 915 {
                    dmg.min(v.hp.saturating_sub(1))
                } else {
                    dmg
                };
                self.battle
                    .hit_fx
                    .push(legaia_engine_battle::battle_events::BattleHitFx {
                        target_slot: t,
                        amount,
                        is_heal: false,
                        is_crit: false,
                    });
            }
            // PROT 0933's stages nothing (`0x801F7D2C..0x801F7D3C`, HP and on
            // to the next seat): the row shows no reaction until arm 0x14's hit.
            if entry == 933 {
                legaia_engine_cast::seru_magic_ticks::apply_hit_cap_hp_minus_one(
                    &mut v,
                    u32::from(dmg),
                );
            } else if entry == 908 {
                // PROT 0908's stages the flinch with no knockdown fork
                // (`0x801F77B4..0x801F77D4`): arm 9's clip `2` on Gobu Gobu.
                legaia_engine_cast::seru_magic_ticks::apply_hit_cap_hp_minus_one(
                    &mut v,
                    u32::from(dmg),
                );
                legaia_engine_cast::seru_magic_ticks::stage_reaction_alt_only(&mut v);
            } else {
                legaia_engine_cast::seru_magic_ticks::summon_pre_hit(&mut v, u32::from(dmg));
            }
            // PROT 0915's turns the victim to face down the lane whatever it
            // staged (`li v0,0x800` / `sh v0,0x46(s1)` at `0x801F724C..0x801F7254`):
            // the reaction's root motion slides it straight back along `+z`.
            if entry == 915
                && let Some(a) = self.actors.get_mut(usize::from(t))
            {
                a.battle.facing_angle = 0x800;
            }
            // PROT 0928's parks the flinch at rate 0 (`0x801F81C8`); arm 0xF's
            // pass runs its cue cursor off the track and arm 0x13 sets the row
            // going again (`apply_palma_writes`).
            if entry == 928 {
                v.anim_rate = 0;
            }
            self.write_cast_view(t, &v);
        }
    }

    /// PROT 0930's (Horn) seat visibility, on the passes that write it: arm
    /// 1 shows the party (`+0x04 = 0x20080200`, `+0x21C = 0`,
    /// `0x801F6C78..`), arm 3 seats the creature hidden (`+0x21C = 0xFF`, `+0x04 = 0`,
    /// `0x801F6E98..`) and arm 4 shows it white (`+0x04 = 0x3FFFFFFF`,
    /// `+0x21C = 0`, `0x801F6FD4..0x801F6FFC`), the idle arm of the tint
    /// state machine easing it back to neutral; arm 8 hides the party
    /// (`0x801F7428..`); arm `0xA`, the heal, shows the party again and
    /// relights the creature for the closing burst: `+0x04 = 0x3FFFFFFF`,
    /// `+0x0C = 0x1000`, `+0x21C = 0xFF` (the hold state, which leaves the
    /// word drawn), standing at `+0x36 = 0x480`, `+0x38 = -0x200`, facing
    /// `0` (`0x801F77A8..0x801F77EC`); arm `0xB`'s pass hides it
    /// (`+0x21C = 0xFF`, `+0x04 = 0`, `0x801F7D04..0x801F7D10`). The engine's
    /// shown word is its resting `0` under a lowered flag. Arm 1's hide of
    /// the monster row is not carried: nothing in the module shows it again,
    /// and the band's own reset is not modelled.
    pub(in crate::world::battle::cast_band) fn apply_horn_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
        seat_slot: u8,
    ) {
        use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
        /// The white word arms 4 and `0xA` store (`lui 0x3FFF; ori 0xFFFF`).
        const WHITE: u32 = 0x3FFF_FFFF;
        if !passed {
            return;
        }
        let set = |a: &mut crate::world::Actor, hidden: bool| {
            a.battle.render_flag = if hidden { RENDER_FLAG_HIDDEN } else { 0 };
            a.battle.render_color = 0;
        };
        if let Some(a) = self.actors.get_mut(usize::from(seat_slot)) {
            match phase_in {
                3 | 0xB => set(a, true),
                4 => {
                    a.battle.render_flag = 0;
                    a.battle.render_color = WHITE;
                }
                0xA => {
                    a.battle.render_flag = RENDER_FLAG_HIDDEN;
                    a.battle.render_color = WHITE;
                    a.battle.render_blend = 0x1000;
                    a.move_state.world_y = 0x480;
                    a.move_state.world_z = -0x200;
                    a.battle.facing_angle = 0;
                }
                _ => {}
            }
        }
        let party = match phase_in {
            1 | 0xA => Some(false),
            8 => Some(true),
            _ => None,
        };
        if let Some(hidden) = party {
            let pc = usize::from(self.party.party_count);
            for a in self.actors.iter_mut().take(pc) {
                set(a, hidden);
            }
        }
    }

    /// PROT 0929's (Mule) monster-row visibility. Arm 1 shows every living
    /// monster on each of its passes, held or not (`+0x04 = 0x20080200`,
    /// `+0x21C = 0`, `0x801F6D04..0x801F6D50`, ahead of the stream-token
    /// gate), so the row stands in the underwater backdrop while the bubbles
    /// rise; arm 5's pass hides it for the creature's entrance
    /// (`+0x04 = 0`, `+0x21C = 0xFF`, `0x801F7040..`), arm 11's shows it
    /// again for the attack, turned to face the party at half its render
    /// scale (`+0x46 = 0x800`, node `+0x72 >>= 1`, `0x801F7590..0x801F75C8`;
    /// the arm also halves `+0x21D`, `0x801F75D4..0x801F75E0`, and the hit
    /// arm 18 doubles it back on each monster it strikes,
    /// `0x801F888C..0x801F8898`), arm 16's pass hides it,
    /// doubles the scale back and drops it to the floor (`+0x36 = 0`,
    /// `0x801F842C..0x801F8460`; arm 17 starts at `0x801F8488`), and the hit
    /// arm 18 shows each struck monster (`0x801F87C4..`). The engine's shown
    /// word is its resting `0` under a lowered flag. Each pass also halts the parts the arm kills by their
    /// parked handles and runs out the loops arm 16 ends
    /// ([`legaia_engine_cast::cast_module_camera::mule_arm_kills`],
    /// [`legaia_engine_cast::cast_module_camera::mule_arm_loop_ends`]).
    /// PROT 0929's per-pass tail (`0x801F8B08..0x801F8BF8`): arm 0 clears
    /// the follow word and arm 9's pass sets it; while it is set every pass
    /// ends by pointing the scanline backdrop's sample origin
    /// (`ctx[+0x285]` / `+0x286`) at the live camera, after the arm's own
    /// stores ([`legaia_engine_cast::cast_module_camera::mule_backdrop_origin`]).
    pub(in crate::world::battle::cast_band) fn mule_backdrop_follow(
        &mut self,
        phase_in: u8,
        passed: bool,
    ) {
        use legaia_engine_cast::cast_module_camera::{
            MULE_BACKDROP_FOLLOW_ARM, mule_backdrop_origin,
        };
        if phase_in == 0 {
            self.casting.module_mule_backdrop_follow = false;
            return;
        }
        if phase_in == MULE_BACKDROP_FOLLOW_ARM && passed {
            self.casting.module_mule_backdrop_follow = true;
        }
        if !self.casting.module_mule_backdrop_follow {
            return;
        }
        let pose = self.battle_cam_pose();
        let (u, v) = mule_backdrop_origin(
            pose.pitch as i32,
            pose.yaw as i32,
            pose.tr[0] as i32,
            pose.tr[1] as i32,
        );
        let wash = &mut self.casting.module_pres.wash;
        wash.u_base = u;
        wash.v_base = v;
    }

    pub(in crate::world::battle::cast_band) fn apply_mule_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
        monster_count: u8,
        caster: u8,
        spell_id: u8,
    ) {
        use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
        self.mule_row_seat(phase_in, passed, monster_count, caster, spell_id);
        // Arms 15 and 16 copy the creature's display trio over the position
        // of two of arm 14's parked parts on every pass, held or not
        // (`lwl` / `lwr` of `+0x3C..+0x43` into `+0x14..+0x1B` through the
        // handles at `0x801F9A98` / `0x801F9A9C`; `0x801F7BD8..0x801F7C20`,
        // `0x801F81BC..0x801F820C`), so they ride with the creature.
        if matches!(phase_in, 15 | 16)
            && let Some(body) = self
                .casting
                .summon_actor_slot
                .and_then(|c| self.battle_display_trio(usize::from(c)))
            && let Some(scene) = self.casting.active_summon.as_mut()
        {
            for part in scene.parts.iter_mut().filter(|p| {
                !p.finished
                    && p.tag.is_some_and(|t| {
                        legaia_engine_cast::cast_module_camera::MULE_CREATURE_RIDERS.contains(&t)
                    })
            }) {
                part.state.world_x = body[0] as i16;
                part.state.world_y = body[1] as i16;
                part.state.world_z = body[2] as i16;
            }
        }
        if passed && let Some(scene) = self.casting.active_summon.as_mut() {
            let kills = legaia_engine_cast::cast_module_camera::mule_arm_kills(phase_in);
            let ends = legaia_engine_cast::cast_module_camera::mule_arm_loop_ends(phase_in);
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| kills.contains(&t)) {
                    part.finished = true;
                } else if part.tag.is_some_and(|t| ends.contains(&t)) {
                    part.state.field_8c = 0;
                }
            }
        }
        // The creature: arm 11's pass lifts it to `+0x36 = -0x100` facing
        // `0` (`0x801F7668..0x801F7674`), arm 14's lights it white and holds
        // the word (`+0x04 = 0x3FFFFFFF`, `+0x0C = 0x1000`, `+0x21C = 0xFF`,
        // `0x801F7AB0..0x801F7AF0`, beside its clip-2 stage), and arm 16's
        // hides it (`+0x21C = 0xFF`, `+0x04 = 0`, `0x801F83C4..0x801F83D0`).
        if passed
            && matches!(phase_in, 11 | 14 | 16)
            && let Some(c) = self
                .casting
                .summon_actor_slot
                .and_then(|s| self.actors.get_mut(usize::from(s)))
        {
            match phase_in {
                11 => {
                    c.move_state.world_y = -0x100;
                    c.battle.facing_angle = 0;
                }
                14 => {
                    c.battle.render_color = 0x3FFF_FFFF;
                    c.battle.render_blend = 0x1000;
                    c.battle.render_flag = RENDER_FLAG_HIDDEN;
                }
                _ => {
                    c.battle.render_color = 0;
                    c.battle.render_flag = RENDER_FLAG_HIDDEN;
                }
            }
        }
        let hidden = match (phase_in, passed) {
            (1, _) | (11 | 18, true) => false,
            (5 | 16, true) => true,
            _ => return,
        };
        let pc = usize::from(self.party.party_count);
        for a in self.actors.iter_mut().skip(pc) {
            if a.battle_monster_id.is_none() || a.battle.hp == 0 {
                continue;
            }
            a.battle.render_flag = if hidden { RENDER_FLAG_HIDDEN } else { 0 };
            a.battle.render_color = 0;
            let scale = a.battle.render_scale.unwrap_or(0x1000);
            match (phase_in, passed) {
                (11, true) => {
                    a.battle.facing_angle = 0x800;
                    a.battle.render_scale = Some(scale >> 1);
                    // `+0x21D >>= 1` (`0x801F75D4..0x801F75E0`).
                    a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(
                        a.battle.anim_rate.get() >> 1,
                    );
                }
                // The hit arm doubles each struck monster's rate back
                // (`+0x21D <<= 1`, `0x801F888C..0x801F8898`).
                (18, true) => {
                    a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(
                        a.battle.anim_rate.get().wrapping_shl(1),
                    );
                }
                (16, true) => {
                    a.battle.render_scale = Some(scale.wrapping_shl(1));
                    a.move_state.world_y = 0;
                    // `sh zero,0x48` (`0x801F8444`): the roll back to rest.
                    a.battle.render_roll = 0;
                }
                _ => {}
            }
        }
    }

    /// PROT 0929's (Mule) monster-row seat. Arm 0 parks each living
    /// monster's ground trio and facing (`0x801F9A58` / `0x801F9A78`) and
    /// stands it on the alternate-family seat `0x80077628 + (count + 3) *
    /// 0x20` at half its `x` and half its `z` less `0x226`, facing `0x800`
    /// (`0x801F6C04..0x801F6CF0`); the last arm `0xFF` puts every living
    /// monster back on its parked trio and facing (`0x801F8A70..0x801F8AE8`).
    /// Arm 16's every pass walks the row's wave front `0x801F9AC8` (from
    /// arm 15's `-0x100`, `+ step * scalar / 2` a pass) and lifts each
    /// monster it has passed `4 * step * scalar` a pass up to `+0x36 = -0x100`
    /// (`0x801F7D9C..0x801F8108`) and rolls it `+0x48 += 16 * step *
    /// scalar` while the Mystic Shield is down (`0x801F8110..0x801F8164`);
    /// the strike on each passed monster is [`Self::mule_wave_hit`].
    fn mule_row_seat(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        caster: u8,
        spell_id: u8,
    ) {
        let monsters = usize::from(monster_count);
        let map = self.cast_seat_map();
        let slots: Vec<(usize, usize)> = (0..monsters)
            .filter_map(|i| map.engine(3 + i as u8).map(|e| (i, usize::from(e))))
            .filter(|&(_, e)| self.actors.get(e).is_some_and(|a| a.battle.hp != 0))
            .collect();
        let d = legaia_engine_cast::cast_module_camera::DELTA_PER_TICK;
        let t = d * legaia_engine_cast::cast_module_camera::SPEED_SCALAR;
        match (phase, passed) {
            (0, true) => {
                self.casting.module_saved_poses.clear();
                for &(i, e) in &slots {
                    let seat =
                        legaia_engine_battle::battle_seats::monster_seat(monsters as u8, i, true);
                    let Some(a) = self.actors.get_mut(e) else {
                        continue;
                    };
                    let m = &mut a.move_state;
                    self.casting.module_saved_poses.push((
                        e,
                        [m.world_x, m.world_y, m.world_z],
                        a.battle.facing_angle,
                    ));
                    m.world_x = seat.x / 2;
                    m.world_z = seat.z / 2 - 0x226;
                    a.battle.facing_angle = 0x800;
                }
            }
            (15, true) => self.casting.module_mule_front = -0x100,
            (18, _) => self.battle.combo_cluster_held = false,
            (16, _) => {
                // The wave's popups go up without opening the combo cluster:
                // each hit clears the readout flag the damage kernel raised
                // (`sb zero,0x8007B64C` at `0x801F7E60`), as PROT 0959's
                // wrapper hits do. The hit arm 18's own kernel call raises it
                // again, so the hold ends there.
                self.battle.combo_cluster_held = true;
                self.casting.module_mule_front += t / 2;
                let front = self.casting.module_mule_front;
                let shield = self.battle.mystic_shield_up();
                // Every monster seat, fallen ones too: the walk tests HP inside
                // the loop (`0x801F7DD8..0x801F8188`, bound `ctx[+1]`).
                let row: Vec<usize> = (0..monsters)
                    .filter_map(|i| map.engine(3 + i as u8).map(usize::from))
                    .collect();
                for &e in &row {
                    self.mule_wave_hit(e, front, shield, caster, spell_id);
                    let Some(a) = self.actors.get_mut(e) else {
                        continue;
                    };
                    let m = &mut a.move_state;
                    if i32::from(m.world_z) >= front {
                        continue;
                    }
                    if m.world_y >= -0xFF {
                        m.world_y = m.world_y.wrapping_sub((4 * t) as i16);
                    }
                    // The roll `+0x48 += 16 * step * scalar` (`0x801F8110..
                    // 0x801F8164`), skipped while the Mystic Shield is up.
                    if !shield {
                        a.battle.render_roll =
                            a.battle.render_roll.wrapping_add((16 * t) as u16) & 0xFFF;
                    }
                }
            }
            (0xFF, _) => {
                for &(e, pos, facing) in &self.casting.module_saved_poses {
                    if !slots.iter().any(|&(_, s)| s == e) {
                        continue;
                    }
                    if let Some(a) = self.actors.get_mut(e) {
                        [
                            a.move_state.world_x,
                            a.move_state.world_y,
                            a.move_state.world_z,
                        ] = pos;
                        a.battle.facing_angle = facing;
                    }
                }
            }
            _ => {}
        }
    }

    /// One pass of PROT 0929 arm 16's wave hit on monster slot `slot`
    /// (`0x801F7DD8..0x801F80A8`). A living, targetable monster with no clip
    /// playing (`+0x1D9 == 0`) that the wave front `front` has passed, with
    /// the Mystic Shield down, takes a third of the summon roll
    /// `FUN_801DD0AC(0x10, 7, seat)` - `rand() % 16 + 16` when that comes to
    /// zero - plus `rand() % (hit / 8 + 1)`, its popup pushed with that
    /// amount (`FUN_801F44A0`), the HP write clamped to `HP - 1` (the wave
    /// cannot kill) and its reaction clip `+0x1EF` (else `+0x1F0`) staged with
    /// the restage bits `5`. Any monster seat the wave does not hit this pass,
    /// a fallen one included, has its staged clip cleared (`sb zero,0x1DA`, `0x801F809C..0x801F80A8`).
    ///
    /// Not carried: the damage tallies the roll is booked against
    /// (`0x8007BD14`, or `0x801F6980 + seat * 2` under a group target), and
    /// the byte `0x8007B64C` the hit clears.
    /// REF: FUN_801DD0AC (each wave roll, through [`Self::summon_module_roll`])
    /// REF: FUN_801F44A0 (the popup push, `World::battle.hit_fx`)
    fn mule_wave_hit(&mut self, slot: usize, front: i32, shield: bool, caster: u8, spell_id: u8) {
        use legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE;
        let s = slot as u8;
        let v = self.cast_view(s);
        let z = self
            .actors
            .get(slot)
            .map_or(0, |a| i32::from(a.move_state.world_z));
        // `+0x1D9 == 0`: no clip playing - the engine's reaction channel
        // included, where a staged reaction plays (`battle_current_anim`).
        // A fallen seat takes the same skip as the others: its staged clip is
        // cleared (`beq v0,zero,0x801F80A0` at `0x801F7DEC`).
        let hittable =
            v.hp != 0 && v.flags & FLAG_NON_TARGETABLE == 0 && self.battle_current_anim(slot) == 0;
        if !hittable || z >= front || shield {
            let mut v = v;
            v.staged_anim = 0;
            self.write_cast_view(s, &v);
            return;
        }
        // A whole `FUN_801DD0AC` roll, finisher and all (`0x801DD478`): the
        // wave trains the spell and fills the victim's spirit on every hit.
        let Some(roll) = self.summon_module_roll(caster, s, spell_id) else {
            return;
        };
        let mut hit = roll / 3;
        if hit == 0 {
            hit = self.next_rand() % 16 + 16;
        }
        hit += self.next_rand() % (hit / 8 + 1);
        self.battle
            .hit_fx
            .push(legaia_engine_battle::battle_events::BattleHitFx {
                target_slot: s,
                amount: hit.min(u32::from(u16::MAX)) as u16,
                is_heal: false,
                is_crit: false,
            });
        let mut v = self.cast_view(s);
        legaia_engine_cast::seru_magic_ticks::summon_pre_hit(&mut v, hit);
        // Arm 16 stages `+0x1EF`, else `+0x1F0` (`0x801F8058..0x801F8078`),
        // with no `+0x1F2` knockdown test, and always ORs both restage bits
        // (`ori v0,v0,0x5`): a struck monster flinches, it is not knocked
        // down.
        v.staged_anim = if v.reaction_alt != 0 {
            v.reaction_alt
        } else {
            v.reaction_alt2
        };
        v.restage |= 0x05;
        self.write_cast_view(s, &v);
    }

    /// PROT 0915's (Mushura) arms 0 and 1 show the latched victim on every
    /// pass (`sb zero,0x21C` / `sw 0x20080200,0x4` at `0x801F6BB8` /
    /// `0x801F6CB8`): the band's freeze hid it, and the opening framings are
    /// on it. The engine's shown word is its resting `0` under a lowered
    /// flag.
    pub(in crate::world::battle::cast_band) fn apply_mushura_writes(
        &mut self,
        phase_in: u8,
        victim_slot: u8,
    ) {
        if phase_in > 1 {
            return;
        }
        let victim = self.casting.module_cam.victim_slot.unwrap_or(victim_slot);
        if let Some(a) = self.actors.get_mut(usize::from(victim)) {
            a.battle.render_flag = 0;
            a.battle.render_color = 0;
        }
    }

    /// The engine slots of the seated monsters with HP left.
    /// PROT 0928's (Palma) monster-row visibility. Arm 1 shows every living
    /// monster of seats 3..6 on each of its passes, held or not
    /// (`+0x21C = 0`, `+0x04 = 0x20080200`, `0x801F6CFC..0x801F6D48`, ahead
    /// of the side-band token gate at `0x801F6D58`), so the row the band hid
    /// stands in the opening framing; arm 4's pass hides it again
    /// (`+0x21C = 0xFF`, `+0x04 = 0`, `0x801F73BC..0x801F73FC`) as the
    /// creature is seated. The engine's shown word is its resting `0` under a
    /// lowered flag. The row's later choreography, seat by seat over seats
    /// 3..6 (a seat with HP left is "living"):
    ///
    /// - arm 0 parks each living seat's ground trio and facing
    ///   (`0x801FA0A8` / `0x801FA0C8`) and pulls its ground pair to an
    ///   eighth (`sra 3` into `+0x34` / `+0x38`, `0x801F6C44..0x801F6CF0`);
    /// - arm `0xE`'s pass re-shows the living row on the far plane
    ///   (`+0x38 = -0x4000`, facing `0`) and bumps a dead seat's zero facing
    ///   to `1`, the mark the later arms skip it by (`0x801F7F3C..0x801F7FD0`);
    /// - arm `0xF` holds the living row on that plane every pass; once its
    ///   word is under `0x1C1` it hides every seat (`0x801F81CC..0x801F81E4`,
    ///   after the pre-hit), and its pass lights each unmarked seat, the
    ///   colour word `0x3C0` under the soft-red state `7`, standing at
    ///   `+0x38 = |+0x34| / 4 - 0x400`, with the cue cursor `+0x1F6` run off
    ///   the end of its eight-slot track, so the flinch the arm parked plays
    ///   out silent (`0x801F82CC..0x801F8370`);
    /// - arm `0x10` keeps that stand every pass (`0x801F8418..0x801F8484`);
    /// - arm `0x11`'s pass hides the living row (`0x801F8694..0x801F86DC`);
    /// - the hit arm `0x13`'s pass puts each living seat's parked trio and
    ///   facing back, turns it from the creature (`FUN_80019B28` + `0x800`)
    ///   and shows it lit `0x3C0` (`0x801F892C..0x801F8B40`).
    ///
    /// Not carried: the render node's colour word `+0x74 = 0x00F00000` and
    /// the node `+0x26` / `+0x68` halfwords arms `0xE` / `0x10` zero and set.
    pub(in crate::world::battle::cast_band) fn apply_palma_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
    ) {
        use legaia_engine_battle_vm::battle_formulas::STATE_RED_SOFT;
        use legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
        /// The lit colour word arms `0xF` and `0x13` store (`li 0x3C0`).
        const LIT: u32 = 0x3C0;
        /// The far plane arms `0xE` / `0xF` stand the row on.
        const FAR_Z: i16 = -0x4000;
        let pc = usize::from(self.party.party_count);
        let seats: Vec<usize> = (pc..pc + 4)
            .filter(|&s| {
                self.actors
                    .get(s)
                    .is_some_and(|a| a.active && a.battle_monster_id.is_some())
            })
            .collect();
        let living = |w: &Self, s: usize| w.actors.get(s).is_some_and(|a| a.battle.hp != 0);
        let stand = |x: i16| ((i32::from(x).abs() >> 2) - 0x400) as i16;
        // Arms `0xB` and `0xC` tip the creature nose-up every pass, held or
        // not: pitch `+0x44 += (scalar * step) >> 1` (`0x801F7C54..0x801F7C60`,
        // `0x801F7D00..0x801F7D0C`); no later arm sets it back.
        if matches!(phase_in, 0xB | 0xC)
            && let Some(a) = self
                .casting
                .summon_actor_slot
                .and_then(|c| self.actors.get_mut(usize::from(c)))
        {
            let d = (legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK >> 1) as u16;
            a.battle.render_pitch = a.battle.render_pitch.wrapping_add(d) & 0xFFF;
        }
        match (phase_in, passed) {
            (0, true) => {
                self.casting.module_saved_poses.clear();
                for &s in &seats {
                    if !living(self, s) {
                        continue;
                    }
                    let a = &mut self.actors[s];
                    let m = &mut a.move_state;
                    self.casting.module_saved_poses.push((
                        s,
                        [m.world_x, m.world_y, m.world_z],
                        a.battle.facing_angle,
                    ));
                    m.world_x /= 8;
                    m.world_z /= 8;
                }
            }
            // Arm 5's pass lowers the creature's tint state
            // (`sb zero,0x21C(s4)` at `0x801F75DC`): the red word arm 4 held
            // eases back to neutral as the creature rises out of the planet.
            // The rate drops to the `4` the dispatch left in `a3`
            // (`li a3,0x4` at `0x801F6A7C`, `sb a3,0x21D(s4)` at `0x801F75E8`).
            (5, true) => {
                if let Some(a) = self
                    .casting
                    .summon_actor_slot
                    .and_then(|s| self.actors.get_mut(usize::from(s)))
                {
                    a.battle.render_flag = 0;
                    a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(4);
                    // `sb zero,0x21B` / `sh zero,0x176` (`0x801F75E0..0x801F75E4`):
                    // the window arm 4's restage seeded on clip 0 is released,
                    // so the clip runs on to its natural end and commits the
                    // clip 1 this pass stages.
                    a.battle.hit_count_bound = 0;
                    if let Some(p) = a.battle_animation.as_mut() {
                        p.release_loop_window();
                    }
                }
            }
            (1, _) | (4, true) => {
                let hidden = phase_in == 4;
                for slot in self.living_monster_slots().into_iter().take(4) {
                    if let Some(a) = self.actors.get_mut(slot) {
                        a.battle.render_flag = if hidden { RENDER_FLAG_HIDDEN } else { 0 };
                        a.battle.render_color = 0;
                    }
                }
            }
            (0xE, true) => {
                for &s in &seats {
                    let live = living(self, s);
                    let a = &mut self.actors[s];
                    if live {
                        a.battle.render_color = 0;
                        a.battle.render_flag = 0;
                        a.move_state.world_z = FAR_Z;
                        a.battle.facing_angle = 0;
                    } else if a.battle.facing_angle == 0 {
                        a.battle.facing_angle = 1;
                    }
                }
            }
            (0xF, _) => {
                let swept = self.casting.module_cam.countdown.0 < 0x1C1;
                for &s in &seats {
                    let live = living(self, s);
                    let a = &mut self.actors[s];
                    if live {
                        a.move_state.world_z = FAR_Z;
                    }
                    if swept {
                        a.battle.render_color = 0;
                        a.battle.render_flag = RENDER_FLAG_HIDDEN;
                    }
                    if passed && a.battle.facing_angle == 0 {
                        a.move_state.world_z = stand(a.move_state.world_x);
                        a.battle.render_color = LIT;
                        a.battle.render_flag = STATE_RED_SOFT;
                        a.battle_anim_cue_cursor =
                            legaia_engine_battle::anim_cue::ANIM_CUE_TRACK_LEN;
                    }
                }
            }
            (0x10, _) => {
                for &s in &seats {
                    let a = &mut self.actors[s];
                    if a.battle.facing_angle == 0 {
                        a.move_state.world_z = stand(a.move_state.world_x);
                        // ... and pins its pose: node cursor `+0x68 = 0x10`
                        // every pass (`0x801F8418`, `0x801F8474`).
                        if let Some(p) = a.battle_animation.as_mut() {
                            p.seek_sixteenths(0x10);
                        }
                    }
                }
            }
            (0x11, true) => {
                for &s in &seats {
                    if living(self, s) {
                        let a = &mut self.actors[s];
                        a.battle.render_color = 0;
                        a.battle.render_flag = RENDER_FLAG_HIDDEN;
                    }
                }
                // The same exit puts the creature into the defeat fade
                // (`sb v0,0x21C(s4)` / `sb v0,0x225(s4)` with `v0 = 2`,
                // `0x801F7CA4..0x801F7CB0`): it fades out over arm `0x12`'s
                // `96 * scalar`, whose exit zeroes its word (`sw zero,0x4(s4)`
                // at `0x801F7D70`).
                if let Some(a) = self
                    .casting
                    .summon_actor_slot
                    .and_then(|s| self.actors.get_mut(usize::from(s)))
                {
                    a.battle.render_flag = 2;
                    a.battle.capture_state = 2;
                }
            }
            (0x12, true) => {
                if let Some(a) = self
                    .casting
                    .summon_actor_slot
                    .and_then(|s| self.actors.get_mut(usize::from(s)))
                {
                    a.battle.render_color = 0;
                }
            }
            (0x13, true) => {
                let creature = self
                    .casting
                    .module_cam
                    .creature
                    .or(self.casting.module_cam.creature_live);
                for &s in &seats {
                    if !living(self, s) {
                        continue;
                    }
                    let saved = self
                        .casting
                        .module_saved_poses
                        .iter()
                        .find(|p| p.0 == s)
                        .copied();
                    let a = &mut self.actors[s];
                    // The rate arm 0xF parked at `0` (`0x801F8AD8`).
                    a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(2);
                    if let Some((_, pos, facing)) = saved {
                        let m = &mut a.move_state;
                        [m.world_x, m.world_y, m.world_z] = pos;
                        a.battle.facing_angle = facing;
                    }
                    if let Some(c) = creature {
                        let m = &a.move_state;
                        let h = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                            c.z, c.x, m.world_z, m.world_x,
                        );
                        a.battle.facing_angle = h.wrapping_add(0x800) & 0xFFF;
                    }
                    a.battle.render_color = LIT;
                    a.battle.render_flag = 0;
                }
            }
            _ => {}
        }
    }

    /// PROT 0923's (Gilium) creature flight through its portal. Arm 3's
    /// pass seats the creature at `(0, -0x100, -0x800)`, facing `0`
    /// (`0x801F6DB8..0x801F6DD4`, after `FUN_801F19EC`); each pass of arm 4
    /// that still finds its word positive flies it `24` a frame delta along
    /// `+0x38` and `2` down `+0x36` (`0x801F6E30..0x801F6E58`), and arm 4's
    /// pass stands it at `+0x38 = 0x400` (`0x801F718C`). A retail cast in arm
    /// 4 holds the creature at `z = -1184` on its way in and at `0x400` from
    /// arm 5 on; without the flight the engine left it on arm 3's seat, behind
    /// the camera, and drew no creature from the portal on.
    ///
    /// Arm 3 also zeroes the creature's colour word, which the tint pass's
    /// decay arm eases back to neutral (the fade-in), and its render-node
    /// scale `+0x72` (`sh zero,0x72(v1)` at `0x801F6DDC`), which each
    /// draining pass of arm 4 grows by `32 * delta`
    /// (`0x801F6E00..0x801F6E1C`, ahead of the countdown test) and arm 4's
    /// exit stands at `0x1000` (`0x801F71A4`): the creature swells out of
    /// the portal from nothing, the draw `FUN_80048A08` scaling its matrix
    /// by the word.
    pub(in crate::world::battle::cast_band) fn apply_gilium_writes(
        &mut self,
        phase_in: u8,
        passed: bool,
    ) {
        let d = legaia_engine_cast::cast_module_camera::DELTA_PER_TICK as i16;
        let Some(a) = self
            .casting
            .summon_actor_slot
            .and_then(|s| self.actors.get_mut(usize::from(s)))
        else {
            return;
        };
        let m = &mut a.move_state;
        match (phase_in, passed) {
            (3, true) => {
                [m.world_x, m.world_y, m.world_z] = [0, -0x100, -0x800];
                a.battle.facing_angle = 0;
                // `sw zero,0x4(s6)` at `0x801F6DD8`: the creature's colour
                // word, which the tint pass's decay arm (`+0x21C == 0`)
                // eases back up to neutral `8` lanes a vsync - its fade-in
                // through the portal.
                a.battle.render_color = 0;
                a.battle.render_scale = Some(0);
            }
            (4, false) => {
                m.world_z = m.world_z.wrapping_add(24 * d);
                m.world_y = m.world_y.wrapping_add(2 * d);
                if let Some(s) = a.battle.render_scale.as_mut() {
                    *s = s.wrapping_add(32 * d as u16);
                }
            }
            // Arm `0xB`'s pass hides the creature for the rest of the cast
            // (`sw zero,0x4(s6)` / `sb 0xFF,0x21C(s6)` at
            // `0x801F7D38..0x801F7D40`; no later arm stores either).
            (0xB, true) => {
                a.battle.render_color = 0;
                a.battle.render_flag =
                    legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN;
            }
            (4, true) => {
                // The pass that drains the countdown still flies before it
                // falls into the exit (`bgtz a1` at `0x801F6E6C` falls
                // through to `0x801F6E74`), which stands Z but not the
                // height: the creature lands at `+0x36 = 0`.
                m.world_y = m.world_y.wrapping_add(2 * d);
                m.world_z = 0x400;
                a.battle.render_scale = Some(0x1000);
            }
            _ => {}
        }
    }

    pub fn living_monster_slots(&self) -> Vec<usize> {
        let pc = usize::from(self.party.party_count);
        (pc..self.actors.len().min(pc + 5))
            .filter(|&s| {
                self.actors
                    .get(s)
                    .is_some_and(|a| a.active && a.battle_monster_id.is_some() && a.battle.hp != 0)
            })
            .collect()
    }

    /// PROT 0916's light bands for this frame: the spread its arms
    /// `0x11..=0x13` drew them at on the module's last pass, which
    /// `legaia_render_kernels::cast_aluru` builds for both hosts. `None`
    /// outside a live Aluru cast or on a pass that drew none.
    pub fn aluru_band_draw(&self) -> Option<i16> {
        let st = self.casting.summon_stager.as_ref()?;
        (self.mode == legaia_engine_mode::mode::SceneMode::Battle
            && self.cast_module_for(st.spell_id)
                == Some(legaia_engine_cast::cast_module_camera::ALURU_ENTRY))
        .then_some(self.casting.module_cam.band_spread)
        .flatten()
    }
}

#[cfg(test)]
mod palma_tests {
    use super::*;

    /// PROT 0928 arm `0xF`'s pass runs each unmarked seat's cue cursor off its
    /// track (`sb a1,0x1F6(v0)`, `a1 = 8`, `0x801F82D8` / `0x801F8360`), so the
    /// flinch it parked plays out silent; a marked seat keeps its cursor.
    #[test]
    fn arm_f_pass_silences_the_parked_flinch() {
        let mut w = World::new();
        w.party.party_count = 1;
        for (slot, facing) in [(1, 0), (2, 1)] {
            w.actors[slot].active = true;
            w.actors[slot].battle_monster_id = Some(1);
            w.actors[slot].battle.hp = 10;
            w.actors[slot].battle.facing_angle = facing;
        }
        w.casting.module_cam.countdown.0 = 0x1000;
        w.apply_palma_writes(0xF, false);
        assert_eq!(
            w.actors[1].battle_anim_cue_cursor, 0,
            "only the pass stores it"
        );
        w.apply_palma_writes(0xF, true);
        assert_eq!(
            w.actors[1].battle_anim_cue_cursor,
            legaia_engine_battle::anim_cue::ANIM_CUE_TRACK_LEN
        );
        assert_eq!(w.actors[2].battle_anim_cue_cursor, 0);
    }
}
