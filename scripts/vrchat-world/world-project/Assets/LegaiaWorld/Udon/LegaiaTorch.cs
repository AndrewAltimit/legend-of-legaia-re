// Udon behaviour for the carry-able torches and campfires the builder
// places near spawn: pick one up and press Use (left click / trigger)
// to light or snuff it. The flame container (fire + smoke particles and
// a point light this script flickers) and the crackle AudioSource start
// off; `lit` is synced, so a torch someone lights burns for everyone,
// and the pickup's Object Sync carries its position.
//
// Spawn-kinematic like the equipment rack (LegaiaPickupProp): a dynamic
// body waking during world-load hitches can tunnel through the
// paper-thin PSX ground and respawn-loop, so the body only goes
// physical on first drop.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Continuous)]
    public class LegaiaTorch : UdonSharpBehaviour
    {
        [Tooltip("Container holding the flame visuals (fire + smoke particles, point light) - inactive while unlit.")]
        public GameObject flame;

        [Tooltip("Looping fire-crackle AudioSource (spatial), played only while lit.")]
        public AudioSource crackle;

        [Tooltip("The flame's point light - flickered every frame while lit.")]
        public Light fireLight;

        [Tooltip("Base intensity the flicker wobbles around (the builder copies the light's authored intensity here).")]
        public float fireIntensity = 1.4f;

        [UdonSynced]
        public bool lit;

        private bool shown;
        private Rigidbody body;
        private float flickerSeed;

        void Start()
        {
            body = GetComponent<Rigidbody>();
            if (body != null)
                body.isKinematic = true;
            // Per-instance noise offset so nearby fires don't pulse in step.
            Vector3 p = transform.position;
            flickerSeed = (p.x * 3.7f + p.z * 1.3f) % 10f;
            shown = !lit; // force the first Apply
            Apply();
        }

        void Update()
        {
            if (!shown || fireLight == null)
                return;
            // Two Perlin octaves: a slow breathing swell plus a fast
            // crackle jitter - reads as firelight, not a steady lamp.
            float t = Time.time;
            float n = Mathf.PerlinNoise(t * 1.6f, flickerSeed) * 0.6f
                    + Mathf.PerlinNoise(t * 9.5f, flickerSeed + 7.31f) * 0.4f;
            fireLight.intensity = fireIntensity * (0.72f + 0.56f * n);
        }

        public override void OnDrop()
        {
            if (body != null)
                body.isKinematic = false;
        }

        public override void OnPickupUseDown()
        {
            if (!Networking.IsOwner(gameObject))
                Networking.SetOwner(Networking.LocalPlayer, gameObject);
            lit = !lit;
            Apply();
        }

        public override void OnDeserialization()
        {
            Apply();
        }

        void Apply()
        {
            if (shown == lit)
                return;
            shown = lit;
            if (flame != null)
                flame.SetActive(lit);
            if (crackle != null)
            {
                if (lit)
                    crackle.Play();
                else
                    crackle.Stop();
            }
        }
    }
}
