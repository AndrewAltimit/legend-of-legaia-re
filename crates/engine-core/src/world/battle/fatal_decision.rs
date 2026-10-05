//! PROT 0954 (Fatal Decision) on the world: the battle side of
//! [`legaia_engine_vm::cast_fatal_decision`] - the seats it reads, the
//! records it seats and moves, the stop prompt and the outcome banner, and
//! the sixteen outcomes on the victim.
//!
//! The body owns its records: [`World::spawn_cast_module_fx`] stages none of
//! them at the pager seam, and each is seated on the pass its arm spawns it,
//! labelled so the arms that write into a spawned part (the wheel icons'
//! `+0x14` / `+0x16`, the landed icon's `+0x72`, the `flags |= 8` retires)
//! reach the same part.

use super::cast_band::{BATTLE_TABLE_SLOTS, CastModuleCodeRun};
use super::*;
use vm::cast_fatal_decision as fd;

/// The label of the backdrop part.
const TAG_BACKDROP: u32 = 0x0954_0000;
/// The label of wheel icon `i` is this plus `i`.
const TAG_ICON: u32 = 0x0954_0100;
/// The label of the three landing parts.
const TAG_LANDING: u32 = 0x0954_0200;

/// The message-bar element the stop prompt and the steal / gold messages use
/// (`FUN_801D8DE8(0x5B, ..)`).
const MESSAGE_BAR_ELEMENT: u8 = 0x5B;

/// Arm 7's flash (`FUN_80024E80(0x801D9070, 1)`): kind `1`, a `0x20`-frame
/// ramp from white to black, no hold.
fn landing_flash() -> crate::fade::FadeTemplate {
    crate::fade::FadeTemplate {
        kind: 1,
        duration: 0x20,
        start_rgb: [0xFF; 3],
        end_rgb: [0; 3],
        mode: [0, 0, 1],
    }
}

struct Host<'a> {
    world: &'a mut World,
    caster: u8,
    victim: u8,
}

impl fd::FatalDecisionHost for Host<'_> {
    fn rand(&mut self) -> u32 {
        self.world.next_rand()
    }
    fn apply_outcome(&mut self, outcome: fd::FatalOutcome) -> bool {
        self.world
            .apply_fatal_outcome(self.caster, self.victim, outcome)
    }
}

impl World {
    /// One pass of PROT 0954's body, from the capture band's module seam
    /// ([`Self::run_cast_module_code`]).
    pub(in crate::world) fn run_fatal_decision(&mut self, entry: u32) -> CastModuleCodeRun {
        let caster = self.battle_ctx.active_actor;
        let victim = self
            .actors
            .get(usize::from(caster))
            .map(|a| a.battle.active_target)
            .unwrap_or(0);
        let mut ctx = self.cast_module_ctx();
        let mut st = self.casting.fatal_decision.unwrap_or_default();
        let view = self.fatal_decision_view(caster, victim);
        let pass = {
            let mut host = Host {
                world: self,
                caster,
                victim,
            };
            fd::fatal_decision_tick(&mut st, &mut ctx, &view, &mut host)
        };
        self.casting.fatal_decision = Some(st);
        self.apply_fatal_pass(&pass, caster, victim, ctx.party_count);
        self.casting.module_phase = ctx.phase;
        self.casting.module_ctx_278 = ctx.ctx_278;
        CastModuleCodeRun {
            prot_entry: entry,
            phase: ctx.phase,
            ctx_278: ctx.ctx_278,
            busy: !pass.done,
            tick_ported: true,
            camera_shot: pass.shot,
            ..Default::default()
        }
    }

