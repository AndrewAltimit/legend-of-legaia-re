// What the TV plays, and who gets to change it: the parser for the kit's
// SHARED video config, Settings/video.settings.json.
//
// This one file is deliberately not per-scene. A playlist and a cast of
// favourite shows are a property of the WORLD KIT, not of Rim Elm - the
// same six-video loop and the same "Cara puts her Xbox advert on" belong
// in every town built with this kit. A scene that wants something else
// carries a "video" block in its own settings file, which overrides this
// one (see Merge below), so the per-scene escape hatch exists without
// every scene having to restate the default.
//
// Shape:
//
//   "default_playlist": [ {"title": ..., "url": ...}, ... ]
//   "shows": [ {"who": "Cara", "npc": 14, "title": ...,
//               "urls": [...], "console": false}, ... ]
//
// A SHOW is one villager's pick. `who` is the villager's display name -
// what LegaiaLivingTown.VillagerName settled on and what the brain
// carries as `label` - and `npc` is the manifest NPC number as a
// fallback for scenes that never pinned a name (14 -> the token
// "npc_14", 7 -> "npc_07", matching LegaiaSceneSettings.NpcMatch). The
// runtime matches on either, so a show follows the CHARACTER across
// worlds rather than an object path.
//
// One villager may own several shows (Noa has two); the TV round-robins
// between them so a second visit is not a repeat. `urls` with more than
// one entry is a sequence played in order, once, never looped - the
// Spyro run. `console` marks a show that comes with the floor console
// and a controller in the owner's hands.
//
// Merge, when a scene settings file carries a "video" block:
//   default_playlist  - REPLACES the shared list when present
//   shows             - APPENDED to the shared shows, so a town can give
//                       a local resident a pick without restating the
//                       cast; "shows_replace": true drops the shared
//                       ones instead
//
// Nothing here is Sony-derived: these are third-party video links the
// world author chose, stored as plain text.

using System.Collections.Generic;
using System.IO;
using UnityEditor;
using UnityEngine;

namespace LegaiaWorld
{
    /// One entry of the default playlist.
    internal class LegaiaVideoEntry
    {
        public string title = "";
        public string url = "";
    }

    /// One villager's show: a title, one or more URLs played in order,
    /// and the owner's identity in the two forms the runtime can match.
    internal class LegaiaVideoShow
    {
        public string who = "";        // display name ("Noa")
        public string token = "";      // NPC token ("npc_102"), may be empty
        public string title = "";
        public bool console;           // floor console + controller in hand
        public List<string> urls = new List<string>();
    }

    internal class LegaiaVideoSettings
    {
        public const string PATH = LegaiaSceneSettings.DIR + "/video.settings.json";

        public List<LegaiaVideoEntry> defaultPlaylist = new List<LegaiaVideoEntry>();
        public List<LegaiaVideoShow> shows = new List<LegaiaVideoShow>();

        /// True when the shared file was found at all - the builder says so
        /// once, because a missing file means a TV with no playlist and
        /// that is otherwise indistinguishable from an empty one.
        public bool loaded;

        /// The shared config with the scene's "video" block merged over it.
        /// `sceneName` may be null for the shared file alone.
        public static LegaiaVideoSettings Load(string sceneName)
        {
            var s = new LegaiaVideoSettings();
            var shared = ReadJson(PATH);
            if (shared != null)
            {
                s.loaded = true;
                s.ReadInto(shared, false);
            }
            else
            {
                Debug.LogWarning("[Legaia] video config " + PATH + " not found - " +
                    "the TV builds with no default playlist.");
            }
            if (string.IsNullOrEmpty(sceneName))
                return s;

            var scene = ReadJson(LegaiaSceneSettings.DIR + "/" + sceneName + ".settings.json");
            var block = MiniJson.AsObj(MiniJson.Get(scene, "video"));
            if (block != null)
                s.ReadInto(block, true);
            return s;
        }

        static Dictionary<string, object> ReadJson(string path)
        {
            if (!File.Exists(path))
                return null;
            return MiniJson.AsObj(MiniJson.Parse(File.ReadAllText(path)));
        }

