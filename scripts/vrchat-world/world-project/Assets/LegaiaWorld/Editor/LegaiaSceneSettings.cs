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
//       Legaia_night_torches, Legaia_equipment, Legaia_common_prefabs). Generated objects are
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
//
//   "prefab_transforms": {"tv": {"position": [x, y, z], "rotation": [0, yaw, 0]}, ...}
//       Absolute placement for the builder's common prefabs and the camp
//       settings panel (keys: mirror, tv, card_table, pens, the prefab
//       name of an extra slot, menu), replacing the spawn-relative
//       offsets and the face-the-spawn rotation. These objects live
//       under top-level containers at the origin, so the values are
//       EXACTLY the object's Inspector position and rotation (world ==
//       local there - no mirror to trip over). "rotation" is optional.
//       "Legaia > Snapshot placements to scene settings" writes this
//       block (and spawn_position) from the current scene, so the loop
//       is: place by hand, snapshot, port the file back into the kit.
//       The older "prefab_positions": {"tv": [x, y, z]} form still reads.
//
//   "living_town": {"home_cap": 4, "chat_spots": 5, "seed": 20260907,
//                   "daytime_indoors_share": 0.18, "walk_clip": "record_36"}
//       Living-town tuning, overriding the builder foldout's values so a
//       scene keeps them across rebuilds: villagers per house door, how
//       many conversation spots to build, the scene-constant seed that
//       fixes home assignment and personalities, the share of villagers
//       who stay indoors by day, and the clip name bound as the walk
//       cycle. Every key optional.
//
//   "npc_homes": {"npc_12": 2, "grandmother": 0}
//       Pin individual villagers to a house door by index into the homes
//       the pass derives from the manifest's teleports (the same order
//       the `living_town/homes/home_N` markers are numbered in). Matching
//       follows the static_npcs rules: a number token matches npc_<NN>,
//       any other token is a file-name substring. Everyone else is spread
//       by the seeded shuffle.
//
//   "ambience": {"day": "Assets/LegaiaWorld/Audio/day.wav", "night": ...}
//       User-supplied ambience loops, replacing the synthesized ones for
//       the roles named. Keys (all optional): day, night, base, waves,
//       wind_gust, tree_birds, night_wildlife, windmill. Each
//       value is the asset path of any AudioClip in the project; a role
//       with no entry (or an entry that does not load) keeps the clip
//       LegaiaAudioGen generates. Drop better field recordings in and the
//       ambience layer uses them with no code change.
//
//   "slot_machine": {"cabinet": "Assets/Prefabs/legaia slot machine.glb",
//                    "art": "Assets/LegaiaImports/slot-art",
//                    "position": [x, y, z], "rotation": [0, yaw, 0], "scale": 0.012}
//       The casino cabinet: which model asset to instantiate (falls back
//       to a project-wide search by file name when the path moved), the
//       `asset slot-art` folder to build the minigame from, and the
//       cabinet's Inspector placement + uniform scale. With this block the
//       common-prefabs pass places the cabinet and runs the slot-machine
//       builder on it, so the minigame comes back on every rebuild. Also
//       written by the snapshot menu (it finds the cabinet through its
//       LegaiaSlotGame rig, wherever it sits).

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    /// One prefab_transforms entry: Inspector position + optional
    /// Inspector Euler rotation (degrees).
    public struct LegaiaPrefabTransform
    {
        public Vector3 position;
        public bool hasRotation;
        public Vector3 rotation;
    }

    /// The slot_machine block: cabinet asset, art folder, placement.
    public class LegaiaSlotPlacement
    {
        public string cabinetAsset;
        public string artDir;
        public bool hasPosition;
        public Vector3 position;
        public bool hasRotation;
        public Vector3 rotation;
        public bool hasScale;
        public float scale = 1f;
    }

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
        /// Absolute placements for the common-prefab items + the camp
        /// menu, by key (see the header).
        public Dictionary<string, LegaiaPrefabTransform> prefabTransforms =
            new Dictionary<string, LegaiaPrefabTransform>();
        /// The slot_machine block, or null when the file has none.
        public LegaiaSlotPlacement slotMachine;
        /// The "ambience" block: role -> AudioClip asset path (see the
        /// header). Empty when the file has none, and the ambience pass
        /// then generates every role.
        public Dictionary<string, string> ambienceClips =
            new Dictionary<string, string>();
        /// living_town overrides; negative / null = keep the option value.
        public int livingTownHomeCap = -1;
        public int livingTownChatSpots = -1;
        public int livingTownSeed;
        public float livingTownDaytimeIndoors = -1f;
        public string livingTownWalkClip;
        /// npc_homes: villager token -> home index.
        public Dictionary<string, int> npcHomes = new Dictionary<string, int>();
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
            var pp = MiniJson.AsObj(MiniJson.Get(m, "prefab_positions"));
            if (pp != null)
                foreach (var kv in pp)
                {
                    var l = MiniJson.AsList(kv.Value);
                    if (l != null && l.Count >= 3)
                        s.prefabTransforms[kv.Key] = new LegaiaPrefabTransform
                        {
                            position = ReadVec(l),
                        };
                }
            var sm = MiniJson.AsObj(MiniJson.Get(m, "slot_machine"));
            if (sm != null)
            {
                var slot = new LegaiaSlotPlacement
                {
                    cabinetAsset = MiniJson.AsStr(MiniJson.Get(sm, "cabinet")),
                    artDir = MiniJson.AsStr(MiniJson.Get(sm, "art")),
                };
                var slotPos = MiniJson.AsList(MiniJson.Get(sm, "position"));
                if (slotPos != null && slotPos.Count >= 3)
                {
                    slot.hasPosition = true;
                    slot.position = ReadVec(slotPos);
                }
                var slotRot = MiniJson.AsList(MiniJson.Get(sm, "rotation"));
                if (slotRot != null && slotRot.Count >= 3)
                {
                    slot.hasRotation = true;
                    slot.rotation = ReadVec(slotRot);
                }
                if (MiniJson.Get(sm, "scale") is double sc)
                {
                    slot.hasScale = true;
                    slot.scale = (float)sc;
                }
                s.slotMachine = slot;
            }
            var lt = MiniJson.AsObj(MiniJson.Get(m, "living_town"));
            if (lt != null)
            {
                if (MiniJson.Get(lt, "home_cap") is double hc)
                    s.livingTownHomeCap = (int)hc;
                if (MiniJson.Get(lt, "chat_spots") is double cs)
                    s.livingTownChatSpots = (int)cs;
                if (MiniJson.Get(lt, "seed") is double sd)
                    s.livingTownSeed = (int)sd;
                if (MiniJson.Get(lt, "daytime_indoors_share") is double di)
                    s.livingTownDaytimeIndoors = (float)di;
                s.livingTownWalkClip = MiniJson.AsStr(MiniJson.Get(lt, "walk_clip"));
            }
            var nh = MiniJson.AsObj(MiniJson.Get(m, "npc_homes"));
            if (nh != null)
                foreach (var kv in nh)
                    if (kv.Value is double hi)
                        s.npcHomes[kv.Key] = (int)hi;
            var ab = MiniJson.AsObj(MiniJson.Get(m, "ambience"));
            if (ab != null)
                foreach (var kv in ab)
                {
                    string clipPath = MiniJson.AsStr(kv.Value);
                    if (!string.IsNullOrEmpty(clipPath))
                        s.ambienceClips[kv.Key.Trim()] = clipPath.Trim();
                }
            var pt = MiniJson.AsObj(MiniJson.Get(m, "prefab_transforms"));
            if (pt != null)
                foreach (var kv in pt)
                {
                    var pos = MiniJson.AsList(MiniJson.Get(kv.Value, "position"));
                    if (pos == null || pos.Count < 3)
                        continue;
                    var rot = MiniJson.AsList(MiniJson.Get(kv.Value, "rotation"));
                    s.prefabTransforms[kv.Key] = new LegaiaPrefabTransform
                    {
                        position = ReadVec(pos),
                        hasRotation = rot != null && rot.Count >= 3,
                        rotation = rot != null && rot.Count >= 3 ? ReadVec(rot) : Vector3.zero,
                    };
                }
            Debug.Log("[Legaia] scene settings " + p + ": " +
                s.deleteObjects.Count + " deletion(s), " +
                s.staticNpcs.Count + " static NPC rule(s), " +
                s.removeNpcs.Count + " removed NPC rule(s), " +
                s.freezeNpcs.Count + " frozen NPC rule(s)" +
                (s.hasSpawn ? ", spawn override " + s.spawnLocal : "") +
                (s.prefabTransforms.Count > 0
                    ? ", " + s.prefabTransforms.Count + " placement(s)" : "") +
                (s.slotMachine != null ? ", slot machine" : "") +
                (s.ambienceClips.Count > 0
                    ? ", " + s.ambienceClips.Count + " ambience override(s)" : "") + ".");
            return s;
        }

        static Vector3 ReadVec(List<object> l)
        {
            return new Vector3(
                (float)MiniJson.AsNum(l[0]),
                (float)MiniJson.AsNum(l[1]),
                (float)MiniJson.AsNum(l[2]));
        }

        /// Apply a placement override to a built object: position always,
        /// rotation when the entry carries one. Returns false when there
        /// is no entry for `key` (the object keeps its computed placement).
        public static bool ApplyPlacement(Dictionary<string, LegaiaPrefabTransform> t,
            string key, Transform target)
        {
            LegaiaPrefabTransform p;
            if (t == null || !t.TryGetValue(key, out p))
                return false;
            target.localPosition = p.position;
            if (p.hasRotation)
                target.localRotation = Quaternion.Euler(p.rotation);
            return true;
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

        /// The home index pinned for this villager in `npc_homes`, or -1.
        /// Key matching follows the static_npcs rules.
        public int HomeOverride(string file)
        {
            foreach (var kv in npcHomes)
            {
                var one = new List<string> { kv.Key };
                if (NpcMatch(one, file))
                    return kv.Value;
            }
            return -1;
        }

        /// Fold the file's living_town block over the foldout's options.
        public void ApplyLivingTown(LegaiaLivingTownOptions o)
        {
            if (o == null)
                return;
            if (livingTownHomeCap >= 0)
                o.homeCap = livingTownHomeCap;
            if (livingTownChatSpots >= 0)
                o.maxChatSpots = livingTownChatSpots;
            if (livingTownSeed != 0)
                o.seed = livingTownSeed;
            if (livingTownDaytimeIndoors >= 0f)
                o.daytimeIndoorsShare = livingTownDaytimeIndoors;
            if (!string.IsNullOrEmpty(livingTownWalkClip))
                o.walkClip = livingTownWalkClip;
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
                     { "Legaia_camp_props", "Legaia_night_torches", "Legaia_equipment",
                       LegaiaCommonPrefabs.CONTAINER })
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
