// One villager's speech bubble: a billboarded quad above the NPC's head
// that shows an icon ("...", "!", "?", heart, music note, laugh) while the
// town director gives that NPC a turn in a conversation, with the NPC's own
// first dialog line optionally printed under the icon.
//
// ICON SWAPPING WITHOUT MATERIAL WRITES. The atlas is cut into six child
// quads, one per icon, each with its own generated material (LegaiaBubbleArt
// builds the atlas and the quads); Show() enables exactly one of them.
// Renderer.material / MaterialPropertyBlock writes would be the obvious
// route and are the fragile one in Udon - GameObject.SetActive is not.
//
// MIRRORS. The NPC instance carries the builder's handedness mirror
// (negative Z) and the built root carries the explorer-orientation mirror
// (negative X), so a quad parented under an NPC inherits a lossy scale
// whose determinant may be either sign. The builder therefore gives this
// object a local scale that CANCELS the parent chain (world scale comes out
// positive and uniform), which is what lets the billboard below be a plain
// LookRotation: with a mirrored world scale the same code would render the
// text back-to-front, and no rotation could fix it. Start re-measures the
// residual sign anyway (TransformPoint difference, the same recipe the
// locomotion controller uses for facing) and flips 180 degrees when the
// quad's visible front still points away - measured, never derived.
//
// The behaviour's own GameObject stays ACTIVE and only its `visual` child
// is toggled: a U# call into a behaviour on a DISABLED object never runs
// (cross-behaviour calls compile to SendCustomEvent, which a disabled
// UdonBehaviour drops), so a bubble that switched itself off could never
// be switched back on. Update returns immediately while nothing is shown.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using TMPro;
using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaSpeechBubble : UdonSharpBehaviour
    {
        [Tooltip("One child quad per icon: 0 '...', 1 '!', 2 '?', 3 heart, 4 music, 5 laugh.")]
        public GameObject[] icons;

        [Tooltip("Optional world-space TextMeshPro under the icon (legacy TextMesh is not exposed to Udon).")]
        public TextMeshPro label;

        [Tooltip("Longest dialog line shown under the icon; longer lines are cut.")]
        public int maxLabelChars = 28;

        [Tooltip("Container holding the quads + text: toggled on and off (this behaviour's own object must stay active).")]
        public GameObject visual;

        [Tooltip("Turn to face the local player's head while shown.")]
        public bool billboard = true;

        private int shown = -1;
        private float hideAt;
        private bool flip;
        private bool measured;

        void Start()
        {
            HideAll();
            if (visual != null)
                visual.SetActive(false);
        }

        // The bubble's visible front direction, read through the full
        // transform chain (scale mirrors included - TransformDirection would
        // miss them, which is the whole trap this measurement exists for).
        Vector3 VisualFront()
        {
            return transform.TransformPoint(Vector3.forward)
                - transform.TransformPoint(Vector3.zero);
        }

        /// Show icon `icon` (index into `icons`) for `seconds`, with an
        /// optional line of text under it.
        public void Show(int icon, float seconds, string text)
        {
            if (icons == null || icons.Length == 0)
                return;
            if (icon < 0)
                icon = 0;
            if (icon >= icons.Length)
                icon = icons.Length - 1;
            if (visual != null)
                visual.SetActive(true);
            if (icon != shown)
            {
                HideAll();
                if (icons[icon] != null)
                    icons[icon].SetActive(true);
                shown = icon;
            }
            if (label != null)
            {
                if (text == null)
                    text = "";
                if (text.Length > maxLabelChars)
                    text = text.Substring(0, maxLabelChars);
                label.text = text;
            }
            hideAt = Time.time + seconds;
            Aim();
        }

        /// Take the bubble down now.
        public void Hide()
        {
            HideAll();
            shown = -1;
            if (visual != null)
                visual.SetActive(false);
        }

        void HideAll()
        {
            if (icons == null)
                return;
            for (int i = 0; i < icons.Length; i++)
                if (icons[i] != null)
                    icons[i].SetActive(false);
        }

        void Update()
        {
            if (shown < 0)
                return; // nothing up: no billboard work at all
            if (Time.time >= hideAt)
            {
                Hide();
                return;
            }
            if (billboard)
                Aim();
        }

        // Face the local player's head. The 180-degree residual (a mirror
        // the builder's compensation did not fully cancel) is measured once
        // against the rendered front, not assumed.
        void Aim()
        {
            VRCPlayerApi p = Networking.LocalPlayer;
            if (p == null)
                return;
            Vector3 head = p.GetTrackingData(
                VRCPlayerApi.TrackingDataType.Head).position;
            Vector3 to = head - transform.position;
            if (to.sqrMagnitude < 1e-6f)
                return;
            transform.rotation = Quaternion.LookRotation(to.normalized, Vector3.up);
            if (!measured)
            {
                measured = true;
                flip = Vector3.Dot(VisualFront(), to) < 0f;
            }
            if (flip)
                transform.rotation =
                    Quaternion.AngleAxis(180f, Vector3.up) * transform.rotation;
        }
    }
}
