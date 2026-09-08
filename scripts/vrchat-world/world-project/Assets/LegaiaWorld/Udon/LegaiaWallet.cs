// The player's coin purse: one behaviour per world (built under
// Legaia_common_prefabs as `wallet`), holding the LOCAL player's coins
// and persisting them through VRChat's PlayerData, so the balance a
// player leaves the world with is the one they come back to - on any
// instance, on any day.
//
// Every minigame in the kit pays into and out of this purse: the casino
// slot machine debits a spin and credits a payout, the card table's bets
// settle here, a catch on a fishing spot adds a few coins, and a coin
// drop picked up off the ground adds its value. None of them keep a
// balance of their own any more.
//
// Readers: `Coins()` is the local player's balance; `CoinsOf(player)`
// reads ANY player's purse straight from PlayerData (VRChat replicates
// PlayerData to every client, so a table can show what each seated
// player holds without a synced variable). Writers are local-only by
// construction: PlayerData.SetInt writes the local player's own record
// and nobody else's, which is also the anti-cheat model - a client can
// only ever change its own purse.
//
// Restore order: PlayerData is not readable until OnPlayerRestored fires
// for the local player (a few seconds after join). Until then the purse
// holds `startingCoins` in memory and every change is kept locally;
// the first commit after the restore writes it through. A player with no
// record yet (their first visit) is seeded with `startingCoins` - retail's
// 70-coin casino entry, the same fallback the slot machine used to keep
// per machine.
//
// `serial` bumps on every change, so a display can poll cheaply
// (compare, not subscribe: Udon has no events between behaviours).
//
// Requires UdonSharp (bundled with the VRChat worlds SDK) and a worlds
// SDK with Persistence (3.7.4+).

using UdonSharp;
using UnityEngine;
using VRC.SDK3.Persistence;
using VRC.SDKBase;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaWallet : UdonSharpBehaviour
    {
        [Tooltip("PlayerData key the balance is stored under.")]
        public string key = "legaia.coins";

        [Tooltip("Coins a first-time visitor starts with (retail's casino entry balance).")]
        public int startingCoins = 70;

        [Tooltip("The purse never holds more than this.")]
        public int coinCap = 9999999;

        [Tooltip("Optional world-space label kept at 'COINS n' (a HUD somewhere near spawn).")]
        public TMPro.TextMeshPro hudText;

        [Tooltip("Local player's balance (read through Coins()).")]
        [HideInInspector] public int coins;

        [Tooltip("True once PlayerData has been restored for the local player and writes go through.")]
        [HideInInspector] public bool restored;

        [Tooltip("Bumps on every balance change - pollers compare it.")]
        [HideInInspector] public int serial;

        [Tooltip("Coins earned in this session (statistics for panels; not persisted).")]
        [HideInInspector] public int earnedThisSession;

        [Tooltip("Coins spent in this session (statistics for panels; not persisted).")]
        [HideInInspector] public int spentThisSession;

        void Start()
        {
            coins = startingCoins;
            Refresh();
        }

        public override void OnPlayerRestored(VRCPlayerApi player)
        {
            if (player == null || !player.isLocal)
                return;
            int v;
            if (PlayerData.TryGetInt(player, key, out v))
                coins = v < 0 ? 0 : (v > coinCap ? coinCap : v);
            else
                coins = startingCoins;
            restored = true;
            // Write the seed (or the clamp) through so the record exists
            // from the first visit on.
            PlayerData.SetInt(key, coins);
            serial++;
            Refresh();
        }

        /// The local player's balance.
        public int Coins()
        {
            return coins;
        }

        /// Can the local player pay `n`?
        public bool CanAfford(int n)
        {
            return n <= 0 || coins >= n;
        }

        /// Take `n` coins from the local player. False (and nothing taken)
        /// when the purse is short.
        public bool Spend(int n)
        {
            if (n < 0)
                return false;
            if (coins < n)
                return false;
            coins -= n;
            spentThisSession += n;
            Commit();
            return true;
        }

        /// Give the local player `n` coins.
        public void Add(int n)
        {
            if (n <= 0)
                return;
            coins += n;
            if (coins > coinCap)
                coins = coinCap;
            earnedThisSession += n;
            Commit();
        }

        /// Any player's balance, straight from their PlayerData (0 when
        /// they have no record yet or their data has not arrived).
        public int CoinsOf(VRCPlayerApi p)
        {
            if (p == null || !p.IsValid())
                return 0;
            if (p.isLocal)
                return coins;
            int v;
            return PlayerData.TryGetInt(p, key, out v) ? v : 0;
        }

        void Commit()
        {
            serial++;
            if (restored)
                PlayerData.SetInt(key, coins);
            Refresh();
        }

        void Refresh()
        {
            if (hudText != null)
                hudText.text = "COINS " + coins;
        }
    }
}
