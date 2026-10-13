//! The overworld **map display** - the top-down survey screen the L1 edge
//! opens on a kingdom map, with a name label over every discovered place.
//!
//! Retail runs it as game mode `0x0D` out of PROT 0981, a `0x1000`-byte
//! module swapped into slot A over the field overlay's head. The image holds
//! one framed function and three frameless leaves, all ported here:
//!
//! | Retail | Port |
//! |---|---|
//! | `FUN_801CF4AC` enter / reset | [`MapDisplay::enter`] |
//! | `FUN_801CE850` per-frame tick | [`MapDisplay::tick`] |
//! | `FUN_801CF5E8` location stepper | [`step_cursor`] |
//! | `FUN_801CF678` camera clamp | [`clamp_camera`] |
//!
//! The tick is a state machine over the mode word `0x801CF76C`:
//!
//! | Mode | What the frame does |
//! |---|---|
//! | `0` | seed the fade timer with `4` |
//! | `1` | stage the survey camera (`TR`, pitch, projection, tile window), flatten the curvature table |
//! | `2` | count the timer down - the fade back in from the black the entry ramp left - then seat the camera over the player |
//! | `0x32` | interactive: pan, step between places, cycle the overlays, draw |
//! | `3` | count the timer up to `5` - the fade out - then restore the walk camera |
//! | `4` | spawn the walk frame's fade-in |
//! | `5` | hand the frame back to the walk mode |
//!
//! Every pad literal below is a **packed** bit (`_DAT_8007B850` held,
//! `_DAT_8007B874` newly pressed), the layout
//! [`crate::world_map_panel_host::packed_pad`] converts a raw word into.
//!
//! What the module also does that this kernel does not: spawn the player's
//! map pointer (a move-VM effect, program at `0x801CF6E8`, through
//! `FUN_80021B04`) and tick it. [`MapDisplay::pointer_alive`] carries its
//! lifetime so a host that draws one knows when; nothing draws it yet.
//!
//! See `ghidra/scripts/funcs/overlay_monster_test_0981_801ce850.txt` and its
//! three siblings. The `monster_test` stem is CDNAME inheritance; the image
//! is the map display.

/// Interactive mode (`li v0,0x32` at `0x801CE9EC`). Not a jump-table arm: the
/// dispatcher's `sltiu a0,6` falls through and the tail runs alone.
pub const MODE_INTERACTIVE: u32 = 0x32;

/// SFX cue for the exit press (`li v1,0x37; sh v1,-0x4928(v0)` at
/// `0x801CECC8`): a bare store into ring slot 0, `0x8007B6D8`.
pub const SFX_EXIT: i16 = 0x37;
/// SFX cue for a cursor step or an overlay cycle (`li v0,0x20` at
/// `0x801CEE6C` / `0x801CEEA0` / `0x801CEFEC`), stored the same way.
pub const SFX_STEP: i16 = 0x20;

/// Packed edge mask that leaves the screen (`andi v0,v0,0x21` at
/// `0x801CECA8`): L2 or Circle.
pub const PAD_EXIT: u16 = 0x0021;
/// Packed edge mask that cycles the overlays (`andi v0,v0,0x110` at
/// `0x801CEFD8`): Triangle or Select.
pub const PAD_CYCLE: u16 = 0x0110;
/// Packed edge bit that steps to the next place (`andi v0,v0,0x20` at
/// `0x801CEE54`). It is Circle, which [`PAD_EXIT`] tests first on the same
/// frame, so retail never reaches this arm; it is ported as written.
pub const PAD_NEXT: u16 = 0x0020;
/// Packed edge bit that steps to the previous place (`andi v0,v0,0x80` at
/// `0x801CEE8C`): Square.
pub const PAD_PREV: u16 = 0x0080;
/// Packed held bits that pan the camera, one tile a frame
/// (`0x801CEDC8..0x801CEE44`).
pub const PAD_LEFT: u16 = 0x8000;
pub const PAD_RIGHT: u16 = 0x2000;
pub const PAD_UP: u16 = 0x1000;
pub const PAD_DOWN: u16 = 0x4000;

/// One camera step: a 128-unit map tile.
pub const PAN_STEP: i32 = 0x80;

