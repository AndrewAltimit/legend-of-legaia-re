//! The "Characters & monsters" mode: the four party members assembled from
//! their player files with an equipment picker, the monster archive by name,
//! and their battle animation clips.

use std::time::Instant;

use eframe::egui::{self, RichText};

use super::App;
use super::viewport::Viewport;
use crate::actors::{self, Actor, CharacterCatalog, MonsterRow};
use crate::raster::Shading;

/// What is on the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    Character(usize),
    Monster(u16),
}

pub(crate) struct ActorView {
    pick: Pick,
    equipped: [u8; 5],
    actor: Result<Actor, String>,
    clip: usize,
    frame: usize,
    playing: bool,
    since: Instant,
    view: Viewport,
}

#[derive(Default)]
pub(crate) struct ActorsState {
    catalogs: Option<Result<Vec<CharacterCatalog>, String>>,
    monsters: Option<Result<Vec<MonsterRow>, String>>,
    filter: String,
    /// Chosen equipment per party member.
    equipment: [[u8; 5]; 4],
    pick: Option<Pick>,
    view: Option<ActorView>,
    /// Scripted `@clip:frame` to apply once (automation).
    pending_pose: Option<(usize, usize)>,
    auto_done: bool,
}

impl App {
    fn actors_auto(&mut self) {
        if self.actors.auto_done {
            return;
        }
        let Some(spec) = self.auto.actor.clone() else {
            self.actors.auto_done = true;
            return;
        };
        self.actors.auto_done = true;
        let (who, pose) = match spec.split_once('@') {
            Some((w, p)) => (w, Some(p)),
            None => (spec.as_str(), None),
        };
        self.actors.pick = match who.split_once(':') {
            Some(("monster", id)) => id.trim().parse().ok().map(Pick::Monster),
            Some(("char", slot)) => slot.trim().parse().ok().map(Pick::Character),
            _ => None,
        };
        self.actors.pending_pose = pose.and_then(|p| {
            let (c, f) = p.split_once(':')?;
            Some((c.trim().parse().ok()?, f.trim().parse().ok()?))
        });
    }

