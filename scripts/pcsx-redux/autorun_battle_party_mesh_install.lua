-- autorun_battle_party_mesh_install.lua
--
-- Pins the BATTLE party-mesh load callsite: the code that installs the
-- battle-form (PROT 1204) party character TMDs into DAT_8007C018[0..2]
-- when a normal turn-based battle starts.
--
-- Why a runtime probe (docs/formats/character-mesh.md "Battle form" /
-- docs/reference/open-rev-eng-threads.md):
--   A real battle renders the party from PROT 1204, NOT the field pack
--   (PROT 0874). Reading DAT_8007C018[0..2] out of battle save states
--   byte-matches 1204 (e.g. battle Vahn lives at 0x80165F48 vs the field
--   form's 0x8014D554). But the loader that performs that install is in an
--   as-yet-uncaptured battle-setup overlay: the captured battle scene
--   loader FUN_800520F0 only tmd_register's PROT 0x36A into the EFFECT
--   window DAT_8007C018[3..], not the party slots [0..2]. So the party
--   install callsite is unknown from static dumps alone.
--
-- Strategy:
--   Load a FIELD save (game_mode 0x03) whose field VM auto-starts a battle
--   (rim_elm_queen_bee_battle: a scripted Rim Elm boss that crosses into
--   BattleMode on its own, no input). The resident field-form party meshes
--   are already in DAT_8007C018[0..2] at load; when the battle sets up it
--   OVERWRITES those three slots with the 1204 battle-form pointers. A Write
--   watchpoint on the three slots fires at that exact store, logging the
--   writing PC + ra + the new pointer value + a call-context snapshot (GPRs,
--   straddling instructions, and 32 stack words so the caller chain can be
--   walked offline). Two supplementary breakpoints disambiguate the path:
--     * tmd_register (FUN_80026B4C) entry, filtered to party indices
--       (DAT_8007B774 in {0,1,2}): if the battle install reuses the generic
--       registrar, this logs a0 (the TMD pointer) and ra (the REAL caller,
--       before the prologue saves it) directly.
--     * the captured battle scene loader FUN_800520F0 entry: confirms the
--       battle path fired and timestamps it.
--
--   The Write watchpoint catches the install regardless of HOW it writes
--   (generic tmd_register store at PC 0x80026BA8, or a direct overlay `sw`).
--
-- Run (default save = the queen_bee auto-start library backup):
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_battle_party_mesh_install.lua \
--       timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh \
--       --scenario rim_elm_queen_bee_battle
--
--   Or against any field save that crosses into battle, optionally holding a
--   walk direction to roll a random encounter:
--     LEGAIA_SSTATE=/path/field.sstate LEGAIA_HOLD=DOWN LEGAIA_HOLD_FRAMES=1800 \
--     LEGAIA_FRAMES=2400 LEGAIA_LUA=...install.lua \
--         timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local LIB_QUEEN_BEE = "saves/library/pcsx-redux/"
    .. "3d22fa5fd53d47cd22999a7b377ec8ece057fdb5ca164357be0f96a65147ddf3.sstate"

local SSTATE      = probe.getenv("LEGAIA_SSTATE", LIB_QUEEN_BEE)
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 2400)
local HOLD_NAME   = probe.getenv("LEGAIA_HOLD", "")          -- "" = no hold
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD_FRAMES", 0)

-- Globals (docs/formats/character-mesh.md, ghidra/scripts/funcs/80026b4c.txt).
local C018          = 0x8007C018       -- DAT_8007C018[]: char TMD pointer table
local SLOTS         = { C018 + 0, C018 + 4, C018 + 8 } -- party slots 0/1/2
local GAME_MODE     = 0x8007B83C       -- _DAT_8007B83C: field 0x03 / battle 0x15
local B774          = 0x8007B774       -- DAT_8007B774: tmd_register running index
local BD10          = 0x8007BD10       -- DAT_8007bd10[i]: per-slot ACTIVE-MEMBER ID
                                       --   (1=Vahn,2=Noa,3=Gala,0=empty). Vahn-solo
                                       --   = [1,0,0,0]; full party = [1,2,3,0].
                                       --   FUN_800513F0's while<3 loop registers slot
                                       --   i iff BD10[i] != 0 - so a full party
                                       --   installs all three through that loop.
local TMD_REGISTER  = 0x80026B4C       -- FUN_80026B4C: generic TMD registrar
local BATTLE_LOADER = 0x800520F0       -- FUN_800520F0: captured battle scene loader

local writes_csv = probe.csv_open(probe.out_path("party_install.csv"),
    "tick,label,addr,pc,ra,value")
local caller_csv = probe.csv_open(probe.out_path("tmd_register_party.csv"),
    "tick,index,a0_tmd,ra_caller,pc")
local detail_path = probe.out_path("party_install.detail.txt")

local function bd10_str()
    local s = {}
    for i = 0, 3 do s[i + 1] = tostring(probe.read_u8(BD10 + i) or 255) end
    return table.concat(s, ",")
end

