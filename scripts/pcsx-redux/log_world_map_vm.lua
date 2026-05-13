-- log_world_map_vm.lua
--
-- PCSX-Redux Lua script: log every call into the world-map drawing-VM
-- dispatcher (FUN_801D362C) while the emulator runs Legend of Legaia.
-- For each call, captures (call_idx, a0 render context, a1 bytecode
-- PC, the u16 sub-opcode at a1+2, and a 64-byte window starting at a1).
--
-- Each call to FUN_801D362C dispatches one world-map sub-opcode (see
-- `crates/engine-vm/src/world_map_draw_vm.rs` for the canonical opcode
-- table). The 4 continent-render ops are the priority targets:
--   0x2B set slab UV bounds       size 6 halfwords
--   0x2C draw continent region    size 7 halfwords
--   0x2D inc slab UV bounds       size 6 halfwords
--   0x2E build GP0 tpage/clut     size 13 halfwords
--
-- Run the script while in the active world-map view; logs accumulate
-- in `world_map_vm_calls.csv` (working directory). Call
-- `worldMapVmLogger.stop()` from the Lua REPL when you have enough
-- frames. Switch save states between Drake/Sebucus/Karisto with the
-- logger armed and you get one combined CSV across all three.
--
-- HOW TO USE
--   1. Launch PCSX-Redux with the Legaia disc image.
--   2. Boot to the world map (load any saved world-map save).
--   3. Open Tools -> Show Lua console; paste this file's contents and
--      hit "Load and Execute".
--   4. Let it run for a few frames in each continent.
--   5. From the Lua console: `worldMapVmLogger.stop()`
--   6. Process `world_map_vm_calls.csv` with the matching analysis
--      tool (e.g. `tools/analyze_world_map_vm_log.py`).
--
-- NOTES
--  - The breakpoint fires on EVERY hit; execution continues silently
--    after the callback. No emulator pause.
--  - File writes are buffered and flushed every 64 calls for
--    crash-safety without per-call I/O cost. `.stop()` does a final
--    flush + close.
--  - This script is non-destructive: it reads emulator state only.
--    Safe to leave armed while you save/load states or change scenes.

local OUT_PATH    = "world_map_vm_calls.csv"
local DUMP_LEN    = 64                 -- bytes per call to capture
local TARGET_PC   = 0x801D362C         -- FUN_801D362C entry
local FLUSH_EVERY = 64                 -- rows between fsync

-- Open output. PCSX-Redux exposes standard Lua io for working-directory
-- files; if your build restricts it, swap to Support.File.open.
local f = assert(io.open(OUT_PATH, "a"),
    "[world_map_vm] cannot open " .. OUT_PATH .. " for append")
f:seek("end")
if f:seek() == 0 then
    f:write("call_idx,a0_render_ctx,a1_bytecode_pc,sub_op,bytes_hex\n")
end

local mem      = PCSX.getMemoryAsFile()
local call_idx = 0

local function bytes_to_hex(s)
    local out = {}
    for i = 1, #s do out[i] = string.format("%02X", s:byte(i)) end
    return table.concat(out)
end

local function on_hit()
    local ok, err = pcall(function()
        local r = PCSX.getRegisters().GPR.n
        local a0, a1 = r.a0, r.a1   -- s1 (render ctx), s3 (bytecode PC)

        -- Sub-opcode lives at a1+2 (lh v1, 0x2(s3) in the original).
        mem:seek(a1 + 2, "set")
        local sub_op = mem:readU16()

        -- Capture a 64-byte window starting at a1 for offline analysis.
        mem:seek(a1, "set")
        local raw = mem:read(DUMP_LEN)

        f:write(string.format("%d,0x%08X,0x%08X,0x%04X,%s\n",
            call_idx, a0, a1, sub_op, bytes_to_hex(raw)))
        call_idx = call_idx + 1
        if call_idx % FLUSH_EVERY == 0 then f:flush() end
    end)
    if not ok then
        PCSX.log("[world_map_vm] handler error: " .. tostring(err))
    end
end

local bp = PCSX.addBreakpoint(
    TARGET_PC,
    "Exec",
    4,
    "FUN_801D362C (world-map drawing VM)",
    on_hit)

-- Expose a small handle for the Lua REPL.
worldMapVmLogger = {
    stop = function()
        if bp then
            bp:remove()
            bp = nil
        end
        if f then
            f:flush()
            f:close()
            f = nil
        end
        PCSX.log(string.format(
            "[world_map_vm] stopped after %d calls; wrote %s",
            call_idx, OUT_PATH))
    end,
    calls = function() return call_idx end,
    path  = function() return OUT_PATH end,
}

PCSX.log(string.format(
    "[world_map_vm] armed at 0x%08X; logging to %s. " ..
    "Call worldMapVmLogger.stop() to detach.",
    TARGET_PC, OUT_PATH))
