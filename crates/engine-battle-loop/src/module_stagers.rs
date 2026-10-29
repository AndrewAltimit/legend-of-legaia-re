//! The slot-B module stagers' own steps over the cast state: seating a
//! stager spawn record and flushing the module part pool, the Puera ring,
//! Orb, Juggernaut and Megaton Press calls, Slippery Rain's splashes, and
//! the Fatal Decision parts and caption.

use crate::cast_fx_state::CastFxState;
use crate::module_presentation::HookFade;
use crate::orb_stager::{ORB_LIFT_ARM, ORB_POOL_RECORDS};
use legaia_engine_cast::cast_fatal_decision as fd;
use legaia_engine_cast::cast_module_camera::{
    DELTA_PER_TICK, SLIPPERY_COLOUR_ARM, SLIPPERY_RAIN_DRAIN_ARM, SLIPPERY_RAIN_SEED_ARM,
};
use legaia_engine_cast::cast_module_ticks::StagerSpawn;

/// The label of the backdrop part.
pub const TAG_BACKDROP: u32 = 0x0954_0000;

/// The label of wheel icon `i` is this plus `i`.
pub const TAG_ICON: u32 = 0x0954_0100;

/// The label of the three landing parts.
pub const TAG_LANDING: u32 = 0x0954_0200;

/// The grey arm 0 stores in the colour word (`0x801F6BAC`).
pub const RAIN_GREY: u32 = 0x0080_8080;

/// The red arm 9's exit pins (`0x801F77B0`).
pub const RAIN_RED: u32 = 0x00FF_4040;

/// The budget arm 5's exit seeds (`0x801F6FEC`).
pub const RAIN_BUDGET_SEED: i32 = 0x204;

/// The floor arm 6's exit pins (`0x801F712C`).
pub const RAIN_BUDGET_FLOOR: i32 = 4;

