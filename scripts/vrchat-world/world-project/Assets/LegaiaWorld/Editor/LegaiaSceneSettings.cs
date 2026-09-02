// Per-scene refinement settings for the Legaia world builder.
//
// A scene often needs a few hand-tuned corrections on top of the generic
// build - a generated interior shell that swallows the wrong hut, a
// villager who wanders into a doorway, an NPC that reads wrong in VR, a
// spawn point moved to a nicer vantage. Those choices are per-scene and
// must survive re-exports and rebuilds, so they live in a small JSON file
// shipped WITH THE KIT (not with the exported data, which is regenerated):
//
//   Assets/LegaiaWorld/Settings/<scene>.settings.json
//
// Recognised keys (all optional):
//
//   "delete_objects":  ["room_6_shell", "prop_53_anim8/object_1"]
//       Exact GameObject names to remove after the build + realism passes.
//       A name with '/' is a path SUFFIX: the last segment names the
//       object and each earlier segment must match the next parent up
//       ("prop_53_anim8/object_1" hits only the object_1 under a
//       prop_53_anim8, not every glb's object_1). Searched under the
//       built root (children of Legaia_<scene>, inactive included) and
//       the kit's top-level containers (Legaia_camp_props,
//       Legaia_night_torches, Legaia_equipment). Generated objects are
//       destroyed (they come back next build for a scene whose settings
//       drop the name); prefab-instance children (world/prop glb nodes)
//       are disabled instead, since Unity forbids deleting them without
//       unpacking.
//
//   "static_npcs":     [26, 27, "npc_45"]
//       NPCs that keep their looping idle clip but never travel: the
//       realism layer's wander pass skips them. A number N matches the
//       exported file stem npc_<NN>_...; a string matches any part of the
//       file name.
//
//   "remove_npcs":     [28, 10, 11]
//       NPCs not placed at all (same matching rules).
//
//   "freeze_npcs":     [47]
//       NPCs placed with NO animation clip - they hold their rest pose.
//       For prop-kind actors (trees, signs) whose bundle slot carries a
//       generic locomotion record: looping it walks the prop in place.
//
//   "spawn_position":  [-24.85, 1.75, 12.22]
//       Overrides the manifest's suggested spawn: EXACTLY the value the
//       Inspector shows on LegaiaSpawn (its local position under the
//       built root). To tune it, drag LegaiaSpawn where you want it and
//       copy its Inspector position here - the rebuild reproduces it
//       digit for digit. (Not raw world space: the root is X-mirrored,
//       so a world value would come back sign-flipped in the Inspector -
//       the trap this convention exists to avoid.)
//
//   "set_descriptor_spawn": true
//       Point the VRC Scene Descriptor's Spawns[0] at the LegaiaSpawn
//       marker after the build. Defaults to TRUE (with or without a
//       settings file) - it only acts when a descriptor exists in the
//       scene, so a bare Unity project is unaffected.

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    public class LegaiaSceneSettings
    {
        public const string DIR = "Assets/LegaiaWorld/Settings";

        public List<string> deleteObjects = new List<string>();
        public List<string> staticNpcs = new List<string>();
        public List<string> removeNpcs = new List<string>();
        public List<string> freezeNpcs = new List<string>();
        public bool hasSpawn;
        /// LegaiaSpawn's root-local position - what its Inspector shows.
        public Vector3 spawnLocal;
        public bool setDescriptorSpawn = true;
        /// Asset path the settings were read from; null = no file (every
        /// list empty, defaults only).
        public string path;

        public static LegaiaSceneSettings Load(string sceneName)
        {
            var s = new LegaiaSceneSettings();
            string p = DIR + "/" + sceneName + ".settings.json";
            if (!File.Exists(p))
                return s;
            s.path = p;
            object m = MiniJson.Parse(File.ReadAllText(p));
            ReadTokens(MiniJson.Get(m, "delete_objects"), s.deleteObjects);
            ReadTokens(MiniJson.Get(m, "static_npcs"), s.staticNpcs);
            ReadTokens(MiniJson.Get(m, "remove_npcs"), s.removeNpcs);
            ReadTokens(MiniJson.Get(m, "freeze_npcs"), s.freezeNpcs);
            var sp = MiniJson.AsList(MiniJson.Get(m, "spawn_position"))
                ?? MiniJson.AsList(MiniJson.Get(m, "spawn_world")); // old key
            if (sp != null && sp.Count >= 3)
            {
                s.hasSpawn = true;
                s.spawnLocal = new Vector3(
                    (float)MiniJson.AsNum(sp[0]),
                    (float)MiniJson.AsNum(sp[1]),
                    (float)MiniJson.AsNum(sp[2]));
            }
            if (MiniJson.Get(m, "set_descriptor_spawn") is bool b)
                s.setDescriptorSpawn = b;
            Debug.Log("[Legaia] scene settings " + p + ": " +
                s.deleteObjects.Count + " deletion(s), " +
                s.staticNpcs.Count + " static NPC rule(s), " +
                s.removeNpcs.Count + " removed NPC rule(s), " +
                s.freezeNpcs.Count + " frozen NPC rule(s)" +
                (s.hasSpawn ? ", spawn override " + s.spawnLocal : "") + ".");
            return s;
        }

        /// JSON list entries may be numbers (NPC indices) or strings
        /// (name fragments / exact object names); numbers are kept as
        /// their integer text so NpcMatch can re-read them.
        static void ReadTokens(object list, List<string> into)
        {
            foreach (object o in MiniJson.AsList(list) ?? new List<object>())
            {
                string str = MiniJson.AsStr(o);
                if (str != null)
                {
                    if (str.Trim().Length > 0)
                        into.Add(str.Trim());
                }
                else if (o is double d)
                {
                    into.Add(((int)d).ToString());
                }
            }
        }

        public bool NpcIsStatic(string file) => NpcMatch(staticNpcs, file);
        public bool NpcIsRemoved(string file) => NpcMatch(removeNpcs, file);
        public bool NpcIsFrozen(string file) => NpcMatch(freezeNpcs, file);

        /// A purely numeric token N matches the exported stem
        /// npc_<NN>_... exactly (zero-padded to two digits, underscore
        /// required so 1 never matches npc_10); any other token matches
        /// as a substring of the file name.
        static bool NpcMatch(List<string> tokens, string file)
        {
            if (string.IsNullOrEmpty(file))
                return false;
            string stem = Path.GetFileNameWithoutExtension(file);
            foreach (string t in tokens)
            {
                if (int.TryParse(t, out int id))
                {
                    string key = "npc_" + id.ToString("00");
                    if (stem == key || stem.StartsWith(key + "_"))
                        return true;
                }
                else if (file.Contains(t))
                {
                    return true;
                }
            }
            return false;
        }

        /// Remove every object whose name is listed in delete_objects.
        /// Runs after the realism passes (which generate the interior
        /// shells / lamps the names usually refer to), over the built
        /// root plus the kit's top-level containers. Generated objects
        /// are destroyed; prefab-instance children (world glb nodes) are
        /// disabled, since destroying them would require unpacking the
        /// prefab instance.
        public void ApplyDeletions(GameObject root)
        {
            if (deleteObjects.Count == 0 || root == null)
                return;
            var targets = new List<GameObject>();
            var scopes = new List<GameObject> { root };
            foreach (string top in new[]
                     { "Legaia_camp_props", "Legaia_night_torches", "Legaia_equipment" })
            {
                var go = GameObject.Find(top);
                if (go != null)
                    scopes.Add(go);
            }
            var matched = new HashSet<string>();
            foreach (var scope in scopes)
                foreach (var t in scope.GetComponentsInChildren<Transform>(true))
                {
                    if (t == null || t.gameObject == scope)
                        continue;
                    foreach (string token in deleteObjects)
                        if (MatchesPath(t, token))
                        {
                            targets.Add(t.gameObject);
                            matched.Add(token);
                            break;
                        }
                }
            int removed = 0, hidden = 0;
            foreach (var go in targets)
            {
                if (go == null)
                    continue; // parent already destroyed this pass
                if (PrefabUtility.IsPartOfPrefabInstance(go))
                {
                    Undo.RecordObject(go, "Legaia scene settings");
                    go.SetActive(false);
                    hidden++;
                }
                else
                {
                    Undo.DestroyObjectImmediate(go);
                    removed++;
                }
            }
            Debug.Log("[Legaia] scene settings deletions: " + removed +
                " destroyed, " + hidden + " disabled (prefab children).");
            foreach (string n in deleteObjects)
                if (!matched.Contains(n))
                    Debug.LogWarning("[Legaia] delete_objects name not found " +
                        "in the built hierarchy: " + n);
        }

        /// True when `token` names this transform: a bare name is an
        /// exact match on t.name; a '/'-joined token additionally
        /// requires each earlier segment to match the next parent up
        /// (a path SUFFIX - the chain may sit anywhere in the scene).
        static bool MatchesPath(Transform t, string token)
        {
            string[] segs = token.Split('/');
            if (segs.Length == 0 || t.name != segs[segs.Length - 1])
                return false;
            Transform cur = t.parent;
            for (int i = segs.Length - 2; i >= 0; i--)
            {
                if (cur == null || cur.name != segs[i])
                    return false;
                cur = cur.parent;
            }
            return true;
        }

        /// Point the VRC Scene Descriptor's Spawns[0] at `spawn` (the
        /// LegaiaSpawn marker), via reflection so this file compiles
        /// without the VRChat SDK. No-op with a warning when no
        /// descriptor exists in the open scene.
        public static void AssignDescriptorSpawn(Transform spawn)
        {
            var t = LegaiaWorldBuilder.FindType("VRC.SDKBase.VRC_SceneDescriptor");
            if (t == null)
                return; // no VRChat SDK in this project
            var desc = Object.FindObjectOfType(t, true) as Component;
            if (desc == null)
            {
                Debug.LogWarning("[Legaia] no VRC Scene Descriptor in the scene - " +
                    "add the VRCWorld prefab, then rebuild (or set Spawns[0] to " +
                    "LegaiaSpawn by hand).");
                return;
            }
            var f = t.GetField("spawns");
            if (f == null)
            {
                Debug.LogWarning("[Legaia] VRC_SceneDescriptor has no `spawns` " +
                    "field (SDK layout changed?) - set Spawns[0] by hand.");
                return;
            }
            var arr = f.GetValue(desc) as Transform[];
            if (arr == null || arr.Length == 0)
                arr = new Transform[1];
            arr[0] = spawn;
            f.SetValue(desc, arr);
            EditorUtility.SetDirty(desc);
            Debug.Log("[Legaia] VRC Scene Descriptor Spawns[0] -> " +
                spawn.name + " at " + spawn.position + ".");
        }
    }
}