    /// What the body reads off the battle this tick.
    fn fatal_decision_view(&self, caster: u8, victim: u8) -> fd::FatalDecisionView {
        let seats = self.module_cam_seats(caster, victim);
        let v = self.actors.get(usize::from(victim));
        fd::FatalDecisionView {
            caster: seats.caster,
            victim: seats.victim,
            // `+0x3E` is the display height - the live `+0x36` raised by the
            // pose centroid - not the live Y.
            victim_seat_y: self
                .battle_display_trio(usize::from(victim))
                .map_or(seats.victim.y, |d| d[1] as i16),
            victim_radius: self.battle_body_radius(victim),
            depth: seats.depth_raw as i16,
            caster_monster: seats.caster_monster,
            victim_is_party: victim < self.party.party_count,
            confirm: self.input.just_pressed(crate::input::PadButton::Cross),
            victim_status: self.raw_status_word(victim),
            victim_hp: v.map_or(0, |a| a.battle.hp),
            victim_playing: v.map_or(0, |a| a.battle.current_anim),
            victim_tint: v.map_or(0, |a| a.battle.render_color),
        }
    }

    /// `*(actor[+0x22C]) + 0x58`: [`PARTY_BODY_RADIUS`] on a party seat, the
    /// record size class `<< 5` on a monster's.
    fn battle_body_radius(&self, slot: u8) -> i16 {
        match self
            .actors
            .get(usize::from(slot))
            .and_then(|a| a.battle_monster_id)
        {
            Some(id) => self
                .tables
                .monster_catalog
                .get(id)
                .map_or(0, |d| i16::from(d.size_class) << 5),
            None => PARTY_BODY_RADIUS,
        }
    }

    /// A string out of PROT 0954's image, as the display string the HUD
    /// lays out: printable ASCII as itself, any other byte (the `0xCE`
    /// button escape) parked at `U+E000 + byte` so the font gets the byte
    /// back.
    fn fatal_decision_text(&self, va: u32) -> Option<String> {
        let module = self
            .casting
            .effect_pool
            .as_ref()?
            .module(fd::FATAL_DECISION_ENTRY)?;
        let bytes = fd::module_string(&module.bytes, va)?;
        Some(
            bytes
                .iter()
                .map(|&b| {
                    if (0x20..=0x7E).contains(&b) {
                        char::from(b)
                    } else {
                        char::from_u32(0xE000 + u32::from(b)).unwrap_or('?')
                    }
                })
                .collect(),
        )
    }

    #[cfg(test)]
    pub(in crate::world) fn fatal_decision_text_for_test(&self, va: u32) -> Option<String> {
        self.fatal_decision_text(va)
    }

    /// Put `text` in the message bar under the acting seat.
    fn raise_fatal_message(&mut self, text: Option<String>) {
        if let Some(text) = text {
            self.battle.steal_caption = Some(crate::battle_steal::StealCaption {
                text,
                owner: self.battle_ctx.active_actor,
            });
        }
    }

