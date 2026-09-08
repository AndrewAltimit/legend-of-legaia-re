// Udon behaviour for the pickup settings panel the builder places near
// spawn: a small hand-held board with world-space UI buttons. The
// buttons SendCustomEvent into this behaviour:
//
// - ToggleMusic  - mutes/unmutes the scene BGM for THIS player only
//   (a personal preference, not a world vote). The builder can start
//   the town theme muted - the clip still runs, so switching it on
//   joins the loop where the rest of the world is - and the button's
//   label is read off the source at Start rather than assumed, so it
//   never opens saying "Music: On" over silence.
// - SetDay / SetNight - jumps the shared day/night cycle to noon /
//   midnight for EVERYONE (the cycle itself is derived from the server
//   clock, so the jump is a synced offset on LegaiaDayNight).
// - SetClear - the same jump on the shared weather schedule
//   (LegaiaWeather): the next clear spell, for everyone. Inert until the
//   weather pass builds the schedule.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using UnityEngine.UI;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaWorldMenu : UdonSharpBehaviour
    {
        [Tooltip("The scene BGM AudioSource (on the world root). ToggleMusic mutes it locally.")]
        public AudioSource music;

        [Tooltip("The realism pass's day/night behaviour - SetDay/SetNight jump its synced cycle. Left null when the day/night cycle is not built.")]
        public LegaiaDayNight dayNight;

        [Tooltip("The weather pass's schedule behaviour - SetClear jumps its synced spell. Left null when the weather pass is not built.")]
        public LegaiaWeather weather;

        [Tooltip("Label on the music button - updated to show the current on/off state.")]
        public Text musicLabel;

        void Start()
        {
            if (music != null && musicLabel != null)
                musicLabel.text = music.mute ? "Music: Off" : "Music: On";
        }

        public void ToggleMusic()
        {
            if (music == null)
                return;
            music.mute = !music.mute;
            if (musicLabel != null)
                musicLabel.text = music.mute ? "Music: Off" : "Music: On";
        }

        public void SetDay()
        {
            if (dayNight != null)
                dayNight.JumpToDay();
        }

        public void SetNight()
        {
            if (dayNight != null)
                dayNight.JumpToNight();
        }

        public void SetClear()
        {
            if (weather != null)
                weather.JumpToClear();
        }
    }
}
