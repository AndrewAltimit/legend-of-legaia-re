//! The stores PROT 0932 (Meta), 0933 (Terra) and 0934 (Ozma) make on other
//! seats beside their directors: the monster row's seat, colour word and
//! render flag, and the creature's animation rate and tint.
//!
//! See `ghidra/scripts/funcs/overlay_summon_meta_0932_801f6a34.txt`,
//! `overlay_summon_terra_0933_801f6a30.txt` and
//! `overlay_summon_ozma_0934_801f6a40.txt`.

use super::*;
use legaia_engine_battle_vm::battle_anim_rate::AnimRate;

/// The neutral colour word `0x20080200`: each lane at `0x80`.
const WORD_NEUTRAL: u32 = 0x2008_0200;
/// The render flag that parks the tint machine on its word
/// (`FUN_80050120`'s default arm).
const FLAG_PARKED: u8 = 0xFF;
/// The vsyncs one retail frame takes through PROT 0932's arms: its capture
/// reads a frame step of `4` on most of them.
const META_TAIL_TICKS: u32 = 4;
/// The colour word PROT 0934's arm `0x18` keeps on the row.
const OZMA_HIT_WORD: u32 = 0x3FF0_03FF;
/// The word PROT 0919's arm 2 gives the creature.
const SPOON_SEAT_WORD: u32 = 0x000F_FFFF;
/// The top of the clip row PROT 0919's arm 2 opens (`0x8008401E`).
const SPOON_CLIP_START: i16 = -0x280;
/// The engine's stand-in for a retail `+0x04 = 0` under an idle tint arm:
/// retail's first ease takes it off zero, and the engine's zero is its resting
/// neutral.
const NEAR_BLACK_WORD: u32 = 0x0010_0401;
/// Arm `0x17`'s store into the threshold `0x801FA408` (`0x801F8B58`).
const OZMA_LADDER_FIRST: i32 = 0x60;
/// Each hit's fall of that threshold (`0x801F90E8`).
const OZMA_LADDER_STEP: i32 = 0x20;
/// The fade lengths of the ladder's hits and of its closing hit
/// (`0x801F8FF0`, `0x801F9050`).
const OZMA_HIT_FLASH_FRAMES: i16 = 0x10;
const OZMA_LAST_FLASH_FRAMES: i16 = 0xC0;
/// The fade call in arm `0x18`'s hit block.
const OZMA_LADDER_FADE_SITE: u32 = 0x801F_907C;

/// PROT 0931's arm-6 ring records, the `+8` / `+0xC` / `+0x10` handles of the
/// block at `0x801F964C` (seated by `FUN_80050ED4` at `0x801F6D88..0x801F6DC8`).
const JEDO_ARM7_RING_RECORDS: [u32; 3] = [0x801F_8B68, 0x801F_8BC0, 0x801F_8C18];

impl World {
    /// `(engine slot, row index)` of each living monster, in seat order.
    pub(super) fn ra_seru_row(&self, monster_count: u8) -> Vec<(usize, usize)> {
        let map = self.cast_seat_map();
        (0..usize::from(monster_count))
            .filter_map(|i| map.engine(3 + i as u8).map(|e| (usize::from(e), i)))
            .filter(|&(e, _)| self.actors.get(e).is_some_and(|a| a.battle.hp != 0))
            .collect()
    }

    /// The row store `+0x04` / `+0x21C` on every living monster.
    fn ra_seru_row_word(&mut self, monster_count: u8, word: u32, flag: u8) {
        for (e, _) in self.ra_seru_row(monster_count) {
            if let Some(a) = self.actors.get_mut(e) {
                a.battle.render_color = word;
                a.battle.render_flag = flag;
            }
        }
    }

    /// Arm 0's re-seat: each living monster's trio and facing parked, then
    /// `seat` applied to its alternate-family seat `0x80077628 + (count + 3)
    /// * 0x20`, facing `0x800`, at `scale` when the arm halves it.
    fn ra_seru_row_seat(
        &mut self,
        monster_count: u8,
        seat: impl Fn(i16, i16) -> (i16, i16),
        scale: Option<u16>,
    ) {
        self.casting.module_saved_poses.clear();
        for (e, i) in self.ra_seru_row(monster_count) {
            let s = legaia_engine_battle::battle_seats::monster_seat(monster_count, i, true);
            let Some(a) = self.actors.get_mut(e) else {
                continue;
            };
            let m = &mut a.move_state;
            self.casting.module_saved_poses.push((
                e,
                [m.world_x, m.world_y, m.world_z],
                a.battle.facing_angle,
            ));
            (m.world_x, m.world_z) = seat(s.x, s.z);
            a.battle.facing_angle = 0x800;
            if scale.is_some() {
                a.battle.render_scale = scale;
            }
        }
    }

    /// The terminal arm's restore of what [`Self::ra_seru_row_seat`] parked,
    /// for the monsters still alive.
    fn ra_seru_row_restore(&mut self, monster_count: u8, rescale: bool) {
        let row = self.ra_seru_row(monster_count);
        for (e, pos, facing) in self.casting.module_saved_poses.clone() {
            if !row.iter().any(|&(s, _)| s == e) {
                continue;
            }
            if let Some(a) = self.actors.get_mut(e) {
                [
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                ] = pos;
                a.battle.facing_angle = facing;
                if rescale {
                    a.battle.render_scale = None;
                }
            }
        }
    }

