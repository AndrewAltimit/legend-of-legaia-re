// The card table's game: builds the `game` child under the table root
// (LegaiaCardGame - dealer, AI opponents, betting, NPC summoning) and the
// seat panel beside the table (who sits where, the game mode, the pot,
// the action buttons).
//
// The child MUST be named `game` and hang directly off `card_table`: the
// town director finds it by that path
// (Legaia_common_prefabs/card_table/game) and pushes itself into every
// UdonBehaviour there, which is how the table can ask for company without
// either file naming the other's type.
//
// FELT FURNITURE. Each stool gets a `hand_anchor` on the felt in front of
// it - an empty whose +Z points at the table centre and whose +X is the
// tangent, so the dealer fans five cards along it - and the table gets a
// `dealer_anchor` for the blackjack dealer's own row. Nothing is parented
// to the cards: they are ordinary pickups and the game only ever writes
// their position.
//
// THE PANEL is a world-space UI canvas on a post beside the table, read
// from OUTSIDE the table - the same construction as the camp settings
// panel (Canvas + VRCUiShape + GraphicRaycaster + a BoxCollider so the
// pointer has something to hit), and with the same trap avoided: nothing
// with a grab collider is put in front of it, or every press would become
// a pickup grab.
//
// WHICH WAY A CANVAS READS. A Unity UI canvas is legible to a viewer on
// its -Z side, looking along its +Z (the screen-space camera's own
// geometry), and GraphicRaycaster rejects a ray that arrives from the +Z
// side. So the panel root's forward points INTO the table, the board's
// canvas sits on the root's -Z face, and the reader standing outside the
// table is on the canvas's -Z side. The first cut had the root facing
// outward with the canvas on its +Z face: it looked right in a component
// count and in a "faces the player" dot product, and in-world every line
// of text was mirror-written and no button took a press. The check now
// measures the sign the UI system actually cares about. Every button's onClick is a persistent listener onto the
// BACKING UdonBehaviour's SendCustomEvent - a listener on the U# proxy
// does nothing in-world.
//
// The panel is oriented in the TABLE'S OWN LOCAL FRAME, and that is the
// load-bearing detail. A world-space "look at the spawn point" computed
// here is wrong by the time anyone sees it: the common-prefab pass applies
// the scene's hand placement (Legaia > Snapshot placements) to the table
// root AFTER this builder returns, and every child turns with it - which
// silently pointed the whole canvas at the back of the board. An aim
// taken from the table centre is invariant under that, and it is also
// what a player walking up to that side of the table wants.

using System.Collections.Generic;
using UnityEditor;
using UnityEngine;
using UnityEngine.UI;

namespace LegaiaWorld
{
    internal static class LegaiaCardGameBuilder
    {
        // Canvas is authored in pixels and scaled to metres at 1 mm each.
        const float CW = 560f, CH = 780f;

        // Where the panel post stands, in the table root's local frame -
        // clear of all four stools (they sit on a 0.98 m ring).
        static readonly Vector3 PANEL_AT = new Vector3(1.5f, 0f, 0.1f);

