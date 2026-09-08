// What the villagers SAY at the card table - written for the world, not
// lifted from it.
//
// The seat panel used to print each villager's manifest label, which is
// the first line of that actor's retail dialogue: a cutscene fragment
// ("Tetsu: You were a child when the", "I am a dummy.", "Caw, caw."),
// random and mostly meaningless out of its scene. Retail Rim Elm has no
// card table, so there is nothing to lift; the lines here are NEW and
// merely faithful - light, generic table talk that lives where Legend of
// Legaia lives (the Mist and the wall around Rim Elm, the Genesis Tree,
// hunters, Seru, Biron Monastery up the road, Hunter's Spring), with a
// villager's name in front of it (LegaiaLivingTown names the villagers).
//
// Three characters get their own voice. VAHN never speaks in retail - the
// player picks his answers - so his lines are stage directions ("*Vahn
// nods.*") and never a quoted word. NOA was raised by the wolf Terra in
// Snowdrift Cave, speaks in bursts, and calls herself Noa. CARA is Rim
// Elm's bold hunter-type girl, and the card table's night game is HERS:
// she deals it, she posts the four rules on the board beside the table
// (deal five, best hand wins, no cheating, have fun), and she talks like
// somebody who would rather be out at the treeline - short, dry, sure of
// herself, never a shopkeeper's patter. And the
// villagers notice them: when either sits at the table the others get
// a pool of lines ABOUT them (Val's boy, the wolf girl), which is what
// makes the pair feel like they belong to the town rather than to a
// roster. Which pool applies is decided by the seat's `label` - "Vahn"
// and "Noa", case-insensitive - so the settings file's `add_npcs` label
// is the switch, and any other villager named Noa would inherit the
// voice too (a feature, not a bug).
//
// Pure: `Compose` reads its arrays and the `salt` it is handed and
// keeps only a last-index memory per kind so the same line is never
// printed twice in a row. The card game (the table's master) calls it
// and syncs the resulting string, so every client shows the same words
// without ever running this itself.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaTableTalk : UdonSharpBehaviour
    {
        // Kinds. The game passes one of these with the seat that acts.
        public const int T_SIT = 0;      // a villager takes a stool
        public const int T_DEAL = 1;     // the cards come out
        public const int T_RAISE = 2;
        public const int T_CALL = 3;
        public const int T_FOLD = 4;
        public const int T_WIN = 5;      // took the pot / beat the dealer
        public const int T_LOSE = 6;
        public const int T_BUST = 7;     // blackjack: over 21
        public const int T_NATURAL = 8;  // blackjack: 21 off the deal
        public const int T_IDLE = 9;     // between hands
        public const int T_PLAYER = 10;  // a real player sat down
        public const int T_LEAVE = 11;   // a villager stands up
        public const int T_FLOP = 12;    // hold'em: the three come over
        public const int T_TURN = 13;    // hold'em: the fourth
        public const int T_RIVER = 14;   // hold'em: the fifth
        public const int T_BADBEAT = 15; // showed a made hand and still lost
        public const int T_NIGHT = 16;   // Cara opens her poker night
        public const int KIND_COUNT = 17;

        [Tooltip("Longest line a seat's panel row can hold (characters); longer pool entries are still shown, this only bounds the checks).")]
        public int maxLength = 104;

        // --- villagers ----------------------------------------------------------

        private string[] sit =
        {
            "Room for one more? And no Mist talk, it spoils the mood.",
            "One hand before the wall goes dark.",
            "Deal me in. Beats staring at the wall all evening.",
            "Just a few hands. The Elder frowns on cards under the Genesis Tree.",
            "My mother says cards are for people who don't hunt. She's right, of course.",
            "Fine, I'll play. But I'm keeping my supper money.",
        };
        private string[] deal =
        {
            "Cards out. Let's see what the tree gave me.",
            "Hm. Could be worse. Could be a Seru.",
            "Don't look at my hand, the Mist isn't THAT thick.",
            "Come on, come on... one good pair.",
            "I've seen better hands on a scarecrow.",
            "Cards are like the weather off the sea. You take what comes.",
        };
        private string[] raise =
        {
            "Raise. Val always said, be bold or stay behind the wall.",
            "I'll raise. Don't give me that look.",
            "Two more. I can feel it.",
            "Raising. Hunter's luck.",
            "Let's make this interesting. Two coins.",
        };
        private string[] call =
        {
            "Call. Just to see.",
            "I'll match that.",
            "Fine, I'm in.",
            "Call. My father would have folded. I'm not my father.",
        };
        private string[] fold =
        {
            "Fold. Not worth the coins.",
            "I'm out. Save it for the market.",
            "Nope. Not with these.",
            "I know when a hunt is lost. Fold.",
            "Take it. I've got firewood to cut anyway.",
        };
        private string[] win =
        {
            "Ha! That's mine!",
            "The Genesis Tree smiles on me tonight.",
            "That'll buy a good meal at the inn.",
            "Told you. Hunter's luck.",
            "Don't tell the Elder where these coins came from.",
        };
        private string[] lose =
        {
            "Blast it. That was my fish money.",
            "Next hand. The wall's not going anywhere.",
            "Every time. Every single time.",
            "Well. There goes the rope I was saving for.",
            "You'd think the Mist got into my cards.",
        };
        private string[] bust =
        {
            "Bust! Too greedy, too greedy.",
            "One too many. Story of my life.",
            "Ugh. Over twenty-one. Again.",
        };
        private string[] natural =
        {
            "Twenty-one! Right off the deal!",
            "Look at that! A hunter's eye for the count.",
            "Twenty-one. Somebody pinch me.",
        };
        private string[] idle =
        {
            "Quiet night. No Seru at the wall, at least.",
            "Anyone seen Mei? She promised me a new coat.",
            "They say the Mist is thinner near Hunter's Spring these days.",
            "The Genesis Tree looked brighter this morning, didn't it?",
            "Tetsu's still up at the monastery? Biron's a long walk.",
            "Deal already, before the torches burn out.",
            "Val's leg is healing, I hear. Good man, that.",
            "You ever wonder what's past the Mist? ... No. Me neither.",
            "I had a fish on the line THIS big. This big!",
            "Nene was asking after her brother again.",
            "Juno still won't go near the gate. Can't say I blame him.",
        };
        private string[] player =
        {
            "A stranger at the table! How'd you get in, over the wall?",
            "You're not from Rim Elm. Sit, sit. Coins are coins.",
            "Careful, newcomer. We play for keeps here.",
            "Welcome to the table. The Elder needn't know.",
        };
        private string[] leave =
        {
            "I'm off. My supper's getting cold.",
            "Enough for me. Good hands, all.",
            "Off to check the wall. Deal me in tomorrow.",
        };
        private string[] flop =
        {
            "Three at once. Now we'll see who was bluffing.",
            "Ohh. That helps somebody. Not me.",
            "Look at that middle card. Somebody just woke up.",
            "Three on the felt and my hand got no better.",
        };
        private string[] turn =
        {
            "One more. Come on, be kind.",
            "That card changes everything. For someone.",
            "Fourth one down. Anybody still breathing?",
        };
        private string[] river =
        {
            "Last card. No more excuses after this.",
            "There it is. Live or die on that one.",
            "The river gives and the river takes. Mostly takes.",
        };
        private string[] badbeat =
        {
            "I had it! I HAD it, right up to the last card!",
            "You beat THAT? With THAT? Deal again, quickly.",
            "A good hand and it still loses. That's the Mist for you.",
            "I'll be thinking about that one all winter.",
        };
        private string[] night =
        {
            "Cara's rules tonight - five cards, best hand, no cheating.",
            "Night game, then. Five each and no swapping.",
            "Five cards and one round. Cara's way. Fine by me.",
        };

        // What the villagers say about the two heroes when they are at the
        // table (spoken INSTEAD of a generic sit / deal / idle line, every
        // other time, while the hero sits).
        private string[] aboutVahn =
        {
            "Val's boy at the cards table! Don't tell your father, Vahn.",
            "Say something, Vahn. Just once.",
            "He doesn't say much, that Vahn. Never has.",
            "That's Meta on his arm, isn't it? A Ra-Seru at a card table. What a world.",
            "Vahn, Nene was looking for you. She had that look.",
            "Since the Genesis Tree woke, that boy's been different. Quieter, if that's possible.",
        };
        private string[] aboutNoa =
        {
            "That's the girl from Snowdrift Cave, isn't it? Raised by a wolf, they say.",
            "Noa, dear, you can't eat the cards.",
            "She talks about Noa like Noa is someone else. Wolves, I suppose.",
            "The wolf girl plays cards now. Wonders never cease.",
            "Careful, Noa's quicker than she looks. Ask any rabbit on the mountain.",
            "Where's that wolf of hers? Not under the table, I hope.",
        };
        private string[] aboutBoth =
        {
            "Vahn AND Noa at the table? Somebody fetch the Elder's ale.",
            "The quiet one and the loud one. This should be a hand to watch.",
            "Look at those two. The whole village talks about nothing else.",
        };

        // --- Vahn: stage directions, never a spoken word -------------------------

        private string[] vahnSit =
        {
            "*Vahn takes the stool without a word.*",
            "*Vahn nods to the table and sits.*",
        };
        private string[] vahnDeal =
        {
            "*Vahn studies his cards the way he'd study the wall at dusk.*",
            "*Vahn says nothing. He never does.*",
        };
        private string[] vahnRaise =
        {
            "*Vahn pushes two coins forward. Meta glows on his arm.*",
            "*Vahn raises. Nobody can read his face.*",
        };
        private string[] vahnCall =
        {
            "*Vahn calls with a small nod.*",
            "*Vahn matches the bet and waits.*",
        };
        private string[] vahnFold =
        {
            "*Vahn lays his cards down, face down.*",
            "*Vahn folds. His eyes go to the gate.*",
        };
        private string[] vahnWin =
        {
            "*Vahn gathers the pot. He almost smiles.*",
            "*Vahn takes the pot without a word. Nene would be proud.*",
        };
        private string[] vahnLose =
        {
            "*Vahn shrugs. Hunting was never about the prize either.*",
            "*Vahn watches the coins go. Not a flicker.*",
        };
        private string[] vahnBust =
        {
            "*Vahn looks at the extra card a long moment, then sets it down.*",
            "*Vahn turns over twenty-two and says nothing at all.*",
        };
        private string[] vahnNatural =
        {
            "*Twenty-one. Vahn taps the table once.*",
            "*Vahn shows twenty-one. Meta flickers.*",
        };
        private string[] vahnIdle =
        {
            "*Vahn glances toward the gate, then back to the cards.*",
            "*Meta murmurs something only Vahn can hear.*",
            "*Vahn waits. He is good at waiting.*",
        };
        private string[] vahnPlayer =
        {
            "*Vahn looks up at the newcomer and nods once.*",
            "*Vahn slides over to make room. Still nothing said.*",
        };
        private string[] vahnLeave =
        {
            "*Vahn stands and goes, the way he came.*",
        };
        private string[] vahnFlop =
        {
            "*Vahn looks at the three cards, then at everyone else's faces.*",
            "*The flop lands. Vahn does not move at all.*",
        };
        private string[] vahnTurn =
        {
            "*Vahn watches the fourth card turn and taps the felt once.*",
            "*Vahn's eyes go to the board, then back down. Nothing given away.*",
        };
        private string[] vahnRiver =
        {
            "*The last card turns. Vahn is already looking at the pot.*",
            "*Vahn reads the river and lets out a breath nobody hears.*",
        };
        private string[] vahnBadbeat =
        {
            "*Vahn turns over a hand that should have won, and lets it go.*",
            "*Vahn looks a long moment at the board, then pushes his cards away.*",
        };
        private string[] vahnNight =
        {
            "*Vahn nods at the rules on the board and takes his five.*",
            "*Night game. Vahn settles in without a word, as usual.*",
        };

        // --- Noa: quick, loud, raised by a wolf --------------------------------

        private string[] noaSit =
        {
            "Noa wants to play too! What are the little pictures for?",
            "Terra says be careful with strangers. Noa is careful. Mostly.",
            "Move over! Noa sits here now.",
        };
        private string[] noaDeal =
        {
            "Cards! Noa has cards! Are they good ones?",
            "These are prettier than the ones at the cave. Noa had rocks.",
            "Noa smells a good hand. Noa can smell things.",
        };
        private string[] noaRaise =
        {
            "More! Noa bets more! Terra said never back down.",
            "Noa raises. Noa does not know what raise means but it sounds strong!",
        };
        private string[] noaCall =
        {
            "Noa follows. Like tracking a rabbit.",
            "Noa calls! ... Who is Noa calling?",
        };
        private string[] noaFold =
        {
            "Noa folds. Noa is NOT sulking.",
            "Bad cards. Noa gives them back.",
        };
        private string[] noaWin =
        {
            "Noa wins! Noa wins! Did everyone see?",
            "Ha! Terra taught Noa to hunt. Cards are just slow rabbits.",
        };
        private string[] noaLose =
        {
            "Noa lost? ... Noa will win the next one. And the next one.",
            "Hmph. The cave never cheated Noa.",
        };
        private string[] noaBust =
        {
            "Too many! Why did nobody tell Noa twenty-one was the top?",
            "Noa took one more. Noa always takes one more.",
        };
        private string[] noaNatural =
        {
            "Twenty-one! First try! Is that the best? Noa did the best!",
        };
        private string[] noaIdle =
        {
            "Is it lunch yet? Noa could eat a whole fish.",
            "It is warm here. The cave was never warm.",
            "The Mist is far away today. Good.",
            "Vahn is quiet. Noa talks for both.",
        };
        private string[] noaPlayer =
        {
            "A new face! Noa is Noa. Who are you?",
            "Sit, sit! Noa will teach you. Noa learned yesterday.",
        };
        private string[] noaLeave =
        {
            "Noa is going. Noa smells fish!",
        };
        private string[] noaFlop =
        {
            "Three more! Are those Noa's too? No? Whose are they?",
            "Everyone gets those? That is a strange way to hunt.",
        };
        private string[] noaTurn =
        {
            "Another one! Noa likes this game. So many cards.",
            "Four. Noa counted. Noa can count to four easily.",
        };
        private string[] noaRiver =
        {
            "Last one! Terra said the last step matters most.",
            "That is all of them? Noa wanted more.",
        };
        private string[] noaBadbeat =
        {
            "Noa had the good one! The good one lost! How?",
            "Terra never warned Noa about this. Noa is telling Terra.",
        };
        private string[] noaNight =
        {
            "Night cards! Noa knows this one. Five, and the best wins.",
            "Noa likes night. Noa sees better than all of you anyway.",
        };

        // --- Cara: Rim Elm's hunter girl, and the night game is hers ------------

        private string[] caraSit =
        {
            "Shove up. I've been on my feet since the treeline.",
            "One seat, one hand, and I'll take somebody's coins.",
            "Room? Good. I don't ask twice.",
        };
        private string[] caraDeal =
        {
            "Cards out. Nobody peek, I'm watching all four of you.",
            "Right. Let's see who came to play and who came to sit.",
            "Fine hand. Or it will be by the end. Watch.",
        };
        private string[] caraRaise =
        {
            "Raise. You can fold now or fold later, up to you.",
            "Two more. I've tracked worse odds up the mountain.",
            "Up it goes. Don't look so wounded.",
        };
        private string[] caraCall =
        {
            "Call. I want to see this through.",
            "Matched. Go on then.",
            "I'll pay to look. I usually do.",
        };
        private string[] caraFold =
        {
            "Out. Even a good hunter walks away from a bad trail.",
            "Not this one. Take it and be smug quietly.",
            "Fold. I've wasted arrows on worse.",
        };
        private string[] caraWin =
        {
            "Mine. Told you I don't miss twice.",
            "That's the pot and the last word. Deal again.",
            "Ha! Somebody get the Elder, I'm buying.",
        };
        private string[] caraLose =
        {
            "Hmph. Fine. That one was yours.",
            "Take it. I'll have it back before the torches go out.",
            "Beaten. Say one word about it and you're on the wall watch.",
        };
        private string[] caraBust =
        {
            "Over. Serves me right for being greedy.",
            "One card too many. I never learn.",
        };
        private string[] caraNatural =
        {
            "Twenty-one, first look. Some of us are just quick.",
            "Straight off the deal. Don't hate me.",
        };
        private string[] caraIdle =
        {
            "Quiet out there tonight. Too quiet, if you ask me.",
            "I set snares on the north path. Nobody touch them.",
            "Sitting still is the hardest hunting there is.",
            "If the torches burn out we play by the moon. I don't mind.",
        };
        private string[] caraPlayer =
        {
            "New face. Good. The others had stopped being interesting.",
            "Sit down then. I'll go easy for one hand. One.",
        };
        private string[] caraLeave =
        {
            "That's me. Dawn comes early and the deer don't wait.",
            "Enough. Somebody rack the stools.",
        };
        private string[] caraFlop =
        {
            "Three down the middle. Now you all get honest.",
            "There's the flop. Somebody just found their nerve.",
        };
        private string[] caraTurn =
        {
            "Fourth card. Still time to run, if you're the running sort.",
            "That one's interesting. Interesting for me, anyway.",
        };
        private string[] caraRiver =
        {
            "Last card. No more hiding behind it.",
            "River's out. Show me what you came with.",
        };
        private string[] caraBadbeat =
        {
            "I had that won two cards ago. Two!",
            "Beaten on the last card. I'd rather be gored.",
        };
        private string[] caraNight =
        {
            "My night, my rules: five cards, best hand, no cheating, have fun.",
            "Cara's poker night. Five each, one round, and I'll know if you cheat.",
            "Read the board: deal five, best hand takes it, play straight, enjoy it.",
        };

        private int[] lastPick;

        void Start()
        {
            EnsureMemory();
        }

        void EnsureMemory()
        {
            if (lastPick != null && lastPick.Length == KIND_COUNT + 3)
                return;
            lastPick = new int[KIND_COUNT + 3];
            for (int i = 0; i < lastPick.Length; i++)
                lastPick[i] = -1;
        }

        /// One line for the panel, `"Name: words"` (or a bare stage
        /// direction for Vahn), or "" when nothing fits. `speaker` is the
        /// seat's display name, `others` the lower-case labels of everyone
        /// else at the table joined by '|' (so a villager can talk ABOUT the
        /// heroes), `salt` any int the caller varies per call.
        public string Compose(int kind, string speaker, string others, int salt)
        {
            EnsureMemory();
            if (kind < 0 || kind >= KIND_COUNT)
                return "";
            if (speaker == null || speaker.Length == 0)
                speaker = "Villager";
            string who = speaker.ToLower();
            if (others == null)
                others = "";

            string[] pool = null;
            int memory = kind;
            if (who == "vahn")
                pool = VahnPool(kind);
            else if (who == "noa")
                pool = NoaPool(kind);
            else if (who == "cara")
                pool = CaraPool(kind);
            if (pool == null)
            {
                // A villager: every other sit / deal / idle line while a
                // hero sits is about the hero.
                bool vahnHere = Has(others, "vahn");
                bool noaHere = Has(others, "noa");
                bool chatty = kind == T_SIT || kind == T_DEAL || kind == T_IDLE;
                if (chatty && (vahnHere || noaHere) && (salt & 1) == 0)
                {
                    if (vahnHere && noaHere && (salt & 2) == 0)
                    {
                        pool = aboutBoth;
                        memory = KIND_COUNT + 2;
                    }
                    else if (vahnHere && (!noaHere || (salt & 4) == 0))
                    {
                        pool = aboutVahn;
                        memory = KIND_COUNT;
                    }
                    else
                    {
                        pool = aboutNoa;
                        memory = KIND_COUNT + 1;
                    }
                }
                else
                    pool = VillagerPool(kind);
            }
            if (pool == null || pool.Length == 0)
                return "";

            int idx = Pick(pool.Length, memory, salt);
            string line = pool[idx];
            if (line.StartsWith("*"))
                return line.Replace("*", "");
            return speaker + ": " + line;
        }

        /// True when `others` (lower-case labels joined by '|') names `who`.
        public bool Has(string others, string who)
        {
            if (others == null || others.Length == 0)
                return false;
            return others == who || others.StartsWith(who + "|") ||
                   others.EndsWith("|" + who) || others.Contains("|" + who + "|");
        }

        int Pick(int n, int memory, int salt)
        {
            if (n <= 1)
                return 0;
            int h = salt * 1664525 + 1013904223;
            h ^= (h >> 13);
            h = h * 1274126177;
            h ^= (h >> 16);
            int idx = (h & 0x7FFFFFFF) % n;
            if (idx == lastPick[memory])
                idx = (idx + 1) % n;
            lastPick[memory] = idx;
            return idx;
        }

        string[] VillagerPool(int kind)
        {
            switch (kind)
            {
                case T_SIT: return sit;
                case T_DEAL: return deal;
                case T_RAISE: return raise;
                case T_CALL: return call;
                case T_FOLD: return fold;
                case T_WIN: return win;
                case T_LOSE: return lose;
                case T_BUST: return bust;
                case T_NATURAL: return natural;
                case T_IDLE: return idle;
                case T_PLAYER: return player;
                case T_LEAVE: return leave;
                case T_FLOP: return flop;
                case T_TURN: return turn;
                case T_RIVER: return river;
                case T_BADBEAT: return badbeat;
                case T_NIGHT: return night;
            }
            return null;
        }

        string[] VahnPool(int kind)
        {
            switch (kind)
            {
                case T_SIT: return vahnSit;
                case T_DEAL: return vahnDeal;
                case T_RAISE: return vahnRaise;
                case T_CALL: return vahnCall;
                case T_FOLD: return vahnFold;
                case T_WIN: return vahnWin;
                case T_LOSE: return vahnLose;
                case T_BUST: return vahnBust;
                case T_NATURAL: return vahnNatural;
                case T_IDLE: return vahnIdle;
                case T_PLAYER: return vahnPlayer;
                case T_LEAVE: return vahnLeave;
                case T_FLOP: return vahnFlop;
                case T_TURN: return vahnTurn;
                case T_RIVER: return vahnRiver;
                case T_BADBEAT: return vahnBadbeat;
                case T_NIGHT: return vahnNight;
            }
            return null;
        }

        string[] NoaPool(int kind)
        {
            switch (kind)
            {
                case T_SIT: return noaSit;
                case T_DEAL: return noaDeal;
                case T_RAISE: return noaRaise;
                case T_CALL: return noaCall;
                case T_FOLD: return noaFold;
                case T_WIN: return noaWin;
                case T_LOSE: return noaLose;
                case T_BUST: return noaBust;
                case T_NATURAL: return noaNatural;
                case T_IDLE: return noaIdle;
                case T_PLAYER: return noaPlayer;
                case T_LEAVE: return noaLeave;
                case T_FLOP: return noaFlop;
                case T_TURN: return noaTurn;
                case T_RIVER: return noaRiver;
                case T_BADBEAT: return noaBadbeat;
                case T_NIGHT: return noaNight;
            }
            return null;
        }

        string[] CaraPool(int kind)
        {
            switch (kind)
            {
                case T_SIT: return caraSit;
                case T_DEAL: return caraDeal;
                case T_RAISE: return caraRaise;
                case T_CALL: return caraCall;
                case T_FOLD: return caraFold;
                case T_WIN: return caraWin;
                case T_LOSE: return caraLose;
                case T_BUST: return caraBust;
                case T_NATURAL: return caraNatural;
                case T_IDLE: return caraIdle;
                case T_PLAYER: return caraPlayer;
                case T_LEAVE: return caraLeave;
                case T_FLOP: return caraFlop;
                case T_TURN: return caraTurn;
                case T_RIVER: return caraRiver;
                case T_BADBEAT: return caraBadbeat;
                case T_NIGHT: return caraNight;
            }
            return null;
        }

        /// The longest entry across every pool (the checks bound it against
        /// what the panel can hold).
        public int LongestLine()
        {
            int best = 0;
            for (int k = 0; k < KIND_COUNT; k++)
            {
                best = Mathf.Max(best, Longest(VillagerPool(k)));
                best = Mathf.Max(best, Longest(VahnPool(k)));
                best = Mathf.Max(best, Longest(NoaPool(k)));
                best = Mathf.Max(best, Longest(CaraPool(k)));
            }
            best = Mathf.Max(best, Longest(aboutVahn));
            best = Mathf.Max(best, Longest(aboutNoa));
            best = Mathf.Max(best, Longest(aboutBoth));
            return best;
        }

        int Longest(string[] pool)
        {
            int best = 0;
            if (pool == null)
                return 0;
            for (int i = 0; i < pool.Length; i++)
                if (pool[i] != null && pool[i].Length > best)
                    best = pool[i].Length;
            return best;
        }
    }
}
