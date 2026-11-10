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

impl Pick {
    /// The form the settings file and `LEGAIA_EXPLORER_ACTOR` use.
    pub(crate) fn key(self) -> String {
        match self {
            Pick::Character(c) => format!("char:{c}"),
            Pick::Monster(id) => format!("monster:{id}"),
        }
    }

    /// The inverse of [`Self::key`].
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.split_once(':')? {
            ("monster", id) => id.trim().parse().ok().map(Pick::Monster),
            ("char", slot) => slot.trim().parse().ok().map(Pick::Character),
            _ => None,
        }
    }
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
    pub(crate) pick: Option<Pick>,
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
        self.actors.pick = Pick::parse(who);
        self.actors.pending_pose = pose.and_then(|p| {
            let (c, f) = p.split_once(':')?;
            Some((c.trim().parse().ok()?, f.trim().parse().ok()?))
        });
    }

    pub(super) fn actors_mode(&mut self, ui: &mut egui::Ui) {
        let Some(lib) = self.explorer.library.clone() else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("Open a disc image first.");
            });
            return;
        };
        self.actors_auto();
        let side = self.side_width("actors", 330.0);
        let st = &mut self.actors;
        if st.catalogs.is_none() {
            st.catalogs = Some(actors::character_catalogs(&lib).map_err(|e| format!("{e:#}")));
        }
        if st.monsters.is_none() {
            st.monsters = Some(actors::monster_rows(&lib).map_err(|e| format!("{e:#}")));
        }

        // Up / Down walk the party, then the monsters the filter shows.
        let f = st.filter.to_ascii_lowercase();
        let shown = |r: &&actors::MonsterRow| {
            f.is_empty()
                || r.name.to_ascii_lowercase().contains(&f)
                || r.badge.is_some_and(|b| b.eq_ignore_ascii_case(&f))
                || r.id.to_string() == f
        };
        let order: Vec<Pick> = match (&st.catalogs, &st.monsters) {
            (Some(Ok(cats)), Some(Ok(rows))) => cats
                .iter()
                .map(|c| Pick::Character(c.cslot))
                .chain(rows.iter().filter(shown).map(|r| Pick::Monster(r.id)))
                .collect(),
            _ => Vec::new(),
        };
        let cur = order.iter().position(|p| st.pick == Some(*p));
        let walked = (!ui.egui_wants_keyboard_input() && !order.is_empty())
            .then(|| super::list_step(ui, cur, order.len() - 1))
            .flatten()
            .map(|i| order[i]);
        if let Some(p) = walked {
            st.pick = Some(p);
        }
        let side_max = super::side_panel_max(ui, 260.0);
        let panel = egui::Panel::left("actors")
            .resizable(true)
            .default_size(side.min(side_max))
            .min_size(260.0)
            .max_size(side_max)
            .show(ui, |ui| {
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
                match &st.monsters {
                    Some(Ok(rows)) => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                for r in rows.iter().filter(shown) {
                                    let sel = st.pick == Some(Pick::Monster(r.id));
                                    let text = format!(
                                        "{:3}  {:<16} {:<7} HP {}",
                                        r.id,
                                        r.name,
                                        r.badge.unwrap_or(""),
                                        r.hp
                                    );
                                    let row =
                                        ui.selectable_label(sel, RichText::new(text).monospace());
                                    if row.clicked() {
                                        st.pick = Some(Pick::Monster(r.id));
                                    }
                                    if walked == Some(Pick::Monster(r.id)) {
                                        row.scroll_to_me(None);
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
        self.side_widths
            .insert("actors", panel.response.rect.width());
        // Ctrl+E is left for the status line when nothing is shown.
        let key = self.actors.view.is_some() && self.take_export_key();
        let st = &mut self.actors;

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
        egui::CentralPanel::default().show(ui, |ui| {
            let Some(v) = &mut st.view else {
                ui.label("Pick a party member or a monster on the left.");
                ui.label(
                    RichText::new(
                        "The party can wear any equipment you pick. Every battle animation plays or steps \
                         frame by frame, and Export saves the model as a .glb.",
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
            ui.horizontal_wrapped(|ui| {
                ui.heading(&a.label);
                ui.separator();
                v.view.controls(ui, "texel x colour / 128 from the actor's own battle VRAM band");
                if ui
                    .button("Export .glb...")
                    .on_hover_text("Save this model with its animation clips as glTF binary (Ctrl+E)")
                    .clicked()
                    || key
                {
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
                        format!("{}: {name}", short_section(label))
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
                let changed = st.equipment[c]
                    .iter()
                    .take(cat.sections.len())
                    .filter(|&&id| id != 0)
                    .count();
                let mut pickers = |ui: &mut egui::Ui| {
                    // Flowed again in this `Ui`: under the header it is
                    // indented, so narrower.
                    for row in flow_rows(&widths, ui.available_width()) {
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
                };
                if rows.len() <= 1 {
                    pickers(ui);
                } else {
                    // Pickers that need two rows or more fold under one
                    // header, closed by default, so a narrow window keeps
                    // its height for the model view; the header says how
                    // much of the loadout is not the default.
                    egui::CollapsingHeader::new(equipment_header(changed))
                        .id_salt(("equip_fold", c))
                        .show(ui, pickers);
                }
            }
            if !a.clips.is_empty() {
                v.clip = v.clip.min(a.clips.len() - 1);
                let mut changed = false;
                ui.horizontal_wrapped(|ui| {
                    let before = v.clip;
                    // An art's label names its whole command string; uncut it
                    // pushes Play and the frame slider off a small window.
                    let cap = (ui.max_rect().width() * 0.4).clamp(100.0, 360.0);
                    // (A truncating combo box fills the width it is given, and
                    // a wrapping row gives it the whole row.)
                    let clip = &mut v.clip;
                    ui.scope(|ui| {
                        ui.set_max_width(cap);
                        egui::ComboBox::from_id_salt("clip")
                            .selected_text(a.clips[*clip].label.clone())
                            .truncate()
                            .show_ui(ui, |ui| {
                                for (i, c) in a.clips.iter().enumerate() {
                                    ui.selectable_value(
                                        clip,
                                        i,
                                        format!("{} ({} frames)", c.label, c.frames.len()),
                                    );
                                }
                            })
                            .response
                            .on_hover_text(a.clips[*clip].label.as_str());
                    });
                    if v.clip != before {
                        v.frame = 0;
                        v.since = Instant::now();
                        changed = true;
                    }
                    let n = a.clips[v.clip].frames.len();
                    let key = super::transport_key(ui);
                    let label = if v.playing { "Pause" } else { "Play" };
                    if ui.button(label).on_hover_text("Space").clicked()
                        || key == Some(super::Transport::PlayPause)
                    {
                        v.playing = !v.playing;
                        // Play resumes at the frame on screen: the clock is
                        // set back by the frames already shown.
                        let fps = a.clips[v.clip].fps.max(f32::EPSILON);
                        v.since = Instant::now()
                            .checked_sub(std::time::Duration::from_secs_f32(v.frame as f32 / fps))
                            .unwrap_or_else(Instant::now);
                    }
                    if let Some(step @ (super::Transport::Back | super::Transport::Forward)) = key
                        && n > 0
                    {
                        v.playing = false;
                        v.frame = if step == super::Transport::Back {
                            (v.frame + n - 1) % n
                        } else {
                            (v.frame + 1) % n
                        };
                        changed = true;
                    }
                    // egui's 100-point default left a long clip's frames a
                    // pixel apart; the slider takes the row's spare width,
                    // less its value box and label.
                    // (`available_width` in a wrapping row is the whole
                    // row's, not what is left of it.)
                    let left = ui.max_rect().right() - ui.cursor().left();
                    ui.spacing_mut().slider_width = (left - 130.0).clamp(80.0, 480.0);
                    if ui
                        .add(egui::Slider::new(&mut v.frame, 0..=n.saturating_sub(1)).text("frame"))
                        .on_hover_text(format!(
                            "{n} keyframes, played at {:.1} per second",
                            a.clips[v.clip].fps
                        ))
                        .changed()
                    {
                        v.playing = false;
                        changed = true;
                    }
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
            let caption = format!("{} triangles, {} clips", a.model.triangles(), a.clips.len());
            v.view.show(ui, &a.model, &a.vram, &caption);
        });

        if let Some(pick) = export {
            let name = match pick {
                Pick::Character(c) => format!("{}.glb", crate::actors::character_label(c)),
                Pick::Monster(id) => format!("monster_{id:03}.glb"),
            };
            let equipped = match pick {
                Pick::Character(c) => Some(self.actors.equipment[c.min(3)]),
                Pick::Monster(_) => None,
            };
            if let Some(out) = self.export_target(&name, "glb") {
                let lib = std::sync::Arc::clone(&lib);
                let folder = out.parent().map(std::path::Path::to_path_buf);
                self.start_export(name, folder, move || {
                    let glb = match (pick, equipped) {
                        (Pick::Character(c), Some(equipped)) => {
                            actors::character_glb(&lib, c, equipped)?
                        }
                        (Pick::Monster(id), _) => actors::monster_glb(&lib, id)?,
                        (Pick::Character(_), None) => unreachable!("set with the pick"),
                    };
                    crate::export::bytes(&glb, &out)?;
                    Ok(format!("glTF written to {}", out.display()))
                });
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

/// The folded equipment header: how many of the character's sections
/// carry something other than the default piece.
fn equipment_header(changed: usize) -> String {
    match changed {
        0 => "Equipment (all default)".to_string(),
        1 => "Equipment (1 piece changed)".to_string(),
        n => format!("Equipment ({n} pieces changed)"),
    }
}

/// A section picker's label without its index: `Section 2 (Weapon)` reads
/// `Weapon`, so the five pickers flow two or three to a row.
fn short_section(label: &str) -> &str {
    label
        .strip_prefix("Section ")
        .and_then(|rest| rest.split_once(" ("))
        .and_then(|(_, name)| name.strip_suffix(')'))
        .unwrap_or(label)
}

#[cfg(test)]
mod tests {
    use super::{Pick, flow_rows};

    /// The pick the settings file reopens reads back as written, and a hand
    /// edit that means nothing opens on no one.
    #[test]
    fn a_pick_round_trips_its_settings_form() {
        for p in [Pick::Character(2), Pick::Monster(10)] {
            assert_eq!(Pick::parse(&p.key()), Some(p));
        }
        for bad in ["", "monster", "monster:x", "npc:1"] {
            assert_eq!(Pick::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn pickers_flow_into_rows_that_fit() {
        assert_eq!(
            flow_rows(&[300.0, 300.0, 300.0], 700.0),
            vec![vec![0, 1], vec![2]]
        );
        assert_eq!(flow_rows(&[900.0, 100.0], 700.0), vec![vec![0], vec![1]]);
        assert!(flow_rows(&[], 700.0).is_empty());
    }

    #[test]
    fn section_labels_drop_their_index() {
        assert_eq!(super::short_section("Section 2 (Weapon)"), "Weapon");
        assert_eq!(super::short_section("Section 3 (Ra-Seru)"), "Ra-Seru");
        assert_eq!(super::short_section("Body"), "Body");
    }

    #[test]
    fn folded_equipment_header_counts_the_changed_pieces() {
        assert_eq!(super::equipment_header(0), "Equipment (all default)");
        assert_eq!(super::equipment_header(1), "Equipment (1 piece changed)");
        assert_eq!(super::equipment_header(3), "Equipment (3 pieces changed)");
    }
}
