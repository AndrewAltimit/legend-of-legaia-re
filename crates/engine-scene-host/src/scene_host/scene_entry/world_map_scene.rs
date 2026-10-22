//! `SceneHost::enter_world_map_scene`: entering a kingdom's overworld scene.

use super::*;

impl SceneHost {
    /// Enter `name` as the **overworld** (world-map) scene.
    ///
    /// The counterpart to [`Self::enter_field_scene`] for the three kingdom
    /// overworld scenes ([`is_world_map_scene`]). It runs the full field-entry
    /// load first - the world-map-walk overlay shares the field's locomotion,
    /// walkability grid, and asset pipeline (see
    /// `docs/subsystems/world-map.md`) - then:
    ///
    /// 1. Routes the region-keyed random-encounter table from the scene's MAN
    ///    ([`legaia_engine_battle::region_encounter::region_encounter_table_from_man`]) onto
    ///    the overworld via [`legaia_engine_core::world::World::set_world_map_regions`], so
    ///    `tick_world_map`'s per-tile roll fires real encounters.
    /// 2. Switches the world into [`legaia_engine_mode::mode::SceneMode::WorldMap`]
    ///    (installs the camera controller); the player actor + collision grid
    ///    that `enter_field_scene` set up stay in place.
    ///
    /// 3. Installs the scene's **interactive overworld entities** - the
    ///    portals (town/dungeon entrances) and dialog NPCs decoded from the
    ///    MAN actor-placement table and classified by their field-VM scripts
    ///    ([`legaia_engine_field::man_field_scripts::classify_placements`]). Decorative /
    ///    model-only placements are skipped; the entity *kind* comes from the
    ///    per-entity script, so this is disc-sourced, not synthetic.
    ///
    /// The random-encounter driver (the region table) is fully sourced from
    /// the MAN; the per-entity auto-engage trigger (walk onto a portal tile)
    /// is still host-driven via [`legaia_engine_core::world::World::engage_world_map_entity`].
    pub fn enter_world_map_scene(&mut self, name: &str) -> Result<()> {
        // Full field-entry load: resources, walkability grid, player, monster
        // catalog, scene label. Leaves the world in `Field` mode.
        self.enter_field_scene(name, 0)?;
        // Decode the MAN once, then derive the region table + the typed entity
        // configs from it while only `self.scene` / `self.index` are borrowed
        // (both immutable), so the owned results outlive the borrow before the
        // mutable `world` accesses below.
        let man_bytes = self
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&self.index).ok().flatten());
        let table = man_bytes.as_ref().and_then(|man| {
            legaia_engine_battle::region_encounter::region_encounter_table_from_man(name, man)
        });
        // Each interactive placement → (config, spawn world position). The
        // positions drive the auto-engage-on-walkover trigger; Plain
        // (decorative / model-only) placements are skipped.
        let mut entities: Vec<(legaia_engine_field::world_map_entity_config::WorldMapEntityConfig, (i16, i16))> = man_bytes
            .as_ref()
            .and_then(|man| {
                legaia_scene_formats::man_section::parse(man)
                    .ok()
                    .map(|mf| (mf, man))
            })
            .map(|(mf, man)| {
                use legaia_engine_field::man_field_scripts::PlacementKind;
                legaia_engine_field::man_field_scripts::classify_placements(&mf, man)
                    .into_iter()
                    .filter_map(|(p, kind)| {
                        let cfg = match kind {
                            // `PlacementKind::Portal` is gated by
                            // `is_genuine_warp` to `op0` in `100..=106`, so its
                            // payload is the mode-24 minigame sub-id
                            // (`op0 - 100`), not a map id - the same value the
                            // field-VM and walk-touch arms carry. On the three
                            // overworld scenes the disc's only such placements
                            // are the `map02` / `map03` fishing signboards.
                            PlacementKind::Portal { target_map } => {
                                legaia_engine_field::world_map_entity_config::WorldMapEntityConfig::MinigameDoor {
                                    sub_id: target_map,
                                    slot: u8::try_from(p.index).unwrap_or(u8::MAX),
                                }
                            }
                            PlacementKind::Npc {
                                interact_id,
                                dialog_inline,
                            } => legaia_engine_field::world_map_entity_config::WorldMapEntityConfig::Npc {
                                interact_id: interact_id.unwrap_or(0),
                                // No `text_id` from the MAN classifier: the `0x3F`
                                // op it was sourced from is the scene-change
                                // opcode, not a dialog op. The real NPC text is the
                                // structural `inline` block below.
                                text_id: None,
                                inline: dialog_inline.unwrap_or_default(),
                            },
                            PlacementKind::Plain => return None,
                        };
                        Some((cfg, (p.world_x, p.world_z)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Overworld town/dungeon entrances: the `.MAP` walk-on kind-1 tile
        // triggers joined to their partition-2 records' `0x3F`
        // named-scene-change ops (see
        // [`legaia_engine_field::man_field_scripts::overworld_portal_sites`]). This is the
        // real overworld hop mechanism - `map01` has NO partition-1 `Portal`
        // placements (the classifier finds zero there); its town/dungeon
        // entrances are these gate-1 triggers. Each site becomes an
        // `OverworldPortal` entity at its trigger tile centre
        // (`world = tile*128 + 0x40`, the same tile->world mapping the placement
        // spawns and the arrival seat use), and the auto-engage-on-walkover
        // trigger fires it when the player steps onto that tile.
        if let (Some(man), Some(scene)) = (man_bytes.as_ref(), self.scene.as_ref())
            && let Ok(mf) = legaia_scene_formats::man_section::parse(man)
            && let Ok((primary, fallback)) = scene.field_tile_triggers(&self.index)
        {
            let mut triggers = primary;
            triggers.extend(fallback);
            for site in
                legaia_engine_field::man_field_scripts::overworld_portal_sites(&mf, man, &triggers)
            {
                // Story-gate the entrance by its partition-2 record's C1/C2
                // gates (retail `FUN_8003BDE0`): C1 blocks the spawn if ANY
                // listed flag is set, C2 requires ALL set. Most overworld
                // entrances carry empty gates and install unconditionally; the
                // Ravine (`keikoku`) portals carry `C1=[0x193]` so they drop
                // out of the installed set once that story flag latches. A fresh
                // Drake arrival (flag clear) keeps them reachable. See
                // [`docs/subsystems/world-map.md`].
                if let Some((c1, c2)) =
                    legaia_engine_field::man_field_scripts::partition2_record_gates(
                        &mf,
                        man,
                        site.record as usize,
                    )
                    && !self.world.p2_record_gates_pass(&c1, &c2)
                {
                    continue;
                }
                let world = (
                    i16::from(site.overworld_x) * 128 + 0x40,
                    i16::from(site.overworld_z) * 128 + 0x40,
                );
                // Story-conditional entrance: the record selects its
                // destination by op-0x70 flag branches (retail's post-beat
                // dungeon-variant entrance, e.g. `map01`'s dolk -> dolk2 on flag
                // `0x142`, or `map03`'s nested CONCNOW / CONCEND / closed
                // tests). Run the record's branches against the live bank, as
                // the field VM does on the crossing; a path that parks leaves
                // the entrance shut, so no portal is installed. A record the
                // walk cannot decide keeps the static primary / flag-SET pair.
                use legaia_engine_field::man_field_scripts::{
                    RecordPathEnd, partition2_record_path_scene_change,
                };
                let live =
                    partition2_record_path_scene_change(&mf, man, usize::from(site.record), &|f| {
                        self.world.system_flag_test(f)
                    });
                let (scene_name, index, entry_x, entry_z, dir) = match (live, site.conditional) {
                    (Some(RecordPathEnd::SceneChange((index, name, ex, ez, dir))), _) => {
                        (name, index, ex, ez, dir)
                    }
                    (Some(RecordPathEnd::Closed), _) => continue,
                    (_, Some(cd)) if self.world.system_flag_test(cd.flag) => {
                        (cd.scene_name, cd.index, cd.entry_x, cd.entry_z, cd.dir)
                    }
                    _ => (
                        site.scene_name,
                        site.index,
                        site.entry_x,
                        site.entry_z,
                        site.dir,
                    ),
                };
                entities.push((
                    legaia_engine_field::world_map_entity_config::WorldMapEntityConfig::OverworldPortal {
                        scene_name,
                        index,
                        entry_x,
                        entry_z,
                        dir,
                        record: site.record,
                        object: false,
                    },
                    world,
                ));
            }
            // Object-bound entrances: a `.MAP` object whose key tile binds
            // (gate 0, `FUN_8003A55C`) a record that runs to a `0x3F` on
            // contact. The world map's entrances are mostly the gate-1
            // walk-ons above, but `map01`'s Garmel mouth is object `P0[6]`:
            // its record tests `0x2C5` / `0x19A` (the latter parks it shut)
            // and otherwise raises flag 2 and changes to GARMEL. The record
            // is walked against the live flags, so a closed path installs
            // nothing; the portal sits at the object's contact centre.
            // REF: FUN_8003A55C, FUN_801CFC40
            let map_bytes = scene
                .field_map_index(&self.index)
                .and_then(|idx| self.index.entry_bytes_extended(idx).ok());
            if let Some(map) = map_bytes.as_deref() {
                use legaia_engine_field::man_field_scripts::{
                    RecordPathEnd, flat_record_path_walk,
                };
                for (flat, contact) in
                    legaia_engine_field::man_field_scripts::object_script_binds(map, &triggers)
                {
                    let Ok(record) = u8::try_from(flat) else {
                        continue;
                    };
                    let Some((
                        RecordPathEnd::SceneChange((index, scene_name, entry_x, entry_z, dir)),
                        _,
                    )) = flat_record_path_walk(&mf, man, flat, &|f| self.world.system_flag_test(f))
                    else {
                        continue;
                    };
                    let tile = (contact.0 >> 7, contact.1 >> 7);
                    let dup = entities.iter().any(|(cfg, pos)| {
                        matches!(cfg, legaia_engine_field::world_map_entity_config::WorldMapEntityConfig::OverworldPortal {
                            scene_name: s, ..
                        } if *s == scene_name)
                            && (pos.0 >> 7, pos.1 >> 7) == tile
                    });
                    if dup {
                        continue;
                    }
                    entities.push((
                        legaia_engine_field::world_map_entity_config::WorldMapEntityConfig::OverworldPortal {
                            scene_name,
                            index,
                            entry_x,
                            entry_z,
                            dir,
                            record,
                            object: true,
                        },
                        contact,
                    ));
                }
            }
        }
        if let Some(table) = table {
            log::info!(
                "world-map '{name}': routed {} encounter region(s)",
                table.regions.len()
            );
            self.world.set_world_map_regions(table);
        }
        if !entities.is_empty() {
            log::info!(
                "world-map '{name}': installed {} interactive entit(ies) from placements",
                entities.len()
            );
            self.world.install_world_map_entities_at(entities);
        }
        // Switch to world-map mode (idempotent; keeps the installed player +
        // collision grid).
        self.world.enter_world_map();
        Ok(())
    }
}