/// The survey camera's eye-space translation `TR` (`0x800840B8` trio,
/// stored at `0x801CE8EC..0x801CE8FC`).
pub const CAMERA_TR: [i32; 3] = [0, 0, 0x9470];
/// The survey camera's rotation trio (`0x8007B790`, `0x801CE904..0x801CE914`):
/// pitch `0x300` of a `0x1000` turn, no yaw, no roll.
pub const CAMERA_ROT: [i16; 3] = [0x300, 0, 0];
/// The survey camera's projection distance (`0x8007B6F4`, `li v0,0x170`).
pub const CAMERA_H: i32 = 0x170;
/// The visible tile window the survey camera sets (`0x1F8003E8..EB` as
/// `[x_lo, z_lo, x_hi, z_hi]`; `0x801CE92C..0x801CE950`).
pub const TILE_WINDOW: [i8; 4] = [-0x1C, -0x1A, 0x1C, 0x1A];

/// World height every label is projected at (`li v0,-0x200; sh v0,0x32(sp)`
/// at `0x801CF3C0`): 512 units above the map plane.
pub const LABEL_WORLD_Y: i16 = -0x200;

/// The exit's fade-in, the arguments of `FUN_801D58F0` at `0x801CEB98`:
/// `(kind, from_rgb, to_rgb, delay, duration, hold)`.
pub const EXIT_FADE: (i16, u32, u32, i16, i16, i16) = (2, 0x00FF_FFFF, 0, 0, 8, 0);

/// One record of the world-map location table (`DAT_80073EE0`), as the
/// display reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLocation {
    /// Kingdom the place belongs to (`+0x00`).
    pub region: u8,
    /// Map cell (`+0x01`, `+0x02`); world coordinate = `cell << 7`.
    pub map_x: u8,
    pub map_y: u8,
    /// Discovery flag (`+0x03`), a system-flag index.
    pub discovery_flag: u16,
    /// The name's bytes up to its NUL (`+0x08`).
    pub name: Vec<u8>,
}

/// A screen rectangle with its texture source: one `FUN_8001CD68` call
/// (`x, y, w, h, u, v, tpage, clut, semi`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeSprite {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
    pub u: u8,
    pub v: u8,
    pub tpage: u16,
    pub clut: u16,
    pub semi: bool,
}

/// A compass letter: its sprite backing and where the glyph renderer
/// `FUN_80036888` puts the letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompassMark {
    pub letter: u8,
    pub pen: (i16, i16),
    pub sprite: ChromeSprite,
}

/// The mini-map panel of overlay mode `0`, in retail's call order
/// (`0x801CF020..0x801CF344`). `AddPrim` links at the head of the bucket, so
/// the GPU draws these last to first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapChrome {
    /// The player's dot on the mini-map.
    pub player_dot: ChromeSprite,
    /// The box showing which part of the map the camera frames.
    pub view_box: ChromeSprite,
    /// The mini-map itself.
    pub minimap: ChromeSprite,
    /// The semi-transparent backing quad `(x0, y0, x1, y1)` and its GP0
    /// colour word (`0x2A505050`, with `SetDrawMode` tpage `0x4C`).
    pub backing: (i16, i16, i16, i16),
    /// `E`, `W`, `N`, `S`.
    pub compass: [CompassMark; 4],
}

/// GP0 colour of [`MapChrome::backing`] (`lui v1,0x2a50; ori v1,v1,0x5050`).
pub const BACKING_RGB: [u8; 3] = [0x50, 0x50, 0x50];

/// What one tick asked the host for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapDisplayTick {
    /// Cue ids stored into SFX ring slot 0 this frame.
    pub sfx: Vec<i16>,
    /// Mode `4` ran: spawn [`EXIT_FADE`].
    pub spawn_exit_fade: bool,
    /// Mode `5` ran: the map display is over and the walk frame resumes.
    pub exit: bool,
}