local g_elapsed     = 0
local logged_start  = false
local prev_mode     = -1
local prev_bd10     = nil
local battle_seen   = nil
local loader_hits   = 0
local register_hits = 0
local w             -- the write-watch logger

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    hold_button    = (HOLD_NAME ~= "" and pad.BTN[HOLD_NAME]) or nil,
    hold_frames    = HOLD_FRAMES,
    out_path       = probe.out_path("party_install.csv"),
    snapshot_path  = probe.out_path("party_install.hits.txt"),

    on_arm = function()
        local descs = {}

        -- Primary: write-watch the three party TMD-pointer slots.
        w = probe.watch.new{
            csv         = writes_csv,
            detail_path = detail_path,
            max_detail  = 24,
            elapsed     = function() return g_elapsed end,
        }
        for i, addr in ipairs(SLOTS) do
            w:arm(addr, 4, string.format("C018[%d]", i - 1))
            descs[#descs + 1] = { addr = addr,
                name = string.format("C018[%d]", i - 1), hits = 0 }
        end

        -- Supplementary: tmd_register entry, filtered to party indices.
        probe.arm_breakpoint(TMD_REGISTER, "Exec", 4, "tmd_register", function()
            local idx = bit.band(probe.read_u32(B774) or 0xFFFFFFFF, 0xFFFF)
            if idx <= 2 then
                local r  = PCSX.getRegisters()
                local a0 = bit.band(tonumber(r.GPR.n.a0), 0xFFFFFFFF)
                local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
                local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
                register_hits = register_hits + 1
                caller_csv:row("%d,%d,0x%08X,0x%08X,0x%08X",
                    g_elapsed, idx, a0, ra, pc)
                PCSX.log(string.format(
                    "[party] tmd_register idx=%d a0=0x%08X caller_ra=0x%08X bd10=[%s] elapsed=%d",
                    idx, a0, ra, bd10_str(), g_elapsed))
                probe.append_call_context(detail_path,
                    probe.capture_call_context(string.format(
                        "tmd_register party idx=%d a0=0x%08X elapsed=%d",
                        idx, a0, g_elapsed)))
            end
        end)
        descs[#descs + 1] = { addr = TMD_REGISTER, name = "tmd_register", hits = 0 }

        -- Confirm the battle path fired.
        probe.arm_breakpoint(BATTLE_LOADER, "Exec", 4, "battle_loader", function()
            local r  = PCSX.getRegisters()
            local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
            loader_hits = loader_hits + 1
            if loader_hits <= 4 then
                PCSX.log(string.format(
                    "[party] FUN_800520F0 battle loader entered ra=0x%08X elapsed=%d",
                    ra, g_elapsed))
            end
        end)
        descs[#descs + 1] = { addr = BATTLE_LOADER, name = "battle_loader", hits = 0 }

        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed

        if not logged_start then
            logged_start = true
            prev_mode = probe.read_u8(GAME_MODE) or -1
            prev_bd10 = bd10_str()
            PCSX.log(string.format(
                "[party] START mode=0x%02X C018[0..2]=0x%08X 0x%08X 0x%08X b774=%d bd10=[%s]",
                prev_mode,
                probe.read_u32(SLOTS[1]) or 0,
                probe.read_u32(SLOTS[2]) or 0,
                probe.read_u32(SLOTS[3]) or 0,
                bit.band(probe.read_u32(B774) or 0, 0xFFFF),
                prev_bd10))
        end

        -- Trace DAT_8007bd10 transitions: this is the gate on FUN_800513F0's
        -- per-slot install. If a full-party fight ever sets bd10=[1,1,1,...],
        -- the while<3 loop registers all three; if it stays [1,0,0], only the
        -- lead installs there and the rest come via FUN_800542C8.
        local bd10_now = bd10_str()
        if bd10_now ~= prev_bd10 then
            PCSX.log(string.format("[party] bd10 [%s] -> [%s] at elapsed=%d",
                prev_bd10, bd10_now, elapsed))
            prev_bd10 = bd10_now
        end

        local mode = probe.read_u8(GAME_MODE) or prev_mode
        if mode ~= prev_mode then
            PCSX.log(string.format("[party] game_mode 0x%02X -> 0x%02X at elapsed=%d",
                prev_mode, mode, elapsed))
            prev_mode = mode
            if (mode == 0x14 or mode == 0x15) and battle_seen == nil then
                battle_seen = elapsed
            end
        end

        -- Once we are in battle, the install has happened; settle briefly,
        -- then quit early instead of running the full window.
        if battle_seen and (elapsed - battle_seen) > 600 and w:total() > 0 then
            ctx.request_quit = true
        end
    end,

    on_done = function()
        writes_csv:close()
        caller_csv:close()
        PCSX.log(string.format(
            "=== party-install probe: %d slot write(s), %d party-index tmd_register call(s), "
            .. "%d battle-loader entr(y/ies) ===",
            w:total(), register_hits, loader_hits))
        PCSX.log("Slot writes (pc/ra/value) in party_install.csv; "
            .. "caller call-context in party_install.detail.txt")
    end,
})
