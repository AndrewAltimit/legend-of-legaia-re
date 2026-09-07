// What a villager is holding: the carried-item rig on an NPC instance.
// The living town's errands hand things to people - a bucket fetched from
// the shore, a broom swept at a doorway, a bundle of firewood carried
// between two points - and this is the behaviour that shows one.
//
// WHERE THE HAND IS. These rigs are rigid-node models: no skeleton, no
// hand bone, only a dozen mesh nodes under the instance. The editor pass
// (LegaiaLivingTown.BuildCarry) MEASURES the hand rather than assuming it:
// among the body's mesh nodes it takes the ones standing widest of the
// body centreline at arm height and picks the LOWEST of those - which is
// the forearm or hand on every town01 rig family (the ten-node humanoid,
// the six-node robed one, and the short skirted one whose arms are nodes
// 3-6 rather than 2-5). `hand` is an empty parked at that node's measured
// centre, so the items hang off a real arm at any export scale. Rigs with
// no arm at all (a signpost, a tree) fall back to a spot beside the torso.
//
// The items are CHILDREN of `hand`, all inactive, and Show() enables
// exactly one - the same GameObject.SetActive route the speech bubble
// takes, because Renderer.material writes are the fragile ones in Udon.
// This behaviour's own object is the NPC root, which is always active: a
// U# call into a disabled behaviour never runs, so a carry rig that
// switched itself off could never be switched back on.
//
// Nothing here is synced: every client simulates the same villagers, like
// LegaiaNpcWander does, and two players seeing a broom sway on different
// frames is not something anyone can tell.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaNpcCarry : UdonSharpBehaviour
    {
        [Tooltip("Empty at the measured hand point; the items hang off it.")]
        public Transform hand;

        [Tooltip("One child per item: 0 bucket, 1 broom, 2 firewood, 3 basket. All start inactive.")]
        public GameObject[] items;

        [Tooltip("Drop whatever is held after this long, whatever happens - " +
                 "so an errand cut short by nightfall never leaves a villager " +
                 "carrying a bucket for the rest of the world's life.")]
        public float maxCarrySeconds = 240f;

        [Tooltip("Work sway: degrees either side of rest (action 1).")]
        public float swayDegrees = 17f;

        [Tooltip("Work sway: cycles per second.")]
        public float swaySpeed = 2.6f;

        private int shown = -1;
        private int action;
        private float dropAt;
        private Quaternion rest;

        void Start()
        {
            if (hand != null)
                rest = hand.localRotation;
            HideAll();
        }

        /// Put item `kind` in the villager's hand. `act` 0 = just carry it,
        /// 1 = work with it (the sway below).
        public void Show(int kind, int act)
        {
            if (items == null || kind < 0 || kind >= items.Length)
                return;
            if (kind != shown)
            {
                HideAll();
                if (items[kind] != null)
                    items[kind].SetActive(true);
                shown = kind;
            }
            action = act;
            dropAt = Time.time + (maxCarrySeconds < 5f ? 5f : maxCarrySeconds);
        }

        /// Put it down.
        public void Hide()
        {
            HideAll();
            shown = -1;
            action = 0;
            if (hand != null)
                hand.localRotation = rest;
        }

        /// True while something is in hand (the brain carries it between an
        /// errand's stops rather than dropping it at every one).
        public bool Carrying()
        {
            return shown >= 0;
        }

        /// Which item is in hand, or -1.
        public int Item()
        {
            return shown;
        }

        void HideAll()
        {
            if (items == null)
                return;
            for (int i = 0; i < items.Length; i++)
                if (items[i] != null)
                    items[i].SetActive(false);
        }

        void Update()
        {
            // Nothing in hand: no per-frame work at all (this runs on every
            // villager in the town, Quest included).
            if (shown < 0)
                return;
            if (Time.time > dropAt)
            {
                Hide();
                return;
            }
            if (action != 1 || hand == null)
                return;
            // Sweeping / hauling: a slow rock about the hand's own X axis.
            hand.localRotation = rest * Quaternion.AngleAxis(
                Mathf.Sin(Time.time * swaySpeed) * swayDegrees, Vector3.right);
        }
    }
}