/// The map display's state: the module's zeroed block at
/// `0x801CF70C..0x801CF790`, less the walk-camera snapshot a port that
/// recomputes its walk camera every frame has no use for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDisplay {
    /// Mode word (`0x801CF76C`).
    pub mode: u32,
    /// Fade timer (`0x801CF778`).
    pub timer: i32,
    /// Location cursor (`0x801CF77C`): `-1` = the player, else a record index.
    pub cursor: i32,
    /// Camera seat state (`0x801CF784`): `0` = pick a target, `1` = move to
    /// it, `2` = seated, pad pans.
    pub seat: u32,
    /// Overlay mode (`0x801CF73C`): `0` = mini-map and labels, `1` = labels
    /// only, `2` = neither.
    pub overlay: u32,
    /// Camera translation X / Z (`_DAT_80089118` / `_DAT_80089120`): the
    /// negated world position the survey looks at.
    pub cam_x: i32,
    pub cam_z: i32,
    /// The seat target (`0x801CF74C` / `0x801CF754`).
    pub target_x: i32,
    pub target_z: i32,
    /// Mode `1` has staged the survey camera and mode `3` has not yet
    /// restored the walk one.
    pub camera_staged: bool,
    /// The player's map pointer is alive (spawned in mode `1`, killed in
    /// mode `4`).
    pub pointer_alive: bool,
}

/// `-v` truncated toward zero to a whole tile - the `subu; bgez; addiu 0x7f;
/// sra 7; sll 7` run at `0x801CE9FC..0x801CEA10`.
fn neg_to_tile(v: i32) -> i32 {
    let n = -v;
    let n = if n < 0 { n + 0x7F } else { n };
    (n >> 7) << 7
}

/// `v / 128` truncated toward zero (`bgez; addiu 0x7f; sra 7`).
fn tile_of(v: i32) -> i32 {
    (if v < 0 { v + 0x7F } else { v }) >> 7
}

/// `v * 2 / 3` truncated toward zero - the `sll 1; mult 0x55555556; mfhi;
/// subu (v >> 31)` run the mini-map scales a tile index with.
fn two_thirds(v: i32) -> i32 {
    (v * 2) / 3
}

/// Bound the survey camera to the kingdom's interior.
///
/// PORT: FUN_801CF678
pub fn clamp_camera(cam_x: &mut i32, cam_z: &mut i32) {
    if *cam_x >= -0xCFF {
        *cam_x = -0xD00;
    }
    if *cam_x < -0x3380 {
        *cam_x = -0x3380;
    }
    if *cam_z >= -0xBFF {
        *cam_z = -0xC00;
    }
    if *cam_z < -0x3580 {
        *cam_z = -0x3580;
    }
}

/// Walk `cursor` to the next record of `kingdom`, forward (`backward ==
/// false`) or backward, and return it.
///
/// Forward, running off the end parks the cursor on `-1` (the player).
/// Backward, a negative cursor wraps to the last record first. The discovery
/// flag is **not** consulted: the cursor visits a place the labels still
/// hide.
///
/// Retail's backward walk never terminates on a table with no record of the
/// kingdom; the port gives up after one lap and parks on `-1`.
///
/// PORT: FUN_801CF5E8
pub fn step_cursor(cursor: i32, backward: bool, locations: &[MapLocation], kingdom: u32) -> i32 {
    let count = locations.len() as i32;
    let mut c = cursor;
    for _ in 0..=count {
        if backward {
            if c < 0 {
                c = count - 1;
                if c < 0 {
                    return -1;
                }
            }
        } else if c >= count {
            return -1;
        }
        let Some(r) = usize::try_from(c).ok().and_then(|i| locations.get(i)) else {
            return -1;
        };
        if u32::from(r.region) == kingdom {
            return c;
        }
        c += if backward { -1 } else { 1 };
    }
    -1
}

impl MapDisplay {
    /// Open the display: mode `0`, cursor on the player, overlays on.
    ///
    /// PORT: FUN_801CF4AC
    pub fn enter() -> Self {
        Self {
            mode: 0,
            timer: 0,
            cursor: -1,
            seat: 0,
            overlay: 0,
            cam_x: 0,
            cam_z: 0,
            target_x: 0,
            target_z: 0,
            camera_staged: false,
            pointer_alive: false,
        }
    }

