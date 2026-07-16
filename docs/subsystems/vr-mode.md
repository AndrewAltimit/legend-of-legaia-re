# VR mode (WebXR)

The static site's three WebGL2 scene pages - the [world overview](world-overview-viewer.md)
(the three kingdom continents), the assembled full-map / field-scene view
(asset viewer + game-world pages), and the play page - can present their
scene to a VR headset over WebXR: stereo rendering through the
`XRWebGLLayer`, head tracking, and thumbstick locomotion through the world.

**VR is an enhancement retail never had, and it is opt-in.** The 1998 game has no
stereo path; this is a presentation mode the port adds on top of the same geometry,
entered only when a reader presses the `VR:` button on a page. It sits alongside the
engine's other explicit, default-off enhancement toggles (dynamic lighting,
free-angle movement, the debug orbit camera) rather than claiming to reproduce
anything on the disc. Behavioural fidelity to retail remains the baseline for game
logic; nothing here changes simulation, and parity oracles and replays are
unaffected.

Nothing about the flat path changes. A browser that reports no
`immersive-vr` device runs exactly the render code it ran before: no
global is shadowed, no GL context attribute is altered. The VR button
still renders - as a `VR unavailable` affordance whose hover/click text
names the specific reason - so the feature is never silently absent (see
[Availability + diagnostics](#availability--diagnostics)).

The flat renderer stays the geometry source: only the framebuffer (per-eye
`XRWebGLLayer` + scissor) and the view-projection (per-eye XRFrame matrices) fork.

Implementation: [`site/js/vr-mode.js`](../../site/js/vr-mode.js), plus
per-page hooks in `world-overview-app.js`, `field-scene-view.js` and
`play-app.js`.

## Contents

- [Where the render path forks](#where-the-render-path-forks)
- [The world -> XR transform](#the-world---xr-transform) · [why the world is mirrored](#why-the-world-is-mirrored)
- [World scale, and what it means to "be there"](#world-scale-and-what-it-means-to-be-there)
- [Viewing modes: first-person](#viewing-modes-first-person)
- [Spawn placement](#spawn-placement)
- [Locomotion + controls](#locomotion--controls)
- [Availability + diagnostics](#availability--diagnostics)
- [Session lifecycle](#session-lifecycle)
- [Verification](#verification)

## Where the render path forks

The flat renderer stays the single source of geometry. `TmdRenderer`
(`webgl-tmd.js`) is untouched: VR uploads no mesh, compiles no shader and
reorders no draw. Exactly two things fork, and both fork *around*
`renderAssembled` rather than inside it.

**The framebuffer + viewport.** The XR frame loop binds the
`XRWebGLLayer`'s framebuffer, then draws the scene once per `XRView`.
`renderAssembled` sets its own full-canvas viewport and issues an
unconditional `gl.clear`, so per eye the VR loop:

1. sets `gl.scissor` to that eye's `layer.getViewport(view)` rect and
   enables `SCISSOR_TEST` - this is what confines the *clear*, so the
   second eye does not wipe the first;
2. shadows `gl.viewport` on the context instance for the duration of the
   call, so the renderer's own `gl.viewport(0, 0, w, h)` lands on the eye
   rect;
3. calls the page's normal draw closure.

Both shadows are removed in a `finally`.

**The view-projection matrix.** `renderAssembled` builds its VP from the
page's orbit camera by calling the global `buildWorldOrbitVp`
(`webgl-math.js`). Because that is a top-level function declaration in a
classic script, it lives on the global object and the renderer's reference
to it resolves through the global scope *at call time* - so VR mode
replaces the property with a pass-through that returns the XR matrix while
an eye is drawing and delegates to the original otherwise. The shadow is
installed on the first `Enter VR` press and is inert whenever no session is
running.

Per eye:

```
vp = view.projectionMatrix · view.transform.inverse.matrix · W
```

where `W` is the world -> XR transform below. The renderer multiplies that
by each draw's model matrix exactly as it always has.

## The world -> XR transform

The renderer works in retail world units (`+Y` up, after the per-placement
`diag(1, -1, 1)` flip of the PSX `+Y`-down frame). WebXR works in metres.
`W` is the bridge, for a viewer standing at world point `p` facing `yaw`
with `u` world units per metre:

```
W = MirrorX · RotY(-yaw) · Scale(1/u) · Translate(-p)
```

The XR reference space is `local-floor` when the device offers it, so the
XR origin sits on the physical floor and the headset reports itself ~1.6 m
above it. That falls out of `W` as `1.6 · u` world units above the scene's
floor - which is the whole trick: the *only* thing that decides whether you
stand in a town or tower over a continent is `u`. `local` is the fallback,
and there the world anchor is lifted by a hinted eye height instead.

The inverse of `W`'s linear half turns an XR-space direction (a thumbstick
push resolved against the head's forward vector) back into a world-space
delta, so locomotion speed is naturally expressed in metres/second and
means the same thing at every scale.

### Why the world is mirrored

Both flat projections mirror screen X - the retail horizontal flip;
`buildWorldOrbitVp` negates `P[0]`, `buildTopDownVp` swaps ortho's
left/right - and the fragment shader compensates with a hardcoded
`u_normal_sign = -1` because that mirror flips the handedness of the
screen-space derivatives it builds its shading normal from.

An XR view-projection cannot mirror: the projection and view matrices come
from the device. So the mirror moves into the *world* transform instead.
This keeps the shading correct (the net world -> clip determinant sign
matches the flat path, so `u_normal_sign = -1` still holds) and keeps the
VR image oriented like the page the user just pressed the button on.
Stereo is unaffected: both eyes view the same mirrored scene from their
true XR positions, so depth and parallax are exactly as the device intends.

## World scale, and what it means to "be there"

| Page | Units/metre | What that makes the scene |
|---|---|---|
| World overview | 2000 | A 16320-unit continent becomes an ~8 m diorama; the 1.6 m eye height puts the viewer 3200 world units above the sea plane, looking down at the kingdom laid out around them. |
| Full-map view / play | 76 | Human scale. You stand in the town. |

The field figure is anchored on the character mesh: the field-form player
model stands ~130 world units tall (the play page's follow camera is tuned
around that), so a 1.7 m human puts a metre at ~76 units - which
independently makes the 128-unit walkability tile a believable 1.7 m stride
and a village house a believable 8 m.

Scale is not fixed: the grip buttons rescale the world live (clamped to
8..20000 units/metre), around the *head* rather than the feet so the scene
does not lurch through the viewer. Squeezing the world overview down to 76
walks you onto the world map at human height; squeezing a town up turns it
into a model on a table. The XR depth range (`depthNear` / `depthFar`) is
re-derived from the world extent at the current scale on every change.

## Viewing modes: first-person

The play page and the world-overview page each offer two viewing modes,
cycled by a `VR: <mode>` button next to `Enter VR` (usable before entry and
mid-session - a live session is re-placed at the new mode's scale on the
spot). A page attaches them as a `modes` list on the shared handle; each
mode may override the world scale, the spawn, and the locomotion contract.
Pages with a single framing (the viewer / game-world full-map view) attach
no modes and behave exactly as before modes existed.

**Play page: `Spectator` / `First-person`.** Spectator is the free-flying
camera in the running world, spawned where the follow camera sits.
First-person is *what Vahn sees*: the rig's floor origin is pinned to the
player actor's feet every XR frame (read back *after* the engine tick, so
the engine - not the headset - owns the walk), and the world scale is
derived live from the player mesh instead of the 76-unit rule of thumb:
`units/metre = measured standing height of the posed player mesh / 1.7 m`
(~132 units tall in the field, so ~78 units/m; the measurement is taken at
placement time because at scene build the not-yet-posed mesh is an
object-local vertex pile about half the standing height, and anything
under 90 units falls back to the 130-unit anchor). The eye height itself
comes from the physical headset on a `local-floor` device; the `local`
fallback lifts the anchor by a 1.6 m hint.

First-person locomotion drives the **actual player through the engine**,
not a free-fly ghost: each XR frame the left stick is routed into the
engine's free-movement controller - the same collision-checked path the
keyboard uses - by pointing the engine's camera azimuth along the gaze
(rig yaw + head yaw, so "stick forward" walks where you look) and feeding
the stick as the analog axes of the engine's precise-locomotion decode
(`set_precise_movement` / `set_left_stick` on the WASM runtime; a stale
cached WASM without those falls back to quantised 8-way d-pad bits).
Walkability and wall collision therefore apply exactly as on foot, and the
rig follows whatever the engine actually let the player do. The player
mesh is dropped from the eye draws (the viewer is inside it) and the
third-person occluder cull is disabled (there is no lens-to-player segment
to occlude). Trigger or A maps to Cross (talk / confirm), B to Circle, so
NPC dialogue works from inside the world.

**World overview: `Diorama` / `On the ground`.** Diorama is the 2000
units/m tabletop continent. On the ground is human scale (76 units/m, the
same character-height anchor as the towns) standing **on** the continent
terrain: a bilinear sampler over the walk heightfield (vertices on the
128-unit cell grid, world-up Y = the negated stored Y) snaps the rig's
floor origin to the surface under it every frame, the spawn drops onto the
terrain under the framing centre, and the right stick's altitude axis is
disabled - locomotion walks the surface, up hills and down to the coast.
Off-terrain samples fall back to the sea plane (`y = 0`).

## Spawn placement

The world overview spawns at the framing centre on the sea plane (`y = 0`),
which at diorama scale is a table-height view of the whole continent.

A field scene spawns at the **component-wise median of the placed objects**
- not the framing AABB centre. A field map is a 128x128-tile grid whose
town usually occupies a corner of it, so the AABB centre lands on empty
ground with the buildings a hundred metres away; the median lands in the
village. Y comes from the same set, because a placement's world Y *is* the
floor tile it stands on. Terrain tiles are excluded from the median - they
tile the whole grid and would drag it back to the centre.

The play page spawns where its third-person follow camera sits (behind the
character, at their floor height), so entering VR keeps the frame the user
was already looking at.

Heading is the flat camera's heading: the orbit camera at yaw `psi` looks
along `(-sin psi, cos psi)`, and VR yaw 0 faces world `-Z`, so the spawn
yaw is `PI - psi`.

## Locomotion + controls

Smooth locomotion on the left stick, comfort-first turning on the right.
There is no teleport arc: outside the play page's first-person mode the
browser pages carry no collision query (the walkability grid lives in the
engine, not in the viewer's assembled draw list), so a teleport marker
could not be validated against the floor - free flight is honest about
that and the right tool for a diorama, while first-person routes movement
through the engine's real collision instead.

Free-fly modes (spectator / diorama / the full-map view):

| Input | Effect |
|---|---|
| Left stick | Walk / fly horizontally, relative to where you are looking. |
| Right stick X | Snap turn, 30 degrees per flick (latched - no continuous rotation). |
| Right stick Y | Altitude (disabled in the ground-locked world-overview mode). |
| Trigger (either hand) | 4x speed. |
| Grip, left / right | Shrink / grow the world (rescale around the head). |
| A / X (right hand) | Return to the entry pose (position, heading and scale). |

Play-page first-person swaps the contract: left stick = drive the player
through the engine (gaze-relative), right stick X = snap turn, trigger or
A = Cross (talk / confirm), B = Circle (cancel); altitude, rescale and the
home reset are inert (the rig is anchored to the player).

Axes are read `xr-standard`-first (thumbstick on axes 2/3), falling back to
axes 0/1 for simpler mappings, with a dead zone.

## Availability + diagnostics

The VR button is always rendered on a 3D page's control bar - availability
only changes what it says and what a click does. The probe drives five
button states:

| Probe result | Button | Click |
|---|---|---|
| `immersive-vr` supported, scene loaded | `Enter VR` | starts the session |
| supported, scene still loading | `Enter VR` (dimmed) | "the scene is still loading" note |
| probe still running | `VR…` | shows the probing note |
| no `navigator.xr` / unsupported / probe timeout | `VR unavailable` (dashed) | shows the specific diagnostic **and re-runs the probe** |

Each unavailable state carries its own diagnostic, on the button's hover
title and in a dismissible on-page note on click:

- **No `navigator.xr`, insecure context.** WebXR only exists in a
  [secure context](https://developer.mozilla.org/docs/Web/Security/Secure_Contexts) -
  a page served over plain `http://` from a LAN IP (the natural way to
  serve a local static site: `python3 -m http.server` and browse to
  `http://192.168.x.x:8000`) has **no `navigator.xr` at all**. The
  diagnostic names the current origin and says to reopen the site via
  `http://localhost` (an SSH tunnel / port-forward works from another
  machine) or https.
- **No `navigator.xr`, secure context.** The browser has no WebXR
  (Firefox's is effectively dead). The diagnostic says to use Chrome or
  Edge on desktop with SteamVR set as the active OpenXR runtime.
- **`isSessionSupported('immersive-vr')` answers no / rejects.** The
  browser has WebXR but no usable VR runtime. The diagnostic says to start
  SteamVR and set it as the active OpenXR runtime (SteamVR settings →
  OpenXR).
- **The probe never settles.** `isSessionSupported` is specified to
  settle, but a browser that exposes `navigator.xr` with no backing
  runtime can leave it pending forever (headless Chromium does). The probe
  is raced against a timeout and reports "XR device check timed out".

The probe re-runs on the `XRSystem`'s `devicechange` event and on any
click of the unavailable button, so starting SteamVR or plugging the
headset in *after* page load arms the button without a reload.

**Serving for a desktop headset (Valve Index et al.):** SteamVR running
and selected as the OpenXR runtime, Chrome or Edge, and the site reached
over `http://localhost:<port>` or https - `cd site && python3 -m
http.server 8000`, then browse `http://localhost:8000` on the same
machine (or SSH-forward the port from the VR machine).

## Session lifecycle

`navigator.xr.isSessionSupported('immersive-vr')` is probed at script load
(and re-probed on `devicechange` / an unavailable-button click, see above).
Only when it answers yes does the module install its `getContext` hook,
which adds `xrCompatible: true` to WebGL context creation from then on -
so contexts built after the probe never need the retro-fit
`makeXRCompatible()` call, which is allowed to drop the context when it has
to move to another GPU adapter. Contexts that predate the probe still get
the retro-fit call, raced against a timeout: the promise is specified to
settle, but a browser with `navigator.xr` and no backing XR runtime can
leave it pending forever, and a hung await would wedge the button. If the
context genuinely cannot present, the `XRWebGLLayer` constructor throws and
that is the error the page reports.

The button arms for entry only when the device is present *and* the page
has a scene to show; it flips to `Exit VR` while presenting. The host pauses its
own `requestAnimationFrame` loop on entry (the XR frame loop drives the
draw, and on the play page also the engine tick, so NPCs keep moving and
the keyboard still steers the character) and resumes it on exit, with the
flat camera restored to where it was.

A live session survives a scene swap on pages that keep one canvas (the
game-world page's town navigator, a door transition on the play page) - the
GL context and renderer are the same, so the viewer is simply re-placed in
the new map. It cannot survive a *canvas* swap (the world-overview page
rebuilds its canvas per load), so those paths end the session first.

## Verification

Headless Chromium has no XR runtime, so the VR path is exercised against an
injected mock `navigator.xr` that reports `immersive-vr` supported and
hands back synthetic `XRFrame`s with two eye views. The mock's
`XRWebGLLayer` exposes `framebuffer = null`, so the "eyes" rasterise
side-by-side into the page canvas and the drawing buffer can be sampled
from inside the frame callback. That pins:

- both eye rects rasterise (scissor + viewport shadowing works, and the
  second eye's clear does not wipe the first);
- the two halves differ, and diverge hugely when the mock's IPD is widened
  to an absurd baseline - so each eye really is using its own view matrix;
- a thumbstick push moves the viewer, along the direction the (synthetic)
  head is facing, and a head yaw of 90 degrees rotates that direction by
  90 degrees;
- the snap turn is exactly 30 degrees;
- the engine keeps ticking under the XR frame loop on the play page;
- first-person on the play page: the rig's floor origin sits exactly at
  the player actor's feet, the scale equals the measured mesh height over
  1.7 m, a stick push moves the *engine's* player (and the commanded
  heading matches the gaze even when wall collision deflects the realized
  path), the rig tracks the player while walking, and the mode toggle
  swaps scales live (read back through `window.__vrDebug()`);
- ground mode on the world overview: the rig's Y equals the terrain
  sampler's height under it at spawn, after locomotion, and under an
  attempted altitude push (no free-fly), at 76 units/m;
- and, with no `navigator.xr` at all, every page loads with zero console
  errors and a visible `VR unavailable` button whose hover title and
  on-click note carry the secure-context diagnostic; with a mock whose
  `isSessionSupported` never settles, the timeout diagnostic; and a mock
  `devicechange` that flips support on rewrites the button to `Enter VR`
  without a reload.

What a mock cannot pin: real headset presentation - compositor timing,
reprojection, the physical comfort of the chosen scale and speeds, and
whether `makeXRCompatible()` triggers a context loss on a multi-GPU
machine. Those need hardware.
