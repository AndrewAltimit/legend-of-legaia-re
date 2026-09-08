// Wall posters: a framed print hung on one of the world's OWN walls,
// next to a common prefab (the card table's poker-night bill).
//
// The kit has no authored wall list - the town is imported geometry with
// mesh colliders - so the pass FINDS a wall: from the `near` object it
// fires 24 horizontal rays at eye height, keeps the hits whose surface is
// vertical, faces back toward the anchor and continues 0.6 m above the
// ray, then takes the nearest one whose plane also covers the whole print
// at its hanging height (a five-point probe per candidate, measured
// against the floor under THAT wall, not the floor the anchor stands on).
// Nothing found = a warning and no poster; a print floating in the square
// is worse than no print.
//
// Two things about that search are load-bearing, both learned the hard
// way on town01. The probe rays read the whole RaycastAll list rather
// than the nearest hit, because anything standing in front of a wall
// otherwise reports the wall as short. And they probe UP only: the ground
// rises toward a building, so a wall standing on a terrace has nothing
// under the eye ray and is not short at all.
//
// That search is a FALLBACK. `prefab_transforms["poster_<name>"]` wins
// whenever it exists, exactly like every other common prefab: the object
// is a direct child of the container named `poster_<name>`, so
// LegaiaSettingsSnapshot captures it by name (LegaiaCommonPrefabs.
// SettingsKey) - drag the poster where you want it, snapshot, and the
// next build reproduces the hand placement and never searches again.
//
// Geometry: the poster ROOT carries the pose - +Z is the direction the
// print faces (out of the wall) - and its two children sit at the
// origin, so the snapshot's position/rotation is the whole story. The
// print quad is 1 cm off the wall, the frame a slightly larger dark
// quad 4 mm behind it. The quad mesh is generated rather than taken from
// PrimitiveType.Quad for two reasons: the built-in quad faces -Z (a
// poster whose forward pointed INTO the wall would fail every "is it on
// a wall" test that reads transform.forward), and it carries no vertex
// colour, which the kit's lit shaders multiply into the albedo - an
// unbound colour attribute is white on some drivers and BLACK on others,
// so the mesh here writes white explicitly.
//
// Shading: the print material is created directly on
// "Legaia/Lit Vertex Color (Cutout)", so LegaiaRealism.ConvertPropToLit -
// which runs over the whole container after this pass - skips it by
// shader name, the same way the mirror surface and the video screen are
// skipped. `_LightWrap` is raised near 1: a photographic print with a
// |N.L| terminator across it reads as a dirty poster, not a lit one.
// The frame uses the shared Standard "camp_dark" and IS converted, which
// is what keeps it matching the rest of the furniture.
//
// Check entry point (headless, edit mode):
//   Unity.exe -batchmode -nographics -quit -projectPath <project>
//       -executeMethod LegaiaWorld.LegaiaPosters.Check
//       [-legaiaScene Assets/Scenes/<scene>.unity] -logFile <log>

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    internal static class LegaiaPosters
    {
        internal const string PREFIX = "poster_";

        // Search shape: 24 bearings fired from eye height, the two reach
        // radii below, and the vertical run a surface must have to count
        // as a wall at all.
        const int RAYS = 24;
        /// The radius a poster WANTS to stay inside of its anchor.
        const float REACH = 7f;
        /// ...and the one it will settle for. town01's card table stands in
        /// an open yard: the nearest house wall is 8.7 m due +Z, so a hard
        /// 7 m would leave the poster unhung in the shipped scene. Past
        /// REACH the pass says so in the log; past this it gives up.
        const float MAX_REACH = 12f;
        const float EYE = 1.5f;
        const float PROBE = 0.6f;
        const float MIN_WALL = 0.6f;
        /// Height of the print's centre above the floor under the wall.
        const float HANG_HEIGHT = 1.55f;
        /// The print stands this far off the wall plane.
        const float STANDOFF = 0.01f;

        /// Hang every poster in `posters` under `container`. Returns how
        /// many were actually hung (a missing image, a missing anchor or
        /// no wall in range each warn and skip).
        internal static int Build(GameObject container, string genDir,
            Dictionary<string, LegaiaPrefabTransform> placements,
            List<LegaiaPosterRef> posters)
        {
            if (container == null || posters == null || posters.Count == 0)
                return 0;
            int hung = 0;
            foreach (var p in posters)
                if (p != null && BuildOne(container, genDir, placements, p))
                    hung++;
            return hung;
        }

        static bool BuildOne(GameObject container, string genDir,
            Dictionary<string, LegaiaPrefabTransform> placements, LegaiaPosterRef p)
        {
            var tex = LoadImage(p.image);
            if (tex == null)
            {
                Debug.LogWarning("[Legaia] poster '" + p.name + "': no texture at " +
                    p.image + " - drop the image there (it is not part of the kit) " +
                    "or remove the posters entry.");
                return false;
            }
            float w = Mathf.Clamp(p.width, 0.05f, 8f);
            float h = w * (tex.height / (float)Mathf.Max(1, tex.width));

            string key = PREFIX + p.name;
            var go = new GameObject(key);
            go.transform.SetParent(container.transform, false);

            if (LegaiaSceneSettings.ApplyPlacement(placements, key, go.transform))
            {
                Debug.Log("[Legaia] poster '" + p.name + "' placed by hand at " +
                    go.transform.localPosition + " (prefab_transforms[\"" + key +
                    "\"]) - no wall search.");
            }
            else
            {
                var anchor = Anchor(container, p.near);
                if (anchor == null)
                {
                    Debug.LogWarning("[Legaia] poster '" + p.name + "': no object named '" +
                        p.near + "' to hang it near - skipped.");
                    Undo.DestroyObjectImmediate(go);
                    return false;
                }
                Vector3 centre, normal;
                float bearing, span;
                if (!FindWall(anchor.position, container.transform, w, h,
                        out centre, out normal, out bearing, out span))
                {
                    Debug.LogWarning("[Legaia] poster '" + p.name + "': no wall within " +
                        MAX_REACH + " m of '" + p.near + "' at " + anchor.position +
                        " that fits a " + w.ToString("0.00") + " x " + h.ToString("0.00") +
                        " m print - skipped (hand-place it and snapshot to pin it).");
                    Undo.DestroyObjectImmediate(go);
                    return false;
                }
                go.transform.position = centre + normal * STANDOFF;
                go.transform.rotation = Quaternion.LookRotation(normal, Vector3.up);
                if (span > REACH)
                    Debug.LogWarning("[Legaia] poster '" + p.name + "' hangs " +
                        span.ToString("0.0") + " m from '" + p.near + "' - the nearest " +
                        "wall that fits it is past the " + REACH + " m the search " +
                        "prefers. Drag it somewhere better and snapshot the placement " +
                        "if that reads wrong in-world.");
                Debug.Log("[Legaia] poster '" + p.name + "' hung at " +
                    go.transform.position + ", wall normal " + normal +
                    ", " + span.ToString("0.00") + " m from '" + p.near + "' toward " +
                    Bearing(bearing) + " (" + bearing.ToString("0") + " deg), print " +
                    w.ToString("0.00") + " x " + h.ToString("0.00") + " m.");
            }

            var mesh = EnsureQuad(genDir);
            var dark = LegaiaCampProps.EnsureMat(genDir, "camp_dark", "Standard",
                new Color(0.16f, 0.14f, 0.12f));
            Quad(go.transform, "frame", mesh, dark,
                new Vector3(0f, 0f, -0.006f), new Vector3(w + 0.05f, h + 0.05f, 1f));
            Quad(go.transform, "print", mesh, EnsurePrintMaterial(genDir, p.name, tex),
                Vector3.zero, new Vector3(w, h, 1f));
            return true;
        }

        /// The object a poster hangs near: a common-prefab child first
        /// (that is where "card_table" lives), then anything in the scene.
        static Transform Anchor(GameObject container, string near)
        {
            if (string.IsNullOrEmpty(near) || container == null)
                return null;
            var t = container.transform.Find(near);
            if (t != null)
                return t;
            var go = GameObject.Find(near);
            return go != null ? go.transform : null;
        }

        static GameObject Quad(Transform parent, string name, Mesh mesh, Material mat,
            Vector3 localPos, Vector3 localScale)
        {
            var go = new GameObject(name);
            go.transform.SetParent(parent, false);
            go.transform.localPosition = localPos;
            go.transform.localScale = localScale;
            go.AddComponent<MeshFilter>().sharedMesh = mesh;
            var mr = go.AddComponent<MeshRenderer>();
            mr.sharedMaterial = mat;
            mr.shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            return go;
        }

        // --- The wall search ----------------------------------------------

        /// Nearest wall around `from` that can carry a `w` x `h` print.
        /// `centre` comes back ON the wall plane (the caller adds the
        /// standoff), `normal` points out of it, `bearing` is the yaw from
        /// world +Z toward the wall and `span` the horizontal distance.
        static bool FindWall(Vector3 from, Transform container, float w, float h,
            out Vector3 centre, out Vector3 normal, out float bearing, out float span)
        {
            centre = Vector3.zero;
            normal = Vector3.forward;
            bearing = 0f;
            span = 0f;

            Vector3 eye = from + Vector3.up * EYE;
            var hitDist = new List<float>();
            var hitPoint = new List<Vector3>();
            var hitNormal = new List<Vector3>();
            var hitYaw = new List<float>();
            var notes = new List<string>();

            for (int i = 0; i < RAYS; i++)
            {
                float yaw = i * (360f / RAYS);
                Vector3 dir = Quaternion.Euler(0f, yaw, 0f) * Vector3.forward;
                RaycastHit hit;
                if (!Solid(eye, dir, MAX_REACH, container, out hit))
                {
                    notes.Add(yaw.ToString("0") + ": open");
                    continue;
                }
                string what = hit.collider.name + " @" + hit.distance.ToString("0.00") +
                              " n" + hit.normal.ToString("0.00");
                if (hit.distance < MIN_WALL)
                {
                    notes.Add(yaw.ToString("0") + ": " + what + " too close");
                    continue;
                }
                if (Mathf.Abs(hit.normal.y) > 0.35f)
                {
                    notes.Add(yaw.ToString("0") + ": " + what + " not vertical");
                    continue;
                }
                if (Vector3.Dot(hit.normal, -dir) < 0.5f)
                {
                    notes.Add(yaw.ToString("0") + ": " + what + " faces away");
                    continue;
                }
                // UP only. Downward is not a wall test at all: the ground
                // rises toward a building, so a house wall whose base sits
                // on a terrace half a metre above the yard has nothing at
                // all under the eye ray - which is exactly how town01's
                // nearest wall read as "too short". How far down the wall
                // really runs is decided by Covers() below, against the
                // wall's OWN floor rather than the anchor's.
                if (!Continues(eye + Vector3.up * PROBE, dir, hit, container))
                {
                    notes.Add(yaw.ToString("0") + ": " + what + " too short (up " +
                        probeWhy + ")");
                    continue;
                }
                hitDist.Add(hit.distance);
                hitPoint.Add(hit.point);
                hitNormal.Add(hit.normal);
                hitYaw.Add(yaw);
            }

            // Nearest first, then the first candidate whose plane covers
            // the whole print (a wall can pass the 0.6 m probes and still
            // end in a doorway 30 cm to the side).
            int n = hitDist.Count;
            var order = new List<int>();
            for (int i = 0; i < n; i++)
                order.Add(i);
            order.Sort((a, b) => hitDist[a].CompareTo(hitDist[b]));

            foreach (int i in order)
            {
                Vector3 nrm = hitNormal[i].normalized;
                nrm.y = 0f;
                if (nrm.sqrMagnitude < 1e-4f)
                    continue;
                nrm = nrm.normalized;
                float floorY = FloorUnder(hitPoint[i] + nrm * 0.3f, container, from.y);
                Vector3 c = new Vector3(hitPoint[i].x, floorY + HANG_HEIGHT, hitPoint[i].z);
                if (!Covers(c, nrm, w, h, container))
                {
                    notes.Add(hitYaw[i].ToString("0") + ": wall at " +
                        hitDist[i].ToString("0.00") + " too small for the print (" +
                        coverWhy + ")");
                    continue;
                }
                centre = c;
                normal = nrm;
                bearing = hitYaw[i];
                span = hitDist[i];
                return true;
            }

            Debug.Log("[Legaia] poster wall search from " + from + " (eye " + eye +
                ", " + RAYS + " rays x " + REACH + " m, print " + w.ToString("0.00") +
                " x " + h.ToString("0.00") + ") found nothing: " +
                string.Join(", ", notes.ToArray()));
            return false;
        }

        /// Nearest hit along the ray that is world geometry: triggers, the
        /// kit's own containers, the villagers' capsules and anything with
        /// a rigidbody (pickups, coins) are stepped over rather than
        /// stopping the ray - RaycastAll, not Raycast, for exactly that.
        static bool Solid(Vector3 origin, Vector3 dir, float dist, Transform container,
            out RaycastHit best)
        {
            best = default(RaycastHit);
            bool got = false;
            foreach (var h in Physics.RaycastAll(origin, dir, dist, ~0,
                         QueryTriggerInteraction.Ignore))
            {
                if (Ignored(h.collider, container))
                    continue;
                if (!got || h.distance < best.distance)
                {
                    best = h;
                    got = true;
                }
            }
            return got;
        }

        static readonly string[] SKIP_ROOTS =
        {
            LegaiaCommonPrefabs.CONTAINER, "Legaia_camp_props", "npcs",
            "living_town", "teleports", "Legaia_weather",
        };

        static bool Ignored(Collider c, Transform container)
        {
            if (c == null || c.isTrigger)
                return true;
            if (container != null && c.transform.IsChildOf(container))
                return true;
            // A villager's body is a capsule from its render bounds - the
            // same shape test LegaiaNpcWander's probe uses to tell a
            // villager from the world.
            if (c is CapsuleCollider || c.attachedRigidbody != null)
                return true;
            for (var t = c.transform; t != null; t = t.parent)
                foreach (string s in SKIP_ROOTS)
                    if (t.name == s)
                        return true;
            return false;
        }

        // Why the last plane probe said no (diagnostics only - the search
        // log is the only thing that explains an empty square).
        static string probeWhy = "";

        /// Is there a hit at `expected` distance whose plane matches
        /// `normal`, ANYWHERE along the ray - not merely the nearest one?
        /// The distinction is the whole trap: the probe rays that decide
        /// how tall a wall is run past everything standing in front of it,
        /// and a barrel 2.5 m out reported a real house wall 8.7 m away as
        /// "too short" in town01. The wall's own hit is in the RaycastAll
        /// list either way; only `Physics.Raycast` hides it.
        static bool PlaneAt(Vector3 origin, Vector3 dir, float maxDist, float expected,
            float tol, Vector3 normal, float angTol, Transform container)
        {
            float nearest = -1f;
            foreach (var h in Physics.RaycastAll(origin, dir, maxDist, ~0,
                         QueryTriggerInteraction.Ignore))
            {
                if (Ignored(h.collider, container))
                    continue;
                if (nearest < 0f || h.distance < nearest)
                    nearest = h.distance;
                if (Mathf.Abs(h.distance - expected) < tol &&
                    Vector3.Angle(h.normal, normal) < angTol)
                    return true;
            }
            probeWhy = nearest < 0f ? "nothing" : "@" + nearest.ToString("0.00");
            return false;
        }

        /// Does the same surface still stand `origin` high (or low)?
        static bool Continues(Vector3 origin, Vector3 dir, RaycastHit first,
            Transform container)
        {
            return PlaneAt(origin, dir, first.distance + 0.6f, first.distance, 0.35f,
                first.normal, 25f, container);
        }

        /// Every corner (and the middle) of the print's rectangle must land
        /// on the same plane - cast back at the wall from 0.4 m out.
        static string coverWhy = "";

        static bool Covers(Vector3 centre, Vector3 n, float w, float h, Transform container)
        {
            Vector3 right = Vector3.Cross(Vector3.up, n).normalized;
            Vector3 up = Vector3.Cross(n, right).normalized;
            float dx = Mathf.Max(0f, w * 0.5f - 0.04f);
            float dy = Mathf.Max(0f, h * 0.5f - 0.04f);
            float[] sx = { 0f, -1f, 1f, -1f, 1f };
            float[] sy = { 0f, -1f, -1f, 1f, 1f };
            for (int i = 0; i < sx.Length; i++)
            {
                Vector3 o = centre + right * (sx[i] * dx) + up * (sy[i] * dy) + n * 0.4f;
                if (!PlaneAt(o, -n, 0.9f, 0.4f, 0.12f, n, 30f, container))
                {
                    coverWhy = "corner " + i + " " + probeWhy;
                    return false;
                }
            }
            return true;
        }

        /// Floor height under a point next to the wall (the poster hangs a
        /// fixed height above the floor a viewer stands on, not above the
        /// ray that found the wall).
        static float FloorUnder(Vector3 p, Transform container, float fallback)
        {
            RaycastHit hit;
            if (Solid(p + Vector3.up * 2.5f, Vector3.down, 8f, container, out hit))
                return hit.point.y;
            return fallback;
        }

        static string Bearing(float yaw)
        {
            string[] names = { "+Z", "+X/+Z", "+X", "+X/-Z", "-Z", "-X/-Z", "-X", "-X/+Z" };
            int i = Mathf.RoundToInt(((yaw % 360f) + 360f) % 360f / 45f) % 8;
            return names[i];
        }

        // --- Assets -------------------------------------------------------

        /// The poster image, with import settings a photographic print
        /// wants: sRGB, mip maps (the poster is read from across a room),
        /// clamped, 2048 max. Reimported only when something differs.
        static Texture2D LoadImage(string path)
        {
            if (string.IsNullOrEmpty(path))
                return null;
            var tex = AssetDatabase.LoadAssetAtPath<Texture2D>(path);
            if (tex == null && File.Exists(path))
            {
                AssetDatabase.ImportAsset(path, ImportAssetOptions.ForceSynchronousImport);
                tex = AssetDatabase.LoadAssetAtPath<Texture2D>(path);
            }
            if (tex == null)
                return null;
            var imp = AssetImporter.GetAtPath(path) as TextureImporter;
            if (imp != null)
            {
                bool dirty = false;
                if (imp.textureType != TextureImporterType.Default)
                {
                    imp.textureType = TextureImporterType.Default;
                    dirty = true;
                }
                if (!imp.sRGBTexture) { imp.sRGBTexture = true; dirty = true; }
                // The default rescales a non-power-of-two image to the
                // nearest POT - 832 x 1248 becomes 1024 x 1024 and the
                // print comes out SQUARE, aspect and all.
                if (imp.npotScale != TextureImporterNPOTScale.None)
                {
                    imp.npotScale = TextureImporterNPOTScale.None;
                    dirty = true;
                }
                if (!imp.mipmapEnabled) { imp.mipmapEnabled = true; dirty = true; }
                if (imp.wrapMode != TextureWrapMode.Clamp)
                {
                    imp.wrapMode = TextureWrapMode.Clamp;
                    dirty = true;
                }
                if (imp.maxTextureSize != 2048) { imp.maxTextureSize = 2048; dirty = true; }
                if (dirty)
                {
                    imp.SaveAndReimport();
                    tex = AssetDatabase.LoadAssetAtPath<Texture2D>(path);
                }
            }
            return tex;
        }

        /// A 1 x 1 quad facing +Z with white vertex colours (see the header
        /// on why neither of those is the built-in quad's).
        static Mesh EnsureQuad(string genDir)
        {
            string path = genDir + "/poster_quad.asset";
            var m = AssetDatabase.LoadAssetAtPath<Mesh>(path);
            if (m != null)
                return m;
            Directory.CreateDirectory(genDir);
            m = new Mesh { name = "poster_quad" };
            // Seen from +Z, +X is to the LEFT, so u runs along -X for the
            // print to read the right way round.
            m.SetVertices(new List<Vector3>
            {
                new Vector3(0.5f, 0.5f, 0f), new Vector3(-0.5f, 0.5f, 0f),
                new Vector3(-0.5f, -0.5f, 0f), new Vector3(0.5f, -0.5f, 0f),
            });
            m.SetNormals(new List<Vector3>
            {
                Vector3.forward, Vector3.forward, Vector3.forward, Vector3.forward,
            });
            m.SetUVs(0, new List<Vector2>
            {
                new Vector2(0f, 1f), new Vector2(1f, 1f),
                new Vector2(1f, 0f), new Vector2(0f, 0f),
            });
            m.SetColors(new List<Color> { Color.white, Color.white, Color.white, Color.white });
            m.SetTriangles(new[] { 0, 1, 2, 0, 2, 3 }, 0);
            m.RecalculateBounds();
            AssetDatabase.CreateAsset(m, path);
            return m;
        }

        static Material EnsurePrintMaterial(string genDir, string name, Texture2D tex)
        {
            string path = genDir + "/" + PREFIX + LegaiaWorldBuilder.Sanitize(name) + ".mat";
            var shader = Shader.Find("Legaia/Lit Vertex Color (Cutout)");
            if (shader == null)
            {
                Debug.LogWarning("[Legaia] poster: the kit's lit shader is missing - " +
                    "the print falls back to Standard (the container's lit conversion " +
                    "will re-shade it).");
                shader = Shader.Find("Standard");
            }
            var m = AssetDatabase.LoadAssetAtPath<Material>(path);
            if (m == null)
            {
                Directory.CreateDirectory(genDir);
                m = new Material(shader);
                AssetDatabase.CreateAsset(m, path);
            }
            if (m.shader != shader)
                m.shader = shader;
            m.mainTexture = tex;
            if (m.HasProperty("_Color"))
                m.SetColor("_Color", Color.white);
            if (m.HasProperty("_Cutoff"))
                m.SetFloat("_Cutoff", 0.1f);
            // A photo with the |N.L| terminator cutting across it reads as
            // a dirty print; the wrap flattens the angular term and leaves
            // the sun colour and the shadow attenuation.
            if (m.HasProperty("_LightWrap"))
                m.SetFloat("_LightWrap", 0.85f);
            EditorUtility.SetDirty(m);
            return m;
        }

        // --- Headless check -------------------------------------------------

        static void Fail(string msg)
        {
            Debug.LogError("[Legaia] SELFTEST FAIL: " + msg);
            if (Application.isBatchMode)
                EditorApplication.Exit(1);
            throw new System.Exception(msg);
        }

        static string Arg(string name, string fallback)
        {
            var args = System.Environment.GetCommandLineArgs();
            for (int i = 0; i + 1 < args.Length; i++)
                if (args[i] == name)
                    return args[i + 1];
            return fallback;
        }

        /// Build the common prefabs on the saved scene's built root and
        /// assert every poster really hangs on a wall: on it (the back is
        /// within 3 cm of a collider), at reading height, facing open
        /// space, near its anchor and clear of the furniture.
        public static void Check()
        {
            string scenePath = Arg("-legaiaScene", "Assets/Scenes/VRCDefaultWorldScene.unity");
            UnityEditor.SceneManagement.EditorSceneManager.OpenScene(
                scenePath, UnityEditor.SceneManagement.OpenSceneMode.Single);

            GameObject spawn = null;
            foreach (var t in Object.FindObjectsOfType<Transform>())
                if (t.name == "LegaiaSpawn")
                {
                    spawn = t.gameObject;
                    break;
                }
            if (spawn == null)
                Fail("no LegaiaSpawn in " + scenePath + " - build the scene first");
            string sceneName = spawn.transform.parent != null &&
                               spawn.transform.parent.name.StartsWith("Legaia_")
                ? spawn.transform.parent.name.Substring("Legaia_".Length)
                : "selftest";

            LegaiaWorldBuilder.EnsureUdonProgramAssets();
            var settings = LegaiaSceneSettings.Load(sceneName);
            var o = new LegaiaCommonPrefabOptions
            {
                mirror = true, tv = true, cardTable = true, seats = 4,
                sdkPens = AssetDatabase.LoadAssetAtPath<GameObject>(
                    LegaiaCommonPrefabs.SDK_PEN_PREFAB) != null,
            };
            var container = LegaiaCommonPrefabs.Build(
                "Assets/LegaiaGenerated/" + sceneName, spawn.transform.position, o,
                settings.prefabTransforms, settings.slotMachine, settings.posters);
            if (container == null)
                Fail("no container built");

            if (settings.posters.Count == 0)
                Debug.LogWarning("[Legaia] poster check: " + sceneName +
                    " lists no posters - nothing to assert.");

            var table = container.transform.Find("card_table");
            foreach (var p in settings.posters)
            {
                string key = PREFIX + p.name;
                var poster = container.transform.Find(key);
                if (poster == null)
                    Fail("no " + key + " under " + container.name +
                         " - the wall search found nothing (see the search log above)");
                int siblings = 0;
                foreach (Transform t in container.transform)
                    if (t.name == key)
                        siblings++;
                if (siblings != 1)
                    Fail(siblings + " objects named " + key + " under " + container.name);

                var print = poster.Find("print");
                if (print == null)
                    Fail(key + " has no print quad");
                var mr = print.GetComponent<MeshRenderer>();
                if (mr == null || mr.sharedMaterial == null)
                    Fail(key + "/print has no renderer or material");
                var tex = AssetDatabase.LoadAssetAtPath<Texture2D>(p.image);
                if (mr.sharedMaterial.mainTexture != tex)
                    Fail(key + "/print does not show " + p.image + " (mainTexture is " +
                         (mr.sharedMaterial.mainTexture == null ? "null"
                             : mr.sharedMaterial.mainTexture.name) + ")");

                Vector3 c = print.position;
                Vector3 n = poster.forward;

                // ON a wall: the back of the print is against something.
                RaycastHit back;
                if (!Solid(c, -n, 0.6f, container.transform, out back))
                    Fail(key + " has no wall behind it within 0.6 m");
                if (back.distance > 0.03f)
                    Fail(key + " floats " + back.distance.ToString("0.000") +
                         " m off the nearest surface behind it (max 0.03)");

                // Reading height above the floor under it.
                RaycastHit floor;
                if (!Solid(c + n * 0.3f + Vector3.up * 2.5f, Vector3.down, 8f,
                        container.transform, out floor))
                    Fail(key + " has no floor under it");
                float above = c.y - floor.point.y;
                if (above < 1.2f || above > 1.9f)
                    Fail(key + " hangs " + above.ToString("0.00") +
                         " m above the floor (want 1.2 - 1.9)");

                // Facing open space, not into the wall.
                RaycastHit front;
                bool blocked = Solid(c, n, 1.0f, container.transform, out front);
                if (blocked)
                    Fail(key + " faces " + front.collider.name + " " +
                         front.distance.ToString("0.00") + " m away - it is looking " +
                         "into geometry, not into the room");

                // Near its anchor.
                var anchor = Anchor(container, p.near);
                if (anchor == null)
                    Fail(key + ": no '" + p.near + "' to measure against");
                float span = Vector3.Distance(
                    new Vector3(c.x, 0f, c.z), new Vector3(anchor.position.x, 0f, anchor.position.z));
                if (span > MAX_REACH)
                    Fail(key + " is " + span.ToString("0.00") + " m from " + p.near +
                         " (max " + MAX_REACH + ")");

                // Clear of the furniture it hangs next to.
                var bounds = mr.bounds;
                var frame = poster.Find("frame");
                if (frame != null)
                {
                    var fr = frame.GetComponent<MeshRenderer>();
                    if (fr != null)
                        bounds.Encapsulate(fr.bounds);
                }
                if (table != null)
                    foreach (var r in table.GetComponentsInChildren<MeshRenderer>(true))
                        if (r.bounds.Intersects(bounds))
                            Fail(key + " intersects " + r.name + " on the card table");

                Debug.Log("[Legaia] poster check: " + key + " at " + poster.localPosition +
                    ", normal " + n + ", back gap " + back.distance.ToString("0.000") +
                    " m, " + above.ToString("0.00") + " m above the floor, " +
                    span.ToString("0.00") + " m from " + p.near + ", print " +
                    mr.bounds.size.x.ToString("0.00") + " x " +
                    mr.bounds.size.y.ToString("0.00") + " x " +
                    mr.bounds.size.z.ToString("0.00") + " m, shader " +
                    mr.sharedMaterial.shader.name + ", open space ahead > 1 m.");
            }

            Debug.Log("[Legaia] SELFTEST OK: " + settings.posters.Count +
                " poster(s) hang on real walls in " + sceneName +
                " (scene not saved).");
        }
    }
}