    /// PROT 0920 (Slippery)'s creature comes up out of the floor: arm 2's exit
    /// seats it at `y = -(scalar << 8)` (`0x801F6D18..0x801F6D28`), and arm 3
    /// lifts it `4 p` a pass until its word runs out, then stands it on the
    /// floor (`0x801F6D94..0x801F6DE0`). Arm 8's exit hides the creature and
    /// shows every living monster for the victim's shot (`0x801F7518..0x801F7550`).
    pub(in crate::world::battle::cast_band) fn apply_slippery_writes(
        &mut self,
        phase: u8,
        passed: bool,
        creature: u8,
    ) {
        if (phase, passed) == (8, true) {
            let map = self.cast_seat_map();
            for seat in 3..7u8 {
                if let Some(a) = map
                    .engine(seat)
                    .and_then(|e| self.actors.get_mut(usize::from(e)))
                    && a.battle.hp != 0
                {
                    a.battle.render_color = WORD_NEUTRAL;
                    a.battle.render_flag = 0;
                }
            }
            if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                a.battle.render_color = 0;
                a.battle.render_flag = FLAG_PARKED;
            }
        }
        self.slippery_storm_clouds(phase, passed, creature);
        let Some(a) = self.actors.get_mut(usize::from(creature)) else {
            return;
        };
        match (phase, passed) {
            (2, true) => a.move_state.world_y = -0x800,
            (3, true) => a.move_state.world_y = 0,
            (3, false) => {
                let lift =
                    (4 * legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK) as i16;
                a.move_state.world_y = a.move_state.world_y.saturating_add(lift).min(0);
            }
            _ => {}
        }
    }

    /// PROT 0920's arm 6 builds the storm the backdrop's hide (`ctx[+0x278] = 2`)
    /// leaves room for. Every fourth pass (`0x801F71DC..0x801F71EC`) it seats one
    /// of four cloud records, `rand() % 4` (`0x801F7298..0x801F7344`), a quarter of
    /// the trig table behind the creature at `y = -0x280`, on the creature's angle
    /// trio. The arm's exit halts every handle it kept (`0x801F715C..0x801F7184`).
    ///
    /// The exit is the countdown test the arm opens with (`subu` of the whole
    /// pass's drain, `bgtz` at `0x801F70D8`): a pass that leaves halts the
    /// handles and seats nothing, and only a pass that holds reaches the spawn
    /// counter (`0x801F7194..`). The port's pass is `delta` one-vsync ticks and
    /// the leaving tick need not be its first, so the halt runs on whichever
    /// tick leaves, and the first tick seats only when the pass's whole drain
    /// keeps the word above zero.
    fn slippery_storm_clouds(&mut self, phase: u8, passed: bool, creature: u8) {
        use legaia_engine_cast::cast_module_camera::{
            MODULE_DRAIN_PER_TICK, ModuleSpawn, SpawnAnchor, SpawnRecord,
        };
        const CLOUDS: [u32; 4] = [0x801F_8244, 0x801F_8284, 0x801F_82C4, 0x801F_8304];
        if phase != 6 {
            return;
        }
        if passed {
            if let Some(scene) = self.casting.active_summon.as_mut() {
                for part in &mut scene.parts {
                    if part.tag.is_some_and(|t| CLOUDS.contains(&t)) {
                        part.finished = true;
                    }
                }
            }
            return;
        }
        let countdown = self.casting.module_cam.countdown;
        if self.casting.module_pass_repeat
            || countdown.0 - MODULE_DRAIN_PER_TICK * i32::from(countdown.1) <= 0
        {
            return;
        }
        let n = self.casting.module_pass_counter.wrapping_add(1) & 3;
        self.casting.module_pass_counter = n;
        if n != 0 {
            return;
        }
        let Some(a) = self.actors.get(usize::from(creature)) else {
            return;
        };
        let facing = a.battle.facing_angle & 0xFFF;
        let (x, z) = (a.move_state.world_x, a.move_state.world_z);
        let (sin, cos) = legaia_engine_system::trig::trig12(facing);
        let pos = [x.wrapping_sub(sin / 4), -0x280, z.wrapping_sub(cos / 4)];
        let record = CLOUDS[(self.next_rand() % 4) as usize];
        let spawn = [ModuleSpawn {
            record: SpawnRecord::Module(record),
            anchor: SpawnAnchor::LiteralScaled {
                pos,
                rot: [0, facing as i16, 0],
                scale: 0x1000,
            },
        }];
        self.stage_module_arm_spawns(920, &spawn, None, None);
    }

    /// Keep `ModuleCamState::row_tr_y` on the row's middle: `-4 * +0x3E` over
    /// the living monsters, the TR y a cut on the row stands on.
    fn keep_row_tr_y(&mut self, monster_count: u8) {
        let row = self.ra_seru_row(monster_count);
        if !row.is_empty() {
            let sum: i32 = row
                .iter()
                .map(|&(e, _)| -4 * self.battle_display_trio(e).map_or(0, |d| d[1] as i32))
                .sum();
            self.casting.module_cam.row_tr_y = (sum / row.len() as i32) as i16;
        }
    }

    /// PROT 0931 (Jedo)'s stores on the row and the creature, outside its
    /// director.
    ///
    /// PORT: overlay_summon_jedo_0931_801f6a58 (arms 0, 1, 5, 6, 23, 30, 31)
    pub(in crate::world::battle::cast_band) fn apply_jedo_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        // What arm 26's cut reads off the row, its middle.
        self.keep_row_tr_y(monster_count);
        match (phase, passed) {
            // Arm 0's re-seat onto the alternate family at half its reach,
            // facing `0x800` (`0x801F6B74..0x801F6C5C`); arm 1 hides the row.
            (0, true) => self.ra_seru_row_seat(monster_count, |x, z| (x / 2, z / 2), None),
            (1, _) => self.ra_seru_row_word(monster_count, 0, FLAG_PARKED),
            // The advance `+0x38 += step << 2` of arms 5 and 6 (`0x801F70AC`,
            // `0x801F7220`), which arm 10's framing is built on.
            (5 | 6, _) => {
                if let (Some(c), Some(a)) = (
                    self.casting.module_cam.creature,
                    self.actors.get_mut(usize::from(creature)),
                ) {
                    a.move_state.world_z = c.z;
                }
            }
            // Arm 7 shrinks the three rings it keeps by handle (records
            // `0x801F8B68`, `0x801F8BC0`, `0x801F8C18`) on every pass: the drained
            // word `<< 12` over `192` into each part's `+0x72`
            // (`0x801F7354..0x801F73B0`), and the pass halts them.
            (7, _) => {
                let w = self.casting.module_cam.countdown.0 - if passed { 0x20 } else { 0 };
                if let Some(scene) = self.casting.active_summon.as_mut() {
                    for part in &mut scene.parts {
                        if part
                            .tag
                            .is_some_and(|t| JEDO_ARM7_RING_RECORDS.contains(&t))
                        {
                            part.state.field_72 = ((w.max(0) << 12) / 192) as u16;
                            if passed {
                                part.finished = true;
                            }
                        }
                    }
                }
            }
            // Arm 23's pass shows the row, frozen on its last pose, and stands
            // the creature up facing it (`0x801F7DC4..0x801F7E28`).
            (23, true) => {
                self.ra_seru_row_word(monster_count, WORD_NEUTRAL, 0);
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.anim_rate = AnimRate(0);
                    }
                }
                if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                    a.battle.facing_angle = 0x200;
                    a.move_state.world_y = 0x400;
                }
            }
            // Arm 30 lets the row's last frames run out under the shatter
            // (`0x801F8768..0x801F8784`).
            (30, _) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.anim_rate = AnimRate(2);
                    }
                }
            }
            // The last arm puts back what arm 0 parked (`0x801F8904..0x801F8984`).
            (31, true) => self.ra_seru_row_restore(monster_count, false),
            _ => {}
        }
    }

    /// PROT 0914 (Gola Gola).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | `0xB` (`0x801F6F8C`) | pass | creature flag `7`, the red ease |
    /// | `0xC` | pass | row colour `0`, flag `7` |
    /// | `0xD` | pass | row flag `0`, each facing away from the caster; creature colour `0x3FF`, flag `0xFF` |
    /// | `0xE` | pass | creature hidden (`+0x04 = 0`, `+0x21C = 0xFF`) |
    ///
    /// PORT: FUN_801F69F0 (the row and creature colour stores)
    pub(in crate::world::battle::cast_band) fn apply_gola_gola_writes(
        &mut self,
        phase: u8,
        monster_count: u8,
        caster: u8,
        creature: u8,
    ) {
        let (cx, cz) = self
            .actors
            .get(usize::from(caster))
            .map_or((0, 0), |a| (a.move_state.world_x, a.move_state.world_z));
        match phase {
            0xC => self.ra_seru_row_word(monster_count, 0, 7),
            0xD => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = 0;
                        let b = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                            cz,
                            cx,
                            a.move_state.world_z,
                            a.move_state.world_x,
                        );
                        a.battle.facing_angle = b.wrapping_add(0x800) & 0xFFF;
                    }
                }
            }
            _ => {}
        }
        if let Some(a) = self.actors.get_mut(usize::from(creature)) {
            match phase {
                0xB => a.battle.render_flag = 7,
                0xD => {
                    a.battle.render_color = 0x3FF;
                    a.battle.render_flag = FLAG_PARKED;
                }
                0xE => {
                    a.battle.render_color = 0;
                    a.battle.render_flag = FLAG_PARKED;
                }
                _ => {}
            }
        }
    }

    /// PROT 0932 (Meta).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0 (`0x801F6B40..0x801F6C60`) | pass | row parked and re-seated at `x / 4`, `z / 4 + 0xED`, facing `0x800`, scale `0x800`; hidden |
    /// | 1 (`0x801F6CD0`) | every | row hidden |
    /// | 5 (`0x801F7028`) | pass | creature rate `8` |
    /// | `0xA` (`0x801F74F8`) | pass | creature rate `4` |
    /// | `0xB` (`0x801F75E0`, `0x801F7610`) | every; pass | creature rate `4`; `0`, row shown |
    /// | `0xC` (`0x801F76F4`) | pass | creature rate `2`, `z = -0x1C0` |
    /// | `0xD`, `0xF` (`0x801F777C`, `0x801F7910`) | every | creature rate `2` |
    /// | `0xE` (`0x801F78D4`) | pass | creature rate `2` |
    /// | `0x10`, `0x11` (`0x801F7A20`, `0x801F7A3C`) | pass; every | creature rate `4` |
    /// | `0x12` (`0x801F7C54`, `0x801F7C84`) | pass | row flag `0xFF`, rate `2` |
    /// | `0x13` (`0x801F7CC8`) | every | row rate `0` |
    /// | `0x15` (`0x801F7FE0`, `0x801F7FEC`) | pass | row flag `0`, rate `2` |
    /// | `0x16` (`0x801F8318`) | pass | row restored, scale `0x1000` |
    ///
    /// Not carried: the render node's distance-fade radius `+0x58` the
    /// arms halve and restore, and the creature's object-clip row.
    ///
    /// PORT: overlay_summon_meta_0932_801f6a34 (the row and creature rate stores)
    pub(in crate::world::battle::cast_band) fn apply_meta_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        match (phase, passed) {
            (0, true) => {
                self.ra_seru_row_seat(monster_count, |x, z| (x / 4, z / 4 + 0xED), Some(0x800));
                self.ra_seru_row_word(monster_count, 0, FLAG_PARKED);
            }
            (1, _) => self.ra_seru_row_word(monster_count, 0, FLAG_PARKED),
            (0xB, true) => self.ra_seru_row_word(monster_count, WORD_NEUTRAL, 0),
            // Arm 4 saved channels 14 and 16 of the creature's model table
            // (`0x801F6F5C..0x801F6F94`) and this pass stores the second into the
            // first (`0x801F7738..0x801F7758`): the sword in its hand, which no
            // stream's part count reaches on its own channel.
            (0xC, true) => self.casting.creature_channel_object = Some((14, 16)),
            (0x12, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = FLAG_PARKED;
                        a.battle.anim_rate = AnimRate(2);
                    }
                }
            }
            (0x13, _) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.anim_rate = AnimRate(0);
                    }
                }
            }
            (0x15, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = 0;
                        a.battle.anim_rate = AnimRate(2);
                    }
                }
            }
            (0x16, true) => self.ra_seru_row_restore(monster_count, true),
            _ => {}
        }
        let rate = match (phase, passed) {
            (5, true) => Some(8),
            (0xA, true) | (0xB, false) | (0x10, true) | (0x11, _) => Some(4),
            (0xB, true) => Some(0),
            (0xC, true) | (0xD, _) | (0xE, true) | (0xF, _) => Some(2),
            _ => None,
        };
        if rate.is_some() {
            self.casting.module_cam.creature_rate = rate;
        }
        if (phase, passed) == (0xC, true)
            && let Some(a) = self.actors.get_mut(usize::from(creature))
        {
            a.move_state.world_z = -0x1C0;
        }
        self.meta_tail(phase, creature);
    }

    /// PROT 0932's per-pass tail once arm 4 has seated the creature
    /// (`0x801F8384..0x801F8420`, behind the word `0x801F8F50`): on its idle
    /// clip it rides `(cursor - 800) * 16` above its seat, on any other clip
    /// at `0`; below arm `0x16` a playing clip sheds two effect-`0xB` puffs
    /// every other pass, at `(word, -0x280, -0x1E3 - word / 2)` and `(-0x96,
    /// -0x380, -0x240)`, the word `-0x96` until arm `0xF` makes it `-0xD6`.
    ///
    /// A pass is a frame and the frame step reads `4` through these arms, so
    /// the puffs count per [`META_TAIL_TICKS`] engine ticks.
    fn meta_tail(&mut self, phase: u8, creature: u8) {
        if phase <= 4 {
            self.casting.module_meta_tail = 0;
            return;
        }
        let Some(a) = self.actors.get_mut(usize::from(creature)) else {
            return;
        };
        let clip = a.battle.current_anim;
        a.move_state.world_y = match a.battle_animation.as_ref() {
            Some(p) if clip == 0 => ((i32::from(p.cursor_sixteenths()) - 800) * 16) as i16,
            _ => 0,
        };
        if phase >= 0x16 || clip == 0 {
            return;
        }
        let tick = self.casting.module_meta_tail;
        self.casting.module_meta_tail = tick.wrapping_add(1);
        if tick.is_multiple_of(2 * META_TAIL_TICKS) {
            let word: i16 = if phase >= 0xF { -0xD6 } else { -0x96 };
            self.try_spawn_effect(0xB, [word, -0x280, -0x1E3 - word / 2], 0);
            self.try_spawn_effect(0xB, [-0x96, -0x380, -0x240], 0);
        }
    }

    /// PROT 0933 (Terra).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0 (`0x801F6B20..0x801F6C80`) | pass | backdrop weight `0x1000`; row parked and re-seated at `x / 4`, `z / 4 + 0x500`, facing `0x800`, scale `0x800` |
    /// | 1 (`0x801F6CF4`) | every | row hidden |
    /// | 6, 7 (`0x801F70C8..0x801F7154`, `0x801F7234..0x801F72C0`) | every | each part in the pool but handle `0`: `+0x7A += step`, `+0x16 += 4 * step`, killed at a level `+0x78` of `0xA00` or under |
    /// | 8 (`0x801F745C`, `0x801F7478`) | pass | creature rate `2`; row shown |
    /// | 9 (`0x801F7590`) | pass | orbit words zeroed |
    /// | `0xA`, `0xC` (`0x801F75E4..`) | every | the row's orbit about `(0, 0x613)` |
    /// | `0xA`, `0xB` (`0x801F7820`, `0x801F7910`) | pass | creature rate `4`, `2` |
    /// | `0xC` (`0x801F7C8C..`) | pass | row rate `0`, back on its seat, facing `0x800` |
    /// | `0xE` (`0x801F7E18`) | pass | creature rate `8` |
    /// | `0x11` | pass | row height `0` |
    /// | `0x13`, `0x14` (`0x801F84CC`) | every | creature `z += 0x20` |
    /// | `0x13` (`0x801F843C..`) | pass | row flag `0`, rate `2`, facing away from `(0, 0x613)` |
    /// | `0x14` (`0x801F8514`) | every | the row's facing `+ 0x20`, rate `2`, on a seat still drawn |
    /// | `0x14` (`0x801F85FC`) | pass | creature hidden |
    /// | `0x15` | pass | row restored, scale `0x1000` |
    ///
    /// PORT: overlay_summon_terra_0933_801f6a30 (the row and creature stores)
    pub(in crate::world::battle::cast_band) fn apply_terra_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        // The pool the arms walk holds the stager's two whirl records, one a
        // pass's worth of them each; a pass runs a step of these ticks.
        if matches!(phase, 6 | 7)
            && let Some(scene) = self.casting.active_summon.as_mut()
        {
            const WHIRL_RECORDS: [u32; 2] = [0x801F_8838, 0x801F_88F0];
            for part in scene
                .parts
                .iter_mut()
                .filter(|p| !p.finished && p.tag.is_some_and(|t| WHIRL_RECORDS.contains(&t)))
            {
                part.state.field_7a = part.state.field_7a.wrapping_add(1);
                part.state.world_y = part.state.world_y.wrapping_add(4);
                if part.state.field_78 <= 0xA00 {
                    part.finished = true;
                }
            }
        }
        use legaia_engine_cast::cast_module_camera::DELTA_PER_TICK as D;
        const ORBIT_Z: i32 = 0x613;
        let seat_of = |count: u8, i: usize| {
            let s = legaia_engine_battle::battle_seats::monster_seat(count, i, true);
            (s.x / 4, s.z / 4 + 0x500)
        };
        match (phase, passed) {
            (0, true) => {
                self.battle.backdrop_cue =
                    legaia_engine_battle_vm::battle_ground_grid::BACKDROP_CUE_FULL;
                self.ra_seru_row_seat(monster_count, |x, z| (x / 4, z / 4 + 0x500), Some(0x800));
            }
            (1, _) => self.ra_seru_row_word(monster_count, 0, FLAG_PARKED),
            // Arm 2's pass seats the creature hidden (`0x801F6DAC`, `0x801F6DB0`),
            // so its clip holds until arm 4's pass shows it (`0x801F6FB0`,
            // `0x801F6FC0`).
            (2, true) => {
                if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = FLAG_PARKED;
                }
            }
            (4, true) => {
                if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                    a.battle.render_color = 0x3FFF_FFFF;
                    a.battle.render_flag = 0;
                }
            }
            (8, true) => self.ra_seru_row_word(monster_count, WORD_NEUTRAL, 0),
            (9, true) => self.casting.module_terra_orbit = (0, 0),
            (0x11, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.move_state.world_y = 0;
                    }
                }
            }
            (0x13, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = 0;
                        a.battle.anim_rate = AnimRate(2);
                        let b = legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                            ORBIT_Z as i16,
                            0,
                            a.move_state.world_z,
                            a.move_state.world_x,
                        );
                        a.battle.facing_angle = b.wrapping_add(0x800) & 0xFFF;
                    }
                }
            }
            // Every seat `3..7` with a word up, a killed one too (the loop
            // tests `+0x04` alone, `0x801F84F8..0x801F8548`): the rate holds a
            // dying monster's clip at `2`, over its commit's rise to `4`.
            (0x14, _) => {
                let map = self.cast_seat_map();
                let seats: Vec<usize> = (0..monster_count)
                    .filter_map(|i| map.engine(3 + i).map(usize::from))
                    .collect();
                for e in seats {
                    if let Some(a) = self.actors.get_mut(e)
                        && (a.battle.render_color != 0 || a.battle.render_flag != FLAG_PARKED)
                    {
                        a.battle.facing_angle =
                            a.battle.facing_angle.wrapping_add((0x20 * D) as u16) & 0xFFF;
                        a.battle.anim_rate = AnimRate(2);
                    }
                }
            }
            (0x15, true) => self.ra_seru_row_restore(monster_count, true),
            _ => {}
        }
        // Arm `0xC`'s cut stands on the row's middle (`0x801F7C70..0x801F7DA4`).
        if phase == 0xC {
            self.keep_row_tr_y(monster_count);
        }
        if matches!(phase, 0xA | 0xC) {
            let (speed, angle) = self.casting.module_terra_orbit;
            let speed = speed + 3 * D;
            let angle = angle + speed;
            self.casting.module_terra_orbit = (speed, angle);
            for (e, i) in self.ra_seru_row(monster_count) {
                let (px, pz) = seat_of(monster_count, i);
                let (px, pz) = (i32::from(px), i32::from(pz) - ORBIT_Z);
                let a0 = i32::from(
                    legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                        pz as i16, px as i16, 0, 0,
                    ),
                );
                let (sin, cos) = legaia_engine_system::trig::trig12(a0 as u16);
                let r = (i32::from(sin) * px / 0x1000).abs() + (i32::from(cos) * pz / 0x1000).abs();
                let (s2, c2) =
                    legaia_engine_system::trig::trig12(((a0 + (angle & 0xFFFF)) & 0xFFF) as u16);
                if let Some(a) = self.actors.get_mut(e) {
                    a.move_state.world_x = (-i32::from(s2) * r / 0x1000) as i16;
                    a.move_state.world_z = (-i32::from(c2) * r / 0x1000 + ORBIT_Z) as i16;
                    a.battle.facing_angle =
                        a.battle.facing_angle.wrapping_add(speed as u16) & 0xFFF;
                    a.move_state.world_y = (-speed) as i16;
                }
            }
        }
        if (phase, passed) == (0xC, true) {
            for (e, i) in self.ra_seru_row(monster_count) {
                let (x, z) = seat_of(monster_count, i);
                if let Some(a) = self.actors.get_mut(e) {
                    a.battle.anim_rate = AnimRate(0);
                    a.move_state.world_x = x;
                    a.move_state.world_z = z;
                    a.battle.facing_angle = 0x800;
                }
            }
        }
        let rate = match (phase, passed) {
            (8, true) | (0xB, true) => Some(2),
            (0xA, true) => Some(4),
            (0xE, true) => Some(8),
            _ => None,
        };
        if rate.is_some() {
            self.casting.module_cam.creature_rate = rate;
        }
        if let Some(a) = self.actors.get_mut(usize::from(creature)) {
            if matches!(phase, 0x13 | 0x14) {
                a.move_state.world_z = a.move_state.world_z.wrapping_add((0x20 * D) as i16);
            }
            if (phase, passed) == (0x14, true) {
                a.battle.render_color = 0;
                a.battle.render_flag = FLAG_PARKED;
            }
        }
    }

    /// PROT 0934 (Ozma).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0 (`0x801F6B28..0x801F6BCC`) | pass | row parked and re-seated at `x / 2`, `z / 2 - 0x226`, facing `0x800` |
    /// | 1 (`0x801F6C74`) | pass | row shown |
    /// | 6 (`0x801F72B8`) | pass | row hidden |
    /// | 8 (`0x801F76A4`) | pass | creature rate `2` |
    /// | `0xC` (`0x801F79BC`) | pass | creature rate `4` |
    /// | `0xF` (`0x801F7DB8..`) | pass | the backdrop pair's weight `0x1000` |
    /// | `0x10` (`0x801F7E78`) | pass | creature rate `2` |
    /// | `0x11` (`0x801F7F2C`, `0x801F7F30`) | every | creature rate `2`, flag `4` (the blue ease) |
    /// | `0x12` (`0x801F7FC8..0x801F7FE8`) | pass | creature rate `4`, clip `3`'s window `0x22..0x2C` |
    /// | `0x15` (`0x801F85BC`) | pass | row flag `0` |
    /// | `0x19` (`0x801F9184`, `0x801F9188`) | every from word `0x80` | creature flag `2`, `+0x225 = 2`, selector `0` |
    /// | `0x19` (`0x801F91F0..`) | pass | row restored |
    ///
    /// PORT: overlay_summon_ozma_0934_801f6a40 (the row and creature stores)
    pub(in crate::world::battle::cast_band) fn apply_ozma_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
        caster: u8,
        spell_id: u8,
    ) {
        match (phase, passed) {
            (0, true) => {
                self.ra_seru_row_seat(monster_count, |x, z| (x / 2, z / 2 - 0x226), None);
            }
            (1, true) => self.ra_seru_row_word(monster_count, WORD_NEUTRAL, 0),
            (6, true) => self.ra_seru_row_word(monster_count, 0, FLAG_PARKED),
            (0xF, true) => {
                self.battle.backdrop_cue =
                    legaia_engine_battle_vm::battle_ground_grid::BACKDROP_CUE_FULL;
            }
            (0x15, true) => {
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_flag = 0;
                    }
                }
            }
            (0x17, true) => self.casting.module_ozma_ladder = OZMA_LADDER_FIRST,
            (0x18, _) => self.ozma_hit_ladder(passed, monster_count, caster, spell_id),
            (0x19, true) => self.ra_seru_row_restore(monster_count, false),
            _ => {}
        }
        let rate = match (phase, passed) {
            (8, true) | (0x10, true) | (0x11, _) => Some(2),
            (0xC, true) | (0x12, true) => Some(4),
            _ => None,
        };
        if rate.is_some() {
            self.casting.module_cam.creature_rate = rate;
        }
        if (phase, passed) == (0x12, true)
            && let Some(p) = self
                .actors
                .get_mut(usize::from(creature))
                .and_then(|a| a.battle_animation.as_mut())
        {
            p.set_loop_frames(0x22, 0x2C);
        }
        // Arm `0x19` raises every pool part's level `+0x78` off one word,
        // `0x14` a frame up to `0x1000`, which the arm's exit from `0x18` left
        // at `0` (`0x801F9144..0x801F91C0`).
        if phase == 0x19 {
            let level = (self.casting.module_ozma_ladder + 0x14).min(0x1000);
            self.casting.module_ozma_ladder = level;
            if let Some(scene) = self.casting.active_summon.as_mut() {
                for part in &mut scene.parts {
                    part.state.field_78 = level as u16;
                }
            }
        }
        let fading = phase == 0x19 && self.casting.module_cam.countdown.0 < 0x81;
        if (phase == 0x11 || fading)
            && let Some(a) = self.actors.get_mut(usize::from(creature))
        {
            a.battle.render_flag = if fading { 2 } else { 4 };
            if fading {
                a.battle.capture_state = 2;
                a.battle.impact_state = 0;
            }
        }
    }

    /// PROT 0934's arm `0x18` (`0x801F8D00..0x801F90F0`). Every pass lights
    /// the row (`+0x04 = 0x3FF003FF`, `+0x0C = 0x1000`, `+0x21F = 2`). The
    /// arm's word falling to the threshold `0x801FA408`, which arm `0x17`
    /// leaves at `0x60` and each hit lowers by `0x20`, hits every living,
    /// targetable monster: the summon roll `FUN_801DD0AC(0x10, 7, seat)`
    /// quartered, plus `0x40 + rand() % 0x40`, its popup pushed, the HP write
    /// clamped to `HP - 1`, the flinch `+0x1EF` (else `+0x1F0`) staged with
    /// the restage bits `5`, and per monster a fade over `0x10` frames. The
    /// fourth hit, at threshold `0`, is the band's fold on the arm's pass.
    /// Each ladder roll trains the spell and fills the victim's spirit gauge
    /// as the fold's does ([`Self::summon_module_roll`]).
    ///
    /// Not carried: the damage tallies the quartered part is booked against
    /// (`0x8007BD14`, or `0x801F6980 + seat * 2` under a group target).
    /// REF: FUN_801DD0AC (each ladder roll, through [`Self::summon_module_roll`])
    fn ozma_hit_ladder(&mut self, passed: bool, monster_count: u8, caster: u8, spell_id: u8) {
        use legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE;
        for (e, _) in self.ra_seru_row(monster_count) {
            if let Some(a) = self.actors.get_mut(e) {
                a.battle.render_color = OZMA_HIT_WORD;
                a.battle.render_blend = legaia_engine_battle_vm::battle_formulas::TINT_BLEND_FULL;
                a.battle.impact_state = 2;
            }
        }
        let threshold = self.casting.module_ozma_ladder;
        // The closing hit is the band's fold, with the same flash per monster
        // over `0xC0` frames (`li v0,0xC0` at `0x801F9050`).
        if passed {
            if threshold <= 0 {
                self.ozma_hit_flashes(monster_count, OZMA_LAST_FLASH_FRAMES);
            }
            return;
        }
        if threshold <= 0 || self.casting.module_cam.countdown.0 > threshold {
            return;
        }
        self.casting.module_ozma_ladder = threshold - OZMA_LADDER_STEP;
        for (e, _) in self.ra_seru_row(monster_count) {
            let s = e as u8;
            if self.cast_view(s).flags & FLAG_NON_TARGETABLE != 0 {
                continue;
            }
            // The whole roll through the finisher, as every `FUN_801DD0AC`
            // call runs it (`0x801DD478`): the spell XP, the victim's spirit
            // and the struck mark land on the unquartered roll.
            let Some(roll) = self.summon_module_roll(caster, s, spell_id) else {
                continue;
            };
            // The readout flag kept down (`0x801F8E30`).
            self.battle.combo_cluster_held = true;
            let hit = (roll >> 2) + 0x40 + self.next_rand() % 0x40;
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
            v.staged_anim = if v.reaction_alt != 0 {
                v.reaction_alt
            } else {
                v.reaction_alt2
            };
            v.restage |= 0x05;
            self.write_cast_view(s, &v);
        }
        self.ozma_hit_flashes(monster_count, OZMA_HIT_FLASH_FRAMES);
    }

    /// The fade one ladder hit spawns inside its loop, one per monster it
    /// hits (`0x801F9048..0x801F907C`): additive, `(0x60, 0x60, 0x18)` to
    /// black over `frames`.
    fn ozma_hit_flashes(&mut self, monster_count: u8, frames: i16) {
        use legaia_engine_cast::cast_module_ticks::FLAG_NON_TARGETABLE;
        let t = legaia_engine_system::fade::FadeTemplate {
            kind: 1,
            duration: frames,
            start_rgb: [0x60, 0x60, 0x18],
            end_rgb: [0, 0, 0],
            mode: [0, 0, 0],
        };
        for (e, _) in self.ra_seru_row(monster_count) {
            if self.cast_view(e as u8).flags & FLAG_NON_TARGETABLE != 0 {
                continue;
            }
            let mut seat = None;
            // Id `1` (`li a1,0x1` at `0x801F903C`), the bucket under the HUD:
            // a retail table of arm `0x18` links the fade's quad there.
            legaia_engine_system::fade::spawn_fade(&mut seat, &t, 1);
            if let Some(mut state) = seat {
                state.step_vsyncs(self.battle_frame_step().max(1));
                self.casting.module_pres.fades.push(HookFade {
                    site: OZMA_LADDER_FADE_SITE,
                    slot: None,
                    state,
                });
            }
        }
    }

    /// PROT 0919's arms 5..7, every pass: arm 4's four parts per eye stand on
    /// the creature's pose objects `0..5` (`ctx+0x6F4`, twelve bytes an object,
    /// the pool the creature's draw just filled), turned by its facing and
    /// raised `8`. Arm 7 pulls eyes `1..` in to four fifths (`/ 0x1400`), holds
    /// the top eye at `0x20` above its object and drops the lower two by `0x20`;
    /// its pass retires all twenty (`0x801F7CE0..`).
    fn seat_spoon_eye_parts(&mut self, phase: u8, passed: bool, creature: u8) {
        use legaia_engine_cast::cast_module_camera::SPOON_EYE_RECORDS;
        let Some(a) = self.actors.get(usize::from(creature)) else {
            return;
        };
        let (cx, cz, f) = (
            i32::from(a.move_state.world_x),
            i32::from(a.move_state.world_z),
            a.battle.facing_angle,
        );
        let Some(pose) = crate::world::battle::locomotion::stream_pose(a) else {
            return;
        };
        let (sin_f, cos_f) = legaia_engine_system::trig::trig12(f & 0xFFF);
        let (sin_b, cos_b) = legaia_engine_system::trig::trig12(0xFFF - (f & 0xFFF));
        let seats: Vec<[i16; 3]> = pose
            .iter()
            .take(5)
            .enumerate()
            .map(|(i, (p, _))| {
                let div = if phase == 7 && i != 0 { 0x1400 } else { 0x1000 };
                let (px, py, pz) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
                let x = cx + i32::from(sin_f) * pz / div + i32::from(cos_b) * px / div;
                let z = cz + i32::from(sin_b) * px / div + i32::from(cos_f) * pz / div;
                let y = match (phase, i) {
                    (7, 0) => py + 0x20,
                    (7, 3..) => py + 8 - 0x20,
                    _ => py + 8,
                };
                [x as i16, y as i16, z as i16]
            })
            .collect();
        let Some(scene) = self.casting.active_summon.as_mut() else {
            return;
        };
        let parts = scene
            .parts
            .iter_mut()
            .filter(|p| p.tag.is_some_and(|t| SPOON_EYE_RECORDS.contains(&t)));
        for (k, part) in parts.enumerate() {
            let Some(seat) = seats.get(k / SPOON_EYE_RECORDS.len()) else {
                break;
            };
            if phase == 7 && passed {
                part.finished = true;
                continue;
            }
            [part.state.world_x, part.state.world_y, part.state.world_z] = *seat;
            part.state.world_y_mirror = seat[1];
        }
    }

    /// PROT 0919 (Spoon).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0 (`0x801F6BA0..0x801F6C80`) | pass | every present party seat and living monster hidden |
    /// | 2 (`0x801F6DB0`) | pass | the creature's word `0xFFFFF`, which the idle tint arm eases to neutral |
    /// | 7 (`0x801F7DCC..0x801F7E54`) | pass | the creature hidden, the party shown |
    pub(in crate::world::battle::cast_band) fn apply_spoon_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        // The creature's rate: `1` on arm 4's pass (`0x801F70A4`), which the
        // clip that pass stages rises from to `4` at its commit (`0x8004B080`),
        // so arm 5 runs at `4`; `1` again on every pass of arm 6 (`0x801F77FC`),
        // so the clip it holds the eye close-up on barely moves; `4` through
        // arm 7.
        match (phase, passed) {
            (4, true) => {
                if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                    a.battle.anim_rate = AnimRate(1);
                }
            }
            (6, _) => self.casting.module_cam.creature_rate = Some(1),
            (7, _) => self.casting.module_cam.creature_rate = Some(4),
            _ => {}
        }
        if matches!(phase, 5..=7) {
            self.seat_spoon_eye_parts(phase, passed, creature);
        }
        // Arm 3 lifts the row's top a frame product a pass, and its pass puts
        // the creature back on no clip (`0x801F6F28`, `0x801F6F70`).
        if phase == 3 {
            let step = legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK as i16;
            self.casting.module_spoon_clip = self.casting.module_spoon_clip.saturating_add(step);
            self.object_effect
                .write(1, [0, 0, 0, -0x8000, self.casting.module_spoon_clip]);
            if passed {
                self.casting.module_creature_clip_kind = 0;
                self.casting.module_cam.creature_floor_clip = false;
                self.object_effect.write(1, [0, 0, 0, -0x7FFF, 0]);
                if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                    a.battle.floor_clip = false;
                }
            }
        }
        if !passed {
            return;
        }
        let party = usize::from(self.party.party_count);
        match phase {
            0 => {
                for a in self.actors.iter_mut().take(party) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = FLAG_PARKED;
                }
                self.ra_seru_row_word(monster_count, 0, FLAG_PARKED);
            }
            2 => {
                // Object-effect row 1 from `-0x280` up, `+0x42 = 2`
                // (`0x801F6DB8..0x801F6DE4`): the creature rises through it.
                self.casting.module_spoon_clip = SPOON_CLIP_START;
                self.object_effect
                    .write(1, [0, 0, 0, -0x8000, SPOON_CLIP_START]);
                self.casting.module_creature_clip_kind = 2;
                self.casting.module_cam.creature_floor_clip = true;
                if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                    a.battle.render_color = SPOON_SEAT_WORD;
                    a.battle.render_flag = 0;
                    a.battle.floor_clip = true;
                }
            }
            7 => {
                if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = FLAG_PARKED;
                }
                for a in self.actors.iter_mut().take(party) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = 0;
                }
            }
            _ => {}
        }
    }

    /// PROT 0921 (Iota).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0 (`0x801F6BB8..0x801F6C04`) | pass | the row parked and re-seated at half its alternate seat, `0x400` further on, facing `0x800` |
    /// | 0..=3 | every pass | the creature not drawn, its word `0` until arm 3's seat (`0x801F6FCC`), from which the idle tint arm eases it up |
    /// | 7 (`0x801F7470`) | pass | the row shown |
    /// | 8 (`0x801F75F4`) | pass | the creature hidden |
    /// | 9 (`0x801F76C8..0x801F77E4`) | every pass | the row rising, `+0x36 -= step / 4`, its plane pair closing on the origin by as much, its scale falling by `4 * step` |
    /// | `0xA` (`0x801F7C6C..0x801F7D10`) | pass | the row at full scale on the floor of its half seat, turned from `(0, 0x400)` |
    /// | `0xB` (`0x801F7F6C`) | pass | the row put back |
    pub(in crate::world::battle::cast_band) fn apply_iota_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        use legaia_engine_cast::cast_module_camera::MODULE_DRAIN_PER_TICK as STEP;
        if phase <= 3
            && let Some(a) = self.actors.get_mut(usize::from(creature))
        {
            if phase == 3 && passed {
                a.battle.render_color = NEAR_BLACK_WORD;
                a.battle.render_flag = 0;
            } else {
                a.battle.render_color = 0;
                a.battle.render_flag = FLAG_PARKED;
            }
        }
        match (phase, passed) {
            (0, true) => {
                self.ra_seru_row_seat(monster_count, |x, z| (x / 2, z / 2 + 0x400), None);
            }
            (7, true) => self.ra_seru_row_word(monster_count, 0, 0),
            (8, true) => {
                if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                    a.battle.render_color = 0;
                    a.battle.render_flag = FLAG_PARKED;
                }
            }
            (9, _) => {
                let quarter = (STEP / 4) as i16;
                for (e, _) in self.ra_seru_row(monster_count) {
                    if let Some(a) = self.actors.get_mut(e) {
                        let m = &mut a.move_state;
                        m.world_y -= quarter;
                        m.world_x -= m.world_x.signum() * quarter.min(m.world_x.abs());
                        m.world_z -= m.world_z.signum() * quarter.min(m.world_z.abs());
                        let scale = i32::from(a.battle.render_scale.unwrap_or(0x1000));
                        a.battle.render_scale = Some((scale - 4 * STEP).max(0) as u16);
                    }
                }
            }
            (0xA, true) => {
                for (e, i) in self.ra_seru_row(monster_count) {
                    let s =
                        legaia_engine_battle::battle_seats::monster_seat(monster_count, i, true);
                    if let Some(a) = self.actors.get_mut(e) {
                        a.battle.render_scale = None;
                        let m = &mut a.move_state;
                        m.world_y = 0;
                        m.world_x = s.x / 2;
                        m.world_z = s.z / 2 + 0x400;
                        let from = legaia_engine_cast::cast_module_camera::ModuleSeat {
                            x: 0,
                            y: 0,
                            z: 0x400,
                            facing: 0,
                        };
                        let to = legaia_engine_cast::cast_module_camera::ModuleSeat {
                            x: m.world_x,
                            y: 0,
                            z: m.world_z,
                            facing: 0,
                        };
                        a.battle.facing_angle =
                            legaia_engine_cast::cast_module_camera::heading(from, to)
                                .wrapping_add(0x800)
                                & 0xFFF;
                    }
                }
            }
            (0xB, true) => self.ra_seru_row_restore(monster_count, true),
            _ => {}
        }
    }

    /// PROT 0925 (the Spikefish flute), the stores beside its director
    /// (`FUN_801F6A00`).
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0, 1 (`0x801F6AEC..0x801F6B78`, `0x801F6C98..0x801F6D24`) | every pass | every party seat and living monster hidden |
    /// | 5 (`0x801F7018..0x801F7030`) | every pass | the creature's node scale `+0x72 += 10 * step` |
    /// | 5 (`0x801F70AC..0x801F713C`) | pass | those seats shown again, `+0x04 = 0x20080200` |
    /// | 6 (`0x801F71DC..0x801F71E4`) | pass | the creature frozen (`+0x21D = 0`), its red tint state `+0x21C = 3` |
    pub(in crate::world::battle::cast_band) fn apply_spikefish_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        creature: u8,
    ) {
        const SHOWN: u32 = 0x2008_0200;
        const SCALE_PER_TICK: u16 = 10;
        let party = usize::from(self.party.party_count);
        if phase <= 1 {
            for a in self.actors.iter_mut().take(party) {
                a.battle.render_color = 0;
                a.battle.render_flag = FLAG_PARKED;
            }
            self.ra_seru_row_word(monster_count, 0, FLAG_PARKED);
        }
        if phase == 5 {
            if let Some(a) = self.actors.get_mut(usize::from(creature)) {
                a.battle.render_scale = Some(
                    a.battle
                        .render_scale
                        .unwrap_or(0x1000)
                        .wrapping_add(SCALE_PER_TICK),
                );
            }
            if passed {
                for a in self.actors.iter_mut().take(party) {
                    a.battle.render_color = SHOWN;
                    a.battle.render_flag = 0;
                }
                self.ra_seru_row_word(monster_count, SHOWN, 0);
            }
        }
        if phase == 6
            && passed
            && let Some(a) = self.actors.get_mut(usize::from(creature))
        {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(0);
            a.battle.render_flag = 3;
        }
    }

    /// PROT 0924 (Ultimate Rave), the stores beside its director.
    ///
    /// | Arm | Edge | Store |
    /// |---|---|---|
    /// | 0..=2 (`0x801F6B18`, `0x801F6D74`) | every pass | every party seat and living monster hidden |
    /// | 3, 5 | pass | the creature put where the director stood it |
    /// | 7 (`0x801F7300`) | pass | the victim shown |
    /// | 11 (`0x801F77A4`) | every pass | the creature rising, `+0x36 += 4 * step`; under `0x81` its fade `+0x21C = 2` |
    pub(in crate::world::battle::cast_band) fn apply_rave_writes(
        &mut self,
        phase: u8,
        passed: bool,
        monster_count: u8,
        victim: u8,
        creature: u8,
    ) {
        let party = usize::from(self.party.party_count);
        if phase <= 2 {
            for a in self.actors.iter_mut().take(party) {
                a.battle.render_color = 0;
                a.battle.render_flag = FLAG_PARKED;
            }
            self.ra_seru_row_word(monster_count, 0, FLAG_PARKED);
        }
        let seat = self.casting.module_cam.creature;
        let countdown = self.casting.module_cam.countdown.0;
        if passed
            && matches!(phase, 3 | 5)
            && let Some(c) = seat
            && let Some(a) = self.actors.get_mut(usize::from(creature))
        {
            a.move_state.world_x = c.x;
            a.move_state.world_z = c.z;
            a.battle.facing_angle = c.facing;
        }
        if phase == 7
            && passed
            && let Some(a) = self.actors.get_mut(usize::from(victim))
        {
            a.battle.render_color = 0;
            a.battle.render_flag = 0;
        }
        if phase == 0xB
            && let Some(a) = self.actors.get_mut(usize::from(creature))
        {
            a.move_state.world_y = a
                .move_state
                .world_y
                .wrapping_add(4 * legaia_engine_cast::cast_module_camera::DELTA_PER_TICK as i16);
            if countdown < 0x81 {
                a.battle.render_flag = 2;
                a.battle.capture_state = 2;
            }
        }
    }
}
