-- autorun_summon_model_base.lua
--
-- Pin gp[0x754] -- the additive base for a part record's model_sel
-- (mesh = DAT_8007C018[model_sel + gp[0x754]]) -- at the moment the
-- shared spawn stager FUN_80021B04 stages a part. This is the one
-- residual blocking BOTH render threads that share the stager:
--   * player Seru-magic SUMMONS (legaia_asset::summon_overlay), and
--   * battle move-power effect-FX (the 0x801f6324 prototype records,
--     which are byte-identical summon-format move-VM records --
--     see docs/formats/move-power.md).
-- gp[0x754] is only READ in the corpus (the lhu at 0x80021B50 inside
-- FUN_80021B04); no static writer exists, so it must be observed live.
--
-- The probe arms an Exec breakpoint at FUN_80021B04 and, on each hit,
-- records:
--   * $gp and the ABSOLUTE global address gp+0x754 (a fixed SDA
--     global -- knowing it enables a later Write-watch to find the
--     writer, and tells the engine which global to mirror),
--   * base = *(u16)(gp+0x754)  -- the value we're after,
--   * a2 = the part record pointer, and the record's model_sel /
--     flags at [a2]/[a2+2],
--   * the resolved pool index (base + model_sel) for mesh parts,
--   * a0/a1/a3 = world pos / src pos / mode (0x1000) the stager gets.
--
-- Default state = gimard_summon_start (sstate5), the spawn-stager
-- window for the player Gimard cast. Re-point LEGAIA_SSTATE at an
-- ENEMY special-attack frame to capture the move-FX context's base
-- and confirm whether the two contexts share gp[0x754].
--
-- Usage:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_summon_model_base.lua \
--   LEGAIA_FRAMES=120 \
--       bash scripts/pcsx-redux/run_probe.sh
--
-- Output (under captures/summon_model_base/<ts>/ unless LEGAIA_OUT_DIR set):
--   summon_model_base.csv       one row per FUN_80021B04 hit
--   summon_model_base.detail.txt first N full call contexts

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local BP_STAGER = 0x80021B04 -- FUN_80021B04 (SPAWN_HELPER)
local GP754_OFF = 0x754

local csv = probe.csv_open(probe.out_path("summon_model_base.csv"),
    "tick,pc,ra,gp,gp754_addr,base,a2_record,model_sel,flags,pool_index,a0,a1,a3")
local detail_path = probe.out_path("summon_model_base.detail.txt")

local armed        = false
local hit_count    = 0
local detail_count = 0
local g_elapsed    = 0
local MAX_DETAIL   = 24
local CAP          = 64

local function s16(v)
    if v >= 0x8000 then return v - 0x10000 end
    return v
end

local function on_hit()
    if hit_count >= CAP then return end
    hit_count = hit_count + 1
    local r = PCSX.getRegisters()
    local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
    local g = r.GPR.n
    local function rr(x) return bit.band(tonumber(g[x]), 0xFFFFFFFF) end

    local gp = rr("gp")
    local gp754 = bit.band(gp + GP754_OFF, 0xFFFFFFFF)
    local base = probe.read_u16(gp754) or 0
    local a2 = rr("a2")
    local model_sel_raw = probe.read_u16(a2) or 0
    local flags = probe.read_u16(bit.band(a2 + 2, 0xFFFFFFFF)) or 0
    local model_sel = s16(model_sel_raw)
    -- Only a plain library mesh (0 <= model_sel < 0x100) resolves a pool index.
    local pool_index = (model_sel >= 0 and model_sel < 0x100)
        and (base + model_sel) or -1

    csv:row("%d,0x%08X,0x%08X,0x%08X,0x%08X,%d,0x%08X,%d,0x%04X,%d,0x%08X,0x%08X,0x%08X",
        g_elapsed, pc, rr("ra"), gp, gp754, base,
        a2, model_sel, flags, pool_index, rr("a0"), rr("a1"), rr("a3"))

    if detail_count < MAX_DETAIL then
        detail_count = detail_count + 1
        probe.append_call_context(detail_path,
            probe.capture_call_context(string.format(
                "FUN_80021B04 hit #%d  gp=0x%08X gp+0x754=0x%08X base=%d model_sel=%d -> pool[%d]",
                hit_count, gp, gp754, base, model_sel, pool_index)))
    end
end

local function arm()
    probe.arm_breakpoint(BP_STAGER, "Exec", 4, "stager_80021B04", on_hit)
    PCSX.log(string.format(
        "[model-base] armed Exec BP at 0x%08X (FUN_80021B04 / SPAWN_HELPER)",
        BP_STAGER))
    armed = true
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 120),
    snapshot_path  = probe.out_path("summon_model_base.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(_ctx, elapsed)
        g_elapsed = elapsed
        if not armed and elapsed >= 2 then
            arm()
        end
    end,
    on_done = function()
        csv:close()
        PCSX.log(string.format(
            "[model-base] done. FUN_80021B04 hits=%d (recorded=%d). "
            .. "Read 'base' from summon_model_base.csv = gp[0x754].",
            hit_count, hit_count))
    end,
})
