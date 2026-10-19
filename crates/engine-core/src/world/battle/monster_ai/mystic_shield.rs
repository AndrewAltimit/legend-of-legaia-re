//! The Mystic Shield: the up-check, the effect it seats, and its per-frame
//! pose / break tick.

use super::*;

/// The shield's sphere record, `FUN_80021B04`'s `a2` at `0x801F765C` in
/// PROT 0940 (`lui a2,0x8008; addiu a2,a2,-0x79BC`): a default-arm draw-kind-4
/// disc, camera-relative (`+0x52 = 0x380`), textured off page `0x16` / CLUT
/// `0x7940` - the boss texture page the battle loader stages for every
/// monster id above `0xB2` ([`legaia_actor_formats::monster_archive::boss_effect_page`]).
pub const MYSTIC_SHIELD_SPHERE_RECORD: u32 = 0x8007_8644;
/// The break burst `FUN_8004CE2C` seats where the sphere stood
/// (`addiu a2,v0,-0x793C` at `0x8004D5DC`).
pub const MYSTIC_SHIELD_BURST_RECORD: u32 = 0x8007_86C4;
/// The texture scroller (`0x801F7680`, `addiu a2,a2,-0x78E0`): a render-mode-4
/// part rolling the rect `(0x180, 0x100, 0x20, 0xFF)` - the sphere's page.
pub const MYSTIC_SHIELD_SCROLLER_RECORD: u32 = 0x8007_8720;
/// The SCUS-resident records around the shield's, in address order: each
/// record's program is bounded by the next record's start.
const SCUS_RECORD_RUN: [u32; 5] = [
    MYSTIC_SHIELD_SPHERE_RECORD,
    MYSTIC_SHIELD_BURST_RECORD,
    MYSTIC_SHIELD_SCROLLER_RECORD,
    0x8007_8740,
    0x8007_8760,
];
/// `SCUS_942.54` is linked at `0x80010000` behind a `0x800`-byte header.
const SCUS_FILE_BIAS: u32 = 0x8000_F800;

/// The scale `FUN_80021B04` seats both shield parts at (`li a3,0x1000`).
const SHIELD_SCALE: u16 = 0x1000;

impl World {
    /// Whether Cort's **Mystic Shield** is up - retail's `_DAT_8007BD84`,
    /// the effect handle PROT 0940's `0xAC` arm 4 stores (`0x801F7678`) and
    /// the engine mirrors as [`legaia_engine_battle::monster_ai::MonsterAiState::flag_bd84`].
    /// While it is non-null the damage finisher halves every hit on an enemy
    /// defender (`FUN_801DDB30`, `0x801DDB98..0x801DDBC8`).
    pub(in crate::world) fn mystic_shield_up(&self) -> bool {
        self.battle.monster_ai_state.flag_bd84 != 0
    }

    /// The parts of the SCUS-resident move-VM record at `va`, parsed off the
    /// disc's executable ([`crate::world::DiscTables::scus`]).
    fn scus_record_parts(
        &self,
        va: u32,
    ) -> Option<(
        std::sync::Arc<[u8]>,
        Vec<legaia_overlay_images::summon_overlay::SummonPart>,
    )> {
        let scus = self.tables.scus.clone()?;
        let offs: Vec<usize> = SCUS_RECORD_RUN
            .iter()
            .map(|v| (v - SCUS_FILE_BIAS) as usize)
            .collect();
        let off = (va.checked_sub(SCUS_FILE_BIAS)?) as usize;
        let parts: Vec<_> = legaia_overlay_images::summon_overlay::parse_records_at(&scus, &offs)
            .into_iter()
            .filter(|p| p.record_off == off)
            .collect();
        (!parts.is_empty()).then_some((scus, parts))
    }

    /// Raise Cort's Mystic Shield the way PROT 0940's arm 4 does
    /// (`0x801F7640..0x801F7684`): seat the sphere and its texture scroller
    /// on the first monster seat - position its display trio `+0x3C..+0x40`,
    /// rotation `+0x44..+0x48`, scale `0x1000` - and store the sphere as the
    /// shield word `_DAT_8007BD84`.
    ///
    /// The word is raised whether or not the records resolve (a disc-free
    /// battle has no executable to read them from): its damage halve and the
    /// Evil Seru Magic gate do not depend on the visual.
    ///
    /// REF: FUN_801F7240 (arm 4's two `FUN_80021B04` calls and the
    /// `sw v0,-0x427C(v1)` at `0x801F7678`)
    pub fn raise_mystic_shield(&mut self) {
        self.battle.monster_ai_state.flag_bd84 = 1;
        let seat = self
            .engine_slot_for_retail_pool(legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT)
            .unwrap_or(legaia_engine_cast::cast_module_ticks::FIRST_MONSTER_SEAT);
        let Some((pos, rot)) = self.shield_anchor(usize::from(seat)) else {
            return;
        };
        let mut scene: Option<legaia_engine_effects::summon::SummonScene> = None;
        for va in [MYSTIC_SHIELD_SPHERE_RECORD, MYSTIC_SHIELD_SCROLLER_RECORD] {
            let Some((bytes, parts)) = self.scus_record_parts(va) else {
                return;
            };
            match scene.as_mut() {
                None => {
                    let mut s = legaia_engine_effects::summon::SummonScene::spawn_parts(
                        &parts,
                        &bytes,
                        crate::world::EFFECT_MODEL_LIBRARY_BASE,
                        pos,
                    );
                    s.pose_parts(rot, SHIELD_SCALE);
                    s.retail_wait_drain = true;
                    scene = Some(s);
                }
                Some(s) => s.push_parts_scaled(&parts, &bytes, pos, rot, SHIELD_SCALE),
            }
        }
        self.casting.mystic_shield_fx = scene;
    }

