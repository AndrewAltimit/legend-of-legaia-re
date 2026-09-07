// Living props: the pass that plants NPC activity stations in the world
// itself, in a "<root>/living_props" container the NPC director finds by
// scanning for LegaiaNpcStation. Today that means the shoreline fishing
// spots; the card table's seat stations live on the stools instead (they
// are part of the common-prefabs pass).
//
// A fishing spot is for PLAYERS too: each one gets a stake with a bucket
// beside the stand point, carrying the Interact box ("Fish") and a
// world-space label the handler writes the bite cue and the reward line
// into. The stake stands off to one side so it never intersects whoever
// is standing on the spot, villager or player.
//
// FINDING A FISHING SPOT, from the world mesh alone (no authored data):
//
//   1. Water = the same sheet the collider pass keeps a floor over: a
//      semi-transparent (BLEND) submesh that is large and flat and
//      horizontal (LegaiaWorldBuilder.SubmeshCollides' rule - see the
//      README's "Transparent surfaces"). Its triangle centroids mark the
//      water cells on a 0.75 m XZ grid, and their mean height is the
//      water surface.
//   2. Land = the lowest upward-facing OPAQUE triangle per cell (the
//      grass pass's ground rule, without the green test) - a raycast
//      cannot tell land from water here, because the merged collider
//      deliberately includes the water sheet.
//   3. A shore cell is a land cell 0.02-2 m above the water, not itself
//      water, with water within ~1.5 m, on the village side of the map
//      (inside the interior-room distance from spawn).
//   4. It must survive physics: a downward ray lands on the collider at
//      about the cell's height, and a 0.35 m capsule at chest height
//      stands clear.
//   5. Spread: pick the requested number nearest the spawn, keeping a
//      separation that relaxes (8 / 6 / 4 / 2.5 m) until enough fit.
//
// Everything the pass builds is primitives + generated textures.
//
// Idempotent: the container is destroyed and rebuilt per run.

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaLivingProps
    {
        internal const string CONTAINER = "living_props";
        const float CELL = 0.75f;
        /// Metres to the right of the stand point the stake goes, clear of
        /// whoever is standing on the spot.
        const float STAKE_SIDE = 0.55f;

        internal static void Remove(GameObject root)
        {
            var old = root.transform.Find(CONTAINER);
            if (old != null)
                Object.DestroyImmediate(old.gameObject);
        }

        internal static GameObject Apply(GameObject root, string sceneName,
            LegaiaRealismOptions o)
        {
            Remove(root);
            int want = Mathf.Clamp(o.fishingSpots, 0, 8);
            if (want == 0)
                return null;
            var world = root.transform.Find("world");
            if (world == null)
            {
                Debug.LogWarning("[Legaia] no 'world' child under " + root.name +
                                 " - living props skipped.");
                return null;
            }
            Vector3 spawnW = root.transform.position;
            var spawnT = root.transform.Find("LegaiaSpawn");
            if (spawnT != null)
                spawnW = spawnT.position;

            var shore = FindShorePoints(world, spawnW, o, want, out float waterY);
            if (shore.Count == 0)
            {
                Debug.LogWarning("[Legaia] living props: no standable shoreline " +
                    "found (no large flat transparent water sheet, or none " +
                    "within " + o.interiorRoomDistance + " m of the spawn) - " +
                    "no fishing spots built.");
                return null;
            }

            string genDir = "Assets/LegaiaGenerated/" + sceneName + "/living";
            Directory.CreateDirectory(genDir);
            var container = new GameObject(CONTAINER);
            container.transform.SetParent(root.transform, false);
            // Cancel the built root's mirror so a spot's +Z really is the
            // direction it faces (the station's StandForward contract).
            Vector3 ls = root.transform.lossyScale;
            container.transform.localScale = new Vector3(
                Mathf.Approximately(ls.x, 0f) ? 1f : 1f / ls.x,
                Mathf.Approximately(ls.y, 0f) ? 1f : 1f / ls.y,
                Mathf.Approximately(ls.z, 0f) ? 1f : 1f / ls.z);

            for (int i = 0; i < shore.Count; i++)
                BuildFishingSpot(container.transform, genDir, i,
                    shore[i].pos, shore[i].toWater, waterY);

            Debug.Log("[Legaia] living props: " + shore.Count +
                " fishing spot(s) on the shoreline at y=" + waterY.ToString("0.00") +
                " (LegaiaNpcStation kind 1).");
            return container;
        }

        // --- Shoreline search ------------------------------------------------

        struct Shore
        {
            public Vector3 pos;
            public Vector3 toWater;
            public float distToSpawn;
        }

        static Vector2Int CellOf(Vector3 p) => new Vector2Int(
            Mathf.FloorToInt(p.x / CELL), Mathf.FloorToInt(p.z / CELL));

        static List<Shore> FindShorePoints(Transform world, Vector3 spawnW,
            LegaiaRealismOptions o, int want, out float waterY)
        {
            var water = new Dictionary<Vector2Int, Vector3>();
            var ground = new Dictionary<Vector2Int, float>();
            double ySum = 0.0, yWeight = 0.0;

            foreach (var r in world.GetComponentsInChildren<Renderer>(false))
            {
                Mesh mesh = MeshOf(r);
                if (mesh == null || !mesh.isReadable)
                    continue;
                var mats = r.sharedMaterials;
                var verts = mesh.vertices;
                Matrix4x4 toWorld = r.transform.localToWorldMatrix;
                for (int sm = 0; sm < mesh.subMeshCount; sm++)
                {
                    Material mat = sm < mats.Length ? mats[sm] : null;
                    bool blend = mat != null && mat.renderQueue >=
                        (int)UnityEngine.Rendering.RenderQueue.Transparent;
                    var tris = mesh.GetTriangles(sm);
                    if (tris.Length == 0)
                        continue;
                    if (blend)
                    {
                        // The water-sheet rule: large, flat, horizontal.
                        Bounds b = new Bounds(
                            toWorld.MultiplyPoint3x4(verts[tris[0]]), Vector3.zero);
                        for (int i = 1; i < tris.Length; i++)
                            b.Encapsulate(toWorld.MultiplyPoint3x4(verts[tris[i]]));
                        if (!(b.size.y <= 2f && Mathf.Max(b.size.x, b.size.z) >= 10f))
                            continue;
                        for (int i = 0; i < tris.Length; i += 3)
                        {
                            Vector3 a = toWorld.MultiplyPoint3x4(verts[tris[i]]);
                            Vector3 c = toWorld.MultiplyPoint3x4(verts[tris[i + 1]]);
                            Vector3 d = toWorld.MultiplyPoint3x4(verts[tris[i + 2]]);
                            Vector3 cen = (a + c + d) / 3f;
                            water[CellOf(cen)] = cen;
                            ySum += cen.y;
                            yWeight += 1.0;
                        }
                        continue;
                    }
                    // Opaque: record the lowest upward-facing surface per
                    // cell (Abs on the normal - the source winding is mixed).
                    for (int i = 0; i < tris.Length; i += 3)
                    {
                        Vector3 a = toWorld.MultiplyPoint3x4(verts[tris[i]]);
                        Vector3 c = toWorld.MultiplyPoint3x4(verts[tris[i + 1]]);
                        Vector3 d = toWorld.MultiplyPoint3x4(verts[tris[i + 2]]);
                        Vector3 cr = Vector3.Cross(c - a, d - a);
                        float area2 = cr.magnitude;
                        if (area2 < 2e-4f)
                            continue;
                        if (Mathf.Abs(cr.y / area2) < 0.65f)
                            continue;
                        Vector3 cen = (a + c + d) / 3f;
                        var cell = CellOf(cen);
                        if (!ground.TryGetValue(cell, out float y) || cen.y < y)
                            ground[cell] = cen.y;
                    }
                }
            }

            waterY = yWeight > 0.0 ? (float)(ySum / yWeight) : 0f;
            var result = new List<Shore>();
            if (water.Count == 0)
                return result;

            var cands = new List<Shore>();
            foreach (var kv in ground)
            {
                if (water.ContainsKey(kv.Key))
                    continue;
                float h = kv.Value - waterY;
                if (h < 0.02f || h > 2f)
                    continue;
                // Water within two cells (~1.5 m).
                Vector3 wsum = Vector3.zero;
                int wn = 0;
                for (int dx = -2; dx <= 2; dx++)
                    for (int dz = -2; dz <= 2; dz++)
                    {
                        Vector3 wc;
                        if (water.TryGetValue(
                                new Vector2Int(kv.Key.x + dx, kv.Key.y + dz), out wc))
                        {
                            wsum += wc;
                            wn++;
                        }
                    }
                if (wn == 0)
                    continue;
                Vector3 p = new Vector3((kv.Key.x + 0.5f) * CELL, kv.Value,
                                        (kv.Key.y + 0.5f) * CELL);
                Vector3 dSpawn = p - spawnW;
                dSpawn.y = 0f;
                if (dSpawn.magnitude > o.interiorRoomDistance)
                    continue; // the far side of the map / a room's own pond
                Vector3 toWater = wsum / wn - p;
                toWater.y = 0f;
                if (toWater.sqrMagnitude < 1e-4f)
                    continue;

                // Physics: a real floor here, and room to stand.
                if (!Physics.Raycast(p + Vector3.up * 2f, Vector3.down,
                        out RaycastHit hit, 4f, -1, QueryTriggerInteraction.Ignore))
                    continue;
                if (Mathf.Abs(hit.point.y - kv.Value) > 0.6f)
                    continue;
                if (Physics.CheckSphere(hit.point + Vector3.up * 1f, 0.35f,
                        -1, QueryTriggerInteraction.Ignore))
                    continue;

                var s = new Shore();
                s.pos = hit.point;
                s.toWater = toWater.normalized;
                s.distToSpawn = dSpawn.magnitude;
                cands.Add(s);
            }
            if (cands.Count == 0)
                return result;
            cands.Sort((a, b) => a.distToSpawn.CompareTo(b.distToSpawn));

            // Spread them along the shore: keep the widest separation that
            // still yields the requested count.
            float[] seps = { 8f, 6f, 4f, 2.5f };
            for (int si = 0; si < seps.Length; si++)
            {
                result.Clear();
                foreach (var c in cands)
                {
                    bool ok = true;
                    foreach (var got in result)
                    {
                        Vector3 d = got.pos - c.pos;
                        d.y = 0f;
                        if (d.magnitude < seps[si])
                        {
                            ok = false;
                            break;
                        }
                    }
                    if (ok)
                        result.Add(c);
                    if (result.Count >= want)
                        break;
                }
                if (result.Count >= want)
                    break;
            }
            return result;
        }

        static Mesh MeshOf(Renderer r)
        {
            if (r is SkinnedMeshRenderer smr)
                return smr.sharedMesh;
            var mf = r.GetComponent<MeshFilter>();
            return mf != null ? mf.sharedMesh : null;
        }

        // --- One fishing spot --------------------------------------------------

        static void BuildFishingSpot(Transform parent, string genDir, int index,
            Vector3 pos, Vector3 toWater, float waterY)
        {
            var wood = LegaiaCampProps.EnsureMat(genDir, "rod_wood", "Standard",
                new Color(0.42f, 0.29f, 0.16f));
            var floatMat = LegaiaCampProps.EnsureMat(genDir, "float_red", "Standard",
                new Color(0.78f, 0.18f, 0.12f));
            var fishMat = LegaiaCampProps.EnsureMat(genDir, "fish_silver", "Standard",
                new Color(0.72f, 0.76f, 0.8f));
            var lineMat = LegaiaCampProps.EnsureMat(genDir, "fishing_line", "Unlit/Color",
                new Color(0.9f, 0.9f, 0.88f));

            var spot = new GameObject("fishing_spot_" + index);
            spot.transform.SetParent(parent, false);
            spot.transform.position = pos;
            spot.transform.rotation = Quaternion.LookRotation(toWater, Vector3.up);

            var gear = new GameObject("gear");
            gear.transform.SetParent(spot.transform, false);

            // Rod: a thin cylinder along its own +Y (Unity's cylinder is 2
            // units tall, so scale.y 0.6 = a 1.2 m rod). The behaviour
            // positions and angles it against the NPC every frame.
            var rod = Prim(PrimitiveType.Cylinder, "rod", gear.transform,
                Vector3.zero, new Vector3(0.018f, 0.6f, 0.018f), wood);
            var tip = new GameObject("tip");
            tip.transform.SetParent(rod.transform, false);
            tip.transform.localPosition = new Vector3(0f, 1f, 0f); // the far end

            // Float: an empty the behaviour moves, carrying the visible ball
            // and the ripple emitter (so neither inherits the ball's scale).
            var bobber = new GameObject("float");
            bobber.transform.SetParent(gear.transform, false);
            bobber.transform.position = pos + toWater * 2.2f + Vector3.up * (waterY - pos.y);
            Prim(PrimitiveType.Sphere, "ball", bobber.transform,
                Vector3.zero, Vector3.one * 0.07f, floatMat);
            var ripples = BuildRipples(bobber.transform, genDir);

            var lineGo = new GameObject("line");
            lineGo.transform.SetParent(gear.transform, false);
            var lr = lineGo.AddComponent<LineRenderer>();
            lr.useWorldSpace = true;
            lr.positionCount = 2;
            lr.widthMultiplier = 0.006f;
            lr.numCapVertices = 0;
            lr.sharedMaterial = lineMat;
            lr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            lr.receiveShadows = false;

            var fish = Prim(PrimitiveType.Quad, "fish", gear.transform,
                Vector3.zero, new Vector3(0.17f, 0.08f, 1f), fishMat);
            fish.SetActive(false);
            gear.SetActive(false);

            // The player's way in: a stake and a bucket a step to the
            // right of the stand point, and the Interact box on the spot
            // itself (an Udon behaviour is only reached through a collider
            // on its OWN GameObject). Trigger, so nobody walks into it.
            var stake = new GameObject("stake");
            stake.transform.SetParent(spot.transform, false);
            stake.transform.localPosition = new Vector3(STAKE_SIDE, 0f, 0f);
            Prim(PrimitiveType.Cylinder, "post", stake.transform,
                new Vector3(0f, 0.45f, 0f), new Vector3(0.05f, 0.45f, 0.05f), wood);
            Prim(PrimitiveType.Cylinder, "bucket", stake.transform,
                new Vector3(0.22f, 0.11f, 0f), new Vector3(0.20f, 0.11f, 0.20f), wood);
            var box = spot.AddComponent<BoxCollider>();
            box.isTrigger = true;
            box.center = new Vector3(STAKE_SIDE, 0.5f, 0f);
            box.size = new Vector3(0.4f, 1.0f, 0.4f);

            // The label faces back down the spot's -Z: the stand point
            // looks out over the water, so that is the reader's side.
            var labelGo = new GameObject("reward_label");
            labelGo.transform.SetParent(stake.transform, false);
            labelGo.transform.localPosition = new Vector3(0f, 1.12f, 0f);
            labelGo.transform.localRotation = Quaternion.Euler(0f, 180f, 0f);
            var label = labelGo.AddComponent<TMPro.TextMeshPro>();
            label.text = "";
            label.fontSize = 1.6f;
            label.alignment = TMPro.TextAlignmentOptions.Center;
            label.color = new Color(1f, 0.93f, 0.6f);
            label.rectTransform.sizeDelta = new Vector2(1.4f, 0.4f);
            label.GetComponent<MeshRenderer>().shadowCastingMode =
                UnityEngine.Rendering.ShadowCastingMode.Off;

            var station = LegaiaWorldBuilder.TryAttachUdon(spot, "LegaiaNpcStation");
            LegaiaWorldBuilder.SetUdonField(station, "kind", 1); // fishing
            LegaiaWorldBuilder.SetUdonField(station, "standPoint", spot.transform);
            LegaiaWorldBuilder.SetUdonField(station, "dwellSeconds", 40f);
            LegaiaWorldBuilder.SetUdonField(station, "indoors", false);

            var handler = LegaiaWorldBuilder.TryAttachUdon(spot, "LegaiaFishingSpot");
            LegaiaWorldBuilder.SetUdonField(handler, "station", station);
            LegaiaWorldBuilder.SetUdonField(handler, "gear", gear);
            LegaiaWorldBuilder.SetUdonField(handler, "rod", rod.transform);
            LegaiaWorldBuilder.SetUdonField(handler, "rodTip", tip.transform);
            LegaiaWorldBuilder.SetUdonField(handler, "bobber", bobber.transform);
            LegaiaWorldBuilder.SetUdonField(handler, "line", lr);
            LegaiaWorldBuilder.SetUdonField(handler, "ripples", ripples);
            LegaiaWorldBuilder.SetUdonField(handler, "fish", fish.transform);
            LegaiaWorldBuilder.SetUdonField(handler, "waterY", waterY);
            LegaiaWorldBuilder.SetUdonField(handler, "rewardText", label);
            // The purse, when the common-prefabs pass has already built it;
            // the handler resolves it by path in Start otherwise.
            LegaiaWorldBuilder.SetUdonField(handler, "wallet",
                LegaiaCommonPrefabs.FindWallet());
            LegaiaWorldBuilder.SyncUdonProxy(handler);

            LegaiaWorldBuilder.SetUdonField(station, "handler", handler);
            LegaiaWorldBuilder.SyncUdonProxy(station);
        }

        /// Flat rings spreading on the water under the float - horizontal
        /// billboards so they lie ON the surface instead of facing the
        /// camera like smoke.
        static ParticleSystem BuildRipples(Transform parent, string genDir)
        {
            var go = new GameObject("ripples");
            go.transform.SetParent(parent, false);
            var ps = go.AddComponent<ParticleSystem>();
            var main = ps.main;
            main.loop = true;
            main.playOnAwake = false;
            main.duration = 3f;
            main.simulationSpace = ParticleSystemSimulationSpace.World;
            main.startLifetime = new ParticleSystem.MinMaxCurve(1.6f, 2.4f);
            main.startSpeed = new ParticleSystem.MinMaxCurve(0f);
            main.startSize = new ParticleSystem.MinMaxCurve(0.10f, 0.16f);
            main.startColor = new ParticleSystem.MinMaxGradient(
                new Color(0.85f, 0.92f, 1f, 0.5f));
            main.maxParticles = 24;

            var emission = ps.emission;
            emission.rateOverTime = new ParticleSystem.MinMaxCurve(1.6f);

            var shape = ps.shape;
            shape.shapeType = ParticleSystemShapeType.Circle;
            shape.radius = 0.05f;
            shape.radiusThickness = 0f;

            var size = ps.sizeOverLifetime;
            size.enabled = true;
            size.size = new ParticleSystem.MinMaxCurve(
                1f, AnimationCurve.Linear(0f, 0.4f, 1f, 3.2f));

            var color = ps.colorOverLifetime;
            color.enabled = true;
            var grad = new Gradient();
            grad.SetKeys(
                new[] { new GradientColorKey(Color.white, 0f),
                        new GradientColorKey(Color.white, 1f) },
                new[] { new GradientAlphaKey(0f, 0f), new GradientAlphaKey(1f, 0.15f),
                        new GradientAlphaKey(0f, 1f) });
            color.color = new ParticleSystem.MinMaxGradient(grad);

            var pr = go.GetComponent<ParticleSystemRenderer>();
            pr.renderMode = ParticleSystemRenderMode.HorizontalBillboard;
            pr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            pr.receiveShadows = false;
            pr.sharedMaterial = EnsureRippleMaterial(genDir);
            return ps;
        }

        static Material EnsureRippleMaterial(string genDir)
        {
            string texPath = genDir + "/ripple_ring.png";
            if (AssetDatabase.LoadAssetAtPath<Texture2D>(texPath) == null)
            {
                const int S = 64;
                var tex = new Texture2D(S, S, TextureFormat.RGBA32, false);
                for (int y = 0; y < S; y++)
                    for (int x = 0; x < S; x++)
                    {
                        float d = Vector2.Distance(new Vector2(x, y),
                            new Vector2(S / 2f, S / 2f)) / (S / 2f);
                        // A soft annulus peaking at 0.78 of the radius.
                        float a = Mathf.Clamp01(1f - Mathf.Abs(d - 0.78f) / 0.18f);
                        a *= Mathf.Clamp01((1f - d) * 6f);
                        tex.SetPixel(x, y, new Color(1f, 1f, 1f, a * a));
                    }
                tex.Apply();
                File.WriteAllBytes(texPath, tex.EncodeToPNG());
                Object.DestroyImmediate(tex);
                AssetDatabase.ImportAsset(texPath);
            }
            string matPath = genDir + "/ripple_particle.mat";
            var m = AssetDatabase.LoadAssetAtPath<Material>(matPath);
            if (m == null)
            {
                m = new Material(Shader.Find("Legacy Shaders/Particles/Alpha Blended"));
                AssetDatabase.CreateAsset(m, matPath);
            }
            m.mainTexture = AssetDatabase.LoadAssetAtPath<Texture2D>(texPath);
            return m;
        }

        static GameObject Prim(PrimitiveType type, string name, Transform parent,
            Vector3 localPos, Vector3 localScale, Material mat)
        {
            var go = GameObject.CreatePrimitive(type);
            go.name = name;
            Object.DestroyImmediate(go.GetComponent<Collider>());
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;
            go.transform.localScale = localScale;
            var mr = go.GetComponent<MeshRenderer>();
            if (mat != null)
                mr.sharedMaterial = mat;
            mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            return go;
        }
    }
}
