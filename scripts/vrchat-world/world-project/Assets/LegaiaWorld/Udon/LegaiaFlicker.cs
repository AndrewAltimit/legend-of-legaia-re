// Udon behaviour for the always-burning night torches the realism pass
// plants by trees and doorways: flickers the flame's point light so it
// reads as firelight. No sync and no interaction - the torch burns
// whenever its container is active (LegaiaDayNight enables the
// night-torch container only while the sun is down).
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaFlicker : UdonSharpBehaviour
    {
        [Tooltip("The flame's point light - flickered every frame.")]
        public Light fireLight;

        [Tooltip("Base intensity the flicker wobbles around (the builder copies the light's authored intensity here).")]
        public float fireIntensity = 1.4f;

        private float flickerSeed;

        void Start()
        {
            // Per-instance noise offset so nearby fires don't pulse in step.
            Vector3 p = transform.position;
            flickerSeed = (p.x * 3.7f + p.z * 1.3f) % 10f;
        }

        void Update()
        {
            if (fireLight == null)
                return;
            // Two Perlin octaves: a slow breathing swell plus a fast
            // crackle jitter - same shape as LegaiaTorch's flicker.
            float t = Time.time;
            float n = Mathf.PerlinNoise(t * 1.6f, flickerSeed) * 0.6f
                    + Mathf.PerlinNoise(t * 9.5f, flickerSeed + 7.31f) * 0.4f;
            fireLight.intensity = fireIntensity * (0.72f + 0.56f * n);
        }
    }
}