    /// This frame's subtractive full-screen fade level, or `None` on the one
    /// frame (mode `0`) retail draws none - `FUN_80024EE4(0, 2, grey *
    /// 0x010101)` at `0x801CEC4C..0x801CEC90`, read after the mode arm ran.
    pub fn fade_grey(&self) -> Option<u8> {
        if self.mode == 0 {
            return None;
        }
        let v = self.timer << 6;
        Some(if v >= 0x100 { 0xFF } else { v.max(0) as u8 })
    }

    /// Whether the labels draw this frame: interactive, and the overlay mode
    /// is not `2` (`beq v1,v0` at `0x801CF360`).
    pub fn labels_shown(&self) -> bool {
        self.mode == MODE_INTERACTIVE && self.overlay != 2
    }

    /// The indices of the records whose label draws this frame, in table
    /// order (`0x801CF368..0x801CF438`): the record's kingdom is the live
    /// one, and its discovery flag is set or `show_all` (`_DAT_8007B868`) is.
    pub fn visible_labels(
        &self,
        locations: &[MapLocation],
        kingdom: u32,
        show_all: bool,
        flag_set: impl Fn(u16) -> bool,
    ) -> Vec<usize> {
        if !self.labels_shown() {
            return Vec::new();
        }
        locations
            .iter()
            .enumerate()
            .filter(|(_, r)| u32::from(r.region) == kingdom)
            .filter(|(_, r)| flag_set(r.discovery_flag) || show_all)
            .map(|(i, _)| i)
            .collect()
    }

    /// The mini-map panel, or `None` unless the frame is interactive with
    /// overlay mode `0` (`bne v0,zero` at `0x801CF018`). `player` is the
    /// leader's world `(x, z)`.
    pub fn chrome(&self, player: (i32, i32)) -> Option<MapChrome> {
        if self.mode != MODE_INTERACTIVE || self.overlay != 0 {
            return None;
        }
        const SHEET_TPAGE: u16 = 0x0D;
        const SHEET_CLUT: u16 = 0x77C0;
        let dot_x = two_thirds(tile_of(player.0)) + 0x0B;
        let dot_y = two_thirds(0x80 - tile_of(player.1)) + 0x81;
        let box_x = (two_thirds(tile_of(-self.cam_x)) - 0x0F).max(3);
        let box_y = (two_thirds(tile_of(self.cam_z)) + 0xC3).max(0x7B);
        let mark = |letter: u8, pen: (i16, i16), at: (i16, i16)| CompassMark {
            letter,
            pen,
            sprite: ChromeSprite {
                x: at.0,
                y: at.1,
                w: 0x10,
                h: 0x10,
                u: 0,
                v: 0x28,
                tpage: SHEET_TPAGE,
                clut: SHEET_CLUT,
                semi: false,
            },
        };
        Some(MapChrome {
            player_dot: ChromeSprite {
                x: dot_x as i16,
                y: dot_y as i16,
                w: 3,
                h: 3,
                u: 0,
                v: 0x10,
                tpage: SHEET_TPAGE,
                clut: SHEET_CLUT,
                semi: true,
            },
            view_box: ChromeSprite {
                x: box_x as i16,
                y: box_y as i16,
                w: 0x30,
                h: 0x30,
                u: 0x10,
                v: 0,
                tpage: SHEET_TPAGE,
                clut: SHEET_CLUT,
                semi: false,
            },
            minimap: ChromeSprite {
                x: 4,
                y: 0x7C,
                w: 0x60,
                h: 0x60,
                u: 0x80,
                v: 0,
                tpage: 0x2D,
                clut: 0x77D0,
                semi: true,
            },
            backing: (3, 0x7B, 0x65, 0xDD),
            compass: [
                mark(b'E', (0x120, 0x68), (0x11E, 0x66)),
                mark(b'W', (0x10, 0x68), (0x0E, 0x66)),
                mark(b'N', (0x98, 0x0A), (0x96, 0x08)),
                mark(b'S', (0x98, 0xCA), (0x96, 0xC8)),
            ],
        })
    }

