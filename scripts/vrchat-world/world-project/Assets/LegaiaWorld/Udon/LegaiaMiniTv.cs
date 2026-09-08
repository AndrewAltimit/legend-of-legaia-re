// The mini CRT on the poker table: the SAME picture as the big set, not a
// second copy of it.
//
// THERE IS NO SECOND VIDEO PLAYER HERE, and that is the whole design. A
// world may run several VRCUnityVideoPlayer / VRCAVProVideoPlayer
// components, but each one is its own decode, its own network fetch and
// its own clock - two players pointed at one URL drift apart within
// seconds and cost twice the frame budget for a screen the size of a
// coaster. The mini set is an extra OUTPUT of the TV's players instead:
//
//   Audio  - both backends take several outputs natively. The Unity
//            player's `targetAudioSources` is an ARRAY (the builder puts
//            the table speaker in it beside the TV's own), and AVPro
//            takes one VRCAVProVideoSpeaker per AudioSource. Same
//            decode, so the two speakers are sample-synchronised; the
//            table one is quiet and short-range so it reads as a set on
//            the table rather than as an echo of the one across the
//            square.
//   Video  - AVPro takes one VRCAVProVideoScreen per renderer, so the
//            mini quad is wired straight to the same player. The Unity
//            player has ONE `targetMaterialRenderer` and no way to add a
//            second, which is what this behaviour is for: it copies
//            whatever texture the big screen is currently showing onto
//            the small one.
//
// WHY THE COPY TRIES TWO SOURCES. Unity's VideoPlayer in MaterialOverride
// mode hands the texture to the target renderer, and where it puts it -
// a per-renderer MaterialPropertyBlock or an instantiated material - is
// native behaviour the SDK does not document and a Unity upgrade may
// change. Both are cheap to read, so `SourceTexture` reads both and
// takes whichever answers. A null answer is never written: while AVPro
// owns the mini screen (the normal case on PC) the property block is
// empty, and blanking the picture on that account would undo the AVPro
// screen's own work every poll.
//
// It POLLS rather than running in Update. The texture OBJECT changes
// only when a video is loaded or its resolution changes - the frames
// themselves arrive inside a texture that keeps its identity - so a
// check a few times a second is enough, and an Udon Update on a prop
// nobody is looking at is a cost every player in the instance pays.

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaMiniTv : UdonSharpBehaviour
    {
        /// The big set's screen: the renderer the TV's video players draw.
        public Renderer sourceScreen;

        /// This set's screen.
        public Renderer screen;

        /// The texture property both screens' shaders read.
        public string textureProperty = "_MainTex";

        /// How often to look for a new texture. Nothing is animated here -
        /// see the header on why polling is the right shape.
        public float pollSeconds = 0.4f;

        private MaterialPropertyBlock block;
        private Texture shown;

        void Start()
        {
            if (sourceScreen == null || screen == null)
                return;
            block = new MaterialPropertyBlock();
            SendCustomEventDelayedSeconds(nameof(Poll), pollSeconds);
        }

        public void Poll()
        {
            Mirror();
            SendCustomEventDelayedSeconds(nameof(Poll),
                pollSeconds < 0.05f ? 0.05f : pollSeconds);
        }

        /// Put the big screen's texture on the small one, when there is one
        /// and it is not already there.
        public void Mirror()
        {
            if (sourceScreen == null || screen == null)
                return;
            Texture t = SourceTexture();
            if (t == null || t == shown)
                return;
            screen.material.SetTexture(textureProperty, t);
            shown = t;
        }

        /// Whatever the big screen is drawing right now, from either of the
        /// two places a video player may have left it (see the header).
        public Texture SourceTexture()
        {
            if (block != null)
            {
                sourceScreen.GetPropertyBlock(block);
                Texture t = block.GetTexture(textureProperty);
                if (t != null)
                    return t;
            }
            return sourceScreen.material.GetTexture(textureProperty);
        }
    }
}
