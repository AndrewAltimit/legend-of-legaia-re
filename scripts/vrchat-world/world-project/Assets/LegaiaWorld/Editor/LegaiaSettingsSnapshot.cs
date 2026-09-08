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

        /// Capture the current placements into the scene's settings file.
        /// Returns the asset path written, or null when there is no built
        /// root to read the scene name from.
        public static string Snapshot()
        {
            GameObject root = null;
            foreach (var go in Object.FindObjectsOfType<GameObject>())
                if (go.transform.parent == null && go.name.StartsWith("Legaia_") &&
                    go.transform.Find("LegaiaSpawn") != null)
                {
                    root = go;
                    break;
                }
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

            // The slot cabinet is found through its rig wherever it sits
            // (top-level from the hand workflow, or under the container).
            GameObject cabinet = null;
            foreach (var t in Object.FindObjectsOfType<Transform>(true))
                if (t.name == "LegaiaSlotGame")
                {
                    cabinet = LegaiaCommonPrefabs.CabinetRootOf(t);
                    break;
                }
            if (cabinet != null)
            {
                var prev = MiniJson.AsObj(MiniJson.Get(doc, "slot_machine"));
                var src = PrefabUtility.GetCorrespondingObjectFromOriginalSource(cabinet);
                string assetPath = src != null ? AssetDatabase.GetAssetPath(src) : null;
                var entry = new Dictionary<string, object>
                {
                    { "cabinet", assetPath ?? MiniJson.AsStr(MiniJson.Get(prev, "cabinet"))
                                 ?? "Assets/Prefabs/" + cabinet.name + ".glb" },
                    { "art", MiniJson.AsStr(MiniJson.Get(prev, "art"))
                             ?? "Assets/LegaiaImports/slot-art" },
                    { "position", Vec(cabinet.transform.localPosition) },
                    { "rotation", Vec(Wrap(cabinet.transform.localEulerAngles)) },
                    { "scale", (double)Round(cabinet.transform.localScale.x) },
                };
                doc["slot_machine"] = entry;
            }

            // Seed from what the file already carries so a key this run
            // cannot see survives (see the merge note in the header).
            var transforms = new Dictionary<string, object>();
            var prevTransforms = MiniJson.AsObj(MiniJson.Get(doc, "prefab_transforms"));
            if (prevTransforms != null)
                foreach (var kv in prevTransforms)
                    transforms[kv.Key] = kv.Value;
            int kept = transforms.Count, captured = 0;

            var common = GameObject.Find(LegaiaCommonPrefabs.CONTAINER);
            if (common != null)
            {
                foreach (Transform t in common.transform)
                {
                    if (t.gameObject == cabinet)
                        continue; // carried by the slot_machine block above
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
            doc["prefab_transforms"] = transforms;
            doc.Remove("prefab_positions"); // superseded by the entries above

            Directory.CreateDirectory(LegaiaSceneSettings.DIR);
            File.WriteAllText(path, Write(doc) + "\n");
            AssetDatabase.ImportAsset(path);
            Debug.Log("[Legaia] snapshot: " + captured + " placement(s) captured, " +
                      (transforms.Count - captured) + " kept from the file" +
                      " (" + kept + " were there)" +
                      (cabinet != null ? " + slot machine" : "") + " + spawn -> " +
                      path + " (copy it back into the kit's Settings/ folder).");
            return path;
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
