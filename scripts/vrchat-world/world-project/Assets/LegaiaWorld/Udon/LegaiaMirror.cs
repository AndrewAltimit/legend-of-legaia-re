// The kit's mirror: two VRC mirror surfaces (a full-quality one that
// reflects the whole world, and a low-quality one that reflects only
// players against the skybox) behind three buttons - Off / Mirror /
// Mirror (low). A mirror renders the scene a second time, so it starts
// OFF, the choice is local (one player's mirror never costs another
// player a frame), and it switches itself off again when the local
// player walks away - the standard VRChat mirror etiquette. The frame
// appears and disappears with the glass, so an off mirror is only its
// button post.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaMirror : UdonSharpBehaviour
    {
        [Tooltip("Mirror surface reflecting every layer (world + players).")]
        public GameObject highMirror;

        [Tooltip("Mirror surface reflecting only players (world replaced by the skybox).")]
        public GameObject lowMirror;

        [Tooltip("The frame around the glass - shown only while a surface is on, so an off mirror leaves just the button post.")]
        public GameObject frame;

        [Tooltip("The three button renderers, in mode order: off, high, low - the active one gets the lit material.")]
        public Renderer[] buttons;

        public Material idleMaterial;
        public Material activeMaterial;

        [Tooltip("Metres from the mirror beyond which it switches itself off (0 disables the auto-off).")]
        public float autoOffDistance = 9f;

        private int mode; // 0 off, 1 high, 2 low
        private float nextCheck;

        void Start()
        {
            Apply(0);
        }

        public void SetOff() { Apply(0); }
        public void SetHigh() { Apply(1); }
        public void SetLow() { Apply(2); }

        void Apply(int m)
        {
            mode = m;
            if (highMirror != null)
                highMirror.SetActive(m == 1);
            if (lowMirror != null)
                lowMirror.SetActive(m == 2);
            if (frame != null)
                frame.SetActive(m != 0);
            if (buttons == null)
                return;
            for (int i = 0; i < buttons.Length; i++)
            {
                if (buttons[i] == null)
                    continue;
                Material want = i == m ? activeMaterial : idleMaterial;
                if (want != null)
                    buttons[i].sharedMaterial = want;
            }
        }

        void Update()
        {
            if (mode == 0 || autoOffDistance <= 0f || Time.time < nextCheck)
                return;
            nextCheck = Time.time + 0.5f;
            VRCPlayerApi local = Networking.LocalPlayer;
            if (local == null || !local.IsValid())
                return;
            Vector3 d = local.GetPosition() - transform.position;
            d.y = 0f;
            if (d.sqrMagnitude > autoOffDistance * autoOffDistance)
                Apply(0);
        }
    }
}
