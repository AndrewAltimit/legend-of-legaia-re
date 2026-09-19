// "Legaia > Snapshot placements to scene settings": the capture half of
// the per-scene tuning loop. Place things by hand in the scene (drag the
// TV, turn the mirror, move the settings panel, move LegaiaSpawn), run
// this, and the current Inspector transforms land in
// Assets/LegaiaWorld/Settings/<scene>.settings.json - so the next
// export + build reproduces the hand placement automatically.
//
// Captured: LegaiaSpawn (spawn_position, root-local as before), every
// direct child of BOTH top-level containers - the common prefabs
// (mirror / tv / card_table / pens / poster_<name> / <prefab name>) and
// the camp props (torch_N / campfire_N / the settings panel as "menu") -
// and the card table's panel, which is a child of the table rather than
// of a container and so needs its own line (card_table_panel,
// card_table_mini_tv).
// Positions are the objects' Inspector values - those containers sit at
// the origin, so local == world - and rotations are Inspector-style
// Euler degrees in (-180, 180]. Other keys in the file are preserved.
//
// The prefab_transforms block MERGES over what the file already holds:
// a key whose object is not in the scene right now (a feature toggled
// off in the builder foldout, a poster not yet built) keeps its stored
// value instead of being dropped. So snapshotting a partially-built
// scene cannot silently lose hand tuning - the cost is that a key for
// something deliberately retired has to be deleted by hand.
//
// "Every direct child" is why nothing here knows what a poster is: a
// wall poster is one more child of that container under its own name
// (LegaiaPosters), so hanging it by hand and snapshotting pins it the
// same way the TV and the mirror are pinned, and the next build stops
// searching for a wall.
//
// Slot machines: every instance of the cabinet asset in the scene is a
// machine - the container's own (`slot_machine`, `slot_machine_2`, ...)
// and any copy dragged in by hand, rig or no rig. One cabinet writes
// the `slot_machine` block as before; more write the `slot_machines`
// list (container ones first in name order, then the strays), and the
// next build places every one of them with a rig of its own.
//
// object_transforms - hand-moved BUILT objects (a world-glb node, a
// villager) - merges the same way: every key already in the file is
// re-read from the object it names, and every world-glb node carrying a
// position / rotation override on the world prefab instance is added
// under its name (moving a node of the world glb by hand is exactly an
// override, so nothing else has to be known about it). A moved villager
// or prop has no such signal - the builder positions it every time - so
// its key is added through "Legaia > Pin selected objects to scene
// settings": select the objects, run it, and they join the block.
//
// The file written is the Unity project's copy; the kit's copy under
// scripts/vrchat-world/world-project/ is the source of truth, so port
// the result back (sync-to-project.ps1 refuses to overwrite a newer
// project-side file until you do).