    /// One mode-`0x0D` frame.
    ///
    /// `held` / `edge` are the packed pad words, `player` the leader's world
    /// `(x, z)`, `locations` the kingdom MAN's table and `kingdom` the live
    /// kingdom index (`0x8007B970`).
    ///
    /// PORT: FUN_801CE850
    pub fn tick(
        &mut self,
        held: u16,
        edge: u16,
        player: (i32, i32),
        locations: &[MapLocation],
        kingdom: u32,
    ) -> MapDisplayTick {
        let mut out = MapDisplayTick::default();
        match self.mode {
            // 0x801CE8B0
            0 => {
                self.timer = 4;
                self.pointer_alive = false;
                self.mode = 1;
            }
            // 0x801CE8D8: the survey camera's constants go in; the camera
            // translation is still the walk frame's, clamped.
            1 => {
                self.cam_x = -player.0;
                self.cam_z = -player.1;
                self.camera_staged = true;
                self.mode = 2;
                clamp_camera(&mut self.cam_x, &mut self.cam_z);
                self.pointer_alive = true;
            }
            // 0x801CE9C4
            2 => {
                self.timer -= 1;
                if self.timer < 0 {
                    self.timer = 0;
                    self.mode = MODE_INTERACTIVE;
                    self.cam_x = neg_to_tile(player.0);
                    self.cam_z = neg_to_tile(player.1);
                    self.target_x = self.cam_x;
                    self.target_z = self.cam_z;
                }
            }
            // 0x801CEA74
            3 => {
                self.timer += 1;
                if self.timer >= 5 {
                    self.camera_staged = false;
                    self.mode = 4;
                }
            }
            // 0x801CEB98
            4 => {
                self.pointer_alive = false;
                out.spawn_exit_fade = true;
                self.mode = 5;
            }
            // 0x801CEBE8
            5 => {
                out.exit = true;
            }
            _ => {}
        }

        // 0x801CEC9C..0x801CECD4: the exit press, live in the interactive
        // mode only.
        if edge & PAD_EXIT != 0 && self.mode == MODE_INTERACTIVE {
            out.sfx.push(SFX_EXIT);
            self.mode = 3;
        }
        // 0x801CECD8..0x801CECF8: every other mode is done here.
        if self.mode < MODE_INTERACTIVE {
            return out;
        }

        // 0x801CED00..0x801CEDA4: pick the seat target.
        if self.seat == 0 {
            self.seat = 1;
            if self.cursor >= 0 {
                if let Some(r) = locations.get(self.cursor as usize) {
                    self.target_x = -(i32::from(r.map_x) << 7);
                    self.target_z = -(i32::from(r.map_y) << 7);
                }
            } else {
                self.target_x = neg_to_tile(player.0);
                self.target_z = neg_to_tile(player.1);
            }
        }

        if self.seat == 2 {
            // 0x801CEDBC..0x801CEE44: pan.
            if held & PAD_LEFT != 0 {
                self.cam_x += PAN_STEP;
            }
            if held & PAD_RIGHT != 0 {
                self.cam_x -= PAN_STEP;
            }
            if held & PAD_UP != 0 {
                self.cam_z -= PAN_STEP;
            }
            if held & PAD_DOWN != 0 {
                self.cam_z += PAN_STEP;
            }
            // 0x801CEE48..0x801CEEC4: step between places.
            if edge & PAD_NEXT != 0 {
                out.sfx.push(SFX_STEP);
                self.seat = 0;
                self.cursor = step_cursor(self.cursor + 1, false, locations, kingdom);
            }
            if edge & PAD_PREV != 0 {
                out.sfx.push(SFX_STEP);
                self.seat = 0;
                self.cursor -= 1;
                if self.cursor != -1 {
                    self.cursor = step_cursor(self.cursor, true, locations, kingdom);
                }
            }
        } else {
            // 0x801CEECC..0x801CEF84: the camera takes the target outright -
            // retail copies the vector before its one-tile-a-frame ease, so
            // the ease never has a distance to close - and seats.
            self.cam_x = self.target_x;
            self.cam_z = self.target_z;
            self.seat = 2;
        }

        // 0x801CEF88..0x801CEFB8
        let count = locations.len() as i32;
        if self.cursor < -1 {
            self.cursor = count - 1;
        }
        if self.cursor >= count {
            self.cursor = -1;
        }
        clamp_camera(&mut self.cam_x, &mut self.cam_z);

        // 0x801CEFCC..0x801CF008
        if edge & PAD_CYCLE != 0 {
            out.sfx.push(SFX_STEP);
            self.overlay += 1;
            if self.overlay >= 3 {
                self.overlay = 0;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(region: u8, x: u8, y: u8) -> MapLocation {
        MapLocation {
            region,
            map_x: x,
            map_y: y,
            discovery_flag: 0x484 + u16::from(x),
            name: b"Place".to_vec(),
        }
    }

    fn open(player: (i32, i32), locs: &[MapLocation]) -> MapDisplay {
        let mut m = MapDisplay::enter();
        for _ in 0..32 {
            if m.mode == MODE_INTERACTIVE {
                break;
            }
            m.tick(0, 0, player, locs, 0);
        }
        assert_eq!(m.mode, MODE_INTERACTIVE);
        m
    }

    /// The fade back in: black for the two staging frames, then three
    /// quarter steps to clear.
    #[test]
    fn the_open_fades_in_from_black_and_seats_over_the_player() {
        let mut m = MapDisplay::enter();
        assert_eq!(m.fade_grey(), None);
        let mut greys = Vec::new();
        let player = (0x1234, 0x2345);
        for _ in 0..7 {
            m.tick(0, 0, player, &[], 0);
            greys.push(m.fade_grey().unwrap());
        }
        assert_eq!(greys, vec![0xFF, 0xFF, 0xC0, 0x80, 0x40, 0, 0]);
        assert_eq!(m.mode, MODE_INTERACTIVE);
        assert!(m.camera_staged);
        // -0x1234 truncated toward zero to a tile.
        assert_eq!(m.cam_x, -0x1200);
        assert_eq!(m.cam_z, -0x2300);
    }

    #[test]
    fn the_clamp_keeps_the_camera_inside_the_kingdom() {
        let (mut x, mut z) = (0, 0);
        clamp_camera(&mut x, &mut z);
        assert_eq!((x, z), (-0xD00, -0xC00));
        let (mut x, mut z) = (-0x8000, -0x8000);
        clamp_camera(&mut x, &mut z);
        assert_eq!((x, z), (-0x3380, -0x3580));
        let (mut x, mut z) = (-0x2000, -0x2000);
        clamp_camera(&mut x, &mut z);
        assert_eq!((x, z), (-0x2000, -0x2000));
    }

    #[test]
    fn the_pad_pans_one_tile_a_frame_once_seated() {
        let mut m = open((0x2000, 0x2000), &[]);
        // The first interactive frame seats; the pad pans from the next.
        m.tick(0, 0, (0x2000, 0x2000), &[], 0);
        assert_eq!(m.seat, 2);
        let (x, z) = (m.cam_x, m.cam_z);
        m.tick(PAD_LEFT | PAD_UP, 0, (0x2000, 0x2000), &[], 0);
        assert_eq!((m.cam_x, m.cam_z), (x + PAN_STEP, z - PAN_STEP));
        m.tick(PAD_RIGHT | PAD_DOWN, 0, (0x2000, 0x2000), &[], 0);
        assert_eq!((m.cam_x, m.cam_z), (x, z));
    }

    /// Square walks the kingdom's records backward, wrapping from the player
    /// to the last one, and the camera snaps to each.
    #[test]
    fn square_steps_back_through_this_kingdoms_places() {
        let locs = [loc(0, 40, 50), loc(1, 60, 60), loc(0, 70, 80)];
        let p = (0x2000, 0x2000);
        let mut m = open(p, &locs);
        m.tick(0, 0, p, &locs, 0);
        let t = m.tick(0, PAD_PREV, p, &locs, 0);
        assert_eq!(t.sfx, vec![SFX_STEP]);
        assert_eq!(m.cursor, 2);
        m.tick(0, 0, p, &locs, 0);
        assert_eq!((m.cam_x, m.cam_z), (-(70 << 7), -(80 << 7)));
        m.tick(0, 0, p, &locs, 0);
        // Record 1 is another kingdom's: the walk skips it.
        m.tick(0, PAD_PREV, p, &locs, 0);
        assert_eq!(m.cursor, 0);
        m.tick(0, 0, p, &locs, 0);
        m.tick(0, 0, p, &locs, 0);
        // From record 0 the step lands on -1 and the stepper is not run.
        m.tick(0, PAD_PREV, p, &locs, 0);
        assert_eq!(m.cursor, -1);
    }

    #[test]
    fn the_forward_stepper_parks_on_the_player_past_the_end() {
        let locs = [loc(0, 1, 1), loc(1, 2, 2)];
        assert_eq!(step_cursor(0, false, &locs, 0), 0);
        assert_eq!(step_cursor(1, false, &locs, 0), -1);
        assert_eq!(step_cursor(0, false, &locs, 1), 1);
        assert_eq!(step_cursor(-2, true, &locs, 0), 0);
        // No record of the kingdom: the port gives up instead of spinning.
        assert_eq!(step_cursor(-2, true, &locs, 2), -1);
    }

    #[test]
    fn triangle_cycles_minimap_then_labels_then_nothing() {
        let locs = [loc(0, 40, 50)];
        let p = (0x2000, 0x2000);
        let mut m = open(p, &locs);
        let all = |_: u16| true;
        assert!(m.chrome(p).is_some());
        assert_eq!(m.visible_labels(&locs, 0, false, all), vec![0]);
        m.tick(0, 0x10, p, &locs, 0);
        assert!(m.chrome(p).is_none());
        assert_eq!(m.visible_labels(&locs, 0, false, all), vec![0]);
        m.tick(0, 0x100, p, &locs, 0);
        assert!(m.visible_labels(&locs, 0, false, all).is_empty());
        m.tick(0, 0x10, p, &locs, 0);
        assert_eq!(m.overlay, 0);
    }

    /// A label needs its kingdom and its discovery flag; the dev flag
    /// bypasses the second only.
    #[test]
    fn labels_are_gated_on_kingdom_and_discovery() {
        let locs = [loc(0, 40, 50), loc(1, 60, 60), loc(0, 70, 80)];
        let m = open((0x2000, 0x2000), &locs);
        let known = |f: u16| f == 0x484 + 70;
        assert_eq!(m.visible_labels(&locs, 0, false, known), vec![2]);
        assert_eq!(m.visible_labels(&locs, 0, true, known), vec![0, 2]);
        assert_eq!(m.visible_labels(&locs, 1, true, known), vec![1]);
    }

    /// Circle leaves: the cue, five frames of fade-out, the walk frame's
    /// fade-in, then the hand-back.
    #[test]
    fn circle_runs_the_exit_sequence() {
        let p = (0x2000, 0x2000);
        let mut m = open(p, &[]);
        let t = m.tick(0, 0x20, p, &[], 0);
        assert_eq!(t.sfx, vec![SFX_EXIT]);
        assert_eq!(m.mode, 3);
        let mut greys = Vec::new();
        let mut fade_at = None;
        let mut exit_at = None;
        for i in 0..10 {
            let t = m.tick(0, 0, p, &[], 0);
            greys.push(m.fade_grey().unwrap());
            if t.spawn_exit_fade {
                fade_at = Some(i);
            }
            if t.exit {
                exit_at = Some(i);
                break;
            }
        }
        assert_eq!(&greys[..5], &[0x40, 0x80, 0xC0, 0xFF, 0xFF]);
        assert_eq!(fade_at, Some(5));
        assert_eq!(exit_at, Some(6));
        assert!(!m.camera_staged);
    }

    #[test]
    fn the_minimap_marks_scale_a_tile_by_two_thirds() {
        let p = (64 << 7, 64 << 7);
        let m = open(p, &[]);
        let c = m.chrome(p).unwrap();
        assert_eq!((c.player_dot.x, c.player_dot.y), (42 + 0x0B, 42 + 0x81));
        // The camera sits over the player: -cam = 64 tiles.
        assert_eq!(c.view_box.x, 42 - 0x0F);
        assert_eq!(c.view_box.y, -42 + 0xC3);
        assert_eq!(c.compass.map(|m| m.letter), [b'E', b'W', b'N', b'S']);
    }
}
