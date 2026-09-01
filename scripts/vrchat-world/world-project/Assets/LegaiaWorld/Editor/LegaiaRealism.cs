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
// - Ambient audio: a synthesized wind/surf noise bed (WriteAmbienceWav -
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
                if (o.lighting)
                {
                    EditorUtility.DisplayProgressBar("Legaia realism", "Lit materials + normals", 0.1f);
                    ConvertToLit(root, genDir, o);
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
                if (o.ambientAudio)
                    AddAmbience(root, genDir, o);
                if (o.npcWander && manifest != null)
                    WireWander(root, manifest, o);
            }
            finally
            {
                EditorUtility.ClearProgressBar();
            }
            AssetDatabase.SaveAssets();
        }

        // --- Lighting -------------------------------------------------------

        static void ConvertToLit(GameObject root, string genDir, LegaiaRealismOptions o)
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

            ApplySun(root, o);
            Debug.Log("[Legaia] lit conversion: " + meshCache.Count + " mesh(es), " +
                      matCache.Count + " material(s).");
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

        static void ApplySun(GameObject root, LegaiaRealismOptions o)
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
                var dnType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaDayNight");
                if (dnType != null && go.GetComponent(dnType) != null)
                    return; // already wired
                var udon = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaDayNight");
                LegaiaWorldBuilder.SetUdonField(udon, "sun", sun);
                LegaiaWorldBuilder.SetUdonField(udon, "cycleMinutes", o.dayNightMinutes);
                LegaiaWorldBuilder.SetUdonField(udon, "dayIntensity", o.sunIntensity);
                LegaiaWorldBuilder.SyncUdonProxy(udon);
            }
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
                            Vector3 p = a + (b - a) * u + (d - a) * w;
                            EmitTuft(p, ground, rng, toLocal, v, c, t);
                            tufts++;
                            if (v.Count > CHUNK_VERTS)
                                FlushGrassChunk(v, c, t, chunks, genDir);
                        }
                    }
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
                        used[ci] = true;
                        grew = 1;
                    }
                }
                Bounds room = b; // pre-margin: where the dressing goes
                b.Expand(o.interiorShellMargin * 2f);
                float radius = b.extents.magnitude + 0.5f;

                var shell = ShellSphere(toLocal, b.center, radius,
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
                    light.range = radius * 1.6f;
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

        /// An inward-facing UV sphere around `centerW` (world), baked into
        /// container-local space. "Inward" must hold in WORLD space, and the
        /// built root usually carries a mirror that flips winding - so the
        /// orientation is settled empirically: sample one face's local
        /// normal, flip everything if the front points the wrong way for
        /// this parent chain.
        static Mesh ShellSphere(Matrix4x4 toLocal, Vector3 centerW, float radius,
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
                    v.Add(toLocal.MultiplyPoint3x4(centerW + new Vector3(
                        Mathf.Sin(phi) * Mathf.Cos(th),
                        Mathf.Cos(phi),
                        Mathf.Sin(phi) * Mathf.Sin(th)) * radius));
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

        static void WireWander(GameObject root, object manifest, LegaiaRealismOptions o)
        {
            var npcRoot = root.transform.Find("npcs");
            if (npcRoot == null)
                return;
            var wanderType = LegaiaWorldBuilder.FindType("LegaiaWorld.LegaiaNpcWander");
            int wired = 0;
            foreach (object n in MiniJson.AsList(MiniJson.Get(manifest, "npcs"))
                     ?? new List<object>())
            {
                if (MiniJson.AsStr(MiniJson.Get(n, "kind")) != "talk")
                    continue;
                Vector3 p = LegaiaWorldBuilder.G2U(MiniJson.GetVec3(n, "position"));
                foreach (Transform child in npcRoot)
                {
                    if ((child.localPosition - p).sqrMagnitude > 1e-3f)
                        continue;
                    if (wanderType != null && child.GetComponent(wanderType) != null)
                        break; // already wired
                    var udon = LegaiaWorldBuilder.TryAttachUdon(
                        child.gameObject, "LegaiaNpcWander");
                    LegaiaWorldBuilder.SetUdonField(udon, "radius", o.wanderRadius);
                    LegaiaWorldBuilder.SyncUdonProxy(udon);
                    if (udon != null)
                        wired++;
                    break;
                }
            }
            Debug.Log("[Legaia] wander wired on " + wired + " villager(s).");
        }
    }
}