impl CastFxState {
    /// A string out of PROT 0954's image, as the display string the HUD
    /// lays out: printable ASCII as itself, any other byte (the `0xCE`
    /// button escape) parked at `U+E000 + byte` so the font gets the byte
    /// back.
    pub fn fatal_decision_text(&self, va: u32) -> Option<String> {
        let module = self
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

    /// Seat the records one pass spawned, each at the origin with no
    /// rotation (every one is camera-relative), labelled for the arms that
    /// write into it later.
    pub fn seat_fatal_records(&mut self, spawns: &[fd::FatalSpawn]) {
        const LINK_BASE: u32 = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let Some(module) = self
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
                legaia_overlay_images::summon_overlay::parse_records_at(&module.bytes, &offs)
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
            let scene = self.active_summon.get_or_insert_with(|| {
                legaia_engine_effects::summon::SummonScene::spawn_parts(
                    &[],
                    &module.bytes,
                    legaia_engine_effects::summon::EFFECT_MODEL_LIBRARY_BASE,
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
    pub fn drive_fatal_parts(&mut self, p: &fd::FatalDecisionPass) {
        let Some(scene) = self.active_summon.as_mut() else {
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

    /// PROT 0911's pool flush (`FUN_80050E74`).
    pub fn orb_pool_flush(&mut self) {
        if let Some(scene) = self.active_summon.as_mut() {
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| ORB_POOL_RECORDS.contains(&t)) {
                    part.finished = true;
                }
            }
        }
    }

    /// Hold the seat arm 5's pass wrote (`0x801F75E4..0x801F7600`) for the
    /// next tick. Retail steps and lifts the creature in the pass that cuts
    /// to it (`FUN_801D829C` at `0x801F7634`), so no frame shows the old shot
    /// with the creature gone; the body-paced director lands that cut on the
    /// next tick ([`ORB_LIFT_ARM`]'s pass moves to arm 9), and the seat moves
    /// with it. Returns whether the seat was held.
    pub fn orb_defer_seat(
        &mut self,
        phase_in: u8,
        phase_out: u8,
        seat: &legaia_engine_cast::seru_magic_ticks::OrbSeatPos,
    ) -> bool {
        if phase_in != ORB_LIFT_ARM || phase_out == ORB_LIFT_ARM {
            return false;
        }
        self.module_orb.deferred_seat = Some([seat.x, seat.y, seat.z]);
        true
    }

    pub fn push_megaton_press_fade(
        &mut self,
        t: &legaia_engine_battle_vm::battle_action::SummonFadeTemplate,
        site: u32,
        slot: Option<legaia_overlay_images::cast_presentation::HandleSlot>,
    ) {
        let mut seat = None;
        legaia_engine_system::fade::spawn_fade(
            &mut seat,
            &legaia_engine_battle_vm::battle_action::summon_fade_template(t),
            1,
        );
        if let Some(state) = seat {
            if let Some(s) = slot {
                for f in self.module_pres.fades.iter_mut() {
                    if f.slot == Some(s) {
                        f.slot = None;
                    }
                }
            }
            self.module_pres.fades.push(HookFade { site, slot, state });
        }
    }

    /// The module-side writes of the stagers' words and handles, on every
    /// tick of the arm that makes them (`passed`: the arm handed over).
    ///
    /// - PROT 0918's arm 4 lifts its stager's arm-0 part (the handle
    ///   `0x801F92C4`) `6 * scalar * step` a battle frame up to `-0x800`,
    ///   on every pass ahead of its countdown gate
    ///   (`0x801F712C..0x801F718C`).
    /// - PROT 0925's arm 7 halts every ring part its stager seated (the
    ///   handle table `0x801F809C`) on the pass it hands over
    ///   (`0x801F7210..0x801F7254`).
    /// - PROT 0934's arms 0 and 10 zero the stager word `0x801FA410`
    ///   (`0x801F6C3C`; `0x801F78F8`, on its pass).
    ///
    /// REF: FUN_801F6C70 (PROT 0918), FUN_801F6A00 (PROT 0925),
    /// FUN_801F6A40 (PROT 0934)
    pub fn late_creature_stager_words(&mut self, entry: u32, phase: u8, passed: bool) {
        use legaia_engine_cast::cast_module_ticks as t;
        match entry {
            918 if phase == 4 => {
                let lift = (t::KEMARO_LIFT_PER_SCALAR
                    * legaia_engine_cast::cast_module_camera::SPEED_SCALAR)
                    as i16;
                if let Some(part) = self.active_summon.as_mut().and_then(|sc| {
                    sc.parts
                        .iter_mut()
                        .rev()
                        .find(|p| p.tag == Some(t::KEMARO_LIFTED_RECORD))
                }) {
                    let y = part.state.world_y.wrapping_sub(lift);
                    part.state.world_y = y.max(t::KEMARO_LIFT_CEILING);
                }
            }
            925 if phase == t::SPIKEFISH_RING_ARM && passed => {
                if let Some(sc) = self.active_summon.as_mut() {
                    for part in &mut sc.parts {
                        if part
                            .tag
                            .is_some_and(|r| t::SPIKEFISH_RING_RECORDS.contains(&r))
                        {
                            part.finished = true;
                        }
                    }
                }
            }
            934 if phase == 0 || (phase == 10 && passed) => self.module_ozma_word = 0,
            _ => {}
        }
    }

    /// The part-pool flush `FUN_80050E74` a capture arm makes on its pass
    /// ([`legaia_engine_cast::cast_module_camera::capture_arm_flushes_pool`]):
    /// every part of the cast's running scene gets its halt bit. The
    /// modules that call it seat every part through the pool wrapper
    /// `FUN_80050ED4`.
    ///
    /// REF: FUN_80050E74
    pub fn flush_module_part_pool(&mut self) {
        if let Some(scene) = self.active_summon.as_mut() {
            for part in &mut scene.parts {
                part.finished = true;
            }
        }
    }

    /// Seat one stager arm's `FUN_80021B04` call into the cast's running
    /// scene: the record parsed against its image's whole record set, with
    /// the arm's bytecode stores applied to the copy it is seated from, and
    /// its stores into the seated part queued behind the part's seat run.
    pub fn seat_stager_spawn(&mut self, entry: u32, spawn: &StagerSpawn) {
        const LINK_BASE: u32 = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let Some(module) = self.effect_pool.as_ref().and_then(|p| p.module(entry)) else {
            return;
        };
        let Some(off) = spawn.record.checked_sub(LINK_BASE).map(|o| o as usize) else {
            return;
        };
        let bytes: std::sync::Arc<[u8]> = if spawn.patches.is_empty() {
            module.bytes.clone()
        } else {
            let mut b = module.bytes.to_vec();
            for &(va, value) in &spawn.patches {
                let Some(at) = va.checked_sub(LINK_BASE).map(|o| o as usize) else {
                    continue;
                };
                if let Some(w) = b.get_mut(at..at + 2) {
                    w.copy_from_slice(&value.to_le_bytes());
                }
            }
            b.into()
        };
        let mut offs: Vec<usize> = module.parts.iter().map(|p| p.record_off).collect();
        offs.push(off);
        let picked: Vec<_> = legaia_overlay_images::summon_overlay::parse_records_at(&bytes, &offs)
            .into_iter()
            .filter(|p| p.record_off == off)
            .collect();
        if picked.is_empty() {
            return;
        }
        let scene = self.active_summon.get_or_insert_with(|| {
            legaia_engine_effects::summon::SummonScene::spawn_parts(
                &[],
                &bytes,
                legaia_engine_effects::summon::EFFECT_MODEL_LIBRARY_BASE,
                spawn.pos,
            )
        });
        let first = scene.parts.len();
        scene.push_parts_scaled(&picked, &bytes, spawn.pos, spawn.rot, spawn.scale);
        for part in &mut scene.parts[first..] {
            part.tag.get_or_insert(spawn.record);
            part.seat_pokes.extend_from_slice(&spawn.pokes);
            part.seat_halve_speed = spawn.halve_speed;
        }
    }

    /// The rain words PROT 0920's arm `phase_in` wrote on this pass;
    /// `passed` is whether the director let the arm through.
    pub fn slippery_rain_words(&mut self, phase_in: u8, passed: bool) {
        let rain = &mut self.module_rain;
        match phase_in {
            0 => {
                // `0x801F6B98..0x801F6BFC`: grey colour, zero budget, every
                // record freed.
                rain.colour = RAIN_GREY;
                rain.budget = 0;
                rain.drops.clear();
            }
            SLIPPERY_RAIN_SEED_ARM if passed => rain.budget = RAIN_BUDGET_SEED,
            SLIPPERY_RAIN_DRAIN_ARM => {
                // `budget += rate * -2` on every pass, before the gate.
                rain.budget -= 2 * DELTA_PER_TICK;
                if passed {
                    rain.budget = RAIN_BUDGET_FLOOR;
                }
            }
            SLIPPERY_COLOUR_ARM => {
                // One step a vsync: red up to `0xFF`, green and blue down
                // together until blue reads `0x40`.
                for _ in 0..DELTA_PER_TICK {
                    if rain.colour & 0xFF_0000 != 0xFF_0000 {
                        rain.colour = rain.colour.wrapping_add(0x1_0000);
                    }
                    if rain.colour & 0xFF != 0x40 {
                        rain.colour = rain.colour.wrapping_sub(0x101);
                    }
                }
                if passed {
                    rain.colour = RAIN_RED;
                }
            }
            legaia_engine_cast::seru_magic_ticks::SERU_B_DONE_PHASE => rain.budget = 0,
            _ => {}
        }
    }

    /// The splashes the rain routine's last call spawned: one
    /// `FUN_80021B04(drop, drop, 0x801F81F0, 0x1000)` each - the module's
    /// own record, whose colour halfwords `+0x22` / `+0x24` / `+0x26` the
    /// routine refreshes from the rain colour word on every call
    /// (`0x801F7C2C..0x801F7C3C`) - seated into the cast's running scene at
    /// the landed drop, with the drop's own trio as the angles (both pointer
    /// arguments are the record). The part's `+0x78`, which retail stores
    /// the halved depth into after the spawn, is not carried.
    pub fn spawn_rain_splashes(&mut self) {
        use legaia_overlay_images::summon_overlay::{SUMMON_OVERLAY_LINK_BASE, parse_records_at};
        let splashes = self.module_rain.frame.splashes.clone();
        if splashes.is_empty() {
            return;
        }
        let Some(module) = self
            .effect_pool
            .as_ref()
            .and_then(|p| p.module(legaia_engine_cast::cast_module_camera::SLIPPERY_ENTRY))
        else {
            return;
        };
        let off = (legaia_render_kernels::cast_rain::RAIN_SPLASH_RECORD - SUMMON_OVERLAY_LINK_BASE)
            as usize;
        let mut offs: Vec<usize> = module.parts.iter().map(|p| p.record_off).collect();
        offs.push(off);
        let mut bytes = module.bytes.to_vec();
        let c = self.module_rain.colour;
        for (k, v) in [
            (0x22, (c >> 16) & 0xFF),
            (0x24, (c >> 8) & 0xFF),
            (0x26, c & 0xFF),
        ] {
            if let Some(b) = bytes.get_mut(off + k..off + k + 2) {
                b.copy_from_slice(&(v as u16).to_le_bytes());
            }
        }
        let parts: Vec<_> = parse_records_at(&bytes, &offs)
            .into_iter()
            .filter(|p| p.record_off == off)
            .collect();
        if parts.is_empty() {
            return;
        }
        let scene = self.active_summon.get_or_insert_with(|| {
            legaia_engine_effects::summon::SummonScene::spawn_parts(
                &[],
                &bytes,
                legaia_engine_effects::summon::EFFECT_MODEL_LIBRARY_BASE,
                [0; 3],
            )
        });
        for s in splashes {
            scene.push_parts(&parts, &bytes, s.at, s.at);
        }
    }

    /// PROT 0927's pool flush on arms 26 and 27 (`jal 0x80050E74`): every
    /// part it seated through the pool wrapper `FUN_80050ED4` - its own
    /// arms' and its stager's - halts; the two records it seats through
    /// `FUN_80021B04` run on.
    pub fn juggernaut_pool_flush(&mut self) {
        use legaia_engine_cast::cast_module_camera::JUGGERNAUT_UNPOOLED;
        if let Some(scene) = self.active_summon.as_mut() {
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| !JUGGERNAUT_UNPOOLED.contains(&t)) {
                    part.finished = true;
                }
            }
        }
    }

    /// Queue one op-`0x20` call into the resident module's stager, as a
    /// part's program makes it (`MoveVmHostImpl::ext_20`); the module's next
    /// tick runs it. A test seam for the stagers only parts reach.
    pub fn queue_cast_stager_call(&mut self, arm: u8) {
        self.module_stager_calls.push((arm, Default::default()));
    }

    /// Run the op-`0x20` calls PROT 0922's parts made since its last tick
    /// through its stager `FUN_801F90E4`: arm 0 raises `ctx[+0x278] = 3`.
    ///
    /// REF: FUN_801F90E4
    pub fn run_puera_stager_calls(
        &mut self,
        ctx: &mut legaia_engine_cast::cast_module_ticks::CastModuleCtx,
    ) {
        for (arm, _) in std::mem::take(&mut self.module_stager_calls) {
            legaia_engine_cast::cast_module_ticks::puera_stager(ctx, arm);
        }
    }

    /// The live parts of arm 18's ring, in the order arm 18 seated them
    /// (the handle slots `0x801F9F90 + (0x16 + i) * 4`).
    pub fn puera_ring_parts(
        &mut self,
    ) -> impl Iterator<Item = &mut legaia_engine_effects::summon::SummonPartRuntime> {
        self.active_summon
            .iter_mut()
            .flat_map(|s| s.parts.iter_mut())
            .filter(|p| {
                !p.finished
                    && p.tag == Some(legaia_engine_cast::cast_module_camera::PUERA_RING_RECORD)
            })
    }
}