    /// Carry one pass's stores out onto the battle.
    fn apply_fatal_pass(
        &mut self,
        p: &fd::FatalDecisionPass,
        caster: u8,
        victim: u8,
        party_count: u8,
    ) {
        if p.hide_all {
            // Every party seat, and every monster seat still standing
            // (`0x801F6C98..0x801F6D40`).
            let monsters = usize::from(self.party.party_count);
            for (i, a) in self.actors.iter_mut().enumerate().take(BATTLE_TABLE_SLOTS) {
                let party = i < usize::from(party_count);
                if party || (i >= monsters && a.battle.hp != 0 && a.battle_monster_id.is_some()) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = fd::HIDDEN_RENDER_FLAG;
                }
            }
        }
        if p.halve_rates || p.restore_rates {
            for slot in [caster, victim] {
                if let Some(a) = self.actors.get_mut(usize::from(slot)) {
                    let r = a.battle.anim_rate.get();
                    a.battle.anim_rate =
                        vm::battle_anim_rate::AnimRate(if p.halve_rates { r >> 1 } else { r << 1 });
                }
            }
        }
        for (slot, render) in [(caster, p.caster_render), (victim, p.victim_render)] {
            if let (Some(r), Some(a)) = (render, self.actors.get_mut(usize::from(slot))) {
                a.battle.render_color = r.tint;
                a.battle.render_flag = r.flag;
            }
        }
        for (slot, facing) in [(caster, p.caster_facing), (victim, p.victim_facing)] {
            if let (Some(f), Some(a)) = (facing, self.actors.get_mut(usize::from(slot))) {
                a.battle.facing_angle = f;
            }
        }
        if !p.spawns.is_empty() {
            self.seat_fatal_records(&p.spawns);
        }
        self.drive_fatal_parts(p);
        if p.flash {
            crate::fade::spawn_fade(&mut self.presentation.fade, &landing_flash(), 1);
        }
        if let Some(id) = p.cue {
            // The open cue is the module's head cue, which the arming seam
            // already raised (`emit_cast_module_voice`).
            if id != fd::OPEN_CUE {
                self.emit_battle_xa_cue(id);
            }
        }
        match p.prompt {
            Some(true) => {
                let text = self.fatal_decision_text(fd::PROMPT_TEXT);
                self.raise_fatal_message(text);
            }
            Some(false) => self.battle.steal_caption = None,
            None => {}
        }
        if let Some(o) = p.banner {
            self.casting.fatal_banner = self.fatal_decision_text(o.name_va());
        }
        if p.react {
            self.fatal_victim_reacts(victim);
        }
    }

    /// Seat the records one pass spawned, each at the origin with no
    /// rotation (every one is camera-relative), labelled for the arms that
    /// write into it later.
    fn seat_fatal_records(&mut self, spawns: &[fd::FatalSpawn]) {
        const LINK_BASE: u32 = legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let Some(module) = self
            .casting
            .effect_pool
            .as_ref()
            .and_then(|p| p.module(fd::FATAL_DECISION_ENTRY))
            .cloned()
        else {
            return;
        };
        let mut landing = 0;
        for spawn in spawns {
            let Some(off) = spawn.record.checked_sub(LINK_BASE).map(|o| o as usize) else {
                continue;
            };
            let mut offs: Vec<usize> = module.parts.iter().map(|p| p.record_off).collect();
            offs.push(off);
            let picked: Vec<_> =
                legaia_asset::summon_overlay::parse_records_at(&module.bytes, &offs)
                    .into_iter()
                    .filter(|p| p.record_off == off)
                    .collect();
            if picked.is_empty() {
                continue;
            }
            let tag = match spawn.wheel_slot {
                Some(i) => TAG_ICON + i as u32,
                None if spawn.record == fd::OPEN_RECORD => TAG_BACKDROP,
                None => {
                    landing += 1;
                    TAG_LANDING + landing
                }
            };
            let scene = self.casting.active_summon.get_or_insert_with(|| {
                crate::summon::SummonScene::spawn_parts(
                    &[],
                    &module.bytes,
                    crate::scene::EFFECT_MODEL_LIBRARY_BASE,
                    [0; 3],
                )
            });
            let first = scene.parts.len();
            scene.push_parts(&picked, &module.bytes, [0; 3], [0; 3]);
            for part in &mut scene.parts[first..] {
                part.tag = Some(tag);
                part.state.field_72 = spawn.scale;
            }
        }
    }

    /// The writes the arms make into parts already spawned: the icons'
    /// placement and the landed icon's growth, and the two retires.
    fn drive_fatal_parts(&mut self, p: &fd::FatalDecisionPass) {
        let Some(scene) = self.casting.active_summon.as_mut() else {
            return;
        };
        for part in &mut scene.parts {
            let Some(tag) = part.tag else {
                continue;
            };
            if tag == TAG_BACKDROP && p.retire_backdrop {
                part.finished = true;
            }
            let Some(i) = tag.checked_sub(TAG_ICON).filter(|&i| i < 8) else {
                continue;
            };
            let i = i as usize;
            if p.retire_wheel {
                part.finished = true;
                continue;
            }
            if let Some([x, y]) = p.place[i] {
                part.state.world_x = x;
                part.state.world_y = y;
            }
            if let Some((g, d)) = p.grow
                && g == i
            {
                part.state.field_72 = part.state.field_72.wrapping_add(d as u16);
                part.state.world_y = part.state.world_y.wrapping_add(d);
            }
        }
    }

    /// Arm 11 (`0x801F8390..0x801F8414`), past the Stone test the body made:
    /// the Numb bit cleared (`andi 0xFBFF`), the reaction clip staged - the
    /// knockdown `+0x1F1` when `+0x1F2` is set or the victim is down, else
    /// the flinch `+0x1EF`, or `+0x1F0` when that is zero.
    fn fatal_victim_reacts(&mut self, victim: u8) {
        use vm::status_effects::StatusKind;
        self.battle.status_effects.cure(victim, StatusKind::Numb);
        let down = match self.actors.get_mut(usize::from(victim)) {
            Some(a) => {
                a.battle.field_flags &= !vm::status_effects::display_flags::NUMB;
                a.battle.hp == 0
            }
            None => return,
        };
        let Some(map) = self.battle_reaction_map(usize::from(victim)) else {
            return;
        };
        let entry = if map[3] != 0 || down {
            map[2]
        } else if map[0] != 0 {
            map[0]
        } else {
            map[1]
        };
        self.commit_battle_reaction_entry(usize::from(victim), entry);
    }

    /// One outcome on the victim (arm 10's jump table `0x801F6A18`).
    /// Returns whether the reaction arm runs.
    pub(in crate::world) fn apply_fatal_outcome(
        &mut self,
        caster: u8,
        victim: u8,
        outcome: fd::FatalOutcome,
    ) -> bool {
        use fd::FatalOutcome;
        let _ = caster;
        match outcome {
            FatalOutcome::StealItem => return self.fatal_steal(),
            FatalOutcome::GoldTithe => {
                // `0x801F8330`: the gold word `0x8008459C` loses a tenth, and
                // the message bar says so.
                self.party.money = fd::gold_tithe(self.party.money);
                let text = self.fatal_decision_text(fd::GOLD_TEXT);
                self.raise_fatal_message(text);
                self.battle_ctx.message_id = MESSAGE_BAR_ELEMENT;
                return true;
            }
            _ => {}
        }
        let Some(a) = self.actors.get(usize::from(victim)) else {
            return true;
        };
        let (udf, ldf) = self
            .battle
            .defense_split
            .get(usize::from(victim))
            .copied()
            .flatten()
            .unwrap_or((0, 0));
        let mut v = fd::FatalVictim {
            hp: a.battle.hp,
            max_hp: a.battle.max_hp,
            mp: a.battle.mp,
            // The battle actor carries current MP alone; the ceiling every
            // battle surface reads is the per-seat `character_max_mp`.
            max_mp: self
                .tables
                .character_max_mp
                .get(usize::from(victim))
                .copied()
                .unwrap_or(0),
            hp_bar_delta: a.battle.hp_bar_pending,
            mp_readout: a.battle.last_mp_cost,
            status: self.raw_status_word(victim),
            atk: a.battle.atk_working,
            atk_base: a.battle.atk_base,
            udf,
            udf_base: udf,
            ldf,
            ldf_base: ldf,
        };
        let step = fd::apply_outcome_to(&mut v, outcome);
        if let Some(a) = self.actors.get_mut(usize::from(victim)) {
            a.battle.hp = v.hp;
            a.battle.max_hp = v.max_hp;
            a.battle.mp = v.mp;
            a.battle.hp_bar_pending = v.hp_bar_delta;
            a.battle.last_mp_cost = v.mp_readout;
            a.battle.atk_working = v.atk;
            a.battle.atk_base = v.atk_base;
            if step.status_cleared {
                a.battle.field_flags = 0;
            }
        }
        if victim < self.party.party_count {
            self.set_character_max_mp(victim, v.max_mp);
        }
        if let Some(s) = self.battle.defense_split.get_mut(usize::from(victim))
            && s.is_some()
        {
            *s = Some((v.udf, v.ldf));
        }
        if step.status_cleared {
            self.battle.status_effects.cure_all(victim);
        }
        self.install_fatal_status(victim, step.status_set);
        if step.cancels_action {
            self.stone_cancels_queued_action(victim);
        }
        if let Some((amount, restores)) = step.popup {
            self.battle.hit_fx.push(crate::battle_events::BattleHitFx {
                target_slot: victim,
                amount,
                is_heal: restores,
                is_crit: false,
            });
        }
        step.reaction
    }

    /// The status bits an outcome ORs into `+0x16E`, through the typed
    /// tracker the engine's consumers read (the word is its packed form).
    fn install_fatal_status(&mut self, victim: u8, bits: u16) {
        use vm::status_effects::{StatusKind, display_flags as f};
        for (bit, kind) in [
            (f::VENOM, StatusKind::Venom),
            (f::TOXIC, StatusKind::Toxic),
            (f::STONE, StatusKind::Stone),
            (f::NUMB, StatusKind::Numb),
            (f::CURSE, StatusKind::Curse),
        ] {
            if bits & bit != 0 {
                self.battle.status_effects.apply(victim, kind);
            }
        }
        let limbs: Vec<u8> = (0..3u8)
            .filter(|&l| bits & f::ROT_LIMBS[usize::from(l)] != 0)
            .collect();
        if !limbs.is_empty() {
            self.battle.status_effects.apply(victim, StatusKind::Rot);
            for limb in limbs {
                self.battle.status_effects.set_rot_limb(victim, limb);
            }
        }
    }

    /// The steal outcome (`0x801F8164..0x801F8314`): one `rand()` gate, then
    /// PROT 0941's bag draw over the whole 256-slot array with no floor (up
    /// to `0x400` draws, id / count / price all non-zero). A hit destroys the
    /// item - the thief's cell `0x801C8FE0 + (seat - 3) * 4` is written and
    /// cleared again in the same arm, so a slain caster hands nothing back -
    /// and names it in the message bar; a miss says "no effect" and skips the
    /// reaction arm.
    fn fatal_steal(&mut self) -> bool {
        let gate = self.next_rand();
        let item = if gate == 0 {
            None
        } else {
            let bag: Vec<(u8, u8)> = self.party.inventory.slots().to_vec();
            let mut priced = [true; 256];
            if let Some(data) = self.shops.item_shop_data.as_ref() {
                for (id, cell) in priced.iter_mut().enumerate() {
                    *cell = data.price(id as u8) != 0;
                }
            }
            vm::cast_arm_ticks::steal_pick_bag_slot(
                &bag,
                None,
                || self.next_rand(),
                |id| priced[usize::from(id)],
            )
            .and_then(|s| bag.get(usize::from(s)).map(|&(id, _)| id))
        };
        self.battle_ctx.message_id = MESSAGE_BAR_ELEMENT;
        match item {
            Some(id) => {
                let name = self
                    .menu
                    .text
                    .as_ref()
                    .and_then(|t| t.item_name(id))
                    .map(str::to_string)
                    .or_else(|| self.tables.item_catalog.get(id).map(|e| e.name.to_string()))
                    .unwrap_or_default();
                let suffix = self
                    .fatal_decision_text(fd::STOLEN_SUFFIX_TEXT)
                    .unwrap_or_default();
                let _ = self.take_one_from_bag(id);
                self.raise_fatal_message(Some(format!("{name}{suffix}")));
                true
            }
            None => {
                let text = self.fatal_decision_text(fd::NO_EFFECT_TEXT);
                self.raise_fatal_message(text);
                false
            }
        }
    }
}