using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    public static class LegaiaSettingsSnapshot
    {
        [MenuItem("Legaia/Snapshot placements to scene settings")]
        static void SnapshotMenu()
        {
            string written = Snapshot();
            if (written != null)
                EditorUtility.DisplayDialog("Legaia settings snapshot",
                    "Wrote " + written + ".\n\nPort it back into the kit's " +
                    "Settings/ folder so future imports reproduce this placement.",
                    "OK");
        }

        [MenuItem("Legaia/Pin selected objects to scene settings")]
        static void PinSelectedMenu()
        {
            var picked = new List<Transform>();
            foreach (var go in Selection.gameObjects)
                if (go != null)
                    picked.Add(go.transform);
            if (picked.Count == 0)
            {
                EditorUtility.DisplayDialog("Legaia settings snapshot",
                    "Select the built objects to pin (a world mesh, a villager, " +
                    "a prop) and run this again.", "OK");
                return;
            }
            string written = Snapshot(picked);
            if (written != null)
                EditorUtility.DisplayDialog("Legaia settings snapshot",
                    "Pinned " + picked.Count + " object(s) into object_transforms and " +
                    "wrote " + written + ".\n\nPort it back into the kit's " +
                    "Settings/ folder so future imports reproduce this placement.",
                    "OK");
        }

        /// Capture the current placements into the scene's settings file.
        /// `pin` adds these built objects to object_transforms under the
        /// keys ObjectKeyFor derives (the pin menu); null pins nothing new.
        /// Returns the asset path written, or null when there is no built
        /// root to read the scene name from.
        public static string Snapshot(List<Transform> pin = null)
        {
            GameObject root = FindBuiltRoot();
            if (root == null)
            {
                Debug.LogWarning("[Legaia] snapshot: no built Legaia_<scene> root " +
                    "with a LegaiaSpawn in this scene.");
                return null;
            }
            string sceneName = root.name.Substring("Legaia_".Length);
            string path = LegaiaSceneSettings.DIR + "/" + sceneName + ".settings.json";

            Dictionary<string, object> doc = null;
            if (File.Exists(path))
                doc = MiniJson.AsObj(MiniJson.Parse(File.ReadAllText(path)));
            if (doc == null)
                doc = new Dictionary<string, object> { { "scene", sceneName } };

            var spawn = root.transform.Find("LegaiaSpawn");
            doc["spawn_position"] = Vec(spawn.localPosition);

            var common = GameObject.Find(LegaiaCommonPrefabs.CONTAINER);
            var cabinets = SnapshotSlotMachines(doc, common);

            // Seed from what the file already carries so a key this run
            // cannot see survives (see the merge note in the header).
            var transforms = new Dictionary<string, object>();
            var prevTransforms = MiniJson.AsObj(MiniJson.Get(doc, "prefab_transforms"));
            if (prevTransforms != null)
                foreach (var kv in prevTransforms)
                    transforms[kv.Key] = kv.Value;
            int kept = transforms.Count, captured = 0;

            if (common != null)
            {
                foreach (Transform t in common.transform)
                {
                    if (cabinets.Contains(t.gameObject))
                        continue; // carried by the slot machine block above
                    transforms[LegaiaCommonPrefabs.SettingsKey(t.gameObject)] = Entry(t);
                    captured++;
                }
                // These hang off the TABLE, not off the container, so
                // the loop above never reaches them - and each entry is
                // the child's LOCAL transform under the table, which is
                // exactly what Entry() reads and what the builder applies.
                foreach (var child in new[] { "panel", "mini_tv" })
                {
                    var t = common.transform.Find("card_table/" + child);
                    if (t == null)
                        continue;
                    transforms["card_table_" + child] = Entry(t);
                    captured++;
                }
            }
            var camp = GameObject.Find(LegaiaCampProps.CONTAINER);
            if (camp != null)
                foreach (Transform t in camp.transform)
                {
                    transforms[LegaiaCampProps.SettingsKey(t.gameObject)] = Entry(t);
                    captured++;
                }
            // The equipment rack is pinned as a GROUP: its container's own
            // Inspector transform (top-level, so world numbers), which the
            // equipment pass re-applies after placing the rack.
            var rack = GameObject.Find("Legaia_equipment");
            if (rack != null && rack.transform.parent == null)
            {
                transforms["equipment"] = Entry(rack.transform);
                captured++;
            }
            doc["prefab_transforms"] = transforms;
            doc.Remove("prefab_positions"); // superseded by the entries above

            int objects = SnapshotObjectTransforms(doc, root, pin);

            Directory.CreateDirectory(LegaiaSceneSettings.DIR);
            File.WriteAllText(path, Write(doc) + "\n");
            AssetDatabase.ImportAsset(path);
            Debug.Log("[Legaia] snapshot: " + captured + " placement(s) captured, " +
                      (transforms.Count - captured) + " kept from the file" +
                      " (" + kept + " were there)" +
                      (cabinets.Count == 1 ? " + slot machine"
                          : cabinets.Count > 1 ? " + " + cabinets.Count + " slot machines" : "") +
                      (objects > 0 ? " + " + objects + " object transform(s)" : "") +
                      " + spawn -> " + path + " (copy it back into the kit's Settings/ folder).");
            return path;
        }

        /// The built Legaia_<scene> root: top-level, carrying LegaiaSpawn.
        static GameObject FindBuiltRoot()
        {
            foreach (var go in Object.FindObjectsOfType<GameObject>())
                if (go.transform.parent == null && go.name.StartsWith("Legaia_") &&
                    go.transform.Find("LegaiaSpawn") != null)
                    return go;
            return null;
        }

        /// Write the slot machine block(s) from every cabinet instance in
        /// the scene; returns the cabinet roots so the container loop can
        /// skip them. The asset is the file's (first) cabinet path, else
        /// the builder default, else - for a scene that never had one -
        /// whatever asset a LegaiaSlotGame rig was built onto.
        static List<GameObject> SnapshotSlotMachines(Dictionary<string, object> doc,
            GameObject common)
        {
            var prevList = MiniJson.AsList(MiniJson.Get(doc, "slot_machines"));
            var prevSingle = MiniJson.AsObj(MiniJson.Get(doc, "slot_machine"));
            var prevEntries = new List<Dictionary<string, object>>();
            if (prevList != null)
                foreach (object e in prevList)
                    if (MiniJson.AsObj(e) is Dictionary<string, object> d)
                        prevEntries.Add(d);
            if (prevEntries.Count == 0 && prevSingle != null)
                prevEntries.Add(prevSingle);
            var first = prevEntries.Count > 0 ? prevEntries[0] : null;

            string assetPath = MiniJson.AsStr(MiniJson.Get(first, "cabinet"));
            var asset = LegaiaCommonPrefabs.LoadCabinetAsset(assetPath)
                        ?? LegaiaCommonPrefabs.LoadCabinetAsset(
                            new LegaiaCommonPrefabOptions().slotCabinetPath);
            if (asset == null)
            {
                // No known asset: learn it from a rig, wherever it sits.
                foreach (var t in Object.FindObjectsOfType<Transform>(true))
                    if (t.name == "LegaiaSlotGame")
                    {
                        var cab = LegaiaCommonPrefabs.CabinetRootOf(t);
                        var src = cab != null
                            ? PrefabUtility.GetCorrespondingObjectFromOriginalSource(cab) : null;
                        if (src != null)
                        {
                            asset = src;
                            break;
                        }
                    }
            }
            if (asset != null)
                assetPath = AssetDatabase.GetAssetPath(asset);

            // Container cabinets in name order (slot_machine, slot_machine_2,
            // ...) keep their list index; strays follow in scene order.
            var cabinets = new List<GameObject>();
            if (common != null && asset != null)
            {
                var named = new List<KeyValuePair<int, GameObject>>();
                foreach (Transform t in common.transform)
                {
                    int i = LegaiaCommonPrefabs.SlotIndexOf(t.name);
                    if (i >= 0)
                        named.Add(new KeyValuePair<int, GameObject>(i, t.gameObject));
                }
                named.Sort((a, b) => a.Key.CompareTo(b.Key));
                foreach (var kv in named)
                    cabinets.Add(kv.Value);
            }
            cabinets.AddRange(LegaiaCommonPrefabs.StrayCabinets(
                asset, common != null ? common.transform : null));
            if (cabinets.Count == 0)
                return cabinets; // keep whatever the file says

            var entries = new List<object>();
            for (int i = 0; i < cabinets.Count; i++)
            {
                var cab = cabinets[i];
                var prev = i < prevEntries.Count ? prevEntries[i] : first;
                var entry = new Dictionary<string, object>
                {
                    { "position", Vec(cab.transform.localPosition) },
                    { "rotation", Vec(Wrap(cab.transform.localEulerAngles)) },
                    { "scale", (double)Round(cab.transform.localScale.x) },
                };
                // The first entry names the asset and art folder; a later
                // one only when it differs from the first (the reader
                // inherits the rest).
                string art = MiniJson.AsStr(MiniJson.Get(prev, "art"))
                             ?? MiniJson.AsStr(MiniJson.Get(first, "art"))
                             ?? "Assets/LegaiaImports/slot-art";
                string firstArt = MiniJson.AsStr(MiniJson.Get(first, "art"))
                                  ?? "Assets/LegaiaImports/slot-art";
                if (i == 0)
                {
                    var ordered = new Dictionary<string, object>
                    {
                        { "cabinet", assetPath ?? "Assets/Prefabs/" + cab.name + ".glb" },
                        { "art", art },
                    };
                    foreach (var kv in entry)
                        ordered[kv.Key] = kv.Value;
                    entry = ordered;
                }
                else if (art != firstArt)
                {
                    var ordered = new Dictionary<string, object> { { "art", art } };
                    foreach (var kv in entry)
                        ordered[kv.Key] = kv.Value;
                    entry = ordered;
                }
                entries.Add(entry);
            }
            if (entries.Count == 1)
            {
                doc["slot_machine"] = entries[0];
                doc.Remove("slot_machines");
            }
            else
            {
                doc["slot_machines"] = entries;
                doc.Remove("slot_machine");
            }
            return cabinets;
        }

        /// Merge object_transforms: re-capture every stored key from the
        /// object it names, add every world-glb node with a transform
        /// override, add `pin`. Returns how many entries the block holds.
        static int SnapshotObjectTransforms(Dictionary<string, object> doc, GameObject root,
            List<Transform> pin)
        {
            var block = new Dictionary<string, object>();
            var prev = MiniJson.AsObj(MiniJson.Get(doc, "object_transforms"));
            if (prev != null)
                foreach (var kv in prev)
                    block[kv.Key] = kv.Value;

            // Stored keys: re-read the object (a key naming nothing keeps
            // its value, like prefab_transforms).
            foreach (string key in new List<string>(block.Keys))
            {
                var hits = LegaiaSceneSettings.FindObjects(root, key);
                if (hits.Count > 0)
                    block[key] = Entry(hits[0]);
            }

            // World-glb nodes moved by hand: the only transform overrides
            // the world prefab instance can carry are the user's (the
            // builder never touches a node of the world).
            var world = root.transform.Find("world");
            if (world != null && PrefabUtility.IsPartOfPrefabInstance(world.gameObject))
            {
                var mods = PrefabUtility.GetPropertyModifications(world.gameObject);
                var movedSources = new HashSet<Object>();
                if (mods != null)
                    foreach (var mod in mods)
                        if (mod.target is Transform &&
                            (mod.propertyPath.StartsWith("m_LocalPosition") ||
                             mod.propertyPath.StartsWith("m_LocalRotation")))
                            movedSources.Add(mod.target);
                foreach (var t in world.GetComponentsInChildren<Transform>(true))
                {
                    if (t == world)
                        continue; // the instance root: the builder parents it
                    var src = PrefabUtility.GetCorrespondingObjectFromSource(t);
                    if (src == null || !movedSources.Contains(src))
                        continue;
                    block[LegaiaSceneSettings.ObjectKeyFor(root, t)] = Entry(t);
                }
            }

            if (pin != null)
                foreach (var t in pin)
                {
                    if (t == null || t == root.transform)
                        continue;
                    string key = LegaiaSceneSettings.ObjectKeyFor(root, t);
                    var hits = LegaiaSceneSettings.FindObjects(root, key);
                    if (hits.Count == 0 || hits[0] != t)
                    {
                        Debug.LogWarning("[Legaia] snapshot: '" + LegaiaSceneSettings.ScenePath(t) +
                            "' is not under the built root or a kit container (or its " +
                            "name is not unique enough) - not pinned.");
                        continue;
                    }
                    block[key] = Entry(t);
                }

            if (block.Count > 0)
                doc["object_transforms"] = block;
            return block.Count;
        }

        static Dictionary<string, object> Entry(Transform t)
        {
            return new Dictionary<string, object>
            {
                { "position", Vec(t.localPosition) },
                { "rotation", Vec(Wrap(t.localEulerAngles)) },
            };
        }

        /// Inspector-style angles: (-180, 180] instead of [0, 360).
        static Vector3 Wrap(Vector3 e)
        {
            for (int i = 0; i < 3; i++)
            {
                float v = e[i] % 360f;
                if (v > 180f) v -= 360f;
                if (v <= -180f) v += 360f;
                e[i] = v;
            }
            return e;
        }

        static List<object> Vec(Vector3 v)
        {
            return new List<object>
            {
                (double)Round(v.x), (double)Round(v.y), (double)Round(v.z),
            };
        }

        static float Round(float v) => Mathf.Round(v * 1000f) / 1000f;

        // --- Writer (the reader's mirror image: objects / lists / strings /
        // numbers / bools / null, two-space indent, one vector per line) ---

        static string Write(object v)
        {
            var sb = new StringBuilder();
            WriteValue(sb, v, 0);
            return sb.ToString();
        }

        static void WriteValue(StringBuilder sb, object v, int depth)
        {
            switch (v)
            {
                case null: sb.Append("null"); break;
                case bool b: sb.Append(b ? "true" : "false"); break;
                case string s: WriteString(sb, s); break;
                case double d: sb.Append(d.ToString("0.###", CultureInfo.InvariantCulture)); break;
                case float f: sb.Append(f.ToString("0.###", CultureInfo.InvariantCulture)); break;
                case int i: sb.Append(i.ToString(CultureInfo.InvariantCulture)); break;
                case Dictionary<string, object> o:
                {
                    sb.Append("{\n");
                    int n = 0;
                    foreach (var kv in o)
                    {
                        sb.Append(' ', (depth + 1) * 2);
                        WriteString(sb, kv.Key);
                        sb.Append(": ");
                        WriteValue(sb, kv.Value, depth + 1);
                        sb.Append(++n < o.Count ? ",\n" : "\n");
                    }
                    sb.Append(' ', depth * 2).Append('}');
                    break;
                }
                case List<object> l:
                {
                    // Scalar lists (vectors, name lists) stay on one line.
                    bool flat = l.TrueForAll(x => !(x is Dictionary<string, object>) &&
                                                  !(x is List<object>));
                    sb.Append('[');
                    for (int i = 0; i < l.Count; i++)
                    {
                        if (!flat)
                            sb.Append('\n').Append(' ', (depth + 1) * 2);
                        else if (i > 0)
                            sb.Append(' ');
                        WriteValue(sb, l[i], depth + 1);
                        if (i + 1 < l.Count)
                            sb.Append(',');
                    }
                    if (!flat)
                        sb.Append('\n').Append(' ', depth * 2);
                    sb.Append(']');
                    break;
                }
                default: WriteString(sb, v.ToString()); break;
            }
        }

        static void WriteString(StringBuilder sb, string s)
        {
            sb.Append('"');
            foreach (char c in s)
            {
                if (c == '"' || c == '\\') sb.Append('\\').Append(c);
                else if (c == '\n') sb.Append("\\n");
                else if (c < ' ') sb.Append("\\u").Append(((int)c).ToString("x4"));
                else sb.Append(c);
            }
            sb.Append('"');
        }
    }
}