        /// `tableRoot` is the card_table object; `host` the LegaiaCardTableHost,
        /// `deck` the LegaiaCardDeck, `seatStations` / `seatChairs` the per-stool
        /// LegaiaNpcStation / LegaiaSeat (same order), `cards` the 52 LegaiaCard,
        /// `wallet` the LegaiaWallet (may be null when UdonSharp is missing).
        internal static GameObject Build(GameObject tableRoot, string genDir, Vector3 spawnW,
            Component host, Component deck, List<Component> seatStations,
            List<Component> seatChairs, List<Component> cards, Component wallet)
        {
            if (tableRoot == null)
                return null;
            int seats = seatChairs != null ? seatChairs.Count : 0;

            var go = new GameObject("game");
            go.transform.SetParent(tableRoot.transform, false);
            var game = LegaiaWorldBuilder.TryAttachUdon(go, "LegaiaCardGame");

            // Felt anchors: one fan spot per seat, one dealer row.
            var handAnchors = new Transform[seats];
            for (int i = 0; i < seats; i++)
            {
                Vector3 dir = FlatDir(seatChairs[i].transform.localPosition);
                var a = new GameObject("hand_anchor_" + i);
                a.transform.SetParent(tableRoot.transform, false);
                a.transform.localPosition = new Vector3(dir.x * 0.5f, 0.767f, dir.z * 0.5f);
                a.transform.localRotation = Quaternion.LookRotation(-dir, Vector3.up);
                handAnchors[i] = a.transform;
            }
            var dealer = new GameObject("dealer_anchor");
            dealer.transform.SetParent(tableRoot.transform, false);
            dealer.transform.localPosition = new Vector3(0f, 0.767f, 0.22f);
            var stack = tableRoot.transform.Find("deck_anchor");

            var panel = BuildPanel(tableRoot, genDir, spawnW, game, seats);

            LegaiaWorldBuilder.SetUdonField(game, "host", host);
            LegaiaWorldBuilder.SetUdonField(game, "deck", deck);
            LegaiaWorldBuilder.SetUdonField(game, "wallet", wallet);
            LegaiaWorldBuilder.SetUdonField(game, "stations",
                Typed(seatStations, "LegaiaNpcStation"));
            LegaiaWorldBuilder.SetUdonField(game, "chairs",
                Typed(seatChairs, "LegaiaSeat"));
            LegaiaWorldBuilder.SetUdonField(game, "cards",
                Typed(cards, "LegaiaCard"));
            LegaiaWorldBuilder.SetUdonField(game, "handAnchors", handAnchors);
            LegaiaWorldBuilder.SetUdonField(game, "dealerAnchor", dealer.transform);
            LegaiaWorldBuilder.SetUdonField(game, "stackAnchor", stack);
            foreach (var kv in panel)
                LegaiaWorldBuilder.SetUdonField(game, kv.Key, kv.Value);
            LegaiaWorldBuilder.SyncUdonProxy(game);
            return go;
        }

        static Vector3 FlatDir(Vector3 v)
        {
            v.y = 0f;
            return v.sqrMagnitude < 1e-6f ? Vector3.forward : v.normalized;
        }

        static System.Array Typed(List<Component> comps, string typeName)
        {
            var t = LegaiaWorldBuilder.FindType("LegaiaWorld." + typeName);
            if (t == null || comps == null)
                return null;
            var arr = System.Array.CreateInstance(t, comps.Count);
            for (int i = 0; i < comps.Count; i++)
                arr.SetValue(comps[i], i);
            return arr;
        }

        // --- the seat panel -----------------------------------------------------

