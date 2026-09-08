// The kit's TV: a synced video player over the SDK's two players, with a
// default playlist and a cast of villagers who each have a favourite show.
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
// WHAT IS PLAYING, AND WHO MAY CHANGE IT. `source` says where the
// current video came from, and it is the whole arbitration rule:
//
//   SRC_PLAYLIST (0) - the default playlist, looping. This is the only
//                      state in which a villager may take the TV over.
//   SRC_SHOW (1)     - a villager's favourite. Another villager may NOT
//                      interrupt it, and neither may the playlist.
//   SRC_GUEST (2)    - a URL a player typed. Nothing but a player
//                      touches it: no villager can talk over a guest.
//
// A player always wins - the URL field and the panel buttons work from
// any state, because the person in the room outranks the simulation.
// Everything else only ever moves DOWN to the playlist: a show that ends
// or whose owner walks away returns the TV to the playlist, and so does
// a guest video that plays out. That is what keeps the rule stable
// rather than a race: nothing except the playlist can be interrupted, so
// there is never a question of who was first.
//
// A show is one or more URLs played IN ORDER, ONCE - the sequence never
// loops, because a three-part run that restarts forever is a wall, not a
// visit. The default playlist does loop.
//
// IN THE EDITOR THE SET CANNOT PLAY YOUTUBE, and that is not a fault in
// this file. ClientSim's AVPro is a stub whose LoadURL does nothing and
// whose IsReady is always false (see
// com.vrchat.worlds/Integrations/ClientSim/Runtime/Stubs/
// ClientSimAVProVideoStub.cs) - no video, no error, no event. The Unity
// player is real, but nothing in the editor resolves a youtu.be PAGE
// into a stream, so it hands the raw link to Windows Media Foundation
// and gets 0xc00d36c4, "the byte stream type of the given URL is
// unsupported". Both halves of that are environmental: in the VRChat
// client AVPro plays and VRChat's own resolver does the yt-dlp step.
// So the TV does not thrash at it. The two players are not
// interchangeable: AVPro plays site links and streams, the Unity player
// plays FILES. Handing a youtu.be page to the Unity player cannot
// work - it goes to Windows Media Foundation as a byte stream and comes
// back 0xc00d36c4 - so the watchdog only falls back to the Unity player
// for a URL that looks like a direct media file, and falls back to
// AVPro from anything. And after three give-ups in a row the set stops
// advancing and says so on the panel, instead of walking the whole
// playlist to collect one error per entry for ever. Any button clears
// that. A direct .mp4 link typed into the URL field does play in the
// editor - that is the way to see the screen light up without a
// Build & Test.
//
// ON BY DEFAULT. The set is meant to be playing when you walk in, so
// starting the playlist is not a single shot that can be missed: the
// first client re-tries while ownership settles, a new owner picks it up
// when the old one leaves, and a load that never becomes ready is not
// allowed to leave the screen dark for ever - a watchdog falls back to
// the OTHER video player once (which is also what makes the set play in
// the editor, where AVPro does nothing at all) and then steps past a
// playlist entry that will not load. If the playlist is empty the panel
// says so in those words, because an unwired TV and a switched-off one
// look exactly alike.
//
// Villagers ask through LegaiaTvWatchSpot (the NPC station in front of
// the set). Only the network OWNER's request is acted on: every client
// simulates its own villagers, so nine clients would otherwise fire nine
// requests at the same instant and fight over ownership. The visuals
// that go with a show - the floor console, the controller in the owner's
// hands - follow the SYNCED state on every client, so everyone sees the
// console when Noa's Spyro run is on, whatever their local Noa is doing.
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
        public const int SRC_PLAYLIST = 0;
        public const int SRC_SHOW = 1;
        public const int SRC_GUEST = 2;

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

        // --- Playlist + shows (builder-filled from Settings/video.settings.json) ---

        [Tooltip("Default playlist, played in order and looped. Empty = the TV starts dark.")]
        public VRCUrl[] playlistUrls;

        [Tooltip("Titles for the playlist, same order (status line only).")]
        public string[] playlistTitles;

        [Tooltip("Every show's URLs end to end - Udon has no jagged arrays; showStart/showCount window them.")]
        public VRCUrl[] showUrls;

        [Tooltip("First index in showUrls of each show.")]
        public int[] showStart;

        [Tooltip("How many URLs each show plays, in order, once.")]
        public int[] showCount;

        [Tooltip("Show titles (status line).")]
        public string[] showTitles;

        [Tooltip("Display name of each show's owner (matched against the brain's label).")]
        public string[] showOwners;

        [Tooltip("NPC token of each show's owner (\"npc_102\"), matched against the NPC object name.")]
        public string[] showTokens;

        [Tooltip("Show comes with the floor console + a controller in the owner's hands.")]
        public bool[] showConsole;

        [Tooltip("The floor console prop, shown while a console show plays.")]
        public GameObject consoleProp;

        [Tooltip("Seconds to wait after joining before the owner starts the playlist.")]
        public float autoStartDelay = 4f;

        [Tooltip("Seconds a load may take to become ready before the TV gives up on it.")]
        public float loadWatchdogSeconds = 18f;

        [Tooltip("Take the keyboard back off the URL field once the player is this far away (metres).")]
        public float keyboardReleaseDistance = 3f;

        [UdonSynced] private VRCUrl syncedUrl = VRCUrl.Empty;
        [UdonSynced] private int loadSerial;
        [UdonSynced] private bool syncedPlaying;
        [UdonSynced] private double startTime; // server seconds at playhead 0
        [UdonSynced] private float pausedAt;   // playhead while paused
        [UdonSynced] private int source;       // SRC_*
        [UdonSynced] private int listIndex;    // position in the default playlist
        [UdonSynced] private int showIndex = -1;
        [UdonSynced] private int showItem;     // position within the show

        private BaseVRCVideoPlayer player;
        private VRCUrl currentUrl = VRCUrl.Empty;
        private int currentSerial;
        private bool videoReady;
        private bool loadQueued;
        private float lastLoadAt = -100f;
        private int retriesLeft;
        private bool driftLoopRunning;
        private int pickCursor;   // round-robin over one villager's shows
        private bool consoleShown;
        private int startAttempts;
        private int loadTicket;      // one per load attempt
        private int watchdogFor = -1;
        private bool triedOtherPlayer;
        private BaseVRCVideoPlayer preferred;
        private int failures;        // loads given up on, in a row
        private bool playbackBroken; // nothing here can play these links

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
            preferred = player;
            ApplyConsole();
            if (player == null)
            {
                SetStatus("No video player");
                return;
            }
            SetStatus(PlaylistLength() > 0
                ? "Starting the playlist..."
                : "No playlist - rebuild the common prefabs");
            SendCustomEventDelayedSeconds(nameof(KeyboardGuard), 1f);
            // Late joiners get the running state through OnDeserialization;
            // the first client in the instance is the one that has to start
            // it, after a beat for ownership and the video player to settle.
            SendCustomEventDelayedSeconds(nameof(AutoStart), autoStartDelay);
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

        int PlaylistLength()
        {
            return playlistUrls == null ? 0 : playlistUrls.Length;
        }

        int ShowsLength()
        {
            return showStart == null ? 0 : showStart.Length;
        }

        /// The first client in an empty instance starts the playlist. A
        /// TV that already carries a URL (someone got here first, or a
        /// late joiner deserialized before this fired) is left alone.
        ///
        /// Re-armed rather than fired once: ownership settles a moment
        /// after a join, and "the set is on when you walk in" should not
        /// depend on winning that race. After half a minute of an empty
        /// set the MASTER takes the TV and starts it - one nominated
        /// client, so a full instance cannot stampede the ownership.
        public void AutoStart()
        {
            if (PlaylistLength() == 0)
            {
                SetStatus("No playlist - rebuild the common prefabs");
                return;
            }
            if (!string.IsNullOrEmpty(UrlText(syncedUrl)))
                return; // something is already on
            if (Networking.IsOwner(gameObject))
            {
                PlayPlaylistAt(listIndex);
                return;
            }
            startAttempts++;
            if (startAttempts >= 6)
            {
                VRCPlayerApi me = Networking.LocalPlayer;
                if (me != null && me.isMaster)
                {
                    TakeOwnership();
                    PlayPlaylistAt(listIndex);
                }
                return;
            }
            SendCustomEventDelayedSeconds(nameof(AutoStart), 5f);
        }

        /// The owner left. Whoever inherits the set keeps it running - an
        /// instance whose first player walks out should not go dark.
        public override void OnOwnershipTransferred(VRCPlayerApi newOwner)
        {
            // NB `player` is this behaviour's video player; the new owner
            // is the argument, so it does not get that name.
            if (newOwner == null || !newOwner.isLocal)
                return;
            if (PlaylistLength() == 0 || !string.IsNullOrEmpty(UrlText(syncedUrl)))
                return;
            PlayPlaylistAt(listIndex);
        }

        /// The URL field must never keep the keyboard once the player has
        /// walked off. Unity's navigation can hand a field the selection
        /// on its own (the builder switches that off), and a player who
        /// clicks in and then walks away has no obvious way to get their
        /// keys back - so the set takes them back on their behalf.
        public void KeyboardGuard()
        {
            if (urlField != null)
            {
                VRCPlayerApi me = Networking.LocalPlayer;
                if (me != null && Vector3.Distance(me.GetPosition(),
                        urlField.transform.position) > keyboardReleaseDistance)
                    ReleaseKeyboard();
            }
            SendCustomEventDelayedSeconds(nameof(KeyboardGuard), 0.5f);
        }

        /// Drop the text cursor, if the field is holding it.
        public void ReleaseKeyboard()
        {
            if (urlField != null)
                urlField.DeactivateInputField();
        }

        // --- Arbitration ----------------------------------------------------

        /// Which show belongs to this villager, or -1. `name` is the
        /// brain's display label, `objectName` the NPC root's object name
        /// (matched against the token as a prefix, "npc_14" vs
        /// "npc_14_kor3-127"). A villager with several shows gets a
        /// different one each time they visit.
        public int ShowFor(string name, string objectName)
        {
            int n = ShowsLength();
            if (n == 0)
                return -1;
            int matches = 0;
            for (int i = 0; i < n; i++)
                if (OwnsShow(i, name, objectName))
                    matches++;
            if (matches == 0)
                return -1;
            int wanted = pickCursor % matches;
            pickCursor++;
            for (int i = 0; i < n; i++)
            {
                if (!OwnsShow(i, name, objectName))
                    continue;
                if (wanted == 0)
                    return i;
                wanted--;
            }
            return -1;
        }

        /// Public form of OwnsShow: does this show belong to that
        /// villager? The watch spot asks about the show that is actually
        /// PLAYING rather than about the one its own client requested -
        /// two clients' round-robins can differ, the synced show cannot.
        public bool ShowBelongsTo(int show, string name, string objectName)
        {
            return show >= 0 && show < ShowsLength() && OwnsShow(show, name, objectName);
        }

        bool OwnsShow(int i, string name, string objectName)
        {
            if (showOwners != null && i < showOwners.Length &&
                !string.IsNullOrEmpty(showOwners[i]) && !string.IsNullOrEmpty(name) &&
                showOwners[i].ToLower() == name.ToLower())
                return true;
            if (showTokens == null || i >= showTokens.Length)
                return false;
            string token = showTokens[i];
            if (string.IsNullOrEmpty(token) || string.IsNullOrEmpty(objectName))
                return false;
            return objectName == token || objectName.StartsWith(token + "_");
        }

        /// A villager asks for their show. Accepted ONLY while the default
        /// playlist is what is playing - never over a guest's video and
        /// never over another villager's show. Returns false when the
        /// request was refused, which is the normal case and not an error.
        public bool RequestShow(int show)
        {
            // Every client simulates its own villagers; only the owner's
            // copy may drive the shared TV.
            if (!Networking.IsOwner(gameObject))
                return false;
            if (!ShowRequestAllowed(show))
                return false;
            source = SRC_SHOW;
            showIndex = show;
            showItem = 0;
            PlayUrl(ShowUrl(show, 0));
            return true;
        }

        /// THE RULE, with no network in it: may this show go on right
        /// now? Only from the default playlist, and only if the show has
        /// anything to play. Split out from RequestShow so the headless
        /// check can assert the arbitration table directly - the proxy is
        /// an ordinary MonoBehaviour in the editor, but anything calling
        /// Networking is not.
        public bool ShowRequestAllowed(int show)
        {
            if (show < 0 || show >= ShowsLength())
                return false;
            if (showCount == null || show >= showCount.Length || showCount[show] <= 0)
                return false;
            return source == SRC_PLAYLIST;
        }

        /// Where the current video came from (SRC_*). Read by the checks
        /// and by anything that wants to know whether the house playlist
        /// is what is on.
        public int Source()
        {
            return source;
        }

        /// The villager who put a show on has left (or the spot released
        /// them). Only ends the show if it is still theirs - by the time
        /// this arrives a player may have taken the TV.
        public void EndShow(int show)
        {
            if (!Networking.IsOwner(gameObject))
                return;
            if (source != SRC_SHOW || showIndex != show)
                return;
            BackToPlaylist();
        }

        public bool IsShowPlaying(int show)
        {
            return source == SRC_SHOW && showIndex == show;
        }

        /// True while the playing show is one that comes with the console.
        public bool ConsoleShowPlaying()
        {
            return source == SRC_SHOW && showIndex >= 0 && showConsole != null &&
                   showIndex < showConsole.Length && showConsole[showIndex];
        }

        /// The show currently playing, or -1.
        public int PlayingShow()
        {
            return source == SRC_SHOW ? showIndex : -1;
        }

        VRCUrl ShowUrl(int show, int item)
        {
            if (showUrls == null || show < 0 || show >= ShowsLength())
                return VRCUrl.Empty;
            int at = showStart[show] + item;
            if (item < 0 || item >= showCount[show] || at < 0 || at >= showUrls.Length)
                return VRCUrl.Empty;
            return showUrls[at];
        }

        void BackToPlaylist()
        {
            source = SRC_PLAYLIST;
            showIndex = -1;
            showItem = 0;
            if (PlaylistLength() == 0)
            {
                StopInternal();
                return;
            }
            PlayPlaylistAt(listIndex);
        }

        void PlayPlaylistAt(int index)
        {
            int n = PlaylistLength();
            if (n == 0)
            {
                StopInternal();
                return;
            }
            listIndex = ((index % n) + n) % n;
            source = SRC_PLAYLIST;
            showIndex = -1;
            PlayUrl(playlistUrls[listIndex]);
        }

        /// Common tail of every state change the owner makes: stamp the
        /// URL, bump the serial so all clients reload, start the clock.
        void PlayUrl(VRCUrl url)
        {
            syncedUrl = url == null ? VRCUrl.Empty : url;
            loadSerial++;
            syncedPlaying = true;
            pausedAt = 0f;
            startTime = Networking.GetServerTimeInSeconds();
            RequestSerialization();
            ApplySynced();
        }

        void StopInternal()
        {
            syncedUrl = VRCUrl.Empty;
            loadSerial++;
            syncedPlaying = false;
            pausedAt = 0f;
            RequestSerialization();
            ApplySynced();
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
            ClearFailures();
            source = SRC_GUEST;
            showIndex = -1;
            showItem = 0;
            PlayUrl(url);
            // Submitting is leaving: hand the keys back rather than making
            // the player find the way out of a text box.
            ReleaseKeyboard();
        }

        /// Panel button: hand the set back to the default playlist. The way
        /// out of a guest video or a show that someone would rather not
        /// sit through - and the only way back after Stop.
        public void PlayPlaylist()
        {
            if (PlaylistLength() == 0)
                return;
            TakeOwnership();
            ClearFailures();
            source = SRC_PLAYLIST;
            showIndex = -1;
            showItem = 0;
            PlayPlaylistAt(listIndex);
        }

        /// Panel button: next item of the playlist (from anywhere - it
        /// lands on the playlist, which is what "skip" means here).
        public void NextInPlaylist()
        {
            if (PlaylistLength() == 0)
                return;
            TakeOwnership();
            ClearFailures();
            source = SRC_PLAYLIST;
            showIndex = -1;
            showItem = 0;
            PlayPlaylistAt(listIndex + 1);
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
            // Stop is a GUEST state: the TV stays off until a player asks
            // for something. A villager may not switch it back on - the
            // playlist is not playing, so the arbitration rule holds.
            source = SRC_GUEST;
            showIndex = -1;
            showItem = 0;
            StopInternal();
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
            ApplyConsole();
            if (player == null)
                return;
            if (loadSerial != currentSerial)
            {
                currentSerial = loadSerial;
                currentUrl = syncedUrl;
                videoReady = false;
                retriesLeft = 3;
                // Each video starts on the preferred player: a fallback is
                // for THIS load, so one awkward link never degrades the
                // set for the rest of the session.
                triedOtherPlayer = false;
                if (preferred != null)
                    player = preferred;
                if (string.IsNullOrEmpty(UrlText(currentUrl)))
                {
                    player.Stop();
                    SetStatus("Off");
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
                SetStatus(NowPlaying());
            }
            else
            {
                player.Pause();
                player.SetTime(pausedAt);
                SetStatus("Paused - " + NowPlaying());
            }
        }

        /// The console prop follows the SYNCED show, so every client shows
        /// it at the same time whatever their own villagers are doing.
        void ApplyConsole()
        {
            if (consoleProp == null)
                return;
            bool want = ConsoleShowPlaying();
            if (want == consoleShown)
                return;
            consoleShown = want;
            consoleProp.SetActive(want);
        }

        /// The status line: what is on, and whose choice it was.
        public string NowPlaying()
        {
            if (source == SRC_SHOW && showIndex >= 0)
            {
                string who = showOwners != null && showIndex < showOwners.Length
                    ? showOwners[showIndex] : "";
                string title = showTitles != null && showIndex < showTitles.Length
                    ? showTitles[showIndex] : "";
                string part = showCount != null && showIndex < showCount.Length &&
                              showCount[showIndex] > 1
                    ? " (" + (showItem + 1) + "/" + showCount[showIndex] + ")"
                    : "";
                return (string.IsNullOrEmpty(who) ? "Someone" : who) + "'s pick: " +
                       title + part;
            }
            if (source == SRC_GUEST)
                return "Playing a guest's video";
            int n = PlaylistLength();
            if (n == 0)
                return "Enter a URL";
            string name = playlistTitles != null && listIndex < playlistTitles.Length
                ? playlistTitles[listIndex] : "";
            return "Playlist " + (listIndex + 1) + "/" + n +
                   (string.IsNullOrEmpty(name) ? "" : " - " + name);
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
            loadTicket++;
            watchdogFor = loadTicket;
            SendCustomEventDelayedSeconds(nameof(LoadWatchdog), loadWatchdogSeconds);
            player.LoadURL(currentUrl);
        }

        /// A load that never answers. OnVideoError covers the failures the
        /// player reports; this covers the ones it does not - AVPro in the
        /// editor, where nothing happens at all and no error is ever
        /// raised, and a resolve that hangs. The other player gets one try
        /// (both are wired to the same screen material and the same
        /// speaker, so either can drive the set), and after that a
        /// playlist entry that will not come up is stepped past.
        public void LoadWatchdog()
        {
            if (watchdogFor != loadTicket)
                return; // a newer load owns the watch now
            if (videoReady || string.IsNullOrEmpty(UrlText(currentUrl)))
                return;
            BaseVRCVideoPlayer other = OtherPlayer();
            if (!triedOtherPlayer && other != null && FallbackCanHelp(other))
            {
                triedOtherPlayer = true;
                if (player != null)
                    player.Stop();
                player = other;
                SetStatus("Switching video player...");
                LoadNow();
                return;
            }
            SetStatus(DirectFile(UrlText(currentUrl))
                ? "That video did not load"
                : "This link needs the VRChat client to resolve it");
            GaveUp();
        }

        /// The player this TV is NOT using, when it has one.
        BaseVRCVideoPlayer OtherPlayer()
        {
            if (player == (BaseVRCVideoPlayer)unityPlayer)
                return avproPlayer;
            return unityPlayer;
        }

        /// Is it worth handing this URL to the other player? AVPro always
        /// is - it is the one that plays site links and streams. The Unity
        /// player only plays a FILE, so giving it a youtu.be page is not a
        /// fallback, it is a second way to fail (and a Media Foundation
        /// error in the log to go with it). Nothing in the editor resolves
        /// a page into a stream, which is exactly where that used to bite.
        bool FallbackCanHelp(BaseVRCVideoPlayer other)
        {
            if (other == (BaseVRCVideoPlayer)avproPlayer)
                return true;
            return DirectFile(UrlText(currentUrl));
        }

        /// A URL that names a media file rather than a page to be
        /// resolved. Deliberately generous - a query string after the
        /// extension is normal - because the cost of being wrong is one
        /// extra load attempt, not a wrong answer.
        static bool DirectFile(string url)
        {
            if (string.IsNullOrEmpty(url))
                return false;
            string u = url.ToLower();
            return u.Contains(".mp4") || u.Contains(".webm") || u.Contains(".mov") ||
                   u.Contains(".m4v") || u.Contains(".mkv") || u.Contains(".ogv") ||
                   u.Contains(".avi") || u.Contains(".m3u8") || u.Contains(".mpd");
        }

        /// One load abandoned. A single bad link in the playlist is
        /// stepped past; three in a row means it is not the link, it is
        /// the environment - the editor, a client with video off - and
        /// walking the rest of the playlist would only collect one error
        /// per entry, for ever. So the set stops and says so, and any
        /// button starts it again.
        void GaveUp()
        {
            failures++;
            if (failures >= 3)
            {
                playbackBroken = true;
                SetStatus("No video is playing here - in the Unity editor " +
                          "that is expected (Build & Test to see video)");
                return;
            }
            if (Networking.IsOwner(gameObject) && source == SRC_PLAYLIST &&
                PlaylistLength() > 1)
                SendCustomEventDelayedSeconds(nameof(Advance), 6f);
        }

        /// A person pressed something: whatever was wrong, try again.
        void ClearFailures()
        {
            failures = 0;
            playbackBroken = false;
        }

        public override void OnVideoReady()
        {
            videoReady = true;
            failures = 0;
            playbackBroken = false;
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
            // A dead link in the playlist must not end the evening: step
            // past it. Guest videos and shows stay on the error, which is
            // the honest answer to "why is nothing playing" - somebody
            // asked for that URL.
            GaveUp();
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
            Advance();
        }

        /// What follows the video that just finished. The playlist loops;
        /// a show plays its parts in order and then hands the set back;
        /// a guest video hands it back too, so the room is never left
        /// staring at a stopped player.
        public void Advance()
        {
            if (!Networking.IsOwner(gameObject))
                return;
            if (playbackBroken)
                return;
            if (source == SRC_SHOW && showIndex >= 0)
            {
                int next = showItem + 1;
                if (next < showCount[showIndex])
                {
                    showItem = next;
                    PlayUrl(ShowUrl(showIndex, next));
                    return;
                }
                BackToPlaylist();
                return;
            }
            if (source == SRC_GUEST)
            {
                BackToPlaylist();
                return;
            }
            PlayPlaylistAt(listIndex + 1);
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
