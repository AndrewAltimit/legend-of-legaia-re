/* world-overview-ground.js - ground math for the world-overview page.
 *
 * Two pure functions over the assembled kingdom's geometry: the bilinear
 * terrain-height sampler the VR ground mode stands on, and the top-view
 * camera framing over the ground / placement extent. Neither touches the
 * DOM, the renderer or the page's state; the page hands in the numbers and
 * applies the result.
 *
 * Classic script; loads before world-overview-app.js. */

(function () {
  /* Bilinear terrain-height sampler over the walk heightfield the kingdom
   * load uploaded: vertices sit on the 128-unit cell grid at
   * `(col*128, -lut, row*128)` (pre-Y-flip frame - world-up Y is the
   * negation), so bucketing by grid node and lerping the enclosing four
   * reproduces the surface the GPU rasterises. Cells outside the visible
   * continent fall back to the sea plane (y = 0). This is what lets the VR
   * ground mode STAND on the map: the rig's floor origin snaps to this
   * height every frame.
   *
   * `pos` is the heightfield's flat xyz position list; returns
   * `(x, z) -> terrain surface Y (world up)`, or null when there is no
   * surface to sample. */
  function buildSampler(pos) {
    if (!pos || pos.length < 12) return null;
    const TILE = 128;
    const nodes = new Map();
    for (let i = 0; i + 2 < pos.length; i += 3) {
      const key = Math.round(pos[i] / TILE) * 4096 + Math.round(pos[i + 2] / TILE);
      const y = -pos[i + 1];
      const cur = nodes.get(key);
      if (cur === undefined || y > cur) nodes.set(key, y);
    }
    return (x, z) => {
      const gx = x / TILE, gz = z / TILE;
      const x0 = Math.floor(gx), z0 = Math.floor(gz);
      const fx = gx - x0, fz = gz - z0;
      let sum = 0, wsum = 0;
      for (let dx = 0; dx <= 1; dx++) {
        for (let dz = 0; dz <= 1; dz++) {
          const v = nodes.get((x0 + dx) * 4096 + (z0 + dz));
          if (v === undefined) continue;
          const w = (dx ? fx : 1 - fx) * (dz ? fz : 1 - fz);
          sum += v * w;
          wsum += w;
        }
      }
      return wsum > 1e-6 ? sum / wsum : 0;
    };
  }

  /* The world camera's framing: `{ centerX, centerZ, halfWidth,
   * halfHeight }` for a kingdom whose retail spawn framing is `retail`
   * (or undefined), whose landmark draws are `drawPlacements`, whose
   * placement extent is `ext` and whose ground heightfield AABB is
   * `groundAabb` (or null). */
  function frameCamera(retail, drawPlacements, ext, groundAabb) {
    /* When the continent ground heightfield is present it's the dominant
     * geometry, so frame the whole continent (centred on its XZ centroid,
     * sized to cover its extent + a margin) rather than the sparse
     * landmark cluster. The "lock to retail top-view" button still gives
     * the retail spawn framing on demand. */
    if (groundAabb && groundAabb.sx > 0 && groundAabb.sz > 0) {
      return {
        centerX: groundAabb.cx,
        centerZ: groundAabb.cz,
        halfWidth: groundAabb.sx / 2 + 1000,
        halfHeight: groundAabb.sz / 2 + 1000,
      };
    }
    if (drawPlacements && drawPlacements.length > 0) {
      let xmin = Infinity, xmax = -Infinity, zmin = Infinity, zmax = -Infinity;
      for (const p of drawPlacements) {
        if (p.x < xmin) xmin = p.x;
        if (p.x > xmax) xmax = p.x;
        if (p.z < zmin) zmin = p.z;
        if (p.z > zmax) zmax = p.z;
      }
      if (retail) {
        const dx = Math.max(retail.centerX - xmin, xmax - retail.centerX);
        const dz = Math.max(retail.centerZ - zmin, zmax - retail.centerZ);
        return {
          centerX: retail.centerX,
          centerZ: retail.centerZ,
          halfWidth: Math.max(dx, 1500) + 1500,
          halfHeight: Math.max(dz, 1500) + 1500,
        };
      }
      return {
        centerX: (xmin + xmax) / 2,
        centerZ: (zmin + zmax) / 2,
        halfWidth: Math.max((xmax - xmin) / 2, 1500) + 1500,
        halfHeight: Math.max((zmax - zmin) / 2, 1500) + 1500,
      };
    }
    if (retail) {
      return {
        centerX: retail.centerX,
        centerZ: retail.centerZ,
        halfWidth: ext[0] / 2 + 1000,
        halfHeight: ext[1] / 2 + 1000,
      };
    }
    return {
      centerX: ext[0] / 2,
      centerZ: ext[1] / 2,
      halfWidth: ext[0] / 2 + 1000,
      halfHeight: ext[1] / 2 + 1000,
    };
  }

  window.LegaiaWorldOverviewGround = { buildSampler, frameCamera };
})();
