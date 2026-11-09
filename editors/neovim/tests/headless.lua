-- Original Apache-2.0 acceptance harness. Real buffers and built-in LSP only.
local api = vim.api
vim.o.modeline = false
vim.o.swapfile = false
vim.o.hidden = true
local cfg = vim.json.decode(table.concat(vim.fn.readfile(arg[1]), '\n'))
vim.opt.runtimepath:prepend(cfg.adapter)
local reports = {}
local function check(value, message) assert(value, message) end
local function wait_for(predicate, label)
  check(vim.wait(15000, predicate, 5), 'timeout: ' .. label)
end
local function run()
  local adapter = require('java_analyzer')
  local function open(path)
    local b = vim.fn.bufadd(path)
    vim.fn.bufload(b)
    vim.bo[b].filetype = 'java'
    return b
  end
  local app, target = open(cfg.app), open(cfg.target)
  local function lines(buf, text)
    api.nvim_buf_set_lines(buf, 0, -1, false, vim.split(text, '\n', { plain = true }))
  end
  local function position(buf, word)
    for i, line in ipairs(api.nvim_buf_get_lines(buf, 0, -1, false)) do
      local col = line:find(word, 1, true)
      if col then return { i, col - 1 } end
    end
    error('missing token: ' .. word)
  end
  local function place(word)
    api.nvim_set_current_buf(app)
    api.nvim_win_set_cursor(0, position(app, word))
  end
  local function location_is(buf, word)
    return api.nvim_get_current_buf() == buf
      and vim.deep_equal(api.nvim_win_get_cursor(0), position(buf, word))
  end
  local function request(word)
    place(word)
    local out
    local id, cancel = adapter.definition({ on_result = function(result) out = result end })
    check(id, 'request was not sent')
    return function() return out end, cancel
  end
  local function jump(word)
    local result = request(word)
    wait_for(function() return result() ~= nil end, 'definition')
    check(result().status == 'jumped', vim.inspect(result()))
    check(location_is(target, word), 'wrong actual buffer/cursor target')
  end
  local function record(name) reports[#reports + 1] = name end
  local exits = {}
  adapter.setup({ binary = cfg.proxy, manifest = cfg.manifest,
    on_exit = function(code, signal) exits[#exits + 1] = { code = code, signal = signal } end })
  adapter.start()
  wait_for(function() return adapter.status().ready end, 'initialize')
  local st = adapter.status()
  local client = vim.lsp.get_client_by_id(st.client_id)
  check(client.server_capabilities.positionEncoding == 'utf-16', 'position encoding')
  check(client.server_capabilities.textDocumentSync.change == 1, 'full sync')
  check(client.server_capabilities.definitionProvider == true, 'definition capability')
  check(client.flags.debounce_text_changes == 150, 'test must retain ordinary debounce')
  jump('valueA')
  record('initialize_unicode_uri_and_actual_navigation')

  -- Both buffers change at the same query/target positions without a save.
  lines(app, cfg.app_text:gsub('valueA', 'valueB'))
  lines(target, cfg.target_text:gsub('valueA', 'valueB'))
  jump('valueB')
  record('same_position_two_unsaved_buffers')

  -- No wait between target-only edit and request. Origin stays unchanged.
  local app_tick = api.nvim_buf_get_changedtick(app)
  lines(target, '// target shifted immediately\n' .. cfg.target_text:gsub('valueA', 'valueB'))
  jump('valueB')
  check(api.nvim_buf_get_changedtick(app) == app_tick, 'origin unexpectedly changed')
  record('target_only_edit_immediate_request_flushes_all_buffers')

  for count = 1, 20 do
    lines(target, string.rep('// rapid unsaved change\n', count)
      .. cfg.target_text:gsub('valueA', 'valueB'))
  end
  jump('valueB')
  record('rapid_twenty_target_changes_latest_full_text')

  place('valueB')
  api.nvim_buf_delete(target, { force = true })
  local no_target = request('valueB')
  wait_for(function() return no_target() ~= nil end, 'closed dependency uses disk')
  check(no_target().status == 'empty', vim.inspect(no_target()))
  check(api.nvim_get_current_buf() == app, 'closed overlay still navigated')
  target = open(cfg.target)
  lines(target, cfg.target_text:gsub('valueA', 'valueB'))
  jump('valueB')
  record('close_restores_disk_and_reopen_new_lifetime')

  lines(app, cfg.app_text:gsub('valueA', 'absent'))
  local absent = request('absent')
  wait_for(function() return absent() ~= nil end, 'unresolved')
  check(absent().status == 'empty' and api.nvim_get_current_buf() == app, 'unresolved jump')
  record('unresolved_no_fabricated_navigation')
  lines(app, cfg.binary_text)
  local binary = request('Lib')
  wait_for(function() return binary() ~= nil end, 'binary-only')
  check(binary().status == 'empty' and api.nvim_get_current_buf() == app, 'binary-only jump')
  record('binary_only_no_fabricated_navigation')
  lines(app, cfg.app_text:gsub('valueA', 'valueB'))

  local function hold()
    for _, p in ipairs(vim.fn.glob(cfg.control .. '/held-*.json', false, true)) do
      vim.fn.delete(p)
    end
    vim.fn.writefile({ 'hold next definition response' }, cfg.control .. '/hold-next')
  end
  local function held()
    local paths = vim.fn.glob(cfg.control .. '/held-*.json', false, true)
    if #paths == 0 then return nil end
    local ok, data = pcall(vim.json.decode, table.concat(vim.fn.readfile(paths[1]), '\n'))
    return ok and data or nil
  end
  local function release(data)
    vim.fn.writefile({ 'release' }, cfg.control .. '/release-' .. data.pid)
  end
  local function late(label, mutate, expected)
    hold()
    local response, cancel = request('valueB')
    wait_for(function() return held() ~= nil end, 'response in transit')
    local data = held()
    check(data.message.result ~= vim.NIL and data.message.result ~= nil,
      'held response must be a real resolved definition')
    mutate(cancel)
    local before_buf, before_cursor = api.nvim_get_current_buf(), api.nvim_win_get_cursor(0)
    release(data)
    wait_for(function() return response() ~= nil end, 'late callback')
    check(response().status == expected, vim.inspect(response()))
    check(api.nvim_get_current_buf() == before_buf
      and vim.deep_equal(api.nvim_win_get_cursor(0), before_cursor), label .. ' jumped')
    record(label)
  end
  late('in_transit_target_only_local_edit', function()
    local tick = api.nvim_buf_get_changedtick(app)
    lines(target, '// shifted during response transit\n' .. cfg.target_text:gsub('valueA', 'valueB'))
    check(api.nvim_buf_get_changedtick(app) == tick, 'origin changed in target-only test')
  end, 'stale')
  jump('valueB')
  late('in_transit_target_close_reopen', function()
    api.nvim_buf_delete(target, { force = true })
    target = open(cfg.target)
    lines(target, cfg.target_text:gsub('valueA', 'valueB'))
  end, 'stale')
  jump('valueB')
  late('in_transit_origin_close_reopen', function()
    api.nvim_set_current_buf(target)
    api.nvim_buf_delete(app, { force = true })
    app = open(cfg.app)
    lines(app, cfg.app_text:gsub('valueA', 'valueB'))
    place('valueB')
  end, 'stale')
  jump('valueB')
  late('in_transit_old_session_after_restart', function()
    local old = adapter.status().client_id
    adapter.restart()
    wait_for(function() return adapter.status().ready end, 'new session initialized')
    check(adapter.status().client_id ~= old, 'restart reused old client')
    place('valueB')
  end, 'stale')
  jump('valueB')
  late('explicit_cancel_discards_already_computed_result', function(cancel) cancel() end, 'cancelled')
  jump('valueB')
  adapter.stop()
  wait_for(function() return #vim.lsp.get_clients({ name = 'java-analyzer-preview' }) == 0 end,
    'graceful shutdown')
  check(#exits == 2, 'both client sessions must report exit')
  for _, exited in ipairs(exits) do
    check(exited.code == 0 and exited.signal == 0, 'proxy/session did not exit cleanly: ' .. vim.inspect(exited))
  end
  record('graceful_shutdown_all_sessions')
  return { passed = reports, kind = 'real_neovim_headless', version = vim.version().major
    .. '.' .. vim.version().minor .. '.' .. vim.version().patch, client_exits = exits }
end
local ok, result = xpcall(run, debug.traceback)
vim.fn.writefile({ vim.json.encode(ok and result or { failed = result, passed = reports }) }, cfg.result)
if not ok then io.stderr:write(result .. '\n'); vim.cmd('cquit 1') else vim.cmd('qa!') end