        /// Returns the widget references keyed by the LegaiaCardGame field
        /// they belong on, so the caller wires them in one loop.
        static Dictionary<string, object> BuildPanel(GameObject tableRoot, string genDir,
            Vector3 spawnW, Component game, int seats)
        {
            var w = new Dictionary<string, object>();
            var dark = LegaiaCampProps.EnsureMat(genDir, "camp_dark", "Standard",
                new Color(0.16f, 0.14f, 0.12f));
            var wood = LegaiaCampProps.EnsureMat(genDir, "camp_wood", "Standard",
                new Color(0.36f, 0.24f, 0.13f));

            var root = new GameObject("panel");
            root.transform.SetParent(tableRoot.transform, false);
            root.transform.localPosition = PANEL_AT;
            // Forward INTO the table centre, in the table's own frame (see
            // the header on why a world-space aim does not survive, and on
            // why the canvas must present its -Z to the reader outside).
            root.transform.localRotation = Quaternion.LookRotation(
                -new Vector3(PANEL_AT.x, 0f, PANEL_AT.z).normalized, Vector3.up);

            // Post + board. The post carries the only collider on this
            // assembly and stands well below the buttons.
            var post = GameObject.CreatePrimitive(PrimitiveType.Cube);
            post.name = "post";
            post.transform.SetParent(root.transform, false);
            post.transform.localPosition = new Vector3(0f, 0.38f, 0.02f);
            post.transform.localScale = new Vector3(0.08f, 0.76f, 0.08f);
            post.GetComponent<MeshRenderer>().sharedMaterial = wood;

            var board = GameObject.CreatePrimitive(PrimitiveType.Cube);
            board.name = "board";
            Object.DestroyImmediate(board.GetComponent<Collider>());
            board.transform.SetParent(root.transform, false);
            board.transform.localPosition = new Vector3(0f, 1.16f, 0f);
            board.transform.localScale = new Vector3(0.60f, 0.82f, 0.03f);
            board.GetComponent<MeshRenderer>().sharedMaterial = dark;

            // On the root's -Z face: the outside of the board, presenting
            // the canvas's -Z (its readable side) to the reader.
            var canvasGo = new GameObject("canvas");
            canvasGo.transform.SetParent(root.transform, false);
            canvasGo.transform.localPosition = new Vector3(0f, 1.16f, -0.017f);
            canvasGo.transform.localScale = Vector3.one * 0.001f;
            var canvas = canvasGo.AddComponent<Canvas>();
            canvas.renderMode = RenderMode.WorldSpace;
            var rt = canvasGo.GetComponent<RectTransform>();
            rt.sizeDelta = new Vector2(CW, CH);
            canvasGo.AddComponent<GraphicRaycaster>();
            var shapeType = LegaiaWorldBuilder.FindType("VRC.SDK3.Components.VRCUiShape")
                ?? LegaiaWorldBuilder.FindType("VRC.SDKBase.VRC_UiShape");
            if (shapeType != null)
                canvasGo.AddComponent(shapeType);
            var cbox = canvasGo.AddComponent<BoxCollider>();
            cbox.size = new Vector3(CW, CH, 10f);

            var font = MenuFont();
            var backing = LegaiaCommonPrefabs.BackingUdon(game);
            Transform c = canvasGo.transform;

            Label(c, font, "title", "CARD TABLE", 30, new Vector2(0f, 352f),
                new Vector2(520f, 44f), new Color(1f, 0.9f, 0.7f), TextAnchor.MiddleCenter);
            w["modeText"] = Label(c, font, "mode", "Five-card draw", 22,
                new Vector2(0f, 312f), new Vector2(520f, 34f),
                new Color(0.85f, 0.85f, 0.8f), TextAnchor.MiddleCenter);

            var portraits = new RawImage[seats];
            var names = new Text[seats];
            var coins = new Text[seats];
            var status = new Text[seats];
            for (int i = 0; i < seats; i++)
            {
                float y = 258f - i * 62f;
                var pic = new GameObject("portrait_" + i);
                pic.transform.SetParent(c, false);
                var prt = pic.AddComponent<RectTransform>();
                prt.anchoredPosition = new Vector2(-240f, y);
                prt.sizeDelta = new Vector2(52f, 52f);
                portraits[i] = pic.AddComponent<RawImage>();
                portraits[i].color = new Color(0.45f, 0.55f, 0.75f, 1f);
                portraits[i].enabled = false;

                names[i] = Label(c, font, "name_" + i, "-", 20,
                    new Vector2(-100f, y), new Vector2(200f, 44f),
                    new Color(0.95f, 0.92f, 0.85f), TextAnchor.MiddleLeft);
                coins[i] = Label(c, font, "coins_" + i, "", 20,
                    new Vector2(50f, y), new Vector2(70f, 44f),
                    new Color(0.95f, 0.85f, 0.4f), TextAnchor.MiddleRight);
                status[i] = Label(c, font, "status_" + i, "Empty", 18,
                    new Vector2(185f, y), new Vector2(170f, 44f),
                    new Color(0.75f, 0.8f, 0.85f), TextAnchor.MiddleLeft);
            }
            w["rowPortrait"] = portraits;
            w["rowName"] = names;
            w["rowCoins"] = coins;
            w["rowStatus"] = status;

            w["potText"] = Label(c, font, "pot", "Pot 0", 24, new Vector2(0f, 22f),
                new Vector2(520f, 34f), new Color(0.95f, 0.85f, 0.4f),
                TextAnchor.MiddleCenter);
            w["msgText"] = Label(c, font, "msg", "", 20, new Vector2(0f, -14f),
                new Vector2(520f, 34f), new Color(0.9f, 0.9f, 0.85f),
                TextAnchor.MiddleCenter);

            Text dealLabel, callLabel, raiseLabel;
            w["btnDeal"] = Btn(c, font, backing, "UiDeal", "Deal",
                new Vector2(-140f, -60f), new Vector2(260f, 52f), out dealLabel);
            w["btnDealText"] = dealLabel;
            w["btnMode"] = Btn(c, font, backing, "UiMode", "Mode",
                new Vector2(140f, -60f), new Vector2(260f, 52f), out _);
            w["btnCall"] = Btn(c, font, backing, "UiCall", "Check",
                new Vector2(-186f, -122f), new Vector2(176f, 52f), out callLabel);
            w["btnCallText"] = callLabel;
            w["btnRaise"] = Btn(c, font, backing, "UiRaise", "Bet",
                new Vector2(0f, -122f), new Vector2(176f, 52f), out raiseLabel);
            w["btnRaiseText"] = raiseLabel;
            w["btnFold"] = Btn(c, font, backing, "UiFold", "Fold",
                new Vector2(186f, -122f), new Vector2(176f, 52f), out _);

            var holds = new Button[5];
            var holdText = new Text[5];
            for (int i = 0; i < 5; i++)
                holds[i] = Btn(c, font, backing, "UiHold" + i, "-",
                    new Vector2(-216f + i * 108f, -192f), new Vector2(100f, 62f),
                    out holdText[i], 18);
            w["btnHold"] = holds;
            w["btnHoldText"] = holdText;

            w["btnDraw"] = Btn(c, font, backing, "UiDraw", "Draw",
                new Vector2(-186f, -262f), new Vector2(176f, 52f), out _);
            w["btnHit"] = Btn(c, font, backing, "UiHit", "Hit",
                new Vector2(0f, -262f), new Vector2(176f, 52f), out _);
            w["btnStand"] = Btn(c, font, backing, "UiStand", "Stand",
                new Vector2(186f, -262f), new Vector2(176f, 52f), out _);

            Label(c, font, "hint",
                "Sit on a stool to play. Villagers join on their own.\n" +
                "Bets are paid from your coin purse.", 16,
                new Vector2(0f, -332f), new Vector2(520f, 60f),
                new Color(0.65f, 0.63f, 0.58f), TextAnchor.MiddleCenter);
            return w;
        }

