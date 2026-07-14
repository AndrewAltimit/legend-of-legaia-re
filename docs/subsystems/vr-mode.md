# VR mode (WebXR)

The static site's three WebGL2 scene pages - the [world overview](world-overview-viewer.md)
(the three kingdom continents), the assembled full-map / field-scene view
(asset viewer + game-world pages), and the play page - can present their
scene to a VR headset over WebXR: stereo rendering through the
`XRWebGLLayer`, head tracking, and thumbstick locomotion through the world.

Nothing about the flat path changes. A browser that reports no
`immersive-vr` device runs exactly the code it ran before: the button
stays hidden, no global is shadowed, no GL context attribute is altered.

Implementation: [`site/js/vr-mode.js`](../../site/js/vr-mode.js), plus
per-page hooks in `world-overview-app.js`, `field-scene-view.js` and
`play-app.js`.

## Contents

- [Where the render path forks](#where-the-render-path-forks)
- [The world -> XR transform](#the-world---xr-transform) · [why the world is mirrored](#why-the-world-is-mirrored)
- [World scale, and what it means to "be there"](#world-scale-and-what-it-means-to-be-there)
- [Spawn placement](#spawn-placement)
- [Locomotion + controls](#locomotion--controls)
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
There is no teleport arc: the browser pages carry no collision or
ground-height query (the walkability grid lives in the engine, not in the
viewer's assembled draw list), so a teleport marker could not be validated
against the floor - free flight is both honest about that and the right
tool for a diorama.

| Input | Effect |
|---|---|
| Left stick | Walk / fly horizontally, relative to where you are looking. |
| Right stick X | Snap turn, 30 degrees per flick (latched - no continuous rotation). |
| Right stick Y | Altitude. |
| Trigger (either hand) | 4x speed. |
| Grip, left / right | Shrink / grow the world (rescale around the head). |
| A / X (right hand) | Return to the entry pose (position, heading and scale). |

Axes are read `xr-standard`-first (thumbstick on axes 2/3), falling back to
axes 0/1 for simpler mappings, with a dead zone.

## Session lifecycle

`navigator.xr.isSessionSupported('immersive-vr')` is probed once per page.
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

The button appears only when the device is present *and* the page has a
scene to show; it flips to `Exit VR` while presenting. The host pauses its
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
- and, with no `navigator.xr` at all, every page loads with zero console
  errors and no VR affordance.

What a mock cannot pin: real headset presentation - compositor timing,
reprojection, the physical comfort of the chosen scale and speeds, and
whether `makeXRCompatible()` triggers a context loss on a multi-GPU
machine. Those need hardware.
