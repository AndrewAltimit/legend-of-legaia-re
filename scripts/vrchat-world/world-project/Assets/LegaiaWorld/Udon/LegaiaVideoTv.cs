// The kit's TV: a synced video player over the SDK's two players.
//
// Player choice: AVPro on PC (the only one that plays YouTube's current
// formats after VRChat's yt-dlp resolve), the Unity player on Android /
// Quest (AVPro is unavailable there) - and the Unity player everywhere
// when `preferUnityPlayer` is set, which is also the only player that
// runs inside the editor (AVPro needs a real client build).
//
// Sync model (the USharpVideo shape, reduced): the URL, a load serial,
// the playing flag and a server-time origin are owner-synced. Every
// client loads the URL itself, then seats its playhead at
// `server time - startTime`, so a late joiner lands mid-video where
// everyone else is; pausing stores the position, resuming re-derives
// the origin. A drift check every ten seconds re-seeks anything more
// than a second and a half out. VRChat rate-limits video loads to one
// per five seconds per client, so a load that arrives inside that
// window is queued, and RateLimited / network errors retry a few times
// with a six-second back-off instead of failing once and staying dark.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using UnityEngine.UI;
using VRC.SDK3.Components;
using VRC.SDK3.Components.Video;
using VRC.SDK3.Video.Components;
using VRC.SDK3.Video.Components.AVPro;
using VRC.SDK3.Video.Components.Base;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.Manual)]
    public class LegaiaVideoTv : UdonSharpBehaviour
    {
        [Tooltip("The Unity video player component (Android / Quest, editor, or preferUnityPlayer).")]
        public VRCUnityVideoPlayer unityPlayer;

        [Tooltip("The AVPro video player component (PC clients).")]
        public VRCAVProVideoPlayer avproPlayer;

        [Tooltip("Use the Unity player on every platform (the only player that runs in the editor).")]
        public bool preferUnityPlayer;

        [Tooltip("The URL entry field on the TV's panel.")]
        public VRCUrlInputField urlField;

        [Tooltip("Status line on the TV's panel.")]
        public Text statusText;

        [Tooltip("Seconds to wait between load attempts (VRChat's rate limit is 5 s).")]
        public float loadCooldown = 5.5f;

        [UdonSynced] private VRCUrl syncedUrl = VRCUrl.Empty;
        [UdonSynced] private int loadSerial;
        [UdonSynced] private bool syncedPlaying;
        [UdonSynced] private double startTime; // server seconds at playhead 0
        [UdonSynced] private float pausedAt;   // playhead while paused

        private BaseVRCVideoPlayer player;
        private VRCUrl currentUrl = VRCUrl.Empty;
        private int currentSerial;
        private bool videoReady;
        private bool loadQueued;
        private float lastLoadAt = -100f;
        private int retriesLeft;
        private bool driftLoopRunning;

        static string UrlText(VRCUrl u)
        {
            return u == null ? "" : u.Get();
        }

        void Start()
        {
            if (syncedUrl == null)
                syncedUrl = VRCUrl.Empty;
            if (currentUrl == null)
                currentUrl = VRCUrl.Empty;
            bool useUnity = preferUnityPlayer || avproPlayer == null;
#if UNITY_ANDROID || UNITY_IOS
            useUnity = true;
#endif
            if (useUnity)
                player = unityPlayer;
            else
                player = avproPlayer;
            if (player == null)
                player = unityPlayer != null ? (BaseVRCVideoPlayer)unityPlayer : avproPlayer;
            SetStatus(player == null ? "No video player" : "Enter a URL");
        }

        void SetStatus(string s)
        {
            if (statusText != null)
                statusText.text = s;
        }

        void TakeOwnership()
        {
            if (!Networking.IsOwner(gameObject))
                Networking.SetOwner(Networking.LocalPlayer, gameObject);
        }

        // --- Panel events ---------------------------------------------------

        /// URL field end-edit (wired by the builder as a persistent listener).
        public void OnURLChanged()
        {
            if (urlField == null)
                return;
            VRCUrl url = urlField.GetUrl();
            if (string.IsNullOrEmpty(UrlText(url)))
                return;
            TakeOwnership();
            syncedUrl = url;
            loadSerial++;
            syncedPlaying = true;
            pausedAt = 0f;
            startTime = Networking.GetServerTimeInSeconds();
            RequestSerialization();
            ApplySynced();
        }

        public void TogglePlay()
        {
            if (player == null || string.IsNullOrEmpty(UrlText(currentUrl)))
                return;
            TakeOwnership();
            if (syncedPlaying)
            {
                pausedAt = videoReady ? player.GetTime() : 0f;
                syncedPlaying = false;
            }
            else
            {
                startTime = Networking.GetServerTimeInSeconds() - pausedAt;
                syncedPlaying = true;
            }
            RequestSerialization();
            ApplySynced();
        }

        public void StopVideo()
        {
            TakeOwnership();
            syncedUrl = VRCUrl.Empty;
            loadSerial++;
            syncedPlaying = false;
            pausedAt = 0f;
            RequestSerialization();
            ApplySynced();
        }

        /// Local: re-seat the playhead on the shared clock.
        public void Resync()
        {
            if (player != null && videoReady && syncedPlaying)
            {
                player.SetTime(TargetTime());
                SetStatus("Resynced");
            }
        }

        // --- Sync ---------------------------------------------------------

        public override void OnDeserialization()
        {
            ApplySynced();
        }

        float TargetTime()
        {
            float t = (float)(Networking.GetServerTimeInSeconds() - startTime);
            return t < 0f ? 0f : t;
        }

        void ApplySynced()
        {
            if (player == null)
                return;
            if (loadSerial != currentSerial)
            {
                currentSerial = loadSerial;
                currentUrl = syncedUrl;
                videoReady = false;
                retriesLeft = 3;
                if (string.IsNullOrEmpty(UrlText(currentUrl)))
                {
                    player.Stop();
                    SetStatus("Stopped");
                    return;
                }
                QueueLoad();
                return;
            }
            if (!videoReady)
                return;
            if (syncedPlaying)
            {
                player.Play();
                float target = TargetTime();
                if (Mathf.Abs(player.GetTime() - target) > 1.5f)
                    player.SetTime(target);
                SetStatus("Playing");
            }
            else
            {
                player.Pause();
                player.SetTime(pausedAt);
                SetStatus("Paused");
            }
        }

        void QueueLoad()
        {
            float wait = loadCooldown - (Time.time - lastLoadAt);
            if (wait <= 0f)
            {
                LoadNow();
                return;
            }
            if (loadQueued)
                return;
            loadQueued = true;
            SetStatus("Loading in " + Mathf.CeilToInt(wait) + " s");
            SendCustomEventDelayedSeconds(nameof(LoadNow), wait + 0.1f);
        }

        public void LoadNow()
        {
            loadQueued = false;
            if (player == null || string.IsNullOrEmpty(UrlText(currentUrl)))
                return;
            lastLoadAt = Time.time;
            videoReady = false;
            SetStatus("Loading...");
            player.LoadURL(currentUrl);
        }

        public override void OnVideoReady()
        {
            videoReady = true;
            ApplySynced();
            if (!driftLoopRunning)
            {
                driftLoopRunning = true;
                SendCustomEventDelayedSeconds(nameof(DriftCheck), 10f);
            }
        }

        public override void OnVideoError(VideoError videoError)
        {
            videoReady = false;
            if ((videoError == VideoError.RateLimited || videoError == VideoError.PlayerError)
                && retriesLeft > 0)
            {
                retriesLeft--;
                SetStatus("Retrying (" + ErrorName(videoError) + ")");
                lastLoadAt = Time.time; // re-arm the cooldown from the failure
                QueueLoad();
                return;
            }
            SetStatus("Error: " + ErrorName(videoError));
        }

        static string ErrorName(VideoError e)
        {
            if (e == VideoError.InvalidURL) return "invalid URL";
            if (e == VideoError.AccessDenied) return "access denied - allow untrusted URLs?";
            if (e == VideoError.RateLimited) return "rate limited";
            if (e == VideoError.PlayerError) return "player error";
            return "unknown";
        }

        public override void OnVideoEnd()
        {
            if (!Networking.IsOwner(gameObject))
                return;
            syncedPlaying = false;
            pausedAt = 0f;
            RequestSerialization();
            SetStatus("Ended");
        }

        public void DriftCheck()
        {
            if (player != null && videoReady && syncedPlaying)
            {
                float target = TargetTime();
                if (Mathf.Abs(player.GetTime() - target) > 1.5f)
                    player.SetTime(target);
            }
            SendCustomEventDelayedSeconds(nameof(DriftCheck), 10f);
        }
    }
}