    pub(super) fn actors_mode(&mut self, ctx: &egui::Context) {
        let Some(lib) = self.studio.library.clone() else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Open a disc image first.");
            });
            return;
        };
        self.actors_auto();
        let st = &mut self.actors;
        if st.catalogs.is_none() {
            st.catalogs = Some(actors::character_catalogs(&lib).map_err(|e| format!("{e:#}")));
        }
        if st.monsters.is_none() {
            st.monsters = Some(actors::monster_rows(&lib).map_err(|e| format!("{e:#}")));
        }

        egui::SidePanel::left("actors")
            .resizable(true)
            .default_width(330.0)
            .min_width(260.0)
            .show(ctx, |ui| {
                ui.heading("Party");
                if let Some(Ok(cats)) = &st.catalogs {
                    for c in cats {
                        let sel = st.pick == Some(Pick::Character(c.cslot));
                        if ui.selectable_label(sel, c.name).clicked() {
                            st.pick = Some(Pick::Character(c.cslot));
                        }
                    }
                } else if let Some(Err(e)) = &st.catalogs {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.separator();
                ui.heading("Monsters");
                ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text("name or id"));
                let f = st.filter.to_ascii_lowercase();
                match &st.monsters {
                    Some(Ok(rows)) => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                for r in rows.iter().filter(|r| {
                                    f.is_empty()
                                        || r.name.to_ascii_lowercase().contains(&f)
                                        || r.badge.is_some_and(|b| b.eq_ignore_ascii_case(&f))
                                        || r.id.to_string() == f
                                }) {
                                    let sel = st.pick == Some(Pick::Monster(r.id));
                                    let text = format!(
                                        "{:3}  {:<16} {:<7} HP {}",
                                        r.id,
                                        r.name,
                                        r.badge.unwrap_or(""),
                                        r.hp
                                    );
                                    if ui
                                        .selectable_label(sel, RichText::new(text).monospace())
                                        .clicked()
                                    {
                                        st.pick = Some(Pick::Monster(r.id));
                                    }
                                }
                            });
                    }
                    Some(Err(e)) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    None => {}
                }
            });

        // (Re)build the actor when the pick or the loadout changed.
        if let Some(pick) = st.pick {
            let equipped = match pick {
                Pick::Character(c) => st.equipment[c.min(3)],
                Pick::Monster(_) => [0; 5],
            };
            let stale = st
                .view
                .as_ref()
                .is_none_or(|v| v.pick != pick || v.equipped != equipped);
            if stale {
                let actor = match pick {
                    Pick::Character(c) => actors::character(&lib, c, equipped),
                    Pick::Monster(id) => actors::monster(&lib, id),
                }
                .map_err(|e| format!("{e:#}"));
                let (clip, frame) = st.pending_pose.take().unwrap_or((0, 0));
                let mut v = ActorView {
                    pick,
                    equipped,
                    actor,
                    clip,
                    frame,
                    playing: st.view.as_ref().is_some_and(|v| v.playing),
                    since: Instant::now(),
                    view: Viewport::new(Shading::Textured),
                };
                if let Ok(a) = &mut v.actor {
                    a.pose(clip, frame);
                }
                st.view = Some(v);
            }
        }

        let mut export: Option<Pick> = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(v) = &mut st.view else {
                ui.label("Pick a party member or a monster on the left.");
                ui.label(
                    RichText::new(
                        "Party members are assembled from their player files (PROT 863..866) with \
                         the equipment you choose; monsters come from the monster archive (PROT 867).",
                    )
                    .weak(),
                );
                return;
            };
            let a = match &mut v.actor {
                Ok(a) => a,
                Err(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                    return;
                }
            };
            ui.horizontal(|ui| {
                ui.heading(&a.label);
                ui.label(format!("{} triangles, {} clips", a.model.triangles(), a.clips.len()));
                ui.separator();
                v.view.controls(ui, "texel x colour / 128 from the actor's own battle VRAM band");
                ui.separator();
                if ui.button("Export glTF (.glb)...").clicked() {
                    export = Some(v.pick);
                }
            });
            if let (Pick::Character(c), Some(Ok(cats))) = (v.pick, &st.catalogs)
                && let Some(cat) = cats.get(c)
            {
                // The pickers are flowed into rows by their measured width:
                // a combo box's size is only known once it is placed, so a
                // wrapping layout lets the last one run off the window.
                let font = egui::TextStyle::Button.resolve(ui.style());
                let extra = ui.spacing().button_padding.x * 2.0
                    + ui.spacing().icon_width
                    + ui.spacing().icon_spacing
                    + ui.spacing().item_spacing.x
                    + 8.0;
                let texts: Vec<String> = cat
                    .sections
                    .iter()
                    .enumerate()
                    .map(|(s, (label, ids))| {
                        let cur = st.equipment[c][s];
                        let name = ids
                            .iter()
                            .find(|(id, _)| *id == cur)
                            .map(|(_, n)| n.as_str())
                            .unwrap_or("?");
                        format!("{label}: {name}")
                    })
                    .collect();
                let widths: Vec<f32> = texts
                    .iter()
                    .map(|t| {
                        ui.painter()
                            .layout_no_wrap(t.clone(), font.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                            + extra
                    })
                    .collect();
                let rows = flow_rows(&widths, ui.available_width());
                for row in rows {
                    ui.horizontal(|ui| {
                        for s in row {
                            let ids = &cat.sections[s].1;
                            egui::ComboBox::from_id_salt(("equip", c, s))
                                .selected_text(texts[s].clone())
                                .show_ui(ui, |ui| {
                                    for (id, n) in ids {
                                        ui.selectable_value(&mut st.equipment[c][s], *id, n);
                                    }
                                });
                        }
                    });
                }
            }
            if !a.clips.is_empty() {
                v.clip = v.clip.min(a.clips.len() - 1);
                let mut changed = false;
                ui.horizontal(|ui| {
                    let before = v.clip;
                    egui::ComboBox::from_id_salt("clip")
                        .selected_text(a.clips[v.clip].label.clone())
                        .show_ui(ui, |ui| {
                            for (i, c) in a.clips.iter().enumerate() {
                                ui.selectable_value(
                                    &mut v.clip,
                                    i,
                                    format!("{} ({} frames)", c.label, c.frames.len()),
                                );
                            }
                        });
                    if v.clip != before {
                        v.frame = 0;
                        v.since = Instant::now();
                        changed = true;
                    }
                    let label = if v.playing { "Pause" } else { "Play" };
                    if ui.button(label).clicked() {
                        v.playing = !v.playing;
                        v.since = Instant::now();
                    }
                    let n = a.clips[v.clip].frames.len();
                    if ui
                        .add(egui::Slider::new(&mut v.frame, 0..=n.saturating_sub(1)).text("frame"))
                        .changed()
                    {
                        v.playing = false;
                        changed = true;
                    }
                    ui.label(
                        RichText::new(format!("{:.1} keyframes/s", a.clips[v.clip].fps)).weak(),
                    );
                });
                if v.playing {
                    let c = &a.clips[v.clip];
                    let f = (v.since.elapsed().as_secs_f32() * c.fps) as usize % c.frames.len().max(1);
                    if f != v.frame {
                        v.frame = f;
                        changed = true;
                    }
                    ui.ctx().request_repaint();
                }
                if changed {
                    a.pose(v.clip, v.frame);
                    v.view.version += 1;
                }
            }
            for n in &a.notes {
                ui.label(RichText::new(n).weak());
            }
            ui.label(RichText::new("Drag to orbit, scroll to zoom.").weak());
            v.view.show(ui, &a.model, &a.vram);
        });

        if let Some(pick) = export {
            let (name, res) = match pick {
                Pick::Character(c) => (
                    format!("{}.glb", crate::actors::character_label(c)),
                    actors::character_glb(&lib, c, self.actors.equipment[c.min(3)]),
                ),
                Pick::Monster(id) => (
                    format!("monster_{id:03}.glb"),
                    actors::monster_glb(&lib, id),
                ),
            };
            if let Some(out) = self.export_target(&name, "glb") {
                let res = res.and_then(|b| crate::export::bytes(&b, &out));
                self.report("glTF", res, &out);
            }
        }
    }
}

/// Indices of `widths` grouped into rows that each fit `max` (a row always
/// takes at least one item, so an item wider than `max` gets a row alone).
fn flow_rows(widths: &[f32], max: f32) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for (i, w) in widths.iter().enumerate() {
        match rows.last_mut() {
            Some(row) if used + w <= max => {
                row.push(i);
                used += w;
            }
            _ => {
                rows.push(vec![i]);
                used = *w;
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::flow_rows;

    #[test]
    fn pickers_flow_into_rows_that_fit() {
        assert_eq!(
            flow_rows(&[300.0, 300.0, 300.0], 700.0),
            vec![vec![0, 1], vec![2]]
        );
        assert_eq!(flow_rows(&[900.0, 100.0], 700.0), vec![vec![0], vec![1]]);
        assert!(flow_rows(&[], 700.0).is_empty());
    }
}
