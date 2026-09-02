// Optional realism enhancements over a built Legaia root - the builder's
// "Realism enhancements" foldout. Every pass defaults ON; untick them all
// for the faithful retail-shaded build. Everything is generated from
// scratch (shaders, dome/grass geometry, synthesized audio): no game data
// is created or shipped, and with every option off the build is
// byte-for-byte the faithful retail-shaded scene.
//
// What each pass does, and the source-data constraint it works around:
//
// - Lighting (ConvertToLit): the exported glbs are KHR_materials_unlit
//   with NO normals, so Unity lights can't touch them as imported. The
//   pass duplicates every mesh under the root into Assets/LegaiaGenerated
//   with smoothed normals (position-welded, sign-aligned - the PSX source
//   winding is mixed, so raw face normals point both ways and would
//   cancel; the lit shaders then light with the sign-independent
//   two-sided Lambert |N.L|, because no per-vertex sign choice survives
//   this data - the shader headers keep the failed-attempt history),
//   swaps every material for the kit's lit vertex-colour shaders (the
//   baked COLOR_0 retail shading keeps modulating, so the palette holds),
//   and adds a warm directional sun + trilight ambient + soft shadows.
//
// - Day/night (part of lighting): wires the LegaiaDayNight Udon behaviour
//   onto the sun - a server-time-synced sweep, same angle on every client.
//
// - Sky + fog: a procedural-skybox material (it tracks RenderSettings.sun,
//   so with day/night on the sky darkens by itself) and linear distance
//   fog scaled to the built root's bounds.
//
// - Foliage (ScatterGrass): scatters vertex-coloured grass-blade triangles
//   over upward-facing world triangles whose ground colour reads green
//   (texel x mean COLOR_0 at the triangle centre - the same product the
//   retail shading displays, so painted-flat and textured ground both
//   qualify). Blades are tinted from the sampled ground colour and sway
//   via the Legaia/Grass Wind shader. Deterministic per seed.
//
// - Interior shells (BuildInteriorShells): the doorway-teleport interiors
//   are unused corners of the same map, so from inside a room you see the
//   skybox above and the floating village past the doorway. The pass finds
//   each detached room (teleport endpoints beyond a spawn-distance
//   threshold, clustered), wraps it in a black dome wound to face INWARD
//   only - backface-culled, so it is invisible from outside, and casting
//   no shadows, so the sun still lights the room through it - and adds an
//   optional warm fill light so the room reads window-lit inside its
//   black surround. Retail frames these rooms against black space; this
//   restores that.
//
// - Texture smoothing: bilinear + anisotropic on every texture under the
//   root (the exports pin NEAREST for the PSX look). In-editor asset
//   tweak - a glb reimport resets it, rerun the pass after one.
//
// - Ambient audio: a synthesized wind/surf noise bed plus day (breeze +
//   birds) and night (crickets) beds whose volumes LegaiaDayNight
//   crossfades with the sun (WriteAmbienceWav / LegaiaAudioGen -
//   filtered noise, loop-crossfaded, written to LegaiaGenerated) on a
//   quiet 2D AudioSource. Not disc audio.
//
// - NPC wander: attaches the existing LegaiaNpcWander Udon behaviour to
//   every talk-kind NPC from the manifest (matched by spawn position).
//
// RenderSettings (sun, ambient, skybox, fog) are per-Unity-scene state:
// applying them from one built root is global to the scene, and turning
// the options off later does not revert them - reset via Window >
// Rendering > Lighting, and delete the LegaiaSun child.

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    [System.Serializable]
    public class LegaiaRealismOptions
    {
        public bool lighting = true;
        public float sunElevation = 55f;
        public float sunAzimuth = 40f;
        public float sunIntensity = 1.15f;
        public float shadowStrength = 0.75f;
        public bool dayNight = true;
        public float dayNightMinutes = 20f;
        // Midnight ambient as a fraction of the daytime trilight - the
        // landscape's night darkness (the sun itself is already off).
        // Very low: at 0.05 the ground still read well-lit at night, so
        // night is nearly black and the lamps/fires carve out the light.
        public float nightAmbient = 0.02f;
        // Warm point lights beside each village doorway, enabled by the
        // day/night behaviour only while the sun is down.
        public bool nightLamps = true;
        // Planted stake torches by each tree and each village doorway,
        // burning only while the sun is down.
        public bool nightTorches = true;
        public bool skyAndFog = true;
        public bool foliage = true;
        public float grassDensity = 6f;
        public float grassGreenThreshold = 0.03f;
        public int grassSeed = 1;
        public bool interiorShells = true;
        public bool interiorGlow = true;
        // Defaults sized for the 1 m-per-tile export scale (1/128); a
        // legacy 1/64 export wants roughly double on all three.
        public float interiorShellMargin = 1.5f;
        public float interiorRoomDistance = 30f;
        public bool smoothTextures = true;
        public bool ambientAudio = true;
        public float ambientVolume = 0.15f;
        public bool npcWander = true;
        public float wanderRadius = 1.25f;
        // Per-NPC facing trims that survive rebuilds, for the rare model
        // whose face is authored off the rig's -Z in VERTEX space (no
        // transform measurement can see that). "npc_30:90; npc_07:-90" -
        // each key matches the manifest file stem, value = degrees.
        public string wanderFacingOverrides = "";
        // Flattens the |N.L| angular term on NPC/prop meshes only (0 =
        // full Lambert, 1 = even): on a low-poly villager the lighting
        // terminator cuts a harsh dark band across the face.
        public float characterLightWrap = 0.75f;

        public bool AnyEnabled =>
            lighting || skyAndFog || foliage || interiorShells || smoothTextures ||
            ambientAudio || npcWander;

        public bool NeedsUdon => (lighting && dayNight) || npcWander;
    }

    public static class LegaiaRealism
    {
        public static void Apply(
            GameObject root, object manifest, string sceneName, LegaiaRealismOptions o)
        {
            string genDir = "Assets/LegaiaGenerated/" + sceneName + "/realism";
            Directory.CreateDirectory(genDir);
            try
            {
                // Ambience beds first: ApplySun (inside ConvertToLit) wires
                // the day/night crossfade to sources that must already exist.
                if (o.ambientAudio)
                    AddAmbience(root, genDir, o);
                if (o.lighting)
                {
                    EditorUtility.DisplayProgressBar("Legaia realism", "Lit materials + normals", 0.1f);
                    // Lamps + torches first: ApplySun (inside ConvertToLit)
                    // hands both containers to the day/night behaviour.
                    GameObject lamps = BuildNightLamps(root, manifest, genDir, o);
                    GameObject torches = BuildNightTorches(root, manifest,
                        "Assets/LegaiaGenerated/" + sceneName, o);
                    ConvertToLit(root, genDir, o, lamps, torches);
                }
                if (o.skyAndFog)
                    ApplySkyAndFog(root, genDir);
                if (o.smoothTextures)
                    SmoothTextures(root);
                if (o.foliage)
                {
                    EditorUtility.DisplayProgressBar("Legaia realism", "Scattering foliage", 0.5f);
                    ScatterGrass(root, genDir, o);
                }
                if (o.interiorShells && manifest != null)
                {
                    EditorUtility.DisplayProgressBar("Legaia realism", "Interior shells", 0.75f);
                    BuildInteriorShells(root, manifest, genDir, o);
                }
                if (o.npcWander && manifest != null)
                    WireWander(root, manifest, o,
                        LegaiaSceneSettings.Load(sceneName));
            }
            finally
            {
                EditorUtility.ClearProgressBar();
            }
            AssetDatabase.SaveAssets();
        }

        // --- Lighting -------------------------------------------------------

        static void ConvertToLit(GameObject root, string genDir,
            LegaiaRealismOptions o, GameObject nightLamps, GameObject nightTorches)
        {
            var cutout = Shader.Find("Legaia/Lit Vertex Color (Cutout)");
            var transparent = Shader.Find("Legaia/Lit Vertex Color (Transparent)");
            if (cutout == null || transparent == null)
            {
                Debug.LogError("[Legaia] lit shaders not found - is " +
                    "Assets/LegaiaWorld/Shaders/ imported? Lighting skipped.");
                return;
            }

            var meshCache = new Dictionary<Mesh, Mesh>();
            var matCache = new Dictionary<Material, Material>();
            int meshIdx = 0, matIdx = 0;
            foreach (var mf in root.GetComponentsInChildren<MeshFilter>(true))
                if (mf.sharedMesh != null)
                    mf.sharedMesh = SmoothedCopy(mf.sharedMesh, genDir, meshCache, ref meshIdx);
            foreach (var smr in root.GetComponentsInChildren<SkinnedMeshRenderer>(true))
                if (smr.sharedMesh != null)
                    smr.sharedMesh = SmoothedCopy(smr.sharedMesh, genDir, meshCache, ref meshIdx);

            foreach (var r in root.GetComponentsInChildren<Renderer>(true))
            {
                var mats = r.sharedMaterials;
                bool changed = false;
                for (int i = 0; i < mats.Length; i++)
                {
                    if (mats[i] == null)
                        continue;
                    var lit = LitVariant(mats[i], cutout, transparent, genDir, matCache, ref matIdx);
                    if (lit != mats[i])
                    {
                        mats[i] = lit;
                        changed = true;
                    }
                }
                if (changed)
                    r.sharedMaterials = mats;
            }

            // Character-scale meshes get wrap lighting: on a low-poly
            // villager the |N.L| terminator cuts a harsh dark band right
            // across the face, so their materials flatten the angular term
            // (shadow-map attenuation still applies). World surfaces keep
            // full |N.L| so buildings stay directionally lit. Safe to set
            // in place: NPC/prop glbs import their own material assets, so
            // their lit twins are never shared with world-glb materials.
            int wrapped = 0;
            if (o.characterLightWrap > 0f)
            {
                var seen = new HashSet<Material>();
                foreach (string sub in new[] { "npcs", "props" })
                {
                    var t = root.transform.Find(sub);
                    if (t == null)
                        continue;
                    foreach (var r in t.GetComponentsInChildren<Renderer>(true))
                        foreach (var mat in r.sharedMaterials)
                            if (mat != null && mat.shader != null &&
                                mat.shader.name.StartsWith("Legaia/Lit") &&
                                seen.Add(mat))
                            {
                                mat.SetFloat("_LightWrap", o.characterLightWrap);
                                wrapped++;
                            }
                }
            }

            ApplySun(root, o, nightLamps, nightTorches);
            Debug.Log("[Legaia] lit conversion: " + meshCache.Count + " mesh(es), " +
                      matCache.Count + " material(s), " + wrapped +
                      " character material(s) wrap-lit.");
        }

        /// A smoothed-normal duplicate of `src`, saved as a readable asset
        /// (skinned meshes keep their blendshapes and bindposes, so the
        /// world morph clip still drives the copy). Idempotent: a mesh that
        /// already lives under LegaiaGenerated is returned as-is.
        static Mesh SmoothedCopy(Mesh src, string genDir, Dictionary<Mesh, Mesh> cache, ref int idx)
        {
            if (cache.TryGetValue(src, out var got))
                return got;
            string srcPath = AssetDatabase.GetAssetPath(src);
            if (!string.IsNullOrEmpty(srcPath) && srcPath.Contains("/LegaiaGenerated/"))
            {
                cache[src] = src;
                return src;
            }
            var m = Object.Instantiate(src);
            m.name = src.name; // keep the twin-matching name
            SmoothNormalsInPlace(m);
            string path = genDir + "/" + LegaiaWorldBuilder.Sanitize(src.name)
                + "_lit_" + idx++ + ".asset";
            AssetDatabase.DeleteAsset(path);
            AssetDatabase.CreateAsset(m, path);
            cache[src] = m;
            return m;
        }

        /// Position-welded smooth normals over mixed-winding source data:
        /// contributions are sign-aligned inside each weld cell (opposite
        /// windings reinforce instead of cancelling), then every cell picks
        /// a canonical global sign (up, then +x, then +z) so neighbouring
        /// cells agree and interpolation never crosses zero. The lit
        /// shaders' |N.L| lighting is sign-independent, so the sign only
        /// needs to be locally consistent (for clean interpolation), not
        /// globally correct - and the trilight ambient reads the signed
        /// normal, which is why the canonical up matters for the ground.
        static void SmoothNormalsInPlace(Mesh m)
        {
            var verts = m.vertices;
            var tris = m.triangles;
            var acc = new Dictionary<Vector3Int, Vector3>(verts.Length);
            Vector3Int Key(Vector3 v) => new Vector3Int(
                Mathf.RoundToInt(v.x * 500f),
                Mathf.RoundToInt(v.y * 500f),
                Mathf.RoundToInt(v.z * 500f));

            for (int i = 0; i < tris.Length; i += 3)
            {
                Vector3 fn = Vector3.Cross(
                    verts[tris[i + 1]] - verts[tris[i]],
                    verts[tris[i + 2]] - verts[tris[i]]);
                if (fn.sqrMagnitude < 1e-12f)
                    continue;
                for (int j = 0; j < 3; j++)
                {
                    var k = Key(verts[tris[i + j]]);
                    acc.TryGetValue(k, out var n);
                    acc[k] = n + (Vector3.Dot(n, fn) < 0f ? -fn : fn);
                }
            }

            var normals = new Vector3[verts.Length];
            for (int i = 0; i < verts.Length; i++)
            {
                acc.TryGetValue(Key(verts[i]), out var n);
                if (n.sqrMagnitude < 1e-12f)
                {
                    normals[i] = Vector3.up;
                    continue;
                }
                n.Normalize();
                if (n.y < -0.02f || (Mathf.Abs(n.y) <= 0.02f &&
                    (n.x < -0.02f || (Mathf.Abs(n.x) <= 0.02f && n.z < 0f))))
                    n = -n;
                normals[i] = n;
            }
            m.normals = normals;
        }

        /// A lit twin of a glTFast-imported material: cutout or transparent
        /// by render queue, base texture + tint + cutoff carried over.
        /// Materials already on a Legaia/ shader pass through (idempotent).
        static Material LitVariant(Material src, Shader cutout, Shader transparent,
            string genDir, Dictionary<Material, Material> cache, ref int idx)
        {
            if (src.shader != null && src.shader.name.StartsWith("Legaia/"))
                return src;
            if (cache.TryGetValue(src, out var got))
                return got;
            bool blend = src.renderQueue >=
                (int)UnityEngine.Rendering.RenderQueue.Transparent;
            var m = new Material(blend ? transparent : cutout)
            {
                name = src.name + "_lit"
            };
            var tex = ExtractMainTexture(src);
            if (tex != null)
            {
                m.mainTexture = tex;
                if (src.mainTexture != null)
                {
                    m.mainTextureScale = src.mainTextureScale;
                    m.mainTextureOffset = src.mainTextureOffset;
                }
            }
            foreach (var p in new[] { "_Color", "baseColorFactor", "_BaseColor" })
                if (src.HasProperty(p))
                {
                    m.SetColor("_Color", src.GetColor(p));
                    break;
                }
            if (!blend)
            {
                float cut = 0.5f;
                foreach (var p in new[] { "_Cutoff", "alphaCutoff", "_AlphaCutoff" })
                    if (src.HasProperty(p))
                    {
                        cut = src.GetFloat(p);
                        break;
                    }
                m.SetFloat("_Cutoff", cut > 0f ? cut : 0.5f);
            }
            string path = genDir + "/" + LegaiaWorldBuilder.Sanitize(m.name)
                + "_" + idx++ + ".mat";
            AssetDatabase.DeleteAsset(path);
            AssetDatabase.CreateAsset(m, path);
            cache[src] = m;
            return m;
        }

        static Texture ExtractMainTexture(Material m)
        {
            if (m == null)
                return null;
            if (m.mainTexture != null)
                return m.mainTexture;
            foreach (var name in m.GetTexturePropertyNames())
            {
                var t = m.GetTexture(name);
                if (t != null)
                    return t;
            }
            return null;
        }

        static void ApplySun(GameObject root, LegaiaRealismOptions o,
            GameObject nightLamps, GameObject nightTorches)
        {
            var existing = root.transform.Find("LegaiaSun");
            GameObject go = existing != null ? existing.gameObject : new GameObject("LegaiaSun");
            go.transform.SetParent(root.transform, false);
            var sun = go.GetComponent<Light>();
            if (sun == null)
                sun = go.AddComponent<Light>();
            sun.type = LightType.Directional;
            sun.color = new Color(1f, 0.956f, 0.878f);
            sun.intensity = o.sunIntensity;
            sun.shadows = LightShadows.Soft;
            sun.shadowStrength = o.shadowStrength;
            // .rotation is world-space and ignores the root's mirror scale,
            // so the light direction is exactly what the sliders say.
            go.transform.rotation = Quaternion.Euler(o.sunElevation, o.sunAzimuth, 0f);
            RenderSettings.sun = sun;
            RenderSettings.ambientMode = UnityEngine.Rendering.AmbientMode.Trilight;
            RenderSettings.ambientSkyColor = new Color(0.52f, 0.60f, 0.72f);
            RenderSettings.ambientEquatorColor = new Color(0.40f, 0.42f, 0.45f);
            RenderSettings.ambientGroundColor = new Color(0.23f, 0.20f, 0.17f);

            if (o.dayNight)
            {
                // Re-runs must re-set the fields (the night_lamps container
                // is rebuilt fresh each pass, so a wired-once reference goes
                // stale) - get the existing proxy instead of skipping.
                var dnType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDayNight");
                Component udon = dnType != null ? go.GetComponent(dnType) : null;
                if (udon == null)
                    udon = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaDayNight");
                LegaiaWorldBuilder.SetUdonField(udon, "sun", sun);
                LegaiaWorldBuilder.SetUdonField(udon, "cycleMinutes", o.dayNightMinutes);
                LegaiaWorldBuilder.SetUdonField(udon, "dayIntensity", o.sunIntensity);
                LegaiaWorldBuilder.SetUdonField(udon, "nightAmbientScale", o.nightAmbient);
                LegaiaWorldBuilder.SetUdonField(udon, "nightLights", nightLamps);
                LegaiaWorldBuilder.SetUdonField(udon, "nightTorches", nightTorches);
                // Ambience crossfade: the beds AddAmbience built (if any).
                var amb = root.transform.Find("ambience");
                var dayBed = amb != null ? amb.Find("ambience_day") : null;
                var nightBed = amb != null ? amb.Find("ambience_night") : null;
                LegaiaWorldBuilder.SetUdonField(udon, "dayAmbience",
                    dayBed != null ? dayBed.GetComponent<AudioSource>() : null);
                LegaiaWorldBuilder.SetUdonField(udon, "nightAmbience",
                    nightBed != null ? nightBed.GetComponent<AudioSource>() : null);
                LegaiaWorldBuilder.SyncUdonProxy(udon);

                // The settings panel's Day / Night buttons drive this cycle -
                // wire its dayNight reference (the panel lives in the
                // builder's top-level camp container, if built).
                var menuGo = GameObject.Find("LegaiaMenu");
                if (menuGo != null)
                {
                    var menuType =
                        LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaWorldMenu");
                    var menu = menuType != null ? menuGo.GetComponent(menuType) : null;
                    if (menu != null)
                    {
                        LegaiaWorldBuilder.SetUdonField(menu, "dayNight", udon);
                        LegaiaWorldBuilder.SyncUdonProxy(menu);
                    }
                }
            }
        }

        // --- Night lamps ----------------------------------------------------

        /// A warm light at each village building window, held in one
        /// "night_lamps" container that starts INACTIVE - the day/night
        /// behaviour enables it only while the sun is below the horizon.
        ///
        /// Window positions come from the WORLD MESH itself: the retail
        /// scene authors semi-transparent glow volumes right where light
        /// visibly spills out of a hut window (town01 repeats one
        /// identically-sized glow object on three separate huts), so every
        /// village-side BLEND submesh of window-glow proportions marks "a
        /// lit window of a building" exactly - the night light goes at the
        /// shaft's top-vertex centroid, in the window opening, shining out.
        /// No visible bulb mesh: the light pool on the wall is the effect.
        /// Water sheets and room-side glows are filtered out by shape and by
        /// the interior-room distance. Scenes with no glow volumes fall back to a
        /// small lamp above each village-side doorway (from the manifest's
        /// teleport endpoints).
        ///
        /// Returns null (and removes any stale container) when night lamps
        /// or the day/night cycle are off - without the cycle nothing would
        /// ever switch the lamps on.
        static GameObject BuildNightLamps(GameObject root, object manifest,
            string genDir, LegaiaRealismOptions o)
        {
            var old = root.transform.Find("night_lamps");
            if (old != null)
                Object.DestroyImmediate(old.gameObject);
            if (!o.dayNight || !o.nightLamps || manifest == null)
                return null;
            Vector3 spawnW = root.transform.TransformPoint(LegaiaWorldBuilder.G2U(
                MiniJson.GetVec3(MiniJson.Get(manifest, "spawn"), "position")));
            float DistXZ(Vector3 a, Vector3 b)
            {
                a.y = b.y = 0;
                return (a - b).magnitude;
            }
            var pts = new List<Vector3>();
            void Consider(Vector3 w, float dedupe)
            {
                if (DistXZ(w, spawnW) > o.interiorRoomDistance)
                    return; // room side - interiors have the shell glow
                foreach (var q in pts)
                    if (DistXZ(q, w) < dedupe)
                        return;
                pts.Add(w);
            }

            // Primary: authored window-glow volumes in the world mesh.
            var world = root.transform.Find("world");
            if (world != null)
                foreach (var mf in world.GetComponentsInChildren<MeshFilter>())
                {
                    if (mf.sharedMesh == null)
                        continue;
                    var r = mf.GetComponent<Renderer>();
                    var mats = r != null ? r.sharedMaterials : null;
                    for (int sm = 0; sm < mf.sharedMesh.subMeshCount; sm++)
                    {
                        Material mat = mats != null && sm < mats.Length
                            ? mats[sm] : null;
                        if (mat == null || mat.renderQueue <
                            (int)UnityEngine.Rendering.RenderQueue.Transparent)
                            continue;
                        var verts = mf.sharedMesh.vertices;
                        var tris = mf.sharedMesh.GetTriangles(sm);
                        if (tris.Length == 0)
                            continue;
                        Matrix4x4 toWorld = mf.transform.localToWorldMatrix;
                        var wpts = new List<Vector3>(tris.Length);
                        foreach (int ti in tris)
                            wpts.Add(toWorld.MultiplyPoint3x4(verts[ti]));
                        Bounds b = new Bounds(wpts[0], Vector3.zero);
                        for (int i = 1; i < wpts.Count; i++)
                            b.Encapsulate(wpts[i]);
                        // Window-glow proportions: a couple of meters tall,
                        // not map-spanning (water sheets are flat and wide).
                        if (b.size.y < 0.5f || b.size.y > 4.5f ||
                            Mathf.Max(b.size.x, b.size.z) > 8f)
                            continue;
                        // The glow volume is the light SHAFT spilling down and
                        // out of the window - its centroid hangs in mid-air
                        // off the wall. The shaft's own geometry says where
                        // the window is: its TOP band of vertices sits in the
                        // window opening, its bottom band where the light
                        // pools on the ground. Park the lamp at the top-band
                        // centroid, nudged along the spill direction so it
                        // sits just outside the opening. (Raycast wall-snap
                        // was tried and grabbed unrelated nearby walls - the
                        // palisade - so the anchor is purely geometric now.)
                        float topY = b.max.y - b.size.y * 0.3f;
                        float botY = b.min.y + b.size.y * 0.3f;
                        Vector3 win = Vector3.zero, foot = Vector3.zero;
                        int wc = 0, fc = 0;
                        foreach (var wpt in wpts)
                        {
                            if (wpt.y >= topY)
                            {
                                win += wpt;
                                wc++;
                            }
                            if (wpt.y <= botY)
                            {
                                foot += wpt;
                                fc++;
                            }
                        }
                        if (wc == 0)
                            continue;
                        win /= wc;
                        Vector3 spill = fc > 0 ? foot / fc - win : Vector3.zero;
                        spill.y = 0f;
                        if (spill.sqrMagnitude > 1e-4f)
                            win += spill.normalized * 0.25f;
                        Consider(win, 2f);
                    }
                }
            bool fromGlows = pts.Count > 0;

            // Fallback: a lamp above each village-side doorway.
            if (!fromGlows)
            {
                var tps = MiniJson.AsList(MiniJson.Get(manifest, "teleports"))
                          ?? new List<object>();
                foreach (object tp in tps)
                {
                    Consider(root.transform.TransformPoint(LegaiaWorldBuilder.G2U(
                        MiniJson.GetVec3(MiniJson.Get(tp, "trigger"), "position")))
                        + Vector3.up * 2.2f, 2.5f);
                    Consider(root.transform.TransformPoint(LegaiaWorldBuilder.G2U(
                        MiniJson.GetVec3(MiniJson.Get(tp, "destination"), "position")))
                        + Vector3.up * 2.2f, 2.5f);
                }
            }
            if (pts.Count == 0)
                return null;

            // Light only - no visible bulb mesh. A glowing orb floating by
            // the window reads as an artifact; the warm pool of light on the
            // wall and ground is the whole effect. (Stale orb material from
            // earlier kit versions is cleaned up here.)
            AssetDatabase.DeleteAsset(genDir + "/lamp_glow.mat");

            var container = new GameObject("night_lamps");
            container.transform.SetParent(root.transform, false);
            for (int i = 0; i < pts.Count; i++)
            {
                var go = new GameObject("lamp_" + i);
                go.transform.SetParent(container.transform, false);
                // World-space placement (the root carries the mirror).
                go.transform.position = pts[i];
                var light = go.AddComponent<Light>();
                light.type = LightType.Point;
                // A tight pool by the window, not a street light: the
                // window glow should read local against the dark ambient.
                light.range = 3f;
                light.intensity = 1.2f;
                light.color = new Color(1f, 0.75f, 0.45f);
                // Several per village: keep them cheap.
                light.shadows = LightShadows.None;
            }
            container.SetActive(false); // day/night behaviour turns these on
            Debug.Log("[Legaia] " + pts.Count + " night light(s) placed at " +
                      (fromGlows ? "authored window glows." : "village doorways."));
            return container;
        }

        // --- Night torches --------------------------------------------------

        /// A planted stake torch by each tree and each village doorway,
        /// burning only at night. The container is TOP-LEVEL, outside the
        /// mirrored root (particles and audio under a negative scale
        /// misbehave - same reason the camp props live outside), starts
        /// inactive, and LegaiaDayNight enables it while the sun is down.
        ///
        /// Houses come from the manifest's village-side doorway-teleport
        /// triggers: one stake flanking each door. Trees come from the
        /// world mesh itself - green-reading upward triangles well ABOVE
        /// the local ground (a canopy), grid-clustered in XZ, one stake at
        /// each cluster's trunk. Same texel x COLOR_0 green test and
        /// per-cell floor grid as the grass pass, inverted: grass keeps
        /// ground green, this keeps floating green.
        static GameObject BuildNightTorches(GameObject root, object manifest,
            string campDir, LegaiaRealismOptions o)
        {
            const string NAME = "Legaia_night_torches";
            var old = GameObject.Find(NAME);
            if (old != null)
                Object.DestroyImmediate(old);
            if (!o.dayNight || !o.nightTorches || manifest == null)
                return null;

            Vector3 spawnW = root.transform.TransformPoint(LegaiaWorldBuilder.G2U(
                MiniJson.GetVec3(MiniJson.Get(manifest, "spawn"), "position")));
            float DistXZ(Vector3 a, Vector3 b)
            {
                a.y = b.y = 0;
                return (a - b).magnitude;
            }
            var pts = new List<Vector3>();
            void Consider(Vector3 w, float dedupe)
            {
                if (pts.Count >= 24)
                    return; // keep the dynamic-light budget sane
                if (DistXZ(w, spawnW) > o.interiorRoomDistance)
                    return; // room side - interiors have the shell glow
                foreach (var q in pts)
                    if (DistXZ(q, w) < dedupe)
                        return;
                pts.Add(w);
            }

            // Houses: one torch flanking each village-side doorway. The
            // trigger sits right at the wall, so step a little toward the
            // village and to the side - beside the door, never in it.
            var tps = MiniJson.AsList(MiniJson.Get(manifest, "teleports"))
                      ?? new List<object>();
            foreach (object tp in tps)
            {
                Vector3 w = root.transform.TransformPoint(LegaiaWorldBuilder.G2U(
                    MiniJson.GetVec3(MiniJson.Get(tp, "trigger"), "position")));
                if (DistXZ(w, spawnW) > o.interiorRoomDistance)
                    continue;
                Vector3 d = spawnW - w;
                d.y = 0f;
                d = d.sqrMagnitude > 1e-4f ? d.normalized : Vector3.forward;
                var lateral = new Vector3(d.z, 0f, -d.x);
                Consider(LegaiaCampProps.Ground(
                    w + d * 0.35f + lateral * 0.7f), 2.5f);
            }
            int houseTorches = pts.Count;

            foreach (var t in TreeTrunkPoints(root, spawnW, o))
                Consider(t, 2.5f);

            if (pts.Count == 0)
                return null;

            // Shared camp-prop assets (the camp dir, not realism/ - the
            // pickup torches use the same ones, so they exist only once).
            Directory.CreateDirectory(campDir);
            var fireClip = LegaiaAudioGen.EnsureClip(
                campDir + "/fire_crackle.wav", LegaiaAudioGen.FireCrackle);
            var wood = LegaiaCampProps.EnsureMat(campDir, "camp_wood",
                "Standard", new Color(0.36f, 0.24f, 0.13f));
            var dark = LegaiaCampProps.EnsureMat(campDir, "camp_dark",
                "Standard", new Color(0.16f, 0.14f, 0.12f));
            var flameMat = LegaiaCampProps.EnsureFlameMaterial(campDir);
            var smokeMat = LegaiaCampProps.EnsureSmokeMaterial(campDir);

            var container = new GameObject(NAME);
            for (int i = 0; i < pts.Count; i++)
                LegaiaCampProps.BuildNightTorch(container.transform, pts[i],
                    fireClip, wood, dark, flameMat, smokeMat, i);
            container.SetActive(false); // day/night behaviour turns these on
            Debug.Log("[Legaia] " + pts.Count + " night torch(es): " +
                houseTorches + " by doorways, " + (pts.Count - houseTorches) +
                " by trees.");
            return container;
        }

        /// One trunk-side point per tree-sized canopy: green upward
        /// triangles more than 1.5 m above their 1.5 m-cell's lowest upward
        /// surface, bucketed into 2 m XZ cells, 8-neighbour-merged into
        /// clusters. A cluster with real canopy area drops a ray from just
        /// under its lowest leaf (nudged toward the village so the stake
        /// stands beside the trunk, not inside it) to find the ground.
        static List<Vector3> TreeTrunkPoints(GameObject root, Vector3 spawnW,
            LegaiaRealismOptions o)
        {
            var result = new List<Vector3>();
            var world = root.transform.Find("world");
            if (world == null)
                return result;
            var texCache = new Dictionary<Texture, Texture2D>();
            var groundMinY = new Dictionary<Vector2Int, float>();
            var canopy = new List<(Vector3 cen, float area)>();
            Vector2Int CellOf(Vector3 p) => new Vector2Int(
                Mathf.FloorToInt(p.x / 1.5f), Mathf.FloorToInt(p.z / 1.5f));

            foreach (var r in world.GetComponentsInChildren<Renderer>(false))
            {
                Mesh mesh = MeshOf(r);
                if (mesh == null)
                    continue;
                var mats = r.sharedMaterials;
                var verts = mesh.vertices;
                var uvs = mesh.uv;
                var cols = mesh.colors;
                Matrix4x4 toWorld = r.transform.localToWorldMatrix;
                for (int sm = 0; sm < mesh.subMeshCount; sm++)
                {
                    Texture2D tex = null;
                    if (sm < mats.Length && mats[sm] != null)
                        tex = ReadableCopy(ExtractMainTexture(mats[sm]), texCache);
                    var tris = mesh.GetTriangles(sm);
                    for (int i = 0; i < tris.Length; i += 3)
                    {
                        int t0 = tris[i], t1 = tris[i + 1], t2 = tris[i + 2];
                        Vector3 a = toWorld.MultiplyPoint3x4(verts[t0]);
                        Vector3 b = toWorld.MultiplyPoint3x4(verts[t1]);
                        Vector3 d = toWorld.MultiplyPoint3x4(verts[t2]);
                        Vector3 cr = Vector3.Cross(b - a, d - a);
                        float area = cr.magnitude * 0.5f;
                        if (area < 1e-4f)
                            continue;
                        if (Mathf.Abs(cr.y / (area * 2f)) < 0.65f)
                            continue;
                        Vector3 cen = (a + b + d) / 3f;
                        var cell = CellOf(cen);
                        if (!groundMinY.TryGetValue(cell, out float fY) ||
                            cen.y < fY)
                            groundMinY[cell] = cen.y;
                        Color texel = Color.white;
                        if (tex != null && uvs.Length > 0)
                        {
                            Vector2 uv = (uvs[t0] + uvs[t1] + uvs[t2]) / 3f;
                            texel = tex.GetPixelBilinear(
                                uv.x - Mathf.Floor(uv.x), uv.y - Mathf.Floor(uv.y));
                        }
                        Color vcol = Color.white;
                        if (cols.Length > 0)
                            vcol = (cols[t0] + cols[t1] + cols[t2]) / 3f;
                        Color ground = texel * vcol;
                        if (ground.g - Mathf.Max(ground.r, ground.b) <
                            o.grassGreenThreshold)
                            continue;
                        canopy.Add((cen, area));
                    }
                }
            }

            // Elevated green only (the canopy), bucketed into 2 m XZ cells.
            var cells =
                new Dictionary<Vector2Int, (Vector3 wsum, float area, float minY)>();
            foreach (var (cen, area) in canopy)
            {
                if (!groundMinY.TryGetValue(CellOf(cen), out float fY) ||
                    cen.y <= fY + 1.5f)
                    continue;
                var k = new Vector2Int(
                    Mathf.FloorToInt(cen.x / 2f), Mathf.FloorToInt(cen.z / 2f));
                if (cells.TryGetValue(k, out var agg))
                    cells[k] = (agg.wsum + cen * area, agg.area + area,
                                Mathf.Min(agg.minY, cen.y));
                else
                    cells[k] = (cen * area, area, cen.y);
            }

            // Merge occupied 8-neighbour cells into per-tree clusters.
            var keys = new List<Vector2Int>(cells.Keys);
            var parent = new int[keys.Count];
            var index = new Dictionary<Vector2Int, int>();
            for (int i = 0; i < keys.Count; i++)
            {
                parent[i] = i;
                index[keys[i]] = i;
            }
            int Find(int i) => parent[i] == i ? i : parent[i] = Find(parent[i]);
            for (int i = 0; i < keys.Count; i++)
                for (int dx = -1; dx <= 1; dx++)
                    for (int dz = -1; dz <= 1; dz++)
                        if (index.TryGetValue(
                                new Vector2Int(keys[i].x + dx, keys[i].y + dz),
                                out int j))
                            parent[Find(j)] = Find(i);
            var clusters =
                new Dictionary<int, (Vector3 wsum, float area, float minY)>();
            for (int i = 0; i < keys.Count; i++)
            {
                int rt = Find(i);
                var c = cells[keys[i]];
                if (clusters.TryGetValue(rt, out var agg))
                    clusters[rt] = (agg.wsum + c.wsum, agg.area + c.area,
                                    Mathf.Min(agg.minY, c.minY));
                else
                    clusters[rt] = c;
            }

            foreach (var c in clusters.Values)
            {
                if (c.area < 2.5f)
                    continue; // a stray green scrap, not a tree
                Vector3 cen = c.wsum / c.area;
                Vector3 toSpawn = spawnW - cen;
                toSpawn.y = 0f;
                toSpawn = toSpawn.sqrMagnitude > 1e-4f
                    ? toSpawn.normalized : Vector3.forward;
                // Start just under the lowest leaf so the ray can't land on
                // the canopy itself.
                Vector3 start = new Vector3(cen.x, c.minY - 0.35f, cen.z)
                    + toSpawn * 0.6f;
                if (Physics.Raycast(start, Vector3.down, out RaycastHit hit,
                        30f, ~0, QueryTriggerInteraction.Ignore))
                    result.Add(hit.point);
            }
            return result;
        }

        // --- Sky + fog ------------------------------------------------------

        static void ApplySkyAndFog(GameObject root, string genDir)
        {
            var shader = Shader.Find("Skybox/Procedural");
            if (shader != null)
            {
                string path = genDir + "/skybox.mat";
                var sky = AssetDatabase.LoadAssetAtPath<Material>(path);
                if (sky == null)
                {
                    sky = new Material(shader);
                    sky.SetFloat("_SunSize", 0.045f);
                    sky.SetFloat("_Exposure", 1.15f);
                    AssetDatabase.CreateAsset(sky, path);
                }
                RenderSettings.skybox = sky;
            }
            var rs = root.GetComponentsInChildren<Renderer>(true);
            if (rs.Length > 0)
            {
                Bounds b = rs[0].bounds;
                foreach (var r in rs)
                    b.Encapsulate(r.bounds);
                float d = b.size.magnitude;
                RenderSettings.fog = true;
                RenderSettings.fogMode = FogMode.Linear;
                RenderSettings.fogStartDistance = d * 0.35f;
                RenderSettings.fogEndDistance = d * 1.25f;
                RenderSettings.fogColor = new Color(0.63f, 0.68f, 0.76f);
            }
        }

        // --- Texture smoothing ----------------------------------------------

        static void SmoothTextures(GameObject root)
        {
            var seen = new HashSet<Texture>();
            foreach (var r in root.GetComponentsInChildren<Renderer>(true))
                foreach (var mat in r.sharedMaterials)
                {
                    if (mat == null)
                        continue;
                    foreach (var name in mat.GetTexturePropertyNames())
                    {
                        var tex = mat.GetTexture(name);
                        if (tex == null || !seen.Add(tex))
                            continue;
                        tex.filterMode = FilterMode.Bilinear;
                        tex.anisoLevel = 4;
                    }
                }
            Debug.Log("[Legaia] " + seen.Count + " texture(s) set to bilinear " +
                      "(a glb reimport resets this - rerun the pass after one).");
        }

        // --- Foliage --------------------------------------------------------

        const int MAX_TUFTS = 25000;
        const int CHUNK_VERTS = 30000;

        static void ScatterGrass(GameObject root, string genDir, LegaiaRealismOptions o)
        {
            var world = root.transform.Find("world");
            if (world == null)
            {
                Debug.LogWarning("[Legaia] no 'world' child under " + root.name +
                                 " - foliage skipped.");
                return;
            }
            var grassShader = Shader.Find("Legaia/Grass Wind");
            if (grassShader == null)
            {
                Debug.LogError("[Legaia] Legaia/Grass Wind shader not found - is " +
                    "Assets/LegaiaWorld/Shaders/ imported? Foliage skipped.");
                return;
            }

            // Fresh container each run (rescatter, don't stack).
            var old = root.transform.Find("foliage");
            if (old != null)
                Object.DestroyImmediate(old.gameObject);
            var container = new GameObject("foliage");
            container.transform.SetParent(root.transform, false);
            // Bake blade vertices in container-local space: the world-space
            // sample points go through worldToLocal, which reapplies the
            // root's mirror exactly once (winding may flip - the grass
            // shader culls off).
            Matrix4x4 toLocal = container.transform.worldToLocalMatrix;

            var rng = new System.Random(o.grassSeed);
            var texCache = new Dictionary<Texture, Texture2D>();
            var v = new List<Vector3>();
            var c = new List<Color>();
            var t = new List<int>();
            var chunks = new List<Mesh>();
            int tufts = 0;

            // Grass grows on the GROUND: an upward-facing green triangle
            // floating above other geometry (a tree canopy, a roof) must
            // not scatter. Pass 1 records, per 1.5 m XZ cell, the lowest
            // upward-facing surface height while collecting green
            // candidates; pass 2 emits only candidates sitting within a
            // step of their cell's floor - the ground under a tree is
            // always lower than the canopy above it.
            var groundMinY = new Dictionary<Vector2Int, float>();
            var cand = new List<(Vector3 a, Vector3 b, Vector3 d,
                Color ground, float weight, float area)>();
            Vector2Int CellOf(Vector3 p) => new Vector2Int(
                Mathf.FloorToInt(p.x / 1.5f), Mathf.FloorToInt(p.z / 1.5f));

            foreach (var r in world.GetComponentsInChildren<Renderer>(false))
            {
                Mesh mesh = MeshOf(r);
                if (mesh == null)
                    continue;
                var mats = r.sharedMaterials;
                var verts = mesh.vertices;
                var uvs = mesh.uv;
                var cols = mesh.colors;
                Matrix4x4 toWorld = r.transform.localToWorldMatrix;
                for (int sm = 0; sm < mesh.subMeshCount; sm++)
                {
                    Texture2D tex = null;
                    if (sm < mats.Length && mats[sm] != null)
                        tex = ReadableCopy(ExtractMainTexture(mats[sm]), texCache);
                    var tris = mesh.GetTriangles(sm);
                    for (int i = 0; i < tris.Length; i += 3)
                    {
                        int t0 = tris[i], t1 = tris[i + 1], t2 = tris[i + 2];
                        Vector3 a = toWorld.MultiplyPoint3x4(verts[t0]);
                        Vector3 b = toWorld.MultiplyPoint3x4(verts[t1]);
                        Vector3 d = toWorld.MultiplyPoint3x4(verts[t2]);
                        Vector3 cr = Vector3.Cross(b - a, d - a);
                        float area = cr.magnitude * 0.5f;
                        if (area < 1e-4f)
                            continue;
                        // Upward-facing only; Abs because the mixed source
                        // winding makes the sign meaningless.
                        if (Mathf.Abs(cr.y / (area * 2f)) < 0.65f)
                            continue;
                        Vector3 cen = (a + b + d) / 3f;
                        var cell = CellOf(cen);
                        if (!groundMinY.TryGetValue(cell, out float floorY) ||
                            cen.y < floorY)
                            groundMinY[cell] = cen.y;

                        // Ground colour at the triangle centre = texel x mean
                        // COLOR_0, the product the retail shading displays.
                        Color texel = Color.white;
                        if (tex != null && uvs.Length > 0)
                        {
                            Vector2 uv = (uvs[t0] + uvs[t1] + uvs[t2]) / 3f;
                            texel = tex.GetPixelBilinear(
                                uv.x - Mathf.Floor(uv.x), uv.y - Mathf.Floor(uv.y));
                        }
                        Color vcol = Color.white;
                        if (cols.Length > 0)
                            vcol = (cols[t0] + cols[t1] + cols[t2]) / 3f;
                        Color ground = texel * vcol;
                        float green = ground.g - Mathf.Max(ground.r, ground.b);
                        if (green < o.grassGreenThreshold)
                            continue;
                        float weight = Mathf.Clamp01(green / 0.12f);
                        cand.Add((a, b, d, ground, weight, area));
                    }
                }
            }
            // Pass 2: emit only candidates on their cell's floor - a green
            // canopy or roof sits well above the lowest upward surface at
            // its spot and is skipped.
            foreach (var (a, bb, d, ground, weight, area) in cand)
            {
                Vector3 cen = (a + bb + d) / 3f;
                if (groundMinY.TryGetValue(CellOf(cen), out float floorY) &&
                    cen.y > floorY + 0.75f)
                    continue;
                float expected = area * o.grassDensity * weight;
                int count = (int)expected +
                    (rng.NextDouble() < expected - (int)expected ? 1 : 0);
                for (int k = 0; k < count && tufts < MAX_TUFTS; k++)
                {
                    float u = (float)rng.NextDouble();
                    float w = (float)rng.NextDouble();
                    if (u + w > 1f)
                    {
                        u = 1f - u;
                        w = 1f - w;
                    }
                    Vector3 p = a + (bb - a) * u + (d - a) * w;
                    EmitTuft(p, ground, rng, toLocal, v, c, t);
                    tufts++;
                    if (v.Count > CHUNK_VERTS)
                        FlushGrassChunk(v, c, t, chunks, genDir);
                }
            }

            FlushGrassChunk(v, c, t, chunks, genDir);
            // Drop stale chunks a denser previous run left behind.
            for (int i = chunks.Count; ; i++)
                if (!AssetDatabase.DeleteAsset(genDir + "/grass_" + i + ".asset"))
                    break;

            if (tufts == 0)
            {
                Debug.LogWarning("[Legaia] foliage found no green-reading ground - " +
                    "lower the green threshold if this scene should have grass.");
                Object.DestroyImmediate(container);
                return;
            }
            if (tufts >= MAX_TUFTS)
                Debug.LogWarning("[Legaia] foliage capped at " + MAX_TUFTS +
                    " tufts - lower the density for full coverage.");

            string matPath = genDir + "/grass.mat";
            var grassMat = AssetDatabase.LoadAssetAtPath<Material>(matPath);
            if (grassMat == null)
            {
                grassMat = new Material(grassShader);
                AssetDatabase.CreateAsset(grassMat, matPath);
            }
            foreach (var chunk in chunks)
            {
                var go = new GameObject(chunk.name);
                go.transform.SetParent(container.transform, false);
                go.AddComponent<MeshFilter>().sharedMesh = chunk;
                var mr = go.AddComponent<MeshRenderer>();
                mr.sharedMaterial = grassMat;
                // Blade shadows alias badly at this size and cost a draw pass.
                mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            }
            Debug.Log("[Legaia] scattered " + tufts + " grass tuft(s) across " +
                      chunks.Count + " chunk(s).");
        }

        static Mesh MeshOf(Renderer r)
        {
            if (r is SkinnedMeshRenderer smr)
                return smr.sharedMesh;
            var mf = r.GetComponent<MeshFilter>();
            return mf != null ? mf.sharedMesh : null;
        }

        /// One tuft = 4..7 single-triangle blades around `p`: root vertices
        /// carry a darkened ground tint with sway weight 0, the tip a
        /// brightened green-shifted tint with sway weight 1 (the grass
        /// shader reads the weight from alpha).
        static void EmitTuft(Vector3 p, Color ground, System.Random rng,
            Matrix4x4 toLocal, List<Vector3> v, List<Color> c, List<int> t)
        {
            int blades = 4 + rng.Next(4);
            var rootCol = new Color(ground.r * 0.45f, ground.g * 0.5f, ground.b * 0.4f, 0f);
            for (int b = 0; b < blades; b++)
            {
                float yaw = (float)(rng.NextDouble() * Mathf.PI * 2.0);
                float h = 0.10f + (float)rng.NextDouble() * 0.22f;
                float halfW = 0.008f + (float)rng.NextDouble() * 0.014f;
                float jr = (float)rng.NextDouble() * 0.10f;
                float ja = (float)(rng.NextDouble() * Mathf.PI * 2.0);
                Vector3 basePos = p + new Vector3(Mathf.Cos(ja) * jr, 0f, Mathf.Sin(ja) * jr);
                Vector3 side = new Vector3(Mathf.Cos(yaw), 0f, Mathf.Sin(yaw)) * halfW;
                float la = (float)(rng.NextDouble() * Mathf.PI * 2.0);
                float lean = (float)rng.NextDouble() * 0.35f * h;
                Vector3 tip = basePos + Vector3.up * h
                    + new Vector3(Mathf.Cos(la) * lean, 0f, Mathf.Sin(la) * lean);
                float bright = 1.1f + (float)rng.NextDouble() * 0.45f;
                var tipCol = new Color(
                    Mathf.Clamp01(ground.r * bright * 0.9f),
                    Mathf.Clamp01(ground.g * bright * 1.15f),
                    Mathf.Clamp01(ground.b * bright * 0.75f), 1f);
                int i0 = v.Count;
                v.Add(toLocal.MultiplyPoint3x4(basePos - side));
                v.Add(toLocal.MultiplyPoint3x4(basePos + side));
                v.Add(toLocal.MultiplyPoint3x4(tip));
                c.Add(rootCol);
                c.Add(rootCol);
                c.Add(tipCol);
                t.Add(i0);
                t.Add(i0 + 1);
                t.Add(i0 + 2);
            }
        }

        static void FlushGrassChunk(List<Vector3> v, List<Color> c, List<int> t,
            List<Mesh> chunks, string genDir)
        {
            if (t.Count == 0)
                return;
            var m = new Mesh { name = "grass_" + chunks.Count };
            m.SetVertices(v);
            m.SetColors(c);
            m.SetTriangles(t, 0);
            var normals = new Vector3[v.Count];
            for (int i = 0; i < normals.Length; i++)
                normals[i] = Vector3.up; // lit like the ground it stands on
            m.normals = normals;
            m.RecalculateBounds();
            string path = genDir + "/" + m.name + ".asset";
            AssetDatabase.DeleteAsset(path);
            AssetDatabase.CreateAsset(m, path);
            chunks.Add(m);
            v.Clear();
            c.Clear();
            t.Clear();
        }

        /// A small readable copy of any texture via RenderTexture blit
        /// (imported glb textures are not CPU-readable). sRGB target so the
        /// greenness test sees display-space values.
        static Texture2D ReadableCopy(Texture src, Dictionary<Texture, Texture2D> cache)
        {
            if (src == null)
                return null;
            if (cache.TryGetValue(src, out var got))
                return got;
            int w = Mathf.Min(src.width, 256);
            int h = Mathf.Min(src.height, 256);
            var rt = RenderTexture.GetTemporary(w, h, 0,
                RenderTextureFormat.ARGB32, RenderTextureReadWrite.sRGB);
            var prev = RenderTexture.active;
            Graphics.Blit(src, rt);
            RenderTexture.active = rt;
            var copy = new Texture2D(w, h, TextureFormat.RGBA32, false);
            copy.ReadPixels(new Rect(0, 0, w, h), 0, 0);
            copy.Apply();
            RenderTexture.active = prev;
            RenderTexture.ReleaseTemporary(rt);
            cache[src] = copy;
            return copy;
        }

        // --- Interior shells ------------------------------------------------

        /// Wrap each detached interior room in a black inward-facing dome
        /// (plus optional window-light dressing). Room detection rides the
        /// manifest's own door data: interiors are doorway-teleport
        /// endpoints parked far off the playable village (on town01 every
        /// village-side endpoint sits within ~52m of the spawn and every
        /// room-side one beyond ~86m - a wide gap the distance threshold
        /// splits), clustered into per-room groups, then grown to the
        /// nearby world geometry so the dome clears the whole room.
        static void BuildInteriorShells(GameObject root, object manifest,
            string genDir, LegaiaRealismOptions o)
        {
            // Fresh container each run (re-shell, don't stack).
            var old = root.transform.Find("interiors");
            if (old != null)
                Object.DestroyImmediate(old.gameObject);

            var shellShader = Shader.Find("Legaia/Interior Shell");
            if (shellShader == null)
            {
                Debug.LogError("[Legaia] Legaia/Interior Shell shader not found - is " +
                    "Assets/LegaiaWorld/Shaders/ imported? Interior shells skipped.");
                return;
            }

            var tps = MiniJson.AsList(MiniJson.Get(manifest, "teleports"));
            if (tps == null || tps.Count == 0)
            {
                Debug.Log("[Legaia] no doorway teleports in the manifest - " +
                          "interior shells skipped.");
                return;
            }
            Vector3 spawnW = root.transform.TransformPoint(LegaiaWorldBuilder.G2U(
                MiniJson.GetVec3(MiniJson.Get(manifest, "spawn"), "position")));
            var pts = new List<Vector3>();
            foreach (object tp in tps)
            {
                pts.Add(root.transform.TransformPoint(LegaiaWorldBuilder.G2U(
                    MiniJson.GetVec3(MiniJson.Get(tp, "trigger"), "position"))));
                pts.Add(root.transform.TransformPoint(LegaiaWorldBuilder.G2U(
                    MiniJson.GetVec3(MiniJson.Get(tp, "destination"), "position"))));
            }
            float DistXZ(Vector3 a, Vector3 b)
            {
                a.y = b.y = 0;
                return (a - b).magnitude;
            }
            var roomPts = new List<Vector3>();
            foreach (var p in pts)
                if (DistXZ(p, spawnW) > o.interiorRoomDistance)
                    roomPts.Add(p);
            if (roomPts.Count == 0)
            {
                Debug.Log("[Legaia] no teleport endpoint sits beyond the room " +
                    "distance - no detached interiors detected, shells skipped.");
                return;
            }

            // Union-find clustering into per-room groups. Rooms are compact
            // (their doorway endpoints span a few meters) and distinct rooms
            // sit well apart, so a fixed linkage separates them cleanly.
            const float LINK = 10f;
            var parent = new int[roomPts.Count];
            for (int i = 0; i < parent.Length; i++)
                parent[i] = i;
            int Find(int i) => parent[i] == i ? i : parent[i] = Find(parent[i]);
            for (int i = 0; i < roomPts.Count; i++)
                for (int j = i + 1; j < roomPts.Count; j++)
                    if (DistXZ(roomPts[i], roomPts[j]) <= LINK)
                        parent[Find(j)] = Find(i);
            var clusters = new Dictionary<int, List<Vector3>>();
            for (int i = 0; i < roomPts.Count; i++)
            {
                int r = Find(i);
                if (!clusters.TryGetValue(r, out var list))
                    clusters[r] = list = new List<Vector3>();
                list.Add(roomPts[i]);
            }

            var container = new GameObject("interiors");
            container.transform.SetParent(root.transform, false);
            Matrix4x4 toLocal = container.transform.worldToLocalMatrix;
            var world = root.transform.Find("world");

            // Candidate room geometry: every world mesh that is not
            // map-spanning (the ground heightfield's bounds would balloon a
            // shell to the whole world).
            var candidates = new List<Bounds>();
            if (world != null)
                foreach (var r in world.GetComponentsInChildren<Renderer>(true))
                {
                    Bounds rb = r.bounds;
                    if (rb.size.magnitude <= 30f)
                        candidates.Add(rb);
                }

            string shellMatPath = genDir + "/interior_shell.mat";
            var shellMat = AssetDatabase.LoadAssetAtPath<Material>(shellMatPath);
            if (shellMat == null)
            {
                shellMat = new Material(shellShader);
                AssetDatabase.CreateAsset(shellMat, shellMatPath);
            }

            int roomIdx = 0;
            foreach (var cl in clusters.Values)
            {
                // Endpoint bounds seed the room, then grow to the WHOLE
                // building: the doorway endpoints sit at the edge of a large
                // interior, so a single nearest-meshes pass leaves the dome
                // centred on the door and slicing through the far wall.
                // Flood-fill instead - repeatedly encapsulate any candidate
                // mesh whose XZ footprint overlaps the current region
                // (expanded by the margin) until nothing new joins, capped
                // by distance from the doorway seed so a chain of meshes
                // can't walk the region across the map.
                Bounds b = new Bounds(cl[0], Vector3.zero);
                foreach (var p in cl)
                    b.Encapsulate(p);
                Vector3 seed = b.center;
                var used = new bool[candidates.Count];
                var roomMeshes = new List<Bounds>();
                for (int pass = 0, grew = 1; grew == 1 && pass < 6; pass++)
                {
                    grew = 0;
                    Bounds reach = b;
                    reach.Expand(new Vector3(o.interiorShellMargin * 2f, 0f,
                                             o.interiorShellMargin * 2f));
                    for (int ci = 0; ci < candidates.Count; ci++)
                    {
                        if (used[ci])
                            continue;
                        Bounds rb = candidates[ci];
                        if (rb.min.x > reach.max.x || rb.max.x < reach.min.x ||
                            rb.min.z > reach.max.z || rb.max.z < reach.min.z)
                            continue;
                        if (DistXZ(rb.center, seed) > 25f)
                            continue;
                        b.Encapsulate(rb);
                        roomMeshes.Add(rb);
                        used[ci] = true;
                        grew = 1;
                    }
                }
                Bounds room = b; // pre-margin: where the dressing goes
                b.Expand(o.interiorShellMargin * 2f);
                // An ellipsoid fitted per-axis, not a circumscribing sphere:
                // the old radius was the expanded box's half-DIAGONAL and the
                // sphere reached that far in every direction, so a wide
                // room's dome bled into its neighbour. Start from the box
                // extents and inflate uniformly only as far as the room's
                // actual geometry demands - every member mesh's AABB corner
                // (and doorway endpoint) must stay inside the shell.
                Vector3 radii = b.extents + Vector3.one * 0.5f;
                float need = 1f;
                void Fit(Vector3 p)
                {
                    Vector3 d = p - b.center;
                    float nrm = Mathf.Sqrt(
                        d.x * d.x / (radii.x * radii.x) +
                        d.y * d.y / (radii.y * radii.y) +
                        d.z * d.z / (radii.z * radii.z));
                    if (nrm > need)
                        need = nrm;
                }
                foreach (var p in cl)
                    Fit(p);
                foreach (var rb in roomMeshes)
                    for (int cx = 0; cx < 8; cx++)
                        Fit(new Vector3(
                            (cx & 1) == 0 ? rb.min.x : rb.max.x,
                            (cx & 2) == 0 ? rb.min.y : rb.max.y,
                            (cx & 4) == 0 ? rb.min.z : rb.max.z));
                radii *= need * 1.05f; // small slack past the tightest corner

                var shell = ShellSphere(toLocal, b.center, radii,
                    "shell_" + roomIdx);
                string shellPath = genDir + "/" + shell.name + ".asset";
                AssetDatabase.DeleteAsset(shellPath);
                AssetDatabase.CreateAsset(shell, shellPath);
                var go = new GameObject("room_" + roomIdx + "_shell");
                go.transform.SetParent(container.transform, false);
                go.AddComponent<MeshFilter>().sharedMesh = shell;
                var mr = go.AddComponent<MeshRenderer>();
                mr.sharedMaterial = shellMat;
                // No shadows: the sun keeps lighting the room through the dome.
                mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;

                if (o.interiorGlow)
                {
                    var lgo = new GameObject("room_" + roomIdx + "_light");
                    lgo.transform.SetParent(container.transform, false);
                    lgo.transform.position = room.center + Vector3.up * 0.6f;
                    var light = lgo.AddComponent<Light>();
                    light.type = LightType.Point;
                    light.range = Mathf.Max(radii.x, Mathf.Max(radii.y, radii.z)) * 1.6f;
                    light.intensity = 0.7f;
                    light.color = new Color(1f, 0.92f, 0.78f);
                    light.shadows = LightShadows.None;
                }
                roomIdx++;
            }
            // Drop stale assets a previous run with more rooms left behind,
            // plus the light-shaft assets of the retired shaft dressing.
            for (int i = roomIdx; ; i++)
                if (!AssetDatabase.DeleteAsset(genDir + "/shell_" + i + ".asset"))
                    break;
            for (int i = 0; ; i++)
                if (!AssetDatabase.DeleteAsset(genDir + "/shaft_" + i + ".asset"))
                    break;
            AssetDatabase.DeleteAsset(genDir + "/light_shaft.mat");
            Debug.Log("[Legaia] wrapped " + roomIdx +
                      " interior room(s) in black shells.");
        }

        /// An inward-facing UV ellipsoid around `centerW` (world) with
        /// per-axis semi-axes `radii`, baked into container-local space.
        /// "Inward" must hold in WORLD space, and the built root usually
        /// carries a mirror that flips winding - so the orientation is
        /// settled empirically: sample one face's local normal, flip
        /// everything if the front points the wrong way for this parent
        /// chain.
        static Mesh ShellSphere(Matrix4x4 toLocal, Vector3 centerW, Vector3 radii,
            string name)
        {
            const int SEG = 24, RING = 14;
            var v = new List<Vector3>((SEG + 1) * (RING + 1));
            for (int y = 0; y <= RING; y++)
            {
                float phi = Mathf.PI * y / RING;
                for (int x = 0; x <= SEG; x++)
                {
                    float th = 2f * Mathf.PI * x / SEG;
                    v.Add(toLocal.MultiplyPoint3x4(centerW + Vector3.Scale(
                        new Vector3(
                            Mathf.Sin(phi) * Mathf.Cos(th),
                            Mathf.Cos(phi),
                            Mathf.Sin(phi) * Mathf.Sin(th)), radii)));
                }
            }
            var t = new List<int>(SEG * RING * 6);
            for (int y = 0; y < RING; y++)
                for (int x = 0; x < SEG; x++)
                {
                    int a = y * (SEG + 1) + x;
                    int b = a + SEG + 1;
                    t.Add(a);
                    t.Add(a + 1);
                    t.Add(b);
                    t.Add(a + 1);
                    t.Add(b + 1);
                    t.Add(b);
                }

            // Empirical inward check on a mid-mesh face: Unity's front-face
            // normal is cross(v1-v0, v2-v0); it must point at the centre.
            Vector3 centerL = toLocal.MultiplyPoint3x4(centerW);
            int mid = (t.Count / 6) * 3; // a non-degenerate equatorial tri
            Vector3 fn = Vector3.Cross(v[t[mid + 1]] - v[t[mid]],
                                       v[t[mid + 2]] - v[t[mid]]);
            Vector3 centroid = (v[t[mid]] + v[t[mid + 1]] + v[t[mid + 2]]) / 3f;
            if (Vector3.Dot(fn, centerL - centroid) < 0f)
                for (int i = 0; i < t.Count; i += 3)
                {
                    int tmp = t[i + 1];
                    t[i + 1] = t[i + 2];
                    t[i + 2] = tmp;
                }

            var m = new Mesh { name = name };
            m.SetVertices(v);
            m.SetTriangles(t, 0);
            m.RecalculateBounds();
            return m;
        }

        // --- Ambient audio --------------------------------------------------

        static void AddAmbience(GameObject root, string genDir, LegaiaRealismOptions o)
        {
            string wavPath = genDir + "/ambience_loop.wav";
            if (AssetDatabase.LoadAssetAtPath<AudioClip>(wavPath) == null)
            {
                WriteAmbienceWav(wavPath);
                AssetDatabase.ImportAsset(wavPath);
            }
            var clip = AssetDatabase.LoadAssetAtPath<AudioClip>(wavPath);
            if (clip == null)
            {
                Debug.LogWarning("[Legaia] ambience clip failed to import: " + wavPath);
                return;
            }
            var existing = root.transform.Find("ambience");
            GameObject go = existing != null ? existing.gameObject : new GameObject("ambience");
            go.transform.SetParent(root.transform, false);
            var src = go.GetComponent<AudioSource>();
            if (src == null)
                src = go.AddComponent<AudioSource>();
            src.clip = clip;
            src.loop = true;
            src.playOnAwake = true;
            src.spatialBlend = 0f;
            src.volume = o.ambientVolume;
            LegaiaAudioGen.AddVrcSpatial(go, false, 0f, 0f, 0f);

            // Day / night beds under the same container: breeze + birds vs
            // crickets. Both always play; their volumes are crossfaded by
            // LegaiaDayNight (ApplySun wires them), so without the cycle the
            // day bed simply stays up and the night bed stays silent.
            AmbienceBed(go, genDir + "/ambience_day.wav", "ambience_day",
                LegaiaAudioGen.DayBed, 0.16f);
            AmbienceBed(go, genDir + "/ambience_night.wav", "ambience_night",
                LegaiaAudioGen.NightBed, 0f);
        }

        static void AmbienceBed(GameObject parent, string wavPath, string name,
            System.Func<float[]> gen, float volume)
        {
            var clip = LegaiaAudioGen.EnsureClip(wavPath, gen);
            if (clip == null)
                return;
            var existing = parent.transform.Find(name);
            GameObject go = existing != null ? existing.gameObject : new GameObject(name);
            go.transform.SetParent(parent.transform, false);
            var src = go.GetComponent<AudioSource>();
            if (src == null)
                src = go.AddComponent<AudioSource>();
            src.clip = clip;
            src.loop = true;
            src.playOnAwake = true;
            src.spatialBlend = 0f;
            src.volume = volume;
            LegaiaAudioGen.AddVrcSpatial(go, false, 0f, 0f, 0f);
        }

        /// Synthesize a 12 s seamless wind/surf noise bed: two one-pole
        /// lowpasses over white noise (a deep swell and an airy wash), each
        /// gain-modulated by an LFO with a whole number of cycles per loop,
        /// tail crossfaded onto the head so the loop point is continuous.
        /// Entirely generated - no disc audio.
        static void WriteAmbienceWav(string path)
        {
            const int sr = 44100;
            const int seconds = 12;
            int n = sr * seconds;
            int fade = sr; // 1 s loop crossfade
            var s = new float[n + fade];
            var rng = new System.Random(1234);
            float lpDeep = 0f, lpMid = 0f;
            for (int i = 0; i < s.Length; i++)
            {
                float white = (float)(rng.NextDouble() * 2.0 - 1.0);
                lpDeep += 0.020f * (white - lpDeep);
                lpMid += 0.120f * (white - lpMid);
                float time = (float)i / sr;
                float surge = 0.55f + 0.45f * Mathf.Sin(2f * Mathf.PI * 3f * time / seconds);
                float surge2 = 0.5f + 0.5f * Mathf.Sin(2f * Mathf.PI * 2f * time / seconds + 1.7f);
                s[i] = lpDeep * 2.4f * surge + lpMid * 0.35f * surge2;
            }
            for (int i = 0; i < fade; i++)
            {
                float w = (float)i / fade;
                s[i] = s[i] * w + s[n + i] * (1f - w);
            }
            float peak = 1e-4f;
            for (int i = 0; i < n; i++)
                peak = Mathf.Max(peak, Mathf.Abs(s[i]));
            float gain = 0.8f / peak;

            var bytes = new byte[44 + n * 2];
            void W32(int off, uint val)
            {
                bytes[off] = (byte)val;
                bytes[off + 1] = (byte)(val >> 8);
                bytes[off + 2] = (byte)(val >> 16);
                bytes[off + 3] = (byte)(val >> 24);
            }
            void WTag(int off, string tag)
            {
                for (int i = 0; i < 4; i++)
                    bytes[off + i] = (byte)tag[i];
            }
            WTag(0, "RIFF");
            W32(4, (uint)(36 + n * 2));
            WTag(8, "WAVE");
            WTag(12, "fmt ");
            W32(16, 16);
            bytes[20] = 1; // PCM
            bytes[22] = 1; // mono
            W32(24, sr);
            W32(28, sr * 2);
            bytes[32] = 2;  // block align
            bytes[34] = 16; // bits
            WTag(36, "data");
            W32(40, (uint)(n * 2));
            for (int i = 0; i < n; i++)
            {
                short q = (short)Mathf.RoundToInt(
                    Mathf.Clamp(s[i] * gain, -1f, 1f) * 32767f);
                bytes[44 + i * 2] = (byte)q;
                bytes[44 + i * 2 + 1] = (byte)((ushort)q >> 8);
            }
            File.WriteAllBytes(path, bytes);
        }

        // --- NPC wander -----------------------------------------------------

        static void WireWander(GameObject root, object manifest,
            LegaiaRealismOptions o, LegaiaSceneSettings settings)
        {
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return;
            var wanderType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcWander");
            var overrides = ParseFacingOverrides(o.wanderFacingOverrides);
            int wired = 0;
            foreach (object n in MiniJson.AsList(MiniJson.Get(manifest, "npcs"))
                     ?? new List<object>())
            {
                if (MiniJson.AsStr(MiniJson.Get(n, "kind")) != "talk")
                    continue;
                string file = MiniJson.AsStr(MiniJson.Get(n, "file")) ?? "";
                Vector3 p = LegaiaWorldBuilder.G2U(MiniJson.GetVec3(n, "position"));
                // Per-scene settings: a static NPC keeps its idle clip but
                // never travels; a removed NPC was not placed at all. Strip
                // any wander wired by an earlier pass so re-applying the
                // realism layer honours a newly-added rule too.
                if (settings.NpcIsStatic(file) || settings.NpcIsRemoved(file))
                {
                    if (wanderType != null)
                        StripWander(npcRoot, p, wanderType);
                    continue;
                }
                float yawOff = 0f;
                foreach (var kv in overrides)
                    if (file.Contains(kv.Key))
                    {
                        yawOff = kv.Value;
                        break;
                    }
                foreach (Transform child in npcRoot)
                {
                    if ((child.localPosition - p).sqrMagnitude > 1e-3f)
                        continue;
                    if (wanderType != null && child.GetComponent(wanderType) != null)
                        break; // already wired
                    var udon = LegaiaWorldBuilder.TryAttachUdon(
                        child.gameObject, "LegaiaNpcWander");
                    LegaiaWorldBuilder.SetUdonField(udon, "radius", o.wanderRadius);
                    if (yawOff != 0f)
                        LegaiaWorldBuilder.SetUdonField(udon, "facingYawOffset", yawOff);
                    LegaiaWorldBuilder.SyncUdonProxy(udon);
                    if (udon != null)
                        wired++;
                    break;
                }
            }
            Debug.Log("[Legaia] wander wired on " + wired + " villager(s).");
        }

        /// Remove an already-wired LegaiaNpcWander from the NPC instance at
        /// `p` - proxy AND backing UdonBehaviour (destroying only the U#
        /// proxy leaves the program that actually runs in-world attached).
        static void StripWander(Transform npcRoot, Vector3 p, System.Type wanderType)
        {
            foreach (Transform child in npcRoot)
            {
                if ((child.localPosition - p).sqrMagnitude > 1e-3f)
                    continue;
                var comp = child.GetComponent(wanderType);
                if (comp == null)
                    return;
                var util = LegaiaWorldBuilder.FindType(
                    "UdonSharpEditor.UdonSharpEditorUtility");
                var backing = util?.GetMethod("GetBackingUdonBehaviour")
                    ?.Invoke(null, new object[] { comp }) as Component;
                if (backing != null)
                    Undo.DestroyObjectImmediate(backing);
                Undo.DestroyObjectImmediate(comp);
                Debug.Log("[Legaia] wander stripped from " + child.name +
                    " (static in scene settings).");
                return;
            }
        }

        /// "npc_30:90; npc_07:-90" -> (stem fragment, degrees) pairs. A
        /// hand-set facingYawOffset on a placed instance dies with every
        /// rebuild - this list is the durable home for the rare model whose
        /// face is authored off the rig's -Z in vertex space, where no
        /// transform measurement can recover it.
        static List<KeyValuePair<string, float>> ParseFacingOverrides(string spec)
        {
            var list = new List<KeyValuePair<string, float>>();
            if (string.IsNullOrEmpty(spec))
                return list;
            foreach (string part in spec.Split(';', ','))
            {
                int colon = part.LastIndexOf(':');
                if (colon <= 0)
                    continue;
                string key = part.Substring(0, colon).Trim();
                if (key.Length == 0)
                    continue;
                if (float.TryParse(part.Substring(colon + 1).Trim(),
                        System.Globalization.NumberStyles.Float,
                        System.Globalization.CultureInfo.InvariantCulture,
                        out float deg))
                    list.Add(new KeyValuePair<string, float>(key, deg));
            }
            return list;
        }
    }
}
