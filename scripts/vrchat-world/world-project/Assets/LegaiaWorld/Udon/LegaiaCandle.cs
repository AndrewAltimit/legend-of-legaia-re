// A candle that lights itself at night: the flame (particles + point
// light, built inactive) comes on when the scene's day/night cycle says
// night and goes out at dawn, with a two-octave flicker on the light
// while it burns - the same breathing-plus-crackle shape LegaiaTorch and
// LegaiaFlicker use, so every fire in the town pulses the same way and
// none of them in step.
//
// The day/night reference is resolved at Start by the sun object's name
// when the builder could not wire it (the common-prefabs pass runs
// before the realism pass that creates the cycle on a fresh build). A
// scene with no cycle at all keeps the candle at `alwaysOn`.

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaCandle : UdonSharpBehaviour
    {
        [Tooltip("The flame container (particles + light), inactive by day.")]
        public GameObject flame;

        [Tooltip("The flame's point light - flickered while lit.")]
        public Light fireLight;

        [Tooltip("Base intensity the flicker wobbles around (the builder copies the light's authored intensity here).")]
        public float fireIntensity = 0.7f;

        [Tooltip("The day/night cycle to follow. Null = looked up by the sun object's name at Start.")]
        public LegaiaDayNight dayNight;

        [Tooltip("Name of the object carrying LegaiaDayNight (the realism pass's sun).")]
        public string dayNightObject = "LegaiaSun";

        [Tooltip("With no day/night cycle in the scene: lit, or never.")]
        public bool alwaysOn = true;

        private float flickerSeed;
        private float pollTimer;
        private bool lit;

        void Start()
        {
            if (dayNight == null)
            {
                GameObject g = GameObject.Find(dayNightObject);
                if (g != null)
                    dayNight = g.GetComponent<LegaiaDayNight>();
            }
            Vector3 p = transform.position;
            flickerSeed = (p.x * 3.7f + p.z * 1.3f) % 10f;
            Apply(Wanted());
        }

        bool Wanted()
        {
            return dayNight != null ? dayNight.isNight : alwaysOn;
        }

        void Apply(bool on)
        {
            lit = on;
            if (flame != null)
                flame.SetActive(on);
        }

        void Update()
        {
            pollTimer += Time.deltaTime;
            if (pollTimer >= 0.5f)
            {
                pollTimer = 0f;
                bool want = Wanted();
                if (want != lit)
                    Apply(want);
            }
            if (!lit || fireLight == null)
                return;
            float t = Time.time;
            float n = Mathf.PerlinNoise(t * 1.6f, flickerSeed) * 0.6f
                    + Mathf.PerlinNoise(t * 9.5f, flickerSeed + 7.31f) * 0.4f;
            fireLight.intensity = fireIntensity * (0.72f + 0.56f * n);
        }
    }
}