    /// The display trio `+0x3C..+0x40` and the rotation `+0x44..+0x48` of
    /// battle seat `slot`, as the shield's spawn and pose copies read them.
    fn shield_anchor(&self, slot: usize) -> Option<([i16; 3], [i16; 3])> {
        let p = self.battle_display_trio(slot)?;
        let a = self.actors.get(slot)?;
        Some((
            [p[0] as i16, p[1] as i16, p[2] as i16],
            [0, a.battle.facing_angle as i16, 0],
        ))
    }

    /// The shield's per-frame arm of the actor pass `FUN_8004CE2C`
    /// (`0x8004D534..0x8004D674`).
    ///
    /// Retail's gates, in order: the first formation id `gp[+0x9F4]`
    /// (`0x8007BD0C`) is `0xB4` and the shield word is non-null. Then, while
    /// the first monster seat's colour word `+0x04` is non-zero, the sphere is
    /// drawn (`+0x56 = 4`), takes the seat's render scale (`+0x22C -> +0x72`)
    /// and its display trio (`+0x3C..+0x43` copied over the node's
    /// `+0x14..+0x1B`); a zero colour word hides it (`sh zero,0x56(v1)` at
    /// `0x8004D670`) and skips the break test.
    ///
    /// The break: once the seat's HP `+0x14C` has fallen to `+0x14E >> 1` or
    /// below (`srl` / `sltu` at `0x8004D5CC..0x8004D5D4`), it restores the
    /// reaction run the shield's arm 4 blanked (`+0x1EF = 3`, `+0x1F0 = 2`,
    /// `+0x1F1 = 4`, `+0x1F2 = 5`), seats the burst `0x800786C4` on the
    /// sphere's position and rotation, retires the sphere (`+0x10 |= 8`) and
    /// clears the word (`sw zero` at `0x8004D658`) - so the finisher's halve
    /// lifts and the `0xB4` pick's Evil Seru Magic arm, gated on the word
    /// being null, opens.
    ///
    /// Not ported: the break's cue `0x10D` (`FUN_8004FE5C`).
    ///
    /// PORT: FUN_8004CE2C (the Mystic Shield arm, `0x8004D534..0x8004D674`)
    pub(in crate::world) fn tick_mystic_shield(&mut self) {
        const CORT_FIRST_FORM: u16 = 0xB4;
        if !self.mystic_shield_up() {
            return;
        }
        let seat = self.party.party_count as usize;
        let Some(a) = self.actors.get(seat) else {
            return;
        };
        if a.battle_monster_id != Some(CORT_FIRST_FORM) {
            return;
        }
        let visible = a.battle.render_color != 0;
        let anchor = self.shield_anchor(seat);
        if let Some(sphere) = self
            .casting
            .mystic_shield_fx
            .as_mut()
            .and_then(|s| s.parts.first_mut())
        {
            if visible {
                sphere.state.move_substate =
                    legaia_engine_effects::effect_sprite_arm::DRAW_KIND_MULTI as i16;
                sphere.state.field_72 = SHIELD_SCALE;
                if let Some((pos, _)) = anchor {
                    sphere.state.world_x = pos[0];
                    sphere.state.world_y = pos[1];
                    sphere.state.world_z = pos[2];
                    sphere.origin = Some(pos);
                }
            } else {
                sphere.state.move_substate = 0;
            }
        }
        if !visible {
            return;
        }
        let a = &mut self.actors[seat];
        if a.battle.hp > a.battle.max_hp >> 1 {
            return;
        }
        for (off, clip) in [(0x1EF, 3u8), (0x1F0, 2), (0x1F1, 4), (0x1F2, 5)] {
            if let Some(p) = a.battle.params.get_mut(off - 0x1DF) {
                *p = clip;
            }
        }
        // The burst on the sphere, then the sphere retired.
        let sphere_pose = self
            .casting
            .mystic_shield_fx
            .as_mut()
            .and_then(|s| s.parts.first_mut())
            .map(|p| {
                p.finished = true;
                (
                    [p.state.world_x, p.state.world_y, p.state.world_z],
                    [p.state.render_24, p.state.render_26, p.state.render_28],
                )
            });
        if let Some((pos, rot)) = sphere_pose
            && let Some((bytes, parts)) = self.scus_record_parts(MYSTIC_SHIELD_BURST_RECORD)
        {
            let mut s = legaia_engine_effects::summon::SummonScene::spawn_parts(
                &parts,
                &bytes,
                crate::world::EFFECT_MODEL_LIBRARY_BASE,
                pos,
            );
            s.pose_parts(rot, SHIELD_SCALE);
            s.retail_wait_drain = true;
            self.casting.active_action_fx.push(s);
        }
        self.battle.monster_ai_state.flag_bd84 = 0;
    }

    /// Advance the Mystic Shield's parts one frame through the move VM -
    /// the part tick `FUN_80021DF4` both parts get from the actor lists like
    /// every other effect actor. Drops the scene once every part is done.
    pub(in crate::world) fn tick_mystic_shield_fx(&mut self, channel_delta: u16) {
        let Some(mut scene) = self.casting.mystic_shield_fx.take() else {
            return;
        };
        {
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
                field_record_words: None,
                child_spawns: Vec::new(),
            };
            scene.channel_delta = channel_delta;
            scene.tick(&mut host, channel_delta);
        }
        self.queue_battle_part_scrolls(&mut scene.parts);
        if !scene.finished() {
            self.casting.mystic_shield_fx = Some(scene);
        }
    }
}