        static Font MenuFont()
        {
            var f = Resources.GetBuiltinResource<Font>("LegacyRuntime.ttf");
            if (f == null)
                f = Resources.GetBuiltinResource<Font>("Arial.ttf");
            return f;
        }

        static Text Label(Transform parent, Font font, string name, string label,
            int size, Vector2 pos, Vector2 dims, Color color, TextAnchor anchor)
        {
            var go = new GameObject("text_" + name);
            go.transform.SetParent(parent, false);
            var rt = go.AddComponent<RectTransform>();
            rt.anchoredPosition = pos;
            rt.sizeDelta = dims;
            var text = go.AddComponent<Text>();
            text.font = font;
            text.fontSize = size;
            text.text = label;
            text.color = color;
            text.alignment = anchor;
            text.horizontalOverflow = HorizontalWrapMode.Overflow;
            text.verticalOverflow = VerticalWrapMode.Overflow;
            return text;
        }

        /// A UI button whose click sends `eventName` into the game's backing
        /// UdonBehaviour (recorded as a persistent listener, so it survives
        /// into the client build).
        static Button Btn(Transform parent, Font font, Component backing,
            string eventName, string label, Vector2 pos, Vector2 dims,
            out Text text, int size = 20)
        {
            var go = new GameObject("btn_" + eventName);
            go.transform.SetParent(parent, false);
            var rt = go.AddComponent<RectTransform>();
            rt.anchoredPosition = pos;
            rt.sizeDelta = dims;
            var img = go.AddComponent<Image>();
            img.color = new Color(0.28f, 0.25f, 0.2f, 0.95f);
            var btn = go.AddComponent<Button>();
            var colors = btn.colors;
            colors.highlightedColor = new Color(0.45f, 0.4f, 0.3f);
            colors.pressedColor = new Color(0.6f, 0.5f, 0.32f);
            colors.disabledColor = new Color(0.18f, 0.17f, 0.15f, 0.7f);
            btn.colors = colors;
            text = Label(go.transform, font, eventName, label, size, Vector2.zero,
                new Vector2(dims.x - 8f, dims.y - 6f),
                new Color(0.95f, 0.92f, 0.85f), TextAnchor.MiddleCenter);
            if (backing != null)
            {
                var action = (UnityEngine.Events.UnityAction<string>)
                    System.Delegate.CreateDelegate(
                        typeof(UnityEngine.Events.UnityAction<string>),
                        backing, "SendCustomEvent");
                UnityEditor.Events.UnityEventTools.AddStringPersistentListener(
                    btn.onClick, action, eventName);
            }
            return btn;
        }
    }
}