        void ReadInto(Dictionary<string, object> doc, bool isOverride)
        {
            var list = MiniJson.AsList(MiniJson.Get(doc, "default_playlist"));
            if (list != null)
            {
                // An override REPLACES: a scene that lists a playlist means
                // that playlist, not that playlist appended to the kit's.
                defaultPlaylist = new List<LegaiaVideoEntry>();
                foreach (object o in list)
                {
                    var e = MiniJson.AsObj(o);
                    string url = e != null ? MiniJson.AsStr(MiniJson.Get(e, "url")) : MiniJson.AsStr(o);
                    if (string.IsNullOrEmpty(url))
                        continue;
                    defaultPlaylist.Add(new LegaiaVideoEntry
                    {
                        url = url.Trim(),
                        title = (e != null ? MiniJson.AsStr(MiniJson.Get(e, "title")) : null) ?? "",
                    });
                }
            }

            var showList = MiniJson.AsList(MiniJson.Get(doc, "shows"));
            if (showList == null)
                return;
            bool replace = !isOverride || MiniJson.Get(doc, "shows_replace") is bool r && r;
            if (replace)
                shows = new List<LegaiaVideoShow>();
            foreach (object o in showList)
            {
                var e = MiniJson.AsObj(o);
                if (e == null)
                    continue;
                var show = new LegaiaVideoShow
                {
                    who = MiniJson.AsStr(MiniJson.Get(e, "who")) ?? "",
                    title = MiniJson.AsStr(MiniJson.Get(e, "title")) ?? "",
                    console = MiniJson.Get(e, "console") is bool c && c,
                    token = TokenOf(MiniJson.Get(e, "npc")),
                };
                var urls = MiniJson.AsList(MiniJson.Get(e, "urls"));
                if (urls != null)
                    foreach (object u in urls)
                    {
                        string url = MiniJson.AsStr(u);
                        if (!string.IsNullOrEmpty(url))
                            show.urls.Add(url.Trim());
                    }
                string one = MiniJson.AsStr(MiniJson.Get(e, "url"));
                if (show.urls.Count == 0 && !string.IsNullOrEmpty(one))
                    show.urls.Add(one.Trim());
                if (show.urls.Count == 0)
                {
                    Debug.LogWarning("[Legaia] video config: show '" + show.title +
                        "' for " + show.who + " has no URL - skipped.");
                    continue;
                }
                if (string.IsNullOrEmpty(show.who) && string.IsNullOrEmpty(show.token))
                {
                    Debug.LogWarning("[Legaia] video config: show '" + show.title +
                        "' names neither a villager nor an NPC number - skipped.");
                    continue;
                }
                shows.Add(show);
            }
        }

        /// "npc" as written in the file -> the NPC token the runtime
        /// matches object names against. A number takes the two-digit form
        /// the rest of the settings use (7 -> npc_07, 102 -> npc_102); a
        /// string is taken as the token it already is.
        static string TokenOf(object v)
        {
            string s = MiniJson.AsStr(v);
            if (!string.IsNullOrEmpty(s))
                return s.Trim();
            if (v is double d)
                return "npc_" + ((int)d).ToString("00");
            return "";
        }

        /// Flattened for the runtime: every show's URLs end to end, with a
        /// (start, count) window per show. Udon has no jagged arrays, so
        /// the shape the TV holds is this one, and the builder is what
        /// flattens it.
        public List<string> FlatShowUrls()
        {
            var flat = new List<string>();
            foreach (var s in shows)
                flat.AddRange(s.urls);
            return flat;
        }

        public int[] ShowStarts()
        {
            var starts = new int[shows.Count];
            int at = 0;
            for (int i = 0; i < shows.Count; i++)
            {
                starts[i] = at;
                at += shows[i].urls.Count;
            }
            return starts;
        }

        public int[] ShowCounts()
        {
            var counts = new int[shows.Count];
            for (int i = 0; i < shows.Count; i++)
                counts[i] = shows[i].urls.Count;
            return counts;
        }

        public string[] ShowField(System.Func<LegaiaVideoShow, string> pick)
        {
            var vals = new string[shows.Count];
            for (int i = 0; i < shows.Count; i++)
                vals[i] = pick(shows[i]) ?? "";
            return vals;
        }

        public bool[] ShowConsoles()
        {
            var vals = new bool[shows.Count];
            for (int i = 0; i < shows.Count; i++)
                vals[i] = shows[i].console;
            return vals;
        }

        /// Does any show want the floor console? (The builder only makes
        /// the prop when one does.)
        public bool AnyConsole()
        {
            foreach (var s in shows)
                if (s.console)
                    return true;
            return false;
        }
    }
}
