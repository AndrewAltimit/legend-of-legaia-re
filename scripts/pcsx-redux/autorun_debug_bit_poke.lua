-- autorun_debug_bit_poke.lua
--
-- ACE Phase 0: verify whether a debug menu / warp table is real in the NA
-- retail build by externally POKEing a BSS debug flag and leaving PCSX-Redux
-- RUNNING so the user can drive the UI by hand and watch for the menu.
--
-- Unlike every other autorun_*.lua probe, this one does NOT use probe.run /
-- probe.sm (that driver force-quits PCSX at the end). Phase 0 is a human-in-
-- the-loop observation: the script loads a stable field save state, writes the
-- flag, re-asserts it every vsync (so a stray write or a scene-init revert
-- can't silently undo it), logs a heartbeat, and then gets out of the way.
-- Close the emulator window when you're done looking.
--
-- ── The two flags (they are NOT the same thing) ─────────────────────────────
-- See docs/reference/builds.md "Debug flags" + docs/subsystems/boot.md.
--
--   0x8007B8C2  _DAT_8007B8C2  dev/retail LOADER-PATH flag.
--       Retail boots this = 0. Read by ~26 SCUS functions as `== 0` (retail
--       mode). Setting it to 1 flips asset loaders to the dev PROT-TOC-index
--       path (h:\PROT\FIELD\<stage>\...). This does NOT by itself open a debug
--       menu. WARNING: with this set, the NEXT scene load takes the dev path;
--       on most loaders that lands at the same files, but it can desync or
--       hang a transition. Prefer observing from a stable scene, and reload a
--       fresh state before walking through a door.
--
--   0x8007B98F  _DAT_8007B98F  in-game DEBUG-MENU enable (NA offset).
--       This is the high byte of the master word at 0x8007B98C. The input
--       dispatcher FUN_8001822C does `if (_DAT_8007B98C == 0) mask &= 0xFFFF`
--       (strips the controller-2 / debug bindings) and gates the whole debug
--       combo block on `_DAT_8007B98C != 0`. Writing 0x8007B98F = 1 makes the
--       LE word = 0x01000000 (non-zero) with one byte, flipping the gate on.
--       Combos that should then be live (per builds.md):
--           SELECT + TRIANGLE  -> Debug Menu (item-give, MAP-CHANGE/WARP, ...)
--           SELECT + START     -> start in Debug Mode (asset testers)
--           R1 + R2 + CROSS    -> coordinates overlay + free camera
--       CAVEAT: docs/subsystems/boot.md claims this flag is link-stripped and
--       inert in the retail corpus (zero reads). builds.md documents it as the
--       live gate. THAT CONTRADICTION IS WHAT THIS PHASE 0 RESOLVES: poke it,
--       try SELECT+TRIANGLE, and report whether the menu actually appears.
--
-- ── Which flag this run pokes ───────────────────────────────────────────────
-- Preset via LEGAIA_DEBUG_POKE (default "loader" to match the task's stated
-- default; use "menu" for the debug-menu gate, "both" to set both at once):
--     loader  -> 0x8007B8C2 = 1
--     menu    -> 0x8007B98F = 1   (sets the 0x8007B98C master word non-zero)
--     both    -> both of the above
-- Raw override: set LEGAIA_DEBUG_ADDR (hex, e.g. 0x8007B98F) and optionally
-- LEGAIA_DEBUG_VAL (default 1) to poke a single arbitrary byte instead.
--
-- Run "menu" and "loader" in SEPARATE runs so that, if a menu appears, you
-- know which flag enabled it.
--
-- Run:
--   LEGAIA_SSTATE=~/Tools/pcsx-redux/SCUS94254.sstate2 \
--   LEGAIA_DEBUG_POKE=menu \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_debug_bit_poke.lua
--
-- The --frames flag is irrelevant here (the script never self-quits); just
-- close the PCSX window when finished. Output log lands in the per-run
-- captures/<stem>/<ts>/ subtree (debug_bit_poke.txt) and in PCSX.log.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")

-- ── addresses ──────────────────────────────────────────────────────────────
local LOADER_FLAG   = 0x8007B8C2  -- dev/retail loader-path selector (==0 retail)
local DEBUG_MENU_HI = 0x8007B98F  -- high byte of the master word at 0x8007B98C
local DEBUG_WORD    = 0x8007B98C  -- master debug gate word (FUN_8001822C reads it)
local GAME_MODE     = 0x8007B83C  -- u16; 0x03 = field-run
local BTN_MASK      = 0x8007B850  -- per-frame button mask built by FUN_8001822C

-- ── poke set (resolved from env) ────────────────────────────────────────────
local function parse_addr(s)
    if not s or s == "" then return nil end
    return tonumber(s) -- tonumber handles 0x.. hex
end

local pokes = {}  -- list of {addr=, val=, name=}
local raw_addr = parse_addr(probe.getenv("LEGAIA_DEBUG_ADDR", nil))
if raw_addr then
    local raw_val = probe.getenv_num("LEGAIA_DEBUG_VAL", 1)
    pokes[#pokes + 1] = { addr = raw_addr, val = raw_val, name = "raw" }
else
    local preset = (probe.getenv("LEGAIA_DEBUG_POKE", "loader")):lower()
    if preset == "loader" or preset == "both" then
        pokes[#pokes + 1] = { addr = LOADER_FLAG, val = 1, name = "loader_flag(8007B8C2)" }
    end
    if preset == "menu" or preset == "both" then
        pokes[#pokes + 1] = { addr = DEBUG_MENU_HI, val = 1, name = "debug_menu_hi(8007B98F)" }
    end
    if #pokes == 0 then
        -- unknown preset string; fail safe to the task default
        pokes[#pokes + 1] = { addr = LOADER_FLAG, val = 1, name = "loader_flag(8007B8C2)" }
    end
end

-- ── output ─────────────────────────────────────────────────────────────────
local OUT = probe.out_path("debug_bit_poke.txt")
local f   = assert(io.open(OUT, "w"))

local function logline(s)
    f:write(s .. "\n"); f:flush()
    PCSX.log("[debug-poke] " .. s)
end

local function u16(addr) return mem.read_u16(addr) or 0 end
local function u32(addr) return mem.read_u32(addr) or 0 end
local function u8(addr)  return mem.read_u8(addr)  or 0 end

-- ── vsync-driven state machine (NO auto-quit) ───────────────────────────────
local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)  -- vsyncs before load
local SETTLE     = 5   -- vsyncs after load before the first poke

local vsync       = 0
local loaded_at   = nil
local poked       = false
local last_word   = nil
local revert_count = 0

logline(string.format("sstate=%s", SSTATE))
do
    local names = {}
    for _, p in ipairs(pokes) do
        names[#names + 1] = string.format("0x%08X<-%d (%s)", p.addr, p.val, p.name)
    end
    logline("will poke: " .. table.concat(names, ", "))
    logline("DEBUG-MENU combos to try by hand once 0x8007B98C is non-zero:")
    logline("  SELECT+TRIANGLE = Debug Menu (look for a MAP-CHANGE / warp option)")
    logline("  SELECT+START    = start in Debug Mode")
    logline("  R1+R2+CROSS     = coordinates overlay + free camera")
    logline("This probe will NOT quit PCSX; close the window when finished.")
end

local function do_pokes()
    for _, p in ipairs(pokes) do
        mem.write_u8(p.addr, p.val)
    end
end

local function poke_readback_ok()
    for _, p in ipairs(pokes) do
        if u8(p.addr) ~= (p.val % 0x100) then return false end
    end
    return true
end

local function on_vsync()
    vsync = vsync + 1

    -- Wait for the emulator to settle, then load the stable field state.
    if loaded_at == nil then
        if vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then
                logline("FATAL: could not load save state; leaving PCSX running anyway")
                loaded_at = -1  -- sentinel: don't retry the load
                return
            end
            loaded_at = vsync
            logline(string.format("loaded save state at vsync %d; game_mode=0x%04X",
                vsync, u16(GAME_MODE)))
        end
        return
    end
    if loaded_at < 0 then return end

    local since = vsync - loaded_at

    -- First poke once the loaded scene has settled.
    if not poked and since >= SETTLE then
        do_pokes()
        poked = true
        local ok = poke_readback_ok()
        logline(string.format(
            "POKED at vsync %d (since-load %d); readback %s; game_mode=0x%04X word@8007B98C=0x%08X",
            vsync, since, ok and "OK" or "MISMATCH", u16(GAME_MODE), u32(DEBUG_WORD)))
        last_word = u32(DEBUG_WORD)
        return
    end
    if not poked then return end

    -- Hold the value: re-assert every vsync and notice if anything reverted it
    -- (scene-init reload, or a code writer we didn't expect). Re-asserting is
    -- cheap and makes the observation window stable for the user.
    if not poke_readback_ok() then
        revert_count = revert_count + 1
        if revert_count <= 20 or (revert_count % 60) == 0 then
            logline(string.format(
                "vsync %d: flag reverted (#%d) -> re-asserting; game_mode=0x%04X",
                vsync, revert_count, u16(GAME_MODE)))
        end
        do_pokes()
    end

    -- Note master-word transitions (debug gate turning on/off).
    local w = u32(DEBUG_WORD)
    if w ~= last_word then
        logline(string.format("vsync %d: word@0x8007B98C 0x%08X -> 0x%08X  (debug gate %s)",
            vsync, last_word or 0, w, (w ~= 0) and "ON" or "OFF"))
        last_word = w
    end

    -- Heartbeat every ~2s so the log shows the script is alive and what mode
    -- the game is in while the user navigates.
    if (since % 120) == 0 then
        logline(string.format(
            "alive vsync=%d mode=0x%04X B8C2=%d word@B98C=0x%08X btnmask=0x%08X reverts=%d",
            vsync, u16(GAME_MODE), u8(LOADER_FLAG), w, u32(BTN_MASK), revert_count))
    end
end

-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
logline("vsync listener installed; waiting for boot then loading state")
