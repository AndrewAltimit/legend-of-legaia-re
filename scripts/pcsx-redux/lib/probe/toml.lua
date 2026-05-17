-- probe/toml.lua  -- minimal single-file TOML 1.0 reader.
--
-- Built for the .probe.toml spec files under scripts/pcsx-redux/probes/.
-- Stdlib-only, no external dependencies, loadable from vanilla Lua 5.1
-- as well as the PCSX-Redux LuaJIT environment.
--
-- Subset supported (covers everything our .probe.toml schema needs):
--   - top-level `key = value`
--   - sections: `[name]`
--   - array-of-tables: `[[name]]`
--   - single-line inline arrays:  `arr = [1, 2, 3]`
--   - single-line inline tables:  `tbl = { a = 1, b = "x" }`
--   - integers: decimal (`123`, `-7`), hexadecimal (`0xDEADBEEF`),
--     binary (`0b1010`), octal (`0o755`); underscores tolerated.
--   - floats:   `1.5`, `-0.25` (no exponent form yet)
--   - strings:  basic (`"foo"`) with \n \t \" \\ \r escapes,
--               literal (`'foo'`, raw).
--   - booleans: `true` / `false`
--   - comments: `#` to end of line (string-aware on the same line)
--
-- Explicit non-goals (will throw a parse error):
--   - multi-line arrays / inline tables
--   - multi-line strings (\"\"\" or ''')
--   - dotted keys (foo.bar = 1) beyond what [[a.b]] supports
--   - datetimes
--   - scientific-notation floats
--
-- A user hitting a non-supported feature gets a parse error at the
-- offending line; no silent fallback.
--
-- API:
--   toml.parse(text)        - returns a Lua table for the document string
--   toml.parse_file(path)   - same, after reading from disk

local M = {}

------------------------------------------------------------------
-- Small helpers

local function trim(s)
    return (s:gsub("^%s+", ""):gsub("%s+$", ""))
end

local function err(line_no, msg)
    error(string.format("probe.toml: line %d: %s", line_no, msg), 3)
end

-- Strip a "# ..." comment from the end of a line, but only if the
-- '#' lies outside a string literal on that line.
local function strip_comment(line)
    local in_str = nil  -- nil | '"' | "'"
    local i = 1
    while i <= #line do
        local c = line:sub(i, i)
        if in_str then
            if c == "\\" and in_str == '"' and i < #line then
                i = i + 2  -- skip escape sequence
            else
                if c == in_str then in_str = nil end
                i = i + 1
            end
        else
            if c == "#" then return line:sub(1, i - 1) end
            if c == '"' or c == "'" then in_str = c end
            i = i + 1
        end
    end
    return line
end

------------------------------------------------------------------
-- Value parser. Operates on a single line; advances `i` past each
-- value it consumes and returns `(value, next_i)`.

local function skip_ws(s, i)
    while i <= #s do
        local c = s:sub(i, i)
        if c ~= " " and c ~= "\t" then break end
        i = i + 1
    end
    return i
end

local parse_value  -- forward declare for mutual recursion

local function parse_string_basic(s, i, line_no)
    -- s[i] == '"'
    local out = {}
    i = i + 1
    while i <= #s do
        local c = s:sub(i, i)
        if c == '"' then return table.concat(out), i + 1 end
        if c == "\\" then
            local n = s:sub(i + 1, i + 1)
            if     n == "n"  then out[#out+1] = "\n"
            elseif n == "t"  then out[#out+1] = "\t"
            elseif n == "r"  then out[#out+1] = "\r"
            elseif n == '"'  then out[#out+1] = '"'
            elseif n == "\\" then out[#out+1] = "\\"
            elseif n == "0"  then out[#out+1] = "\0"
            else err(line_no, "unsupported string escape '\\" .. n .. "'") end
            i = i + 2
        else
            out[#out+1] = c
            i = i + 1
        end
    end
    err(line_no, "unterminated basic string")
end

local function parse_string_literal(s, i, line_no)
    -- s[i] == "'"
    local close = s:find("'", i + 1, true)
    if not close then err(line_no, "unterminated literal string") end
    return s:sub(i + 1, close - 1), close + 1
end

local function parse_bool(s, i, line_no)
    if s:sub(i, i + 3) == "true"  then return true,  i + 4 end
    if s:sub(i, i + 4) == "false" then return false, i + 5 end
    err(line_no, "expected boolean")
end

local function parse_number(s, i, line_no)
    local j = i
    local c = s:sub(j, j)
    if c == "+" or c == "-" then j = j + 1 end

    -- Prefix-based integer bases.
    if s:sub(j, j + 1) == "0x" or s:sub(j, j + 1) == "0o" or s:sub(j, j + 1) == "0b" then
        local prefix = s:sub(j, j + 1)
        local class
        if     prefix == "0x" then class = "[0-9A-Fa-f_]"
        elseif prefix == "0o" then class = "[0-7_]"
        else                       class = "[01_]" end
        local body_start = j + 2
        local body_end   = body_start
        while body_end <= #s and s:sub(body_end, body_end):match(class) do
            body_end = body_end + 1
        end
        local body = s:sub(body_start, body_end - 1):gsub("_", "")
        if body == "" then err(line_no, "empty " .. prefix .. " number") end
        local n = tonumber(body, (prefix == "0x") and 16 or (prefix == "0o") and 8 or 2)
        if n == nil then err(line_no, "bad " .. prefix .. " number: " .. body) end
        if c == "-" then n = -n end
        return n, body_end
    end

    -- Decimal integer or float.
    local k = j
    while k <= #s and s:sub(k, k):match("[0-9_]") do k = k + 1 end
    local is_float = false
    if k <= #s and s:sub(k, k) == "." then
        is_float = true
        k = k + 1
        while k <= #s and s:sub(k, k):match("[0-9_]") do k = k + 1 end
    end
    if k == j then err(line_no, "expected number") end
    local raw = s:sub(i, k - 1):gsub("_", "")
    local n = tonumber(raw)
    if n == nil then err(line_no, "bad number: " .. raw) end
    return n, k
end

local function parse_array(s, i, line_no)
    -- s[i] == '['
    local out = {}
    i = skip_ws(s, i + 1)
    if s:sub(i, i) == "]" then return out, i + 1 end
    while true do
        local v
        v, i = parse_value(s, i, line_no)
        out[#out + 1] = v
        i = skip_ws(s, i)
        local c = s:sub(i, i)
        if c == "," then
            i = skip_ws(s, i + 1)
            if s:sub(i, i) == "]" then return out, i + 1 end
        elseif c == "]" then
            return out, i + 1
        else
            err(line_no, "expected ',' or ']' in array")
        end
    end
end

local function parse_inline_table(s, i, line_no)
    -- s[i] == '{'
    local out = {}
    i = skip_ws(s, i + 1)
    if s:sub(i, i) == "}" then return out, i + 1 end
    while true do
        -- key
        local k_start = i
        while i <= #s and s:sub(i, i):match("[A-Za-z0-9_-]") do i = i + 1 end
        local key = s:sub(k_start, i - 1)
        if key == "" then err(line_no, "expected key in inline table") end
        i = skip_ws(s, i)
        if s:sub(i, i) ~= "=" then err(line_no, "expected '=' after key '" .. key .. "'") end
        i = skip_ws(s, i + 1)
        -- value
        local v
        v, i = parse_value(s, i, line_no)
        out[key] = v
        i = skip_ws(s, i)
        local c = s:sub(i, i)
        if c == "," then
            i = skip_ws(s, i + 1)
        elseif c == "}" then
            return out, i + 1
        else
            err(line_no, "expected ',' or '}' in inline table")
        end
    end
end

parse_value = function(s, i, line_no)
    i = skip_ws(s, i)
    if i > #s then err(line_no, "expected value") end
    local c = s:sub(i, i)
    if c == '"' then return parse_string_basic(s, i, line_no) end
    if c == "'" then return parse_string_literal(s, i, line_no) end
    if c == "[" then return parse_array(s, i, line_no) end
    if c == "{" then return parse_inline_table(s, i, line_no) end
    if c == "t" or c == "f" then
        if s:sub(i, i + 3) == "true" or s:sub(i, i + 4) == "false" then
            return parse_bool(s, i, line_no)
        end
    end
    return parse_number(s, i, line_no)
end

------------------------------------------------------------------
-- Document parser: line-oriented dispatch over the trimmed lines.

local function split_dotted(key)
    -- Returns a list of parts for `a.b.c`. Quoted parts not supported.
    local parts = {}
    for p in key:gmatch("[^.]+") do parts[#parts+1] = trim(p) end
    return parts
end

local function descend_or_create(root, parts)
    local t = root
    for _, p in ipairs(parts) do
        if t[p] == nil then
            t[p] = {}
        elseif type(t[p]) ~= "table" then
            error("probe.toml: '" .. p .. "' redefined as table")
        end
        t = t[p]
    end
    return t
end

function M.parse(text)
    local root            = {}
    local current_table   = root
    local line_no         = 0

    for raw in (text .. "\n"):gmatch("([^\n]*)\n") do
        line_no = line_no + 1
        local line = trim(strip_comment(raw))
        if line ~= "" then
            if line:sub(1, 2) == "[[" then
                -- [[array.of.tables]]
                local close = line:find("]]", 3, true)
                if not close or trim(line:sub(close + 2)) ~= "" then
                    err(line_no, "malformed [[array]] header")
                end
                local parts = split_dotted(trim(line:sub(3, close - 1)))
                if #parts == 0 then err(line_no, "empty [[array]] header") end
                local last = parts[#parts]
                table.remove(parts)
                local parent = descend_or_create(root, parts)
                if parent[last] == nil then parent[last] = {} end
                if type(parent[last]) ~= "table" then
                    err(line_no, "'" .. last .. "' is not an array")
                end
                local new_entry = {}
                parent[last][#parent[last] + 1] = new_entry
                current_table = new_entry
            elseif line:sub(1, 1) == "[" then
                -- [section]
                local close = line:find("]", 2, true)
                if not close or trim(line:sub(close + 1)) ~= "" then
                    err(line_no, "malformed [section] header")
                end
                local parts = split_dotted(trim(line:sub(2, close - 1)))
                if #parts == 0 then err(line_no, "empty [section] header") end
                current_table = descend_or_create(root, parts)
            else
                -- key = value
                local eq = line:find("=", 1, true)
                if not eq then err(line_no, "expected '=' in key/value line") end
                local key = trim(line:sub(1, eq - 1))
                local rest = line:sub(eq + 1)
                if key == "" then err(line_no, "missing key before '='") end
                if not key:match("^[A-Za-z0-9_-]+$") then
                    err(line_no, "unsupported key syntax: '" .. key .. "'")
                end
                local v, next_i = parse_value(rest, 1, line_no)
                if trim(rest:sub(next_i)) ~= "" then
                    err(line_no, "trailing text after value: '" .. rest:sub(next_i) .. "'")
                end
                if current_table[key] ~= nil then
                    err(line_no, "key '" .. key .. "' redefined")
                end
                current_table[key] = v
            end
        end
    end

    return root
end

function M.parse_file(path)
    local fh, err_msg = io.open(path, "r")
    if not fh then error("probe.toml: cannot open " .. path .. ": " .. tostring(err_msg), 2) end
    local text = fh:read("*a")
    fh:close()
    return M.parse(text)
end

return M
