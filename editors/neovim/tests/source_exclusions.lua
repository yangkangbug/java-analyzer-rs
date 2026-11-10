-- Original Apache-2.0 real-client source-selection acceptance. No fake results.
local api, uv = vim.api, vim.uv
vim.o.modeline, vim.o.swapfile, vim.o.hidden = false, false, true
local cfg = vim.json.decode(table.concat(vim.fn.readfile(arg[1]), '\n'))
vim.opt.runtimepath:prepend(cfg.adapter)
local adapter = require('java_analyzer')
local observed = { navigation = {}, validations = {}, peak_clients = 0, attachments = {} }
local buffers, notifications = {}, {}
vim.notify = function(value) notifications[#notifications + 1] = tostring(value) end
local function read(path)
  local f = assert(io.open(path, 'rb')); local value = f:read('*a'); f:close(); return value
end
local function write(path, value)
  local f = assert(io.open(path, 'wb')); assert(f:write(value)); assert(f:close())
end
local function wait(predicate, label)
  assert(vim.wait(7000, predicate, 5), 'timeout: ' .. label)
end
local function clients() return #vim.lsp.get_clients({ name = 'java-analyzer-preview', _uninitialized = true }) end
local function pause() vim.wait(200, function() return false end, 5) end
local poll = uv.new_timer()
poll:start(0, 2, vim.schedule_wrap(function() observed.peak_clients = math.max(observed.peak_clients, clients()) end))
local function snapshot()
  local result = {}
  for key, b in pairs(buffers) do
    result[key] = { bufnr = b, name = api.nvim_buf_get_name(b), lines = api.nvim_buf_get_lines(b, 0, -1, false),
      tick = api.nvim_buf_get_changedtick(b), modified = vim.bo[b].modified,
      fileformat = vim.bo[b].fileformat, endofline = vim.bo[b].endofline, fixendofline = vim.bo[b].fixendofline }
  end
  return result
end
local function open(key)
  local b
  if key == 'directory' then
    b = api.nvim_create_buf(true, false); api.nvim_buf_set_name(b, cfg.paths[key])
  else b = vim.fn.bufadd(cfg.paths[key]); vim.fn.bufload(b) end
  buffers[key] = b
  vim.bo[b].filetype = 'java'
  api.nvim_buf_set_lines(b, 0, -1, false, vim.split(cfg.dirty[key], '\n', { plain = true }))
  vim.bo[b].fileformat = key == 'app' and 'dos' or 'unix'
  vim.bo[b].endofline, vim.bo[b].fixendofline = key ~= 'target', false
  assert(vim.bo[b].modified, 'expected dirty fixture')
end
local function position(b, word)
  for row, line in ipairs(api.nvim_buf_get_lines(b, 0, -1, false)) do
    local col = line:find(word, 1, true); if col then return { row, col - 1 } end
  end
  error('missing navigation token')
end
local function query(word, target)
  api.nvim_set_current_buf(buffers.app); api.nvim_win_set_cursor(0, position(buffers.app, word))
  local result, count = nil, 0
  assert(adapter.definition({ on_result = function(value) result, count = value, count + 1 end }))
  wait(function() return result ~= nil end, 'actual native definition')
  assert(count == 1 and result.status == 'jumped', vim.inspect(result))
  local expected = position(buffers[target], word)
  assert(api.nvim_get_current_buf() == buffers[target], 'wrong navigation buffer')
  assert(vim.deep_equal(api.nvim_win_get_cursor(0), expected), 'wrong navigation cursor')
  observed.navigation[#observed.navigation + 1] = { result = result, count = count,
    uri = vim.uri_from_bufnr(api.nvim_get_current_buf()), cursor = api.nvim_win_get_cursor(0), word = word }
end
local function ready()
  wait(function() return adapter.status().ready end, 'selection replay')
  observed.peak_clients = math.max(observed.peak_clients, clients())
end
local function attachments(keys)
  local wanted, actual = {}, {}
  for _, key in ipairs(keys) do wanted[key] = true end
  for key, b in pairs(buffers) do
    actual[key] = vim.lsp.buf_is_attached(b, adapter.status().client_id)
  end
  observed.attachments[#observed.attachments + 1] = actual
  for _, key in ipairs(cfg.keys) do
    if buffers[key] and actual[key] ~= (wanted[key] == true) then
      observed.attachment_failure = { key = key, expected = wanted[key] == true, actual = actual[key] }
      observed.failure_code = actual[key] and 'EXCLUDED_BUFFER_ATTACHED' or 'INCLUDED_BUFFER_DETACHED'
      error('wrong attachment: ' .. key)
    end
  end
end
local function stable()
  assert(vim.deep_equal(observed.before, snapshot()), 'dirty buffer changed across selection refresh')
  for path, data in pairs(cfg.original_sources) do assert(read(path) == data, 'source was saved or changed') end
end
local options = { binary = cfg.proxy, manifest = cfg.manifest,
  lifecycle_timeouts = { initialize_ms = 5000, shutdown_ms = 1500, force_ms = 700 } }
local function run()
  if cfg.scenario == 'validation' then
    for _, case in ipairs(cfg.validation_cases) do
      local value = vim.deepcopy(options); value.manifest = case.path
      local ok = pcall(adapter.setup, value)
      observed.validations[#observed.validations + 1] = { name = case.name, accepted = ok }
      assert(ok == case.accepted, 'client validation mismatch: ' .. case.name)
    end
    assert(clients() == 0, 'configuration validation started a client')
  else
    for _, key in ipairs(cfg.keys) do if key ~= 'late' then open(key) end end
    observed.initial_buffers = snapshot()
    adapter.setup(options); adapter.start(); ready()
    attachments(cfg.initial_keys)
    query('valueB', 'target')
    -- Excluded files can be opened and edited after readiness, without syncing.
    open('late'); api.nvim_set_current_buf(buffers.late)
    for _, key in ipairs(cfg.excluded_keys) do
      api.nvim_buf_set_lines(buffers[key], 0, 0, false, { '// additional unsaved excluded edit' })
    end
    observed.before = snapshot()
    pause(); query('valueB', 'target'); attachments(cfg.initial_keys)
    if cfg.scenario == 'restart' then
      write(cfg.manifest, vim.json.encode(cfg.changed_manifest))
      attachments(cfg.initial_keys); query('valueB', 'target') -- Frozen until explicit restart.
      adapter.restart(); ready(); attachments(cfg.changed_keys); query('valueB', 'target'); stable()
      write(cfg.manifest, cfg.original_manifest)
      adapter.restart(); ready(); attachments(cfg.initial_keys); query('valueB', 'target')
    elseif cfg.scenario == 'nested' then
      query('Nested', 'nested')
      -- Both roots now include the same canonical source: native ownership must refuse.
      write(cfg.manifest, vim.json.encode(cfg.duplicate_manifest))
      adapter.restart()
      wait(function() return adapter.status().phase == 'failed' and adapter.status().cleanup == 'safe' end,
        'native duplicate ownership refusal')
      observed.refused_status = adapter.status()
      assert(not observed.refused_status.restart_blocked and clients() == 0, 'refusal cleanup failed')
      stable(); observed.after_refusal = snapshot()
      write(cfg.manifest, cfg.original_manifest)
      adapter.restart(); ready(); attachments(cfg.initial_keys)
      query('valueB', 'target'); query('Nested', 'nested')
    end
    stable(); observed.after = snapshot()
    adapter.stop(); wait(function() return clients() == 0 end, 'safe native exit'); pause()
  end
  observed.terminal_status = adapter.status()
  assert(not observed.terminal_status.ready and not observed.terminal_status.pending_start
    and not observed.terminal_status.restart_blocked and observed.terminal_status.cleanup == 'safe', 'unsafe terminal status')
  observed.completed = true
end
local ok, failure = xpcall(run, debug.traceback)
if not ok then
  observed.failure, observed.failure_status = failure, adapter.status()
  pcall(adapter.stop); pcall(function() wait(function() return clients() == 0 end, 'failure cleanup'); pause() end)
  observed.failure_cleanup_status = adapter.status()
  observed.failure_buffers = snapshot()
end
observed.before_restore_manifest = read(cfg.manifest)
write(cfg.manifest, cfg.original_manifest)
poll:stop(); poll:close()
observed.notifications = notifications
vim.fn.writefile({ vim.json.encode({ ok = ok, observed = observed }) }, cfg.result)
if ok then vim.cmd('qa!') else io.stderr:write(failure .. '\n'); vim.cmd('cquit 1') end
