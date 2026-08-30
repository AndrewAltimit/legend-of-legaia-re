// Minimal JSON reader for the `legaia-engine export-glb` manifest.json.
// Self-contained (no Newtonsoft dependency) so the builder compiles in any
// Unity project. Parses the standard JSON grammar into:
//   object -> Dictionary<string, object>
//   array  -> List<object>
//   number -> double, string -> string, true/false -> bool, null -> null
// Clean-room utility written for this kit; MIT OR Unlicense like the repo.

using System.Collections.Generic;
using System.Globalization;
using System.Text;

namespace LegaiaWorld
{
    public static class MiniJson
    {
        public static object Parse(string text)
        {
            int i = 0;
            object v = ParseValue(text, ref i);
            SkipWs(text, ref i);
            return v;
        }

        // --- typed helpers over the parsed tree ---
        public static Dictionary<string, object> AsObj(object v)
            => v as Dictionary<string, object>;

        public static List<object> AsList(object v)
            => v as List<object>;

        public static string AsStr(object v)
            => v as string;

        public static double AsNum(object v, double fallback = 0)
            => v is double d ? d : fallback;

        public static object Get(object obj, string key)
        {
            var o = AsObj(obj);
            return o != null && o.TryGetValue(key, out var v) ? v : null;
        }

        public static float GetNum(object obj, string key, float fallback = 0)
            => (float)AsNum(Get(obj, key), fallback);

        /// A `[x, y, z]` array as a float triple (0,0,0 when absent).
        public static UnityEngine.Vector3 GetVec3(object obj, string key)
        {
            var l = AsList(Get(obj, key));
            if (l == null || l.Count < 3) return UnityEngine.Vector3.zero;
            return new UnityEngine.Vector3(
                (float)AsNum(l[0]), (float)AsNum(l[1]), (float)AsNum(l[2]));
        }

        // --- parser ---
        static void SkipWs(string s, ref int i)
        {
            while (i < s.Length && (s[i] == ' ' || s[i] == '\t' || s[i] == '\n' || s[i] == '\r'))
                i++;
        }

        static object ParseValue(string s, ref int i)
        {
            SkipWs(s, ref i);
            if (i >= s.Length) throw new System.Exception("json: unexpected end");
            char c = s[i];
            if (c == '{') return ParseObject(s, ref i);
            if (c == '[') return ParseArray(s, ref i);
            if (c == '"') return ParseString(s, ref i);
            if (c == 't') { Expect(s, ref i, "true"); return true; }
            if (c == 'f') { Expect(s, ref i, "false"); return false; }
            if (c == 'n') { Expect(s, ref i, "null"); return null; }
            return ParseNumber(s, ref i);
        }

        static void Expect(string s, ref int i, string word)
        {
            if (i + word.Length > s.Length || s.Substring(i, word.Length) != word)
                throw new System.Exception("json: expected " + word + " at " + i);
            i += word.Length;
        }

        static Dictionary<string, object> ParseObject(string s, ref int i)
        {
            var o = new Dictionary<string, object>();
            i++; // '{'
            SkipWs(s, ref i);
            if (i < s.Length && s[i] == '}') { i++; return o; }
            while (true)
            {
                SkipWs(s, ref i);
                string key = ParseString(s, ref i);
                SkipWs(s, ref i);
                if (s[i] != ':') throw new System.Exception("json: expected ':' at " + i);
                i++;
                o[key] = ParseValue(s, ref i);
                SkipWs(s, ref i);
                if (s[i] == ',') { i++; continue; }
                if (s[i] == '}') { i++; return o; }
                throw new System.Exception("json: expected ',' or '}' at " + i);
            }
        }

        static List<object> ParseArray(string s, ref int i)
        {
            var a = new List<object>();
            i++; // '['
            SkipWs(s, ref i);
            if (i < s.Length && s[i] == ']') { i++; return a; }
            while (true)
            {
                a.Add(ParseValue(s, ref i));
                SkipWs(s, ref i);
                if (s[i] == ',') { i++; continue; }
                if (s[i] == ']') { i++; return a; }
                throw new System.Exception("json: expected ',' or ']' at " + i);
            }
        }

        static string ParseString(string s, ref int i)
        {
            if (s[i] != '"') throw new System.Exception("json: expected string at " + i);
            i++;
            var sb = new StringBuilder();
            while (s[i] != '"')
            {
                char c = s[i++];
                if (c != '\\') { sb.Append(c); continue; }
                char e = s[i++];
                switch (e)
                {
                    case '"': sb.Append('"'); break;
                    case '\\': sb.Append('\\'); break;
                    case '/': sb.Append('/'); break;
                    case 'b': sb.Append('\b'); break;
                    case 'f': sb.Append('\f'); break;
                    case 'n': sb.Append('\n'); break;
                    case 'r': sb.Append('\r'); break;
                    case 't': sb.Append('\t'); break;
                    case 'u':
                        sb.Append((char)int.Parse(
                            s.Substring(i, 4), NumberStyles.HexNumber,
                            CultureInfo.InvariantCulture));
                        i += 4;
                        break;
                    default: throw new System.Exception("json: bad escape \\" + e);
                }
            }
            i++; // closing '"'
            return sb.ToString();
        }

        static object ParseNumber(string s, ref int i)
        {
            int start = i;
            while (i < s.Length && ("+-0123456789.eE".IndexOf(s[i]) >= 0)) i++;
            return double.Parse(
                s.Substring(start, i - start), CultureInfo.InvariantCulture);
        }
    }
}
