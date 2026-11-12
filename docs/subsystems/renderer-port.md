# The port's wgpu renderer

How [`legaia-engine-render`](../../crates/engine-render/README.md) draws a frame: the PSX-mode rasterisation switch, the enhancement passes, the screen-space overlay, the VRAM capture path, the battle HUD and menu pipelines, the frame profiler and the retail draw-decision kernels kept as reference ports. Retail's own renderer is [`renderer.md`](renderer.md); how a pixel gets its colour is [`shading.md`](shading.md).

## PSX-style rendering

The 3D mesh pipelines carry the PSX rasterisation traits below. Only three
of them - the GTE's register limits, vertex snap and dithering - are gated, by
`Renderer::set_psx_mode(true)` (off by default); the rest are unconditional,
and semi-transparency has its own switch. PSX mode also sets the
field's [draw order](#the-field-sorts-by-each-primitives-link). The hosts run PSX mode on the
[native target](#the-native-target), the frame drawn at the display's own
`320 x 240`.

- **Affine UV interpolation.** UVs interpolate linearly in screen space
  (no perspective-correct division) - this reproduces the warping you
  see on retail PSX surfaces with steep depth gradients. UV is
  `@interpolate(linear)` in WGSL, on every path regardless of PSX mode.
- **The GTE's register limits.** Ahead of the snap, a corner whose
  eye-space X / Y or depth passes the GTE's `IR` / `SZ` saturation moves
  where `RTPS` wrote it (`gte_saturate_clip`, from
  `camera_frame::gte_saturation`'s `H` and `sz_per_w`, on the field and in
  battle); one inside them is untouched
  ([renderer](renderer.md#the-gtes-register-limits)).
- **Sub-pixel vertex snap ("vertex jitter").** Clip-space `x`/`y` are
  truncated onto the 320 x 240 stage's pixel grid (rows top-down), whatever
  scale the host draws the stage at - the GTE's `SX` / `SY` are an
  arithmetic-shift floor ([renderer](renderer.md)). That gives the
  characteristic per-vertex shimmer on slow-moving geometry and puts each
  edge on retail's pixel boundary.
- **The scanline's texel row.** In PSX mode the texel lookup moves up half
  a stage row (`psx_sample_uv`, the page's `uv_dy` term): the GPU takes a
  scanline's texels at the row itself, while the host rasteriser samples
  the pixel centre. Along the scanline the two already agree. The move
  stops `1/64` stage row inside the fragment's own triangle (each vertex
  carries a one-hot weight for its corner of the primitive record, so the
  fragment has its barycentric weights and their row step): retail's row
  point is a point the primitive covers, so its texel is always one the
  primitive maps. The host also covers pixels whose row point lies outside
  - the top sub-rows of every primitive, and all of a sliver thinner than a
  stage row, which the snap makes of an edge-on leaf card. A sliver's UV
  row step is huge, so an unbounded half row reads far outside its texels;
  on `ending_vignette_fullscreen` that drew lines of a neighbouring card's
  leaves across the sky. The margin keeps an edge sample off the row above:
  float interpolation lands a hair either side of the edge's exact `V`, and
  truncating the outside value takes the next card's last row.
- **15-bit ordered dithering.** The shaded colour is dithered with the
  PSX GPU's 4x4 offset matrix and quantized to 5 bits per channel (BGR555
  framebuffer depth) - the cross-hatch gradients of retail output. The
  WGSL helper (`PSX_DITHER_WGSL`) mirrors the unit-tested CPU `psx_dither`
  module, and the composed shaders are naga-validated in the test suite. A
  dithered textured draw blends at the GPU's precision first,
  `(t5 * colour) >> 4` (`psx_modulate_gpu`; [shading](shading.md#step-4-the-colour-word---why-raw-textures-look-darker-or-brighter)).
  Under the PCSX-Redux texel rule the dither and the Gouraud blend foreground
  take that emulator's table instead, an instrument for its captures
  ([retail-compare](../tooling/retail-compare.md#a-pcsx-redux-frame-keeps-what-the-console-floors)).
- **TSB / CBA flat shading per primitive.** Texture page and CLUT base
  remain `@interpolate(flat)` so each triangle samples from the same
  page and palette, matching `GP0(0x24)` semantics.
- **PSX texture blending - the lighting.** Retail runs *no light source* on
  the field path: its two TMD renderers issue exactly one GTE colour op
  between them (`DPCS`, the depth cue) and never an `NC*` op, so the
  shading is baked into each prim's colour word and applied by the GPU as
  `texel * colour / 128` (`0x80` neutral, below darkens, above brightens
  up to ~2x). The mesh shaders do the same: each vertex carries the baked
  colour, `psx_modulate` applies the blend, `psx_depth_cue` applies `DPCS`
  (identity at the field's `IR0 = 0`; set with `Renderer::set_depth_cue`).
  The `psx_light` module mirrors both on the CPU and pins them. There is
  no synthetic Lambert on any path that has real colour data to draw.

- **Semi-transparency blend modes.** Per-prim PSX blending on the
  VRAM-mesh and colour-mesh paths, gated by `Renderer::set_semi_blend`
  (**on by default**, independent of PSX mode) - see
  [Semi-transparency](#semi-transparency) below.

### Semi-transparency

Lives in the `psx_blend` module. The TMD mode byte's ABE bit travels in
bit 15 of the per-vertex TSB attribute (packed by the `legaia_tmd::mesh`
builders); the blend equation is the texpage ABR field (TSB bits 5..=6):
mode 0 `0.5*B + 0.5*F`, 1 `B + F`, 2 `B - F`, 3 `B + 0.25*F`.

**Why two passes.** The STP decision is *per texel* - a texel's BGR555
bit 15 picks blend-vs-opaque inside a single semi-transparent prim. So
the opaque pass draws everything but discards the STP texels of
semi-transparent prims, then a blend pass re-draws only the
semi-transparent triangles keeping only STP texels - a per-ABR-mode index
tail appended at upload time by `psx_blend::append_semi_tail`. The blend pass uses
one fixed-function pipeline per mode, each with its own fragment entry
(`psx_blend::blend_entry`) that writes the mode's foreground term: off PSX
mode `F` times the mode's factor (`0.5` over a `0.5` background constant for
mode 0, `0.25` for mode 3; mode 2 is reverse-subtract), in PSX mode the term
that lands the GPU's truncating 5-bit result
([renderer](renderer.md#psx-mode-blends-at-the-gpus-5-bit-precision)). Blend draws depth-test (`LessEqual`) but don't write
depth, and run after all opaque scene draws. `blend_apply` is the CPU
mirror the blend-state mapping is unit-tested against.

**Untextured prims are the exception.** Untextured (`F*`/`G*`) ABE prims
have no per-texel STP gate - they blend *all* their pixels.
`upload_color_mesh_blended` carries that state in a per-vertex blend
word (same ABE/ABR bit positions, `psx_blend::pack_blend_word`); with
semi-blend on, the opaque colour pass discards ABE prims and the colour-mesh blend
pipelines re-draw their per-ABR-mode index tail
(`psx_blend::append_semi_tail_words`) with the prim colour as
`F`. Untextured TMD prims carry no texpage, so the caller resolves ABR
from draw-env state (mode 0 = the PSX default); plain
`upload_color_mesh` keeps every prim opaque.

**A draw can force semi-transparency on every prim.** Retail's prim
dispatcher `FUN_80043390` reads the actor's colour word (`+0x74`) top byte as
primitive flags and ORs them into every packet it builds: bit 31 into the
command's semi-transparency bit (`0x80043510..0x8004351C`), bits 24..=25 into
each textured packet's texpage ABR (`0x80043504..0x8004350C`, OR'd at e.g.
`0x80044D44`). Field-VM `4C D6` writes that byte
([`script-vm-menuctrl.md`](script-vm-menuctrl.md)). The port carries it as
`DrawCue::blend` (`Some(abr)`): the draw's uniform raises `flags.y` to `2.0`,
which every opaque entry (and the SSAO pre-pass) reads as each prim's ABE set,
and the blend pass draws the mesh's whole opaque index range as one run in that
mode (`psx_blend::push_forced_draw`) in place of its own semi tail; the
textured blend entry's STP-only rule then blends exactly the texels retail
does. Two approximations, neither with a shipped carrier: the run is keyed at
the mesh's bounds centre, not per prim, and a prim whose own texpage carries a
non-zero ABR blends in the forced mode rather than the OR of the two (the one
forced mode on the disc, `3`, absorbs any OR). The browser page does the same
with `u_force_semi` and a placement's `forceSemi`.

**Ordering mirrors the retail ordering table, at per-primitive
granularity.** Every semi prim of every semi-carrying draw (textured and
untextured in one shared sequence) is keyed by its model-space
centroid's clip-space `w` under the draw's MVP
(`psx_blend::prim_depth_key`). By linearity of the MVP that equals the
average of the prim's vertices' clip `w` - the GTE avg-Z the OT bins on.
The whole list blends far-to-near *regardless of draw boundaries*, so
prims that interleave in depth across overlapping draws still blend in
correct global order (`psx_blend::sort_blend_list`).

Equal keys form one OT bucket and draw later-submitted-first - the
retail LIFO bucket order (`AddPrim` prepends to a bucket's list,
`DrawOTag` walks it head-first). A key is snapped to a whole `SZ` first
(`psx_blend::snap_depth_key`): retail's `OTZ` is an integer, while one depth
reached through two model matrices can come out a thousandth apart in `f32`.
`opdeene`'s caption is the case - two camera-locked sprite-arm quads at view
depth `792`, the subtractive shadow keyed `791.999`, which drew it over the
additive face until the keys were snapped. The per-prim metadata
(`psx_blend::SemiPrim`: centroid + ABR mode + tail location) is recorded
once at upload time by the tail builders; the per-frame list lives in a
reused buffer, and contiguous same-draw, same-mode runs coalesce into
single indexed draws (`psx_blend::coalesce_sorted`).

**Dither parity** follows retail's rule that only shading arithmetic is
dithered: the untextured blend entries dither `F` (a gouraud result)
before the blend; the textured blend pass draws raw texels and stays
undithered. Its foreground is truncated to a 5-bit channel before the blend
equation reads it (`psx_blend_foreground`, both hosts, every render mode),
which is how the GPU hands the modulated texel to the blend
([shading](shading.md#step-6-semi-transparency)). In PSX mode the blend
itself is the GPU's 5-bit arithmetic too, on the console's rule or the
PCSX-Redux instrument's, and the native target presents and captures the
frame as the 15-bit framebuffer it models (`fs_scanout15`). The play page's PSX
target copies its frame to the canvas through the same readout, on the same gate:
no SSAO, no bloom, no fog drawn over the frame. Read raw, a blended pixel kept the
background's low bits and stood up to seven levels off the window's; on the host
pair's `cast` scene at a 4-level threshold the share of pixels apart fell from
about 17% to under 3%. The instrument stays the window's alone: the page scores
no PCSX-Redux capture.

In the `legaia-engine play-window` binary PSX mode is opt-in
(`--psx-render`, the launcher's PSX rendering row, or `LEGAIA_PSX_RENDER=1`),
and turns on the native target with it; the play page's "PSX rasterisation"
box is the same switch.

### The native target

`Renderer::set_native_resolution(Some((320, 240)))` draws every pass of a
frame - the 3D scene, the composited screen primitives, the text and sprite
overlays, the fades - into an offscreen texture the size of the PSX display
([`native_target`](../../crates/engine-render/src/renderer/native_target.rs)),
and presents it with a nearest-neighbour blit at the largest whole scale the
window holds, black around it. While it is on, `Renderer::surface_size` is
the native size, so every host lays its 2D out at stage scale 1 without
knowing the window is larger (`Renderer::presented_size` is the window's), and
`capture_rgba` reads back the native frame itself. The browser page does the
same with its canvases: PSX rasterisation gives the GL canvas and the overlay
canvas a `320 x 240` backing store and lets the browser magnify them with
`image-rendering: pixelated`. The page also takes the window's whole step: its
stage takes the largest whole multiple of `320` device pixels (`devicePixelRatio`
times the CSS width) that the layout offers, and the rest of the column, or of
the screen in fullscreen, stays letterbox (`fitPixelExact` in
[`play.html`](../../site/_content/play.html)). Stretched to the layout's width,
a stage pixel came out two or three device pixels wide by turns.

Off PSX mode the same target serves one presentation case
(`fill_target_for`): a window just short of a whole step of the stage - at
least `FILL_MIN_RATIO` (0.95) of the next one, as a 960x720 window that a
768-pixel-tall screen clamps to 960x699 - draws the whole frame at that step
(960x720) and the blit's fit branch shrinks it onto the window. Text and
sprites keep their whole-step grid and stay lined up with the 3D, where the
window-sized frame would fall a whole step (640x480) with black around it.
The *Pixel-exact scaling* option (`OptionsState::integer_scale`) skips this
case, so the stage stays on the whole step below and every stage pixel covers
the same number of window pixels.
Retail's persisting battle frames (`set_frame_persist`) stay a PSX-mode
behaviour: the clean mode's fill target clears every frame as its window
frame does.

The page's context has MSAA, which cannot change after creation and which WebGL
cannot turn off on the canvas's framebuffer. So a PSX frame on the page draws into
a single-sample framebuffer of its own
([`webgl-psx-target.js`](../../site/js/webgl-psx-target.js)) and is copied to the
canvas once: "the screen" is that framebuffer until the frame ends, reads inside
the frame included. On the canvas itself the fill rule below would not hold, since
a polygon edge resolves to a blend of partly covered samples. The native renderer
is single-sample throughout, and the page's enhanced mode keeps its MSAA.
That framebuffer runs bottom-up, so where a snapped edge lies exactly on a row
of pixel centres its rasteriser hands the tie to the other row from the window's
top-left rule: every flat 3D edge drew a row low. The page's vertex stage lifts
the primitive a sixteenth of a row (`PSX_NATIVE_TIE_ROWS` in
[`webgl-shaders.js`](../../site/js/webgl-shaders.js)), the smallest step its
sub-pixel grid keeps, and reads the UV and the Gouraud colour that much lower
again, so the texel and the shade are the pixel's. The lift also moves the tie of a sloped
edge that crosses a pixel centre exactly, and of a centre within a sixteenth of a
row of an edge, which the window's column rule decides: those are the hairline
pixels the pair still reads. A finer lift is under the page's sub-pixel grid on a
real GPU and moves nothing; drawing the pass top-down is what would settle both.

At stage scale 1 PSX mode rasterises the GPU's way, which no fragment trick
at a window-sized target could. The snapped vertices move half a stage pixel
down and right (`psx_native_centre`, PSX mode's `psx_params.z = 2`), so the
host's pixel centre is the integer point the GPU tests a pixel at, and the
host's top-left rule there is the GPU's fill rule: coverage is whole stage
pixels, each owned by one primitive, and a sliver thinner than a pixel lands
on the pixels the GPU gives it. The UV interpolated at that point is the GPU's,
and the texel is that UV plus half a texel on both axes (`psx_texel_uv`) - the
GPU's interpolators start from the vertex value plus one half, so the texel is
the UV rounded. The half-row texel move of the window-sized mode is not used
there: the sample point is already the row's.

On a texture drawn at one texel per pixel the rounding and the
window-sized mode's reading (the texel at the pixel centre along the row and
at the row itself) pick the same texel; they part on scaled texturing, and
there the rounding is what Mednafen captures show: drawn the other way,
`ending_vignette_fullscreen`'s mean pixel error is three times what the
rounding leaves. PCSX-Redux captures are drawn by its software GPU, which
takes the texel with no bias - at the pixel centre along the row and at the
row itself - so scored against the console's rule a scaled texture on a
PCSX-Redux state reads as a miss that is the capture's rasteriser, not the
engine. `Renderer::set_psx_texel_rule(PsxTexelRule::PcsxRedux)` samples that
rule instead; it is an instrument, not a player mode
([retail-compare](../tooling/retail-compare.md#the-image-channel)).

The same emulator draws a `POLY_FT4` whole. The console splits every quad into
two triangles, and so does PCSX-Redux for a Gouraud one (`drawPoly4TGEx8`), but
its `drawPoly4TEx8` walks the flat-shaded quad's left and right outline chains
(`setupSectionsFlatTextured4`) and steps `U` and `V` linearly along each row
between them. Where the four corners lie on no one UV plane the two laws map the
texture differently, by many texels on a large quad. Under the rule, the upload
hands the vertex stage the quad's four UVs on the normal lane, and
`quad_walk_uv` repeats the walk per fragment. Only the TMD builders know a
primitive's kind (`TSB_FLAT_QUAD_BIT`): a Gouraud quad of four equal colours is
common, and it splits. `noaru_field_card_walk`'s ice floor shows it largest: a
raster of the state's own table reads `.921` against its frame by triangles and
`.996` by the walk, and the port's frame moves from `.913` to `.998`.

Taking the row itself puts a fragment on its triangle's top edge exactly on
that edge, whose `V` is the corner's own value. The host's interpolation lands
a hair either side of it, and truncated, the outside value reads the texel row
above the primitive's UVs. On the battle ground grid that row lies outside the
tile's `(192..=255)²` window: VRAM zeros, CLUT index `0` (`0x0220`), which the
stage colour brightens to a pure-green line along every grid row whose top
edge is on screen. Under this rule a fragment on a top edge moves down to
`PSX_ROW_EDGE_MARGIN` inside its triangle (`psx_row_inset`), the same margin
the window-sized mode's row move stops at; a fragment inside its triangle
keeps its row. The console's rule needs no move: its half-texel bias already
rounds the edge value onto the corner's row.

The composited screen primitives take the same pixel centre and texel bias
on the native target: a sprite drawn one texel per pixel samples the same
texels either way, and a scaled quad - an ending vignette's photo shrunk to
a corner - samples the GPU's.
Under the PCSX-Redux rule they take no half-texel bias. That emulator walks a
flat `POLY_FT4` whole from the left edge's `U` / `V` at the first column and
the row's own `V` (`SoftRenderer::drawPoly4TEx4`, `posX = m_leftU`), and splits
a `POLY_GT4` into triangles that read the texel at the pixel centre along the
row (the 3D pass's rule); `screen_prim::FLAG_GOURAUD` tells the two apart. A
quad drawn one texel per pixel reads the same texel under every rule; a scaled
one - a fog sheet - reads its emulator's. Taking the console's bias there had
put every fog sheet on a PCSX-Redux state half a texel off its capture.

The Gouraud colour is not rounded the way the UV is. Rounding the
interpolated colour to a whole step (a half-step start, as an emulator's
colour interpolators take) moves only shaded primitives, and on the retail
states it touches it makes the frame brighter than the capture:
`v0_1_battle_command_menu`'s exactly matching pixels fall by a hundredth,
and no state's score rises. The colour stays the interpolated value.

### What the clean rasteriser leaves on a retail frame

Off PSX mode the engine draws every vertex at its exact sub-pixel position.
Retail's are truncated, so against a capture the clean frame's texture
content sits about half a stage pixel right, and its rows match. The vertex
positions themselves agree to the census: matched against the packets on a
state's own ordering tables (`draw-family-diff.py`'s per-triangle file), the
engine's corners sit `+0.5` from retail's integer ones on both axes, the
truncation's mean, on a card-boot state and a played-forward one alike, and
the HUD lines up exactly. The horizontal half pixel is what is left once the
scanline's texel row is accounted for: a reference rasteriser over the
retail packets matches both emulators' frames with coverage on the integer
pixel and the texel taken at the pixel centre along the row and at the row
itself across it.

PSX mode removes it. On the retail field states it was tried on the offset
goes and the block score rises or holds within a block (`kor5_field_card_boot` `x +0.56` to `+0.04` stage pixels,
`doman_field_card_boot` `+1.39` to `+0.23`, `conc_field_card_boot` `+1.00`
to `+0.17`), and the battle states it was tried on score the same in both
modes.

### What PSX mode still leaves

At a window-sized target PSX mode is not a uniform gain over the clean
rasteriser: the field states mostly rise, while thin geometry drops - a hit
spark's spikes (`battle_melee_hit_spark`), a cast's ice shards
(`nivora_duel_mid_blazing_slash`). The host covers a pixel wherever the
snapped triangle crosses one of its framebuffer sample points, a supersample
of the stage pixel, while the GPU tests one point per stage pixel. That is
what the [native target](#the-native-target) is for, and the retail-compare
image channel scores PSX mode only on it
([retail-compare](../tooling/retail-compare.md#the-image-channel)).

The GPU's rule cannot be had at the window-sized target by discarding
fragments. A fragment can test the stage pixel's own point with the fill
rule's tie break, but the host only rasterises the part of a stage pixel
inside each triangle: where an edge crosses a stage pixel, the triangle the
rule gives the pixel to has fragments on its own side only, and the other
side's fragments are the ones the test drops, so every diagonal shared edge
cracks.

### The field sorts by each primitive's link

In PSX mode the field pass draws each primitive at the depth of its ordering-table
link rather than per pixel (`Renderer::set_field_prim_order`, `prim_mean_depth` on
the vertex stage). Retail has no depth buffer, and three rules set the link:

- A primitive links at the mean `SZ` of its corners (`mfc2 s2, OTZ` / `andi s2,
  s2, 0xFFFC` / `addu s2, s2, t0` in `FUN_80043B58`).
- A primitive of a draw whose colour word carries `0x10000000` links at its
  farthest corner, the banks' max of `SZ0..SZ3` (`FUN_80045194` at
  `0x800452AC..0x800452F8`). The word comes from an object record's flag `0x800`
  ([renderer](renderer.md#a-decoration-cell-with-record-flag-0x800-sorts-at-its-farthest-corner)),
  on a decoration cell and on a placed object alike (`FUN_8003A55C` at `0x8003A754`).
- An actor whose `+0x10` carries a bit of `0x80800000` draws with the table base
  `0x28` lower, ten buckets nearer (`FUN_8001ADA4` at `0x8001B204..0x8001B21C`,
  `FUN_8001B964` the same). A prologue's `31 1F` raises the high bit on a placed
  object or a seated prop, and the floor sampler raises `0x800000` on the player
  each step over a cell without bit `0x1000` (`FUN_80019278` at
  `0x80019314..0x80019328`).

The hosts pass the last two per draw (natively `Renderer::set_draw_order`,
`DRAW_ORDER_FAR_CORNER` / `DRAW_ORDER_NEAR_BUCKETS`; on the play page the placement's
`meanDepth` word, from `field_terrain_draw_order` and `field_placement_draw_order`). A ground cell keeps the bucket
its own emitter gives it (the far bucket, or two behind its farthest corner), so
its vertices never take the primitive's rule.

A drop-shadow cell takes the one depth of its own link in that frame, its mean `SZ`
plus the `0xA0 / 4` bias and less the ten buckets of a near actor (`FUN_8001C394`),
not the per-corner scene depth a clean frame tests it with: against a floor's flat
primitive a per-corner depth loses to whichever row the primitive's mean stands in
front of, and on `bubu2_field_door_hop` the player's blob did not show at all.

The three rules hold against a state's table: on `koin1_field_route_walk` every
`0x7F84` quad of the floor sheet sits at the bucket of its farthest corner, and the
terrain's `0x7DC3` wall quads at their mean, ten buckets behind the sheet's rule.
The mean alone, with no record flags and no ground exception, hides floors and
cabinets. With them `concend_field_door_hop` reads `1.000` (`.942` per pixel), where
the sky's quads link among the far ridge's and cover them. `ropeway_field_route_walk`
reads `.990` (`.979`), and its Octam trees are that same ordering.

The overworld's actors and landmarks go through the same draw kernels as a town's
(`FUN_8001ADA4`, `FUN_80043390`), so the rules above hold there too, and PSX mode draws them
by link on the world map as well. A landmark or decoration record carrying `0x800`
sorts at its farthest corner: on `map03` the Conkram plaza's records carry it, and a
player standing there (a retail frame with him poked onto `(66, 103)`, footing `-96`)
paints over it, retail's draw order, where per-pixel depth buries all but his hair. The ground cells
keep their own `(max SZ >> 5) + 14` bucket (`FUN_801F89B8`). The stage's `flags.w` lane
carries the curvature scale on the world map, so the draw-order word rides in `flags.z`
above the occlusion bit, and the near-bucket shift of `320` `SZ` is taken by that scale.

Not modelled: a walking NPC's `0x800000` (the sampler runs only for actors whose
tick reaches it, which the port does not track per actor), a posed prop's record
flag `0x800`, and the order inside one bucket (retail's last-linked-first). A
clean frame keeps per-pixel depth.

### The battle sorts by each primitive's link

A battle has one ordering table too, and every mesh in it goes through the same
per-prim leaves. The bodies are drawn object by object by `FUN_80048A08`
(`jal 0x80043390` at `0x80048ED0`), the effect parts and draw-kind-4 nodes by
the render dispatcher `FUN_8001ADA4`, and the backdrop shell as a background
actor; each leaf links its primitive at the `AVSZ3` / `AVSZ4` mean of its corners
(`ZSF = 0x555 >> 2` / `0x400 >> 2` under the battle shift `2`, so a bucket is `16`
`SZ`). Nothing in the GPU tests depth, so a body and the additive ring around it
sort by link, not by pixel.

In PSX mode both hosts draw every battle mesh primitive at its corners' mean
`w`, the field's machinery: `camera_frame::prim_link_order` raises
`Renderer::set_field_prim_order` in battle as on the field, which stages
`flags.w = -1` on every draw (`prim_mean_depth` over `prim_c0..prim_c3` on the
vertex stage, the blend pass included, since it shares the vertex stage), and
the play page sets `meanDepth` on every battle placement (`primMeanW`). In the
battle units the port draws `SZ` is `w` itself.

| Class | Clean | PSX mode |
|---|---|---|
| Backdrop shell and spun slot | per pixel | corner mean |
| Ground grid | far bucket (`pin_to_far_bucket`, the page's `u_far_slice`) | far bucket |
| Party and monster bodies | per pixel | corner mean |
| After-image ghosts | per pixel, pushed past the body about the eye | corner mean of the pushed pose |
| Effect models, parts, draw-kind-4 nodes, ground shadows | per pixel | corner mean; farthest corner on a `0x10000000` word |
| Effect-pool billboards | per pixel | corner mean (the quad's centre) |
| Screen primitives (trails, streaks, cast-arm packets) | over the 3D frame in their own order | unchanged |

A part or node whose colour word carries `0x10000000` links at its farthest corner,
the field's rule for a record flag `0x800`. The dispatcher stages the word's top
byte at `0x1F800043` (`0x800433D8`) and forces a cued bank for any non-zero mode
byte (`ori a2,a2,1` at `0x800433CC`), and every cued leaf tests the byte's `0x10`
except bank 3's split quad leaf `FUN_80045BB4`, whose sixteen `GT3`s link on their
own means (`ColourWord::links_at_far_corner`). The hosts draw such a draw with
`DRAW_ORDER_FAR_CORNER`; the page bakes those parts and nodes into a stream of
their own (`play_battle_fx_far_*`, drawn with `meanDepth` `2`). Inferno's (`0x9E`)
tunnel wall is one: word `0x58000000`, `32` quads `2838` units out whose near
corners reach past the camera. By mean they sort in front of the flame sheet
inside them; the retail table holds the wall in buckets `828..938` and the
flames in `397..584`.

A semi-transparent primitive writes no depth and is drawn after every opaque one,
sorted far to near by its triangle's centroid (`psx_blend`); with every
primitive at its mean, the depth test it runs against the opaque ones is the
link comparison.

The retail table of Dohati's Chaos Breath, arm 1, shows the class
(`ot-raster.py` on a RAM image of the recording): the rings' additive `GT4`s
(code `0x3E`, CLUT `0x7F84`) sit in buckets `50..64` with corners from the top
of the screen to below its bottom, and the body's packets (CLUTs
`0x7900..0x7905`) in `113..308`, so the ring paints over the belly. Per pixel the
belly stands in front of the ring wherever it reaches past the ring's radius.

Worked on Sol Tower's shell: `battle_kor_dead_bone_strike` `.969` to `.993`,
`battle_korb2_kemaro_strike` `.971` to `.990`, `battle_kor_spoon_strike` `.966`
to `.984`.

Not modelled: the order inside one bucket (retail's last-linked-first), a ghost's
`0x50`-byte table offset (`FUN_80048A08` at `0x80048E7C`, twenty buckets deeper
than its body; the hosts' eye push stands in), and the screen primitives' links,
which composite over the 3D frame rather than among it. Clean rendering keeps
per-pixel depth: a mean pops as the camera turns, which the retail frame shows
and the clean frame need not.

#### The far bucket under the cast wash

The table holds `4096` words and `SZ` saturates at `0xFFFF`, so a quad whose four
corners stand past that depth links in the last word: `AVSZ4` under `ZSF4 = 0x400 >> 2`
is the sum over `16`, `0x3FFF`, and `& 0xFFFC` gives `ot + 0x3FFC`. No triangle reaches
it (`ZSF3 = 0x555 >> 2`). The GPU walks that bucket first, last-linked first. While a
cast module holds its band wash, the dispatchers' tail links the wash's opaque `G4`s
there before the actor walk draws the scene (`FUN_801F2410`,
[`cast_colour_wash`](../../crates/render-kernels/src/cast_colour_wash.rs)), so every scene
quad linked there later is painted first and covered: nothing of it shows. The hosts
draw the wash as a far-plane screen layer, under everything, so the same quad would
draw over it at its own depth. The shared effect-draw list drops such quads from a
draw-kind-4 node's mesh while the band wash covers the far bucket
(`effect_draws::drop_far_bucket_quads`, `World::cast_wash_covers_far_bucket`), in clean
and PSX mode alike, since retail shows none of it either way. Kemaro's arm-17 discs
are the case ([cast module](cast-module.md#summon-creature-directors)): a retail
table of arm `0x12` rasterised with and without their `32` packets is the same picture.

Not modelled: a mesh part's quads in that bucket (no shipped part is known to reach
it), and the scanline modes' links, which no capture has read.

## Enhanced lighting (enhancement, NOT retail)

`Renderer::set_dynamic_lighting(true)` shades the VRAM-mesh and colour-mesh
passes under a staged `LightingMood` (`set_lighting_mood`), adds the staged
point lights (`set_scene_lights`), draws emissive-tagged prims (TSB / blend
bit 13) at the mood's emissive gain, and composites the staged glow sprites
(`set_glow_sprites` - additive halos + light shafts, the `glow_pipeline`).
**Off by default in the renderer, and off IS retail**: the field path has no
runtime light source (see `psx_light` above), so the WGSL helper
(`dyn_light`) early-returns the input colour when the uniform enable is zero
and the disabled path is pixel-identical to the faithful render.
`play-window` turns it on from the persisted `enhanced_lighting` option
(`I` toggles, `F8` cycles the time of day, `--no-dynamic-lighting` forces
it off).

The derivation - which prims glow, the light list, the moods, the glow
geometry, and the CPU mirror of the shading law - lives in the shared
`legaia_render_kernels::scene_lighting` kernel, so the browser play page lights from the same values.
This crate keeps the WGSL twin (`dyn_light` / `scene_point_gain` in
`shaders.rs`, held to the mirror by the `dyn_light` module's tests) and the
part only wgpu has: `scene_lights::light_view_proj` and the per-light PCF
shadow pass (`set_dyn_shadows`, default on; `--no-dyn-shadows` / the `Y`
key). The full model is on
[renderer.md](renderer.md#enhanced-lighting-enhancement-default-on).

## Camera-occlusion fade (enhancement, NOT retail)

`Renderer::set_occlusion_fade(true)` plus a per-frame
`Renderer::set_occlusion_focus(clip_pos, feet_clip, strength, proj_scale_y)`
dissolve scene fragments that sit between the camera and the player to a
4x4-Bayer screen-door, so the character reads through walls / roofs. **Off
by default in the renderer, and off IS retail** - the disabled path is pixel-identical
(the WGSL `occl_keep` returns 1.0, which no Bayer threshold reaches).
`strength` is the host's eased output of the **visibility gate**
(`engine-scene::field_occlusion`): a 5-point eye-to-player ray-cast against
the static scene that arms the fade only when the character is completely
hidden - a partially visible character never fades. Only draws the frame
draws count: the occluder set is built once per scene from every static
draw, and the gate asks the shared `field_view_window::OccluderGate` for
each range (the terrain crop; for a placed object the tests its own draw
loop makes - not parked at the hide box, live on the windowed static-object
list, through the actor cull at its base position), so a wall the frame does
not draw cannot open a hole over the player. A fragment then fades
only when its draw is environment geometry (the
`set_occlusion_env_draws` watermark exempts the player / NPC draws via
`flags[2]`), it is nearer the camera than the player by more than a small
depth margin (16 units - hugging occluders still open, actors are
protected by the watermark instead), and it lies inside a screen-space
circle around the player's projected centre and above the line through the
projected feet (`feet_clip`, `occlusion_fade::lift_factor` - the ground in
front of the character and the foot of a wall never dissolve); the keep probability
feathers from 1.0 at the rim to 0.25 at the centre, then blends toward
identity by the strength. Implemented as a fragment `discard` in the
opaque + semi-blend scene entries - no extra pipelines, depth writes
intact. The focus rides in the per-frame group-2 scene uniform;
single-mesh pipelines compile a `return 1.0` stub, like
`scene_point_gain`. Tunables + the CPU lockstep mirror live in the
`occlusion_fade` module. In `play-window` this is ON by default
(`--no-occlusion-fade`, or the `F4` key at runtime - a function key precisely
because it must not collide with a pad-bindable letter), and the feature is
deliberately per-fragment + per-triangle - the site's abandoned per-body
occluder cull is the failure mode it avoids (see
`docs/subsystems/renderer.md`). The browser play page ships the GLSL twin
(`occl_keep`/`occl_bayer` in `site/js/webgl-shaders.js`, constants
mirrored there - keep them in lockstep with this module).

`Renderer::set_texture_window(mask_x, mask_y, off_x, off_y)` maps to
GP0(0xE2) "Texture Window setting" - four 5-bit values in 8-pixel steps
that clamp / wrap texture-coordinate sampling to a smaller window inside
the texture page. Default all-zero is a no-op. The fragment shader
applies the per-pixel
`coord = (coord & ~(mask*8)) | ((offset & mask)*8)` transformation
before texture-page lookup. Retail Legaia leaves the register at zero
almost everywhere; the API is wired primarily so future runtime
LoadImage / DMA-to-VRAM trace work can replay the register state
faithfully.

Toggle is global - apply once per frame before submitting draws.

The [`afterimage`](../../crates/render-kernels/src/afterimage.rs) module (wgpu-free, in `legaia-render-kernels`) ports the battle move-FX
streak draw (`FUN_801e1ab0`): `build_afterimage_quad` assembles one
jittered, semi-transparent textured quad (`POLY_FT4`) from four projected
screen corners + the move's trail-texture id, reproducing the per-corner
`rand` wobble, the random brightness band that picks a texture sub-column,
and the exact UV / CLUT / texpage layout. It takes an injected rng (the
retail source is the BIOS `rand`) so the construction is pure and
unit-tested. `screen_overlay::
afterimage_screen_quad` lifts it into a `ScreenPrim` that the
[`screen_overlay`](../../crates/engine-render/src/screen_overlay.rs) pass links into the screen-space
ordering table and the wgpu renderer draws via `RenderTarget::ScreenOverlay`.

## Volumetric ground fog (enhancement, NOT retail)

`Renderer::set_fog_volume` stages one frame's mist bank
(`render-kernels::fog_volume`, simulated by the `World`); `None` draws
nothing and, if the pass was never built, costs nothing. The pass
(`renderer/fog_volume.rs`) draws twelve instanced horizontal sheets over a
48 x 48 quad mesh between the two halves of the scene pass, depth-tested
against the scene and reading its depth for a soft intersection. The browser
play page draws the same frame through `site/js/webgl-fog-volume.js`, a GLSL
transcription. The simulation, the scene styles and the shading recipe are
on [renderer.md](renderer.md#volumetric-ground-fog-enhancement).

**Ground field.** Each field scene's `FogGround`
(`engine-scene::fog_ground::source_for_scene`, captured at scene load by both
hosts over their static draw lists, and built the first time the raised toggle
ticks the bank) puts the sheet mesh on the drawn
floor and stops it over open air, so a tower top such as `teien` no longer
carries a flat bank across the sky around it. Past a rim the mesh keeps
going as a near-vertical spill curtain hugging the wall face, carrying a
third of the bank (`SPILL_SHARE`), then, further out and far below the rim,
as a faint distant haze layer (a negative mesh weight) that lies only where
nothing drawn stands above it.

The field is built from what is on screen, not from what the data holds
(`fog_ground::FogSurfaces`): the placed objects a script hid or scaled to
nothing are left out and the rest are taken at their render scale, as the
hosts draw them; a prim whose texels are almost all the transparent word
(`SURFACE_MIN_OPACITY`) is not a surface; and a semi-transparent prim is
floor only where it lies level (a pond, not a mist veil hung across a
corridor) and is never a wall the spill drapes over. The bank's movers
follow the same rule: a field channel that draws nothing (render scale
`0`, the off-map hide box, a flag word that opts out of the drop shadow)
does not part it. The field also
reaches the shader as an RGBA8 texture at binding 3, visible to both stages
(coverage, spill, flow; at most 256 x 256 texels of 64 world units, uploaded
only when its generation changes): it fades the bank across the floor's edge
at fragment resolution, and breaks the spill into slow, falling tendrils
with gaps - a pour mask along the rim, eroded tendrils, puffs - spread
outward as a slab, with the constants in the frame header
(`FOG_SPILL_CONSTANTS`, `FOG_HAZE_CONSTANTS`). A frame without a field binds
one full-coverage texel, which is the bank as it was before. The recipe is
on [renderer.md](renderer.md#volumetric-ground-fog-enhancement).

**Battle profile.** `fog_volume::BATTLE_FOG` sets a battle's bank to a third
of the field opacity and an ankle-high depth, so the stage floor stays
readable; a field-to-battle switch eases it in from zero instead of popping.

**Cost.** The field is CPU work at scene load only: one rasterisation of
the static triangles plus two nearest-site sweeps over at most 65,536
cells. Per frame the pass adds one texture sample per sheet fragment and
per vertex (four more on spill vertices and fragments), a handful of noise
evaluations on spill and haze fragments, and nothing on the CPU.

## Bloom (enhancement, NOT retail)

`Renderer::set_bloom(true)` makes the bright parts of the 3D frame - lamp
halos, emissive props, lit windows, sunlit sky - bleed a soft glow into
their surroundings. **Off by default, and off IS the frame without it**: none
of the passes below run and the scene is the single pass it always was.

With it on, the scene half of a `RenderTarget::Scene` frame (the 3D draws,
the glow sprites, the volumetric fog) renders into an offscreen target
instead of the presented one, and `renderer/bloom.rs` runs an extract and
four blur passes at a quarter of the surface on each axis, then a composite:

1. **extract** - four bilinear taps cover the 4x4 scene block under one
   bloom texel, each weighted on its own by a soft-knee brightness law (the
   largest channel against a threshold, a quadratic ease across the knee),
   so one bright pixel does not lift its dark neighbours;
2. **blur** - a separable 9-tap Gaussian as five linear taps, horizontal then
   vertical, twice;
3. **composite** - the scene texel plus the glow times the intensity,
   saturating, written to the presented target.

The screen-space layers (the attached-light pools, sprites, text, the
composited ordering-table tail) draw after the composite, so the HUD never
blooms.

The response comes from `legaia_render_kernels::bloom::BloomParams`, keyed on
enhanced lighting's mood: the darker the mood the lower the threshold and the
stronger the glow (`for_glow` over `LightingMood::glow`), so at night the
lamps, halos and the Genesis Tree carry it while daylight takes only a faint
lift on its brightest surfaces; with enhanced lighting off the daylight
response applies. The renderer derives it from its own staged mood.

The browser play page draws the same passes through `site/js/webgl-bloom.js`:
it blits the finished default framebuffer into a texture (it cannot sample
the framebuffer itself), runs the same extract and blur, and adds the glow
back with an additive blend - the sum the native composite writes - at the
engine's response (`play_bloom_params`). The `bloom_parity` test evaluates
each of the three fragment stages on both hosts through `shader_eval` and
requires them to agree.

**Cost.** One full-resolution composite pass plus five passes over a
sixteenth of the pixels; on the page, one extra resolve blit. Nothing on the
CPU beyond two uniform writes a frame.

## Screen-space ambient occlusion (enhancement, NOT retail)

`Renderer::set_ssao(true)` plus a per-frame `Renderer::set_ssao_camera`
(the scene's world-to-clip) darkens opaque scene geometry where other
geometry closes over it: the foot of a wall, the floor under a table or an
eave, the ground at a character's feet - and, with enhanced lighting on,
adds the contact shadows below. **Off by default in the renderer, and off
IS the frame without it**: no pass is encoded, no pipeline is built, and the
scene shaders' own sources carry no SSAO code. `play-window` and the play
page turn it on from the persisted `ssao` option (`Home` toggles on both
hosts, the page's "Ambient occlusion" box, `--no-ssao` forces it off;
`--retail` and Retail-faithful pin it off, so every retail-compare tier and
every replay renders without it). Retail has no ambient term to occlude - the field path
draws baked colour words with no light source - so the AO multiplies the
finished opaque colour.

It is not a deferred renderer. Shading stays forward; a thin G-buffer
pre-pass exists only to feed the screen-space pass:

1. **pre-pass** (`renderer/ssao.rs`) - the scene's textured and untextured
   draws again, under the same uniform slots and the same `vs_main` as the
   main pass, into one `Rgba32Float` target: `rgb` the view-space normal,
   `a` the linear view depth (clip `w`). The same vertex entry under the
   same depth test means the same fragment wins every pixel, and the
   fragment entries (`fs_ssao_prepass`) repeat the opaque entries' discards
   - effect clip, NCLIP, double-sided copies, the occlusion fade's screen
   door, transparent texels and the STP texels a semi prim defers - so the
   G-buffer covers exactly the opaque fragments the frame shows. It borrows
   the main depth target, which the main pass clears again.
2. **AO** - per pixel, twelve samples on a golden-angle spiral rotated by
   interleaved gradient noise, scored with the Alchemy estimator and faded
   to zero at the world radius, which is what keeps a far wall from haloing
   a silhouette in front of it.
3. **blur** - one horizontal and one vertical pass, each tap weighted by a
   Gaussian, the relative depth difference and the normals' agreement, so the
   noise smooths without bleeding across a silhouette or a crease.
4. **apply** - a full-screen triangle inside the main pass right after the
   opaque draws, blending `framebuffer * ao`. The semi-transparency blend
   pass, the glow sprites, the fog, the bloom and every screen-space layer
   draw after it, so water, glass, halos, sprites, effects and the HUD never
   darken; pixels the pre-pass left empty (sky, backdrops, the letterbox)
   take `1`.

**Characters receive none.** On a character mesh the estimator reads the
mesh's own folds - an arm against the torso, a chin over the neck - as
crevices and paints shadows across faces, so actor draws take no AO. The
pre-pass knows a draw is an actor by the draw watermark the occlusion fade
already reads (`Renderer::set_occlusion_env_draws`, `MeshUniforms.flags[2]
== 0`): the player, NPCs, tile actors and spawned meshes on the field and
on expansion maps, the leader on the world map, every body after the
stage in battle - the stage being the backdrop shell, the ground grid and
the backdrop's slot-1 ribbon, so the floor under a fighter takes the same
AO a field floor does. The play window restarts the watermark each frame at
"all environment" and lowers it where each mode's actors begin. An actor
fragment writes its depth with a zero normal (`ssao_is_actor`), so it
still occludes - the ground under a character's feet keeps its contact
shading and contact shadow - while the AO pass leaves its own pixels at
`1`, the blur and the half-resolution upsample give its texels no weight
(the ground beside a foot averages only ground), and the apply leaves its
pixels untouched. Static world geometry and props keep their AO. The
browser twin marks the same draws by its `noOccl` placements
(`u_occl_allow == 0`).

**Where it runs.** The field, expansion maps, the world map and a battle:
the frames drawn through the scene camera with that actor split. A
minigame venue owns its frame instead - the Baka duel, the Muscle Dome and
the fishing pond under cameras of their own, the dance hall as one baked
mesh with its dancers in it, the slot machine as screen primitives - and
marks no actor draws, so the pass would shade fighters and dancers as
walls and reconstruct view space through the wrong camera. Both hosts ask
`legaia_engine_session::ssao_scope::ssao_applies` before staging the
camera, and stage none there.

**Normals.** The TMDs author normals only on their lit rows, so every
fragment takes the flat face normal: the cross product of the screen
derivatives of its reconstructed view position, turned toward the camera.
On this low-polygon geometry flat normals are what the faces are, and they
cost no vertex data. Clip `w` is the one depth that survives retail's
draw-order tricks - the far-bucket and sorted-cell depths move clip `z`
only - so the reconstruction sees true geometry.

**Resolution.** AO runs at full resolution up to a 540-row target, which
includes the native `320 x 240` frame: a half-resolution texel there would
span two stage pixels, and PSX mode's snapped edges walking across it read
as shimmer. Larger targets run AO and the blur at half resolution on each
axis and upsample with a joint-bilateral blend (bilinear weights times the
blur's depth and normal terms). The AO is applied after the PSX dither, so in
PSX mode it is a smooth term over the 15-bit frame.

**Contact shadows.** Folded into the AO term under the same toggle: while
enhanced lighting is on, each AO texel also marches eight steps toward the
mood's key light (`LightingMood::key_dir`) through the same G-buffer, up to
72 world units, and darkens where the ray passes up to 56 units behind
nearer geometry - the short shadow a character, a crate or an eave casts
where it meets the ground, which a mood light without a shadow map cannot
draw. A hit fades with distance along the ray, so the shadow is a soft-ended
sliver, not a cast silhouette; a face turned from the light takes none (the
baked shading owns it); the strength scales with the key light's luminance.
The march multiplies into the AO texel before the blur, so the blur that
smooths the AO's noise smooths the march's per-pixel jitter too, and no
extra pass or target exists. One knob covers both because they are one
term over one pre-pass, applied by one multiply: a separate toggle would
add a key and a row for no independent cost. The key light is aimed through
`contact_light`, which needs to know the frame the staged camera takes
points in - the field and world-map cameras take the Y-up render frame, a
battle's camera composed with the stage model takes retail's Y-down one
(`Renderer::set_ssao_camera_y_down`). With enhanced lighting off the
strength word is zero and the march does not run.

**The browser twin** is `site/js/webgl-ssao.js`, driven from the scene pass
(`renderAssembled` in `webgl-tmd-assembled-scene.js`): the opaque draws run
a second time, through the scene program itself with its `u_gbuf` switch
on, into an `RGBA32F` G-buffer with its own depth buffer, so the vertex
stage and every discard are the program's own and every colour exit of
`main()` writes the G-buffer word instead (`emit`, `gbuf_word` in
`webgl-shaders.js`); then the AO, blur and apply passes run, before the
scene's blend pass. The numbers arrive from the engine
(`LegaiaRuntime::play_ssao_frame`: view scale, `shader_words`,
`contact_light`, with a battle camera composed with the stage model as the
native window stages it), and the GLSL helpers transcribe the WGSL ones
name for name; `ssao_parity` evaluates both texts against
`legaia_render_kernels::ssao`. GL's window `y` runs up, which the page's
`ssao_view_pos` and `view_to_px` absorb. The pass needs
`EXT_color_buffer_float`; a driver without it turns the pass off for the
session, as it does for one that cannot link it. It stays out of a WebXR
session, whose eyes draw into their own framebuffer.

`LEGAIA_SSAO_VIEW=1` in `play-window` draws the AO term alone in place of the
opaque colour - a tuning view.

**Cost.** One extra draw of the opaque scene into the G-buffer, then three
full-screen passes over the AO target (the AO pass reading twelve samples
plus eight march steps per texel); the frame profiler banks their encoding
as `ssao`. Nothing reaches the simulation.

## Screen-space overlay pass

**One stage for every window size.** Every 2D draw and the 3D pass's
viewport (`scene_viewport_for`) sit on the 320x240 stage
`pause_menu::stage_transform` centres at the largest integer scale the
surface holds, up to `BOOT_UI_STAGE_MAX_SCALE` (16x). The frame around it is
therefore under one stage step on each axis: a 960x720 window on a 2x HiDPI
desktop (1920x1440) is the stage at 6x with no border, and a 1.5x one
(1440x1080) is 4x inside an 80 / 60 px frame.

**The tint is retail's, 5-bit.** The text / sprite shader tints every quad
through `ui_tint` - WGSL generated from `legaia_render_kernels::ui_tint`, the
same kernel the browser page tints its atlas copies with: the texel truncated
to 5 bits, times the colour, truncated again (`(T5 * C) >> 7`), and a font
drop shadow under a known ink drawn as that ink's own CLUT entry rather than a
multiply of the fill ([dialog font](../formats/dialog-font.md)). The
truncation is the pipeline-overridable `UI_QUANTIZE`; the renderer keeps both
pipeline sets and `Renderer::set_ui_psx_tint` picks one, retail by default.

[`screen_overlay`](../../crates/engine-render/src/screen_overlay.rs) is the render capability behind
`RenderTarget::ScreenOverlay`: a `ScreenPrim` is either a textured
`POLY_FT4` (`ScreenQuad` - four screen corners + UV/CLUT/texpage + a 24-bit
modulation colour) or a solid/blended `FlatQuad`, each carrying an OT
`ot_index`. `order_primitives` reproduces the retail `AddPrim`/`DrawOTag`
walk - farthest bucket first, later-submitted-first within a bucket (the
same convention as `psx_blend::sort_blend_list`). `build_geometry` emits a
flat NDC vertex/index buffer plus a run list coalesced by blend class, which
the renderer uploads once per frame and draws one indexed run at a time
(opaque pipeline or the matching per-ABR blend pipeline). Semi-transparency
is per texel, as on the GPU: a semi run draws twice, its textured prims'
STP-clear texels solid through `fs_semi_opaque` and then the STP-set texels
plus every pixel of its untextured prims through the blend pipeline. The play
page's screen-prim pass (`u_stp_pass`) and the CPU twin
`screen_prim_raster` apply the same rule. Blending every texel instead is
visible: the fishing HUD draws an additive `m` glyph under its LINE plate,
which retail's all-STP-clear palette hides and a whole-quad blend shows
through the digits.

The corner projection itself is ported in [`billboard`](../../crates/render-kernels/src/billboard.rs)
(`FUN_800195a8`): `project_billboard` transforms a center point to view
space under the ambient camera (MVMVA, low-halfword wrap), fans out the
four ±half-size corners, optionally spins them in-plane (`Rz` from the
12-bit PSX angle space), and perspective-divides each (RTPT×3 + RTPS),
returning the screen corners in the exact order the retail `POLY_FT4`
packet consumes plus the OT-bucket depth. The divide is the GTE's UNR
reciprocal and each stored corner is saturated to the SXY FIFO's signed
11 bits, both through the same kernels the `Camera::transform` COP2
oracle is pinned against - so a behind-camera corner takes the hardware's
`0x1FFFF`-quotient smear rather than a sentinel. `afterimage::
project_streak_corners` reproduces the streak caller's invocation
(`+0x120` Y push, dynamic half-width, half-height `0x100`). `psx_sin` /
`psx_cos` reproduce the retail `RotMatrix*` trig LUT -
`trunc(4096·sin(2π·a/4096))`, pinned entry-for-entry by the disc-gated
`gte_sin_lut_real` oracle in `engine-shell`.

A `ScreenQuad` also carries an optional per-vertex `gouraud` array, which
makes it a `POLY_GT4` rather than a `POLY_FT4`. The transition styles need
it: a built transition quad carries a separate top-edge and bottom-edge
colour, and the gradient between them *is* the effect.

Two render targets consume the pass. `RenderTarget::ScreenOverlay` is the
whole-frame form - it clears and draws nothing but quads.
`RenderTarget::SceneWithScreenPrims` is the compositing form, drawing the
ordered quads on top of a `Scene` in the same frame; that is what any real
consumer needs, since a streak over a battle scene or a transition strip
over a field scene cannot be expressed by the whole-frame form. Retail draws
no such distinction - 3D primitives and screen-space packets share one
ordering table and one `DrawOTag` walk - so the split is a port artifact and
this variant is where the halves meet.

<a id="a-casts-frames-build-on-earlier-ones"></a>

**A cast's frames build on earlier ones.** A retail battle frame's farthest bucket
holds an opaque black rect, and an arm that arms the wash has it hold the subtractive
quad instead, so that frame's additive layers sum over the frames before
([cast modules](cast-module.md#presentation-calls)). `Renderer::set_frame_persist`
reproduces it on the native target, which is the one target that outlives its
frame: while `World::cast_frame_persists` holds, each frame loads an earlier
frame instead of clearing, and a capture seeds from the same one. Retail's
earlier frame is the same display page two game frames back, eight vsyncs at the
frame step a heavy arm runs at, so the renderer keeps a ring of eight frames
rather than building on the last one, which fills the buffer eight times too
fast. The frame step itself is [a table read off a recording](battle-stage-camera.md#what-the-arm-step-table-is), not a
measurement of the running frame's cost.

The ring is kept from the moment a cast module is resident, not from the first
persisting frame (`Renderer::set_frame_ring_keep`, `World::cast_module_resident`).
An arm that arms the wash does so from its own body, a pass after the arm before it
set the phase, so the run's first persisting frame reads a page two passes back that
predates the run. Seeded from the last frame instead, it read the run's own first
frame: Thunder Storm's arm 4 stood its second pass on its first pass's white, where
retail's other page still held the dark frame before the arm (luma `231` against
`144` on the left edge). Kept, the two pages part as retail's do and arm 4 reads
`.862` against `.839` on `noaru_gilium_lv2_thunder_storm`.

The play page does the same under PSX rasterisation: `LegaiaPsxTarget`
([`webgl-psx-target.js`](../../site/js/webgl-psx-target.js)) holds the same ring,
`renderAssembled` asks it for the picture at the scene's clear (after the frame's
ticks, through `play_cast_frame_persists`) and `play_cast_module_resident` keeps
the ring filling while one may be asked for. Bloom stands the frames down on both
hosts, since it reads the frame it adds to. The page's text layer is a canvas of
its own, so a name plaque's letters do not enter the ring as the window's do. The
window's own target and the browser's clean-rasterisation canvas still clear:
there the additive arms draw dimmer than retail. Both hosts advance the ring once
per drawn frame, which is a vsync only on a 60 Hz display.

**Backdrops draw first.** A screen primitive whose four corners sit at the
far plane (`ScreenPrim::is_backdrop`) is one retail links into the farthest
bucket: PROT 0898's cast colour wash (`ot + 0x3FFC`) and the overworld sky
band. Composited after the 3D pass, a far-plane depth test keeps it off
opaque geometry but not off a blended primitive, which writes no depth, so a
wash drawn there erases every additive effect over the sky. Both hosts split the under
list (`screen_prim::split_backdrop`) and draw the backdrops in the scene pass
after the clear and before the first 3D draw: the native renderer's
`draw_screen_overlay_backdrop_runs`, the play page's `preSceneHook` on
`renderAssembled` (`play_screen_prim_backdrop_run_end` names the span).

The primitives are authored in the **PSX display space** (320x240), not the
window: every retail emitter clamps against `0x140`/`0xF0`, so the renderer
hands `build_geometry` that space and the overlay stretches over the whole
surface.

Fixed-point GTE math helpers (`q3.12` rotation, `q19.12` translation)
live in [`gte`](../../crates/render-kernels/src/gte.rs) - hosted wgpu-free in
`legaia-render-kernels` so the browser play page links the same arithmetic;
production rendering still uses f32
wgpu math, but the module is the single citation point for retail-correct
fixed-point arithmetic when re-targeting captured GTE traces.

## Landing a drawn frame back in VRAM

On the console the framebuffer *is* VRAM - the display area is a rect inside
the same 1024x512 halfword page textures are read from - so a primitive can
sample pixels the GPU drew moments earlier. The renderer only ever pushed
the software page *to* the GPU; [`vram_capture`](../../crates/render-kernels/src/vram_capture.rs)
(wgpu-free, in `legaia-render-kernels`) is the
missing direction. `blit_rgba_into_vram` quantises an RGBA8 readback to
BGR555 and writes it into a `psx_tim::Vram` rect, and
`Renderer::capture_into_vram` wires that to `capture_rgba`. The write lands
in the CPU-side page, so the capture is equally visible to `move_image`,
`region_has_data` and the VRAM parity oracle.

The quantisation is exact rather than approximate: the shaders' last stage
expands a 5-bit channel as `(c5 << 3) | (c5 >> 2)`, so `byte >> 3` recovers
`c5` for all 32 values. A capture at the native 320x240 is not resampled at
all; a window-sized frame is point-sampled down.

`FIELD_CAPTURE_ROWS` / `FIELD_CAPTURE_COLS` name the two 15-bpp rects the
transition's texture-page words decode to, each spanning VRAM columns
`320..=639` on its own row. Only **`FIELD_CAPTURE_COLS` is a capture
destination**: the rows rect is the intermediate the curtain's column pass
renders into every frame, and no style samples it from the capture.
Inside the columns page the frame lands four rows down
(`FIELD_CAPTURE_LAND`, retail's drawn-row-to-`v` offset) and the rows around
it are cleared to transparent (`FIELD_CAPTURE_CLEARS`), as the transition
init does
([where the capture lands](battle-stage-camera.md#where-the-capture-lands-in-the-page)).

## Field-to-battle transition emitter

`battle_intro` is the per-frame, per-style working-set
owner that stands between the (already live) transition state machine in
`engine-core` and the ordering table above. It seeds the selected style's
working set, advances it off the transition entity's own clock, and emits
`ScreenPrim`s plus the per-style fade. The emitter itself is wgpu-free and
lives in [`legaia-render-kernels`](../../crates/render-kernels/src/battle_intro.rs) - both hosts arm it -
and [`src/battle_intro.rs`](../../crates/engine-render/src/battle_intro.rs) here holds the
one renderer-bound wrapper, `update_field_capture`, which turns
`Renderer::capture_scene_rgba` output into the emitter's `land_capture_rgba` /
`refresh_captured_page` calls. The target it reads back is the caller's: the
play window hands it the frame it presents, screen layers and enhancement
passes included, less the transition's own primitives
([what the capture holds](battle-stage-camera.md#what-the-capture-holds)).

All five styles tick and all five emit: `emit_particle_field` (both particle
arms) plus `emit_spinup_ring`, `emit_tile`, `emit_swirl_band`, and
`intro_quad_to_screen` for the curtain.

The curtain is the one with a second output. Its packet builder produces
screen-space corners, so there is no projection step to invent - but retail
draws its two passes into two different places. The column pass runs under a
`SetDrawArea(320, 0, 320, 240)` that the row pass then samples as texture pages
`0x105` / `0x108`, so the effect is a render-to-texture and only the row pass is
on screen. `compose_curtain_intermediate` rasterises the column quads into that
rect on the CPU each frame (there is no render-to-VRAM target for a
screen-space quad list), and the one-shot field capture goes into the *columns*
rect only. The tile shatter - the default random-encounter style - has all of
its inputs pinned too: the ten-face packet is `engine-vm`'s `tile_face_quads`,
the projection + accept chain is the FT4 handler's (`emit_tile`, with
`euler_rot_psx` as the `FUN_80026988` port), and the 4bpp shade page its side
faces sample is `field_char_textures` entry 0, re-landed in the transition's
cloned page at capture time.

Two retail nuances stay un-carried and are stated rather than hidden: the
dispatcher's depth-cue bank (the screen overlay has no channel for it) and the
frame-to-frame **accumulation** every style rides on - retail subtracts a small
constant from the display buffer instead of clearing, so its strips leave
fading trails where the port's leave black.

## GTE register-transfer + memory ops

Beyond the cop2 instruction set the [`gte`](../../crates/render-kernels/src/gte.rs) module ships
the four MIPS register-transfer ops (`MFC2` / `MTC2` / `CFC2` / `CTC2`)
and the two memory ops (`LWC2` / `SWC2`) so engines can replay a captured
GTE trace without re-deriving the cop2 register layout. `read_data` /
`write_data` map the 32 cop2 data registers (V0..V2 packed pairs, RGBC,
OTZ, IR0..IR3, SXY-FIFO push slot `SXYP`, SZ-FIFO entries, RGB-FIFO
entries, MAC0..MAC3, packed `IRGB` / `ORGB`, `LZCS` / `LZCR`) to typed
register fields; `read_ctrl` / `write_ctrl` handle the 32 control
registers. LWC2 / SWC2 thread through a `Cop2Mem` trait - `VecMem`
backs replay against captured RAM snapshots; `NullMem` is the default
for tests that don't exercise memory.

## Battle HUD pipeline

`battle_hud_draws_for(font, frame, pen)` produces a `BattleHudDraws` for the
in-battle HUD: a `text` list sampling the dialog-font atlas and a `sprites`
list sampling the resident system-UI atlas. The view types `HudSlotView` /
`HudPopupView` / `HudLogView` keep the renderer agnostic to engine-core /
engine-vm types (matches the existing `ShopRow` / `level_up_draws_for`
pattern).

The default surface is retail's, off the packet-pinned
`engine-ui::battle_chrome`: per-member roster panels (102x48 at `y 164`) at
rest, replaced for the acting member by the full-width active-actor bar at
`(8, 188)`, each carrying name / `HP` label + `cur` right-aligned + `max`
running forward / the same pair for `MP` - and **no gauge bar**. A top-left
plaque names the actor the frame belongs to, which is also the port's whole
monster readout since retail draws no monster gauge. Popups sit at slot_y - 16 (heal = green, crit = yellow, plain damage =
cyan); fade alpha multiplies into the text colour's alpha channel. Monster
rows, the LV / AP tail and the "ENCOUNTER!" banner are diagnostic-only,
behind `LEGAIA_DIAG_HUD`. Geometry provenance, including which retail table
the measurement falsified, is on
[`docs/subsystems/battle.md`](battle.md#the-drawn-surface).

HP and MP readouts are tinted by the **four-tier retail colour law**
(`hp_bar_color_index` / `mp_bar_color_index`, ports of FUN_800349EC /
FUN_80035EA8): danger at `cur <= max >> 2`, caution at `cur <= max >> 1` or any
active status flag, normal above that, and - HP only - a K.O. tier at zero. The
native window is the consumer: `engine-shell/.../window/hud.rs` calls this
builder for every battle frame from the `BattleHud` model that
`window/battle.rs::sync_battle_hud_rows` refreshes.

Column offsets are pinned, not guessed - the bar's and the panel's off the
display-list walk, the diagnostic row's off retail-dialog-font advances. A
disc-gated test walks all three sets against the real font, which is what
catches a field overrunning the next column.

## Menu chrome

`menu_window_chrome_draws_for(rects, dst_rect, origin, scale)` is the
reusable 9-slice bordered-window primitive shared by every faithful menu
panel. It composes the interior fill + border of an arbitrary
`(x, y, w, h)` stage rect from the resident system-UI atlas tiles
(`SaveMenuAtlasRects`, the same `PROT.DAT[0x018E0]` sprite sheet the save
screen builds). `scale_stage_text_draws(draws, origin, scale)` is its text
companion: it maps a menu's glyphs, laid out in 320×240 stage pixels, into
surface coordinates so text and frame stay locked at any window size. The
field pause menu and its sub-screens (status / spells / items / equip /
arts) route through both, framed by the play-window at the placement rects
documented in [`docs/subsystems/field-menu.md`](field-menu.md).

## Frame profiler

`profile` is an opt-in per-frame timing breakdown for `play-window`. It is
off by default and free when off (every entry point short-circuits on one
cached `bool`), so the instrumented call sites cost a predicted branch per
frame. Enable it with `LEGAIA_PROFILE=1`; a rolling one-second summary goes
to stderr:

```text
[profile] 1052.7 fps over 629 frames | frame avg  0.93ms p50  0.82 p99  3.49 \
  | draws 288+92 | tick 0.00 pose:actor 0.25 pose:prop 0.11 pose 0.01 \
    drawlist 0.02 acquire 0.08 uniforms 0.02 encode 0.33 submit 0.07 present 0.03
```

The stages carve the frame at the boundaries that matter: `tick` (world
sim), `pose:*` (per-frame mesh skinning + upload), `drawlist` (building the
scene draw list), then the renderer's own `acquire` (swapchain wait),
`uniforms` (per-draw uniform staging), `encode`, `submit` and `present`.
`draws N+M` is the scene's textured + untextured draw-call count, so the
`encode` cost is attributable per draw.
With SSAO on, `ssao` banks encoding its pre-pass and passes, which `encode`
then excludes.

Two companion knobs make it a repeatable benchmark:

| Env var | Effect |
|---|---|
| `LEGAIA_PROFILE=1` | Enable the breakdown. |
| `LEGAIA_PROFILE_FRAMES=N` | Print a final summary and exit after `N` frames. |
| `LEGAIA_VSYNC=off` | Configure the surface with an uncapped present mode. |
| `LEGAIA_PROFILE_SKIP=N` | Leave the first `N` frames out of the closing `[bench]` line. |

`scripts/ci/frame-bench.py` runs these over a fixed scene set
([`frame-bench.md`](../tooling/frame-bench.md)).

`LEGAIA_VSYNC=off` matters for measurement: with the default `AutoVsync`
the frame time is pinned to the display refresh interval and the whole cost
of a frame lands in `acquire`, so a vsync'd run reads the refresh rate
rather than the engine's own headroom.

**Skinned actors are memoised, not rebuilt.** A field NPC's ANM clip is a
short loop over a fixed set of poses, so the skinned mesh for a given
`(placement slot, clip frame)` is a constant. `play-window` skins and
uploads it on the first visit to that frame and reuses the GPU buffers
afterwards; the playhead still advances every frame, so the animation is
unchanged. Rebuilding it per frame instead - re-deriving identical vertex
bytes into freshly allocated GPU buffers - dominates the field frame in a
populated town. `LEGAIA_POSE_CACHE_VERIFY=1` re-checks the memo against the
live pose on every cache hit and logs any mismatch, which is what pins the
`(slot, frame)` key as non-aliasing.

**A rebuilt mesh writes over last frame's buffers.** A posed body, a live prop
and a morph are rebuilt into a mesh of the same size per frame. A buffer dropped
within two frames of its birth goes back to a pool
([`buffer_pool`](../../crates/engine-render/src/renderer/buffer_pool.rs)), and the
next mesh of its exact size takes it. A long-lived mesh frees its buffers as ever.

**The shadow stage draws only what each light reaches.** A draw whose box lies
wholly outside a lamp's frustum writes no depth into that lamp's layer, so leaving
it out moves no shadow.

## Retail draw-decision kernels (reference ports)

A family of modules ports SCUS passes that *decide* what the draw path does
rather than emit geometry. Each is pure and unit-tested, and none is called by
a host: they carry `REPLACED-BY` naming the engine mechanism
that does the routine's job (or, for `battle_on_screen`, that nothing on the
disc references it).

- [`actor_bind`](../../crates/engine-scene/src/actor_bind.rs) (`FUN_80020f88`) - resolves an actor's
  mesh-pool index off its `.MAP` placement record (`rec[+0x10] + prefix`,
  the rule [`renderer.md`](renderer.md) records as
  replacing the falsified positional one) and reports whether a `0x9C`-byte
  render node must be allocated.
- [`battle_on_screen`](../../crates/render-kernels/src/battle_on_screen.rs) (`FUN_8005126C`) - re-anchors
  a battle sprite on its seat actor and tests the projected box's horizontal
  span against `[0, 0x140]`. Horizontal only: retail reads the X of two
  corners and no Y at all, which is what separates it from the rectangle
  probe `FUN_8001B73C`.
- [`mode_transition`](../../crates/engine-mode/src/mode_transition.rs) (`FUN_80016230`) - the
  mode-entry prologue: frame-pacing reset, the RAM-cached-overlay word-sum
  verdict, and the field snapshot a battle / cutscene / minigame mode is
  entered behind.
- [`contact_candidates`](../../crates/engine-field/src/contact_candidates.rs) (`FUN_801CF754`) - the field actor-contact
  **broad phase**, not a render cull: the per-frame candidate list (a
  `+/-0x180` window around the player, capped at `0x20`, falling back to the
  full actor walk when full) that the contact probe `FUN_801CFC40` reads.
  `REPLACED-BY` the engine's contact probes, which test every candidate
  directly; the retail-numbering reference stays here.

[`gte_trace`](../../crates/engine-render/src/gte_trace.rs) sits beside them as the harness rather than a
kernel: a deterministic record of the cop2 register state before and after
each op, so a captured GTE trace can be replayed against the `gte` module's
arithmetic.
