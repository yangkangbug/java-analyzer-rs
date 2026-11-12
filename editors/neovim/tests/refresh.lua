-- Original Apache-2.0 real-editor lifecycle acceptance. Never fakes analysis.
local api, uv = vim.api, vim.uv
vim.o.modeline, vim.o.swapfile, vim.o.hidden = false, false, true
local cfg = vim.json.decode(table.concat(vim.fn.readfile(arg[1]), '\n'))
vim.opt.runtimepath:prepend(cfg.adapter)
-- Observe actual module-owned public buffer registrations, not only status counters.
local observer_attaches, observer_lines = {}, {}
local original_attach = api.nvim_buf_attach
api.nvim_buf_attach = function(buf, send_buffer, options)
  local caller = debug.getinfo(2, 'S').source
  if caller:match('/lua/java_analyzer%.lua$') then
    observer_attaches[tostring(buf)] = (observer_attaches[tostring(buf)] or 0) + 1
    local on_lines = options.on_lines
    if on_lines then options.on_lines = function(...)
      observer_lines[buf] = (observer_lines[buf] or 0) + 1
      return on_lines(...)
    end end
  end
  return original_attach(buf, send_buffer, options)
end
local adapter = require('java_analyzer')
local observed = { replays = {}, disk_actions = {}, disk_checks = 0, peak_clients = 0, ready_seen = false, failure_stage = 'setup' }
local exits, notifications, expected_disk, buffers = {}, {}, vim.deepcopy(cfg.original_disk), {}
vim.notify = function(message) notifications[#notifications + 1] = tostring(message) end
local function wait_for(predicate, label, timeout)
  observed.failure_stage = ({ ['held initialize'] = 'await_initialize_evidence',
    ['bounded fail-closed status'] = 'await_blocked', ['all observed client exits'] = 'await_client_exit' })[label]
    or 'await_editor_operation'
  assert(vim.wait(timeout or 7000, predicate, 5), 'timeout: ' .. label)
end
local function clients()
  return #vim.lsp.get_clients({ name = 'java-analyzer-preview', _uninitialized = true })
end
local poll = uv.new_timer()
poll:start(0, 2, vim.schedule_wrap(function()
  observed.peak_clients = math.max(observed.peak_clients, clients())
  observed.ready_seen = observed.ready_seen or adapter.status().ready
end))
local function raw_read(path)
  local f = io.open(path, 'rb'); if not f then return nil end
  local text = f:read('*a'); f:close(); return text
end
local function raw_write(path, value)
  local f = assert(io.open(path, 'wb')); assert(f:write(value)); assert(f:close())
end
local function disk_check()
  for path, original in pairs(expected_disk) do
    local expected = original ~= false and original or nil
    assert(raw_read(path) == expected, 'product unexpectedly wrote fixture: ' .. path)
  end
  observed.disk_checks = observed.disk_checks + 1
end
local function disk_write(path, value)
  disk_check(); raw_write(path, value); expected_disk[path] = value
  observed.disk_actions[#observed.disk_actions + 1] = { kind = 'harness_write', path = path, sha256 = vim.fn.sha256(value) }
  disk_check()
end
local function manifest(value) disk_write(cfg.manifest, vim.json.encode(value)) end
local function open(path)
  local b = vim.fn.bufadd(path); vim.fn.bufload(b); vim.bo[b].filetype = 'java'; return b
end
local function text(b, value)
  api.nvim_buf_set_lines(b, 0, -1, false, vim.split(value, '\n', { plain = true }))
end
local function position(b, word)
  for i, line in ipairs(api.nvim_buf_get_lines(b, 0, -1, false)) do
    local col = line:find(word, 1, true); if col then return { i, col - 1 } end
  end
  error('missing token ' .. word)
end
local function snapshot(b)
  return { name = api.nvim_buf_get_name(b), bufnr = b, lines = api.nvim_buf_get_lines(b, 0, -1, false),
    modified = vim.bo[b].modified, tick = api.nvim_buf_get_changedtick(b),
    fileformat = vim.bo[b].fileformat, endofline = vim.bo[b].endofline,
    fixendofline = vim.bo[b].fixendofline }
end
local function snapshots()
  local all = {}; for key, b in pairs(buffers) do all[key] = snapshot(b) end; return all
end
local function unchanged(before)
  for key, value in pairs(before) do assert(vim.deep_equal(value, snapshot(buffers[key])), key .. ' buffer changed') end
  disk_check()
end
local function mark(kind) vim.fn.writefile({ 'hold actual native response' }, cfg.control .. '/hold-next-' .. kind) end
local function held(kind)
  local result
  wait_for(function()
    for _, path in ipairs(vim.fn.glob(cfg.control .. '/held-*-' .. kind .. '.json', false, true)) do
      local ok, value = pcall(vim.json.decode, raw_read(path)); if ok then result = value; return true end
    end
  end, 'held ' .. kind)
  return result
end
local function release(data) vim.fn.writefile({ 'release' }, cfg.control .. '/release-' .. data.pid .. '-' .. data.kind) end
local function pause(ms) vim.wait(ms, function() return false end, 5) end
local function ready() wait_for(function() return adapter.status().ready end, 'ready after full replay') end
local function stopped() wait_for(function() return clients() == 0 end, 'all observed client exits') end
local function place(word)
  api.nvim_set_current_buf(buffers.app); api.nvim_win_set_cursor(0, position(buffers.app, word))
end
local function request(word, callback)
  place(word)
  local result, count
  count = 0
  local id, cancel = adapter.definition({ on_result = function(r)
    result, count = r, count + 1
    if callback then callback(r) end
  end })
  assert(id, 'definition not sent')
  return function() return result, count end, cancel
end
local function query(word, status, target)
  local result = request(word)
  wait_for(function() return result() end, 'definition result')
  local r, count = result()
  assert(r.status == status and count == 1, vim.inspect(r))
  if target then
    assert(api.nvim_get_current_buf() == target, 'wrong actual target buffer')
    assert(vim.deep_equal(api.nvim_win_get_cursor(0), position(target, word)), 'wrong actual target cursor')
  else
    assert(api.nvim_get_current_buf() == buffers.app, 'unexpected navigation')
    assert(vim.deep_equal(api.nvim_win_get_cursor(0), position(buffers.app, word)), 'unexpected cursor move')
  end
  observed.navigation = observed.navigation or {}
  local expected_cursor = position(target or buffers.app, word)
  observed.navigation[#observed.navigation + 1] = { word = word, expected_status = status,
    actual_buf = api.nvim_get_current_buf(), actual_cursor = api.nvim_win_get_cursor(0),
    expected_buf = target or buffers.app, expected_cursor = expected_cursor,
    target_uri = target and vim.uri_from_bufnr(target) or false,
    target_line = target and api.nvim_buf_get_lines(target, expected_cursor[1] - 1, expected_cursor[1], false)[1] or false }
  disk_check()
  observed.results = observed.results or {}; observed.results[#observed.results + 1] = r
  return r
end
local function replay(ordinal, keys)
  local documents = {}
  for _, key in ipairs(keys) do
    local b = buffers[key]
    local ending = vim.bo[b].fileformat == 'dos' and '\r\n' or '\n'
    documents[vim.uri_from_bufnr(b)] = table.concat(api.nvim_buf_get_lines(b, 0, -1, false), ending)
      .. (vim.bo[b].endofline and ending or '')
  end
  observed.replays[#observed.replays + 1] = { ordinal = ordinal, documents = documents }
end
local function blocked()
  wait_for(function() return adapter.status().restart_blocked == true end, 'bounded fail-closed status')
  assert(not adapter.status().ready and not adapter.status().pending_start, 'blocked session still ready/queued')
  assert(type(adapter.status().error) == 'string', 'missing explicit lifecycle failure')
  observed.timeout_status = adapter.status()
  adapter.restart(); adapter.start(); pause(150)
  assert(adapter.status().restart_blocked and not adapter.status().pending_start, 'unsafe replacement accepted')
  stopped()
  -- A late start/restart after observed process exit must remain blocked.
  adapter.restart(); adapter.start(); pause(50)
  observed.post_exit_status = adapter.status()
  assert(clients() == 0 and adapter.status().restart_blocked and not adapter.status().pending_start,
    'uncertain cleanup became restartable after process exit')
end
local function run()
  for _, key in ipairs({ 'app', 'target', 'extra', 'inventory', 'second' }) do buffers[key] = open(cfg[key]) end
  local config = { binary = cfg.proxy, manifest = cfg.manifest,
    lifecycle_timeouts = { initialize_ms = 5000, shutdown_ms = 1500, force_ms = 700 },
    on_exit = function(code, signal)
      exits[#exits + 1] = { code = code, signal = signal }
      if #exits == 1 and cfg.scenario == 'exit_stop' then adapter.stop()
      elseif #exits == 1 and cfg.scenario == 'exit_restart' then adapter.restart() end
    end }
  local initialization_timeout = cfg.scenario == 'initialize_timeout' or cfg.scenario == 'initialize_startup_timeout'
  if initialization_timeout then
    -- Keep the normal startup budget: a cold Python relay on Intel macOS
    -- can take longer than 200 ms even to record its native child. The
    -- unreleased 12-second response gates inject the fault, not a race
    -- against interpreter startup. The product timer still runs unchanged.
    observed.initialize_timeout_ms = config.lifecycle_timeouts.initialize_ms
  end
  if cfg.scenario == 'shutdown_timeout' then config.lifecycle_timeouts.shutdown_ms = 200 end
  adapter.setup(config)
  local initial_hold = cfg.scenario:match('^initializing_') or initialization_timeout
  if initial_hold then mark('initialize') end
  adapter.start()
  observed.expected_spawns = 1
  if not initial_hold then
    ready(); replay(1, { 'app', 'target', 'extra', 'inventory' })
    query('valueA', 'jumped', buffers.target)
  end
  text(buffers.app, cfg.app_text:gsub('valueA', 'valueB'):gsub('deepA', 'deepB'))
  text(buffers.target, '// unsaved shifted target\n' .. cfg.target_text:gsub('valueA', 'valueB'))
  vim.bo[buffers.target].endofline, vim.bo[buffers.target].fixendofline = false, false
  text(buffers.extra, '// unsaved inherited dependency\n' .. cfg.extra_text:gsub('deepA', 'deepB'))
  text(buffers.inventory, '// unsaved inventory\npackage lib; class Inventory {}')
  text(buffers.second, '// unsaved alternative root\n' .. cfg.target_text:gsub('valueA', 'valueB'):gsub(' extends Base', ''))
  observed.before = snapshots()
  disk_check()
  if not initial_hold then query('valueB', 'jumped', buffers.target); query('deepB', 'jumped', buffers.extra) end
  local scenario = cfg.scenario
  local function restart(keys)
    adapter.restart(); observed.expected_spawns = observed.expected_spawns + 1; ready()
    replay(observed.expected_spawns, keys or { 'app', 'target', 'extra', 'inventory' })
  end
  if scenario == 'add' or scenario == 'add_new_buffer' then
    disk_write(cfg.added, 'package lib; public class Added { public int valueA; }\n')
    if scenario == 'add_new_buffer' then
      buffers.added = open(cfg.added)
      text(buffers.added, '// unsaved new source\npackage lib; public class Added { public int valueB; }')
      text(buffers.app, cfg.app_text:gsub('Library', 'Added'):gsub('valueA', 'valueB'))
      observed.before = snapshots()
    end
    query('valueB', 'error')
    restart(scenario == 'add_new_buffer' and { 'app', 'target', 'extra', 'inventory', 'added' } or nil)
    query('valueB', 'jumped', scenario == 'add_new_buffer' and buffers.added or buffers.target)
  elseif scenario == 'delete' then
    disk_check(); assert(uv.fs_unlink(cfg.inventory)); expected_disk[cfg.inventory] = false
    observed.disk_actions[#observed.disk_actions + 1] = { kind = 'harness_delete', path = cfg.inventory }
    query('valueB', 'error'); restart({ 'app', 'target', 'extra' }); query('valueB', 'jumped', buffers.target)
    assert(not vim.lsp.buf_is_attached(buffers.inventory, adapter.status().client_id), 'removed source replayed')
  elseif scenario == 'rename' then
    disk_check(); assert(uv.fs_rename(cfg.target, cfg.renamed))
    expected_disk[cfg.renamed], expected_disk[cfg.target] = expected_disk[cfg.target], false
    observed.disk_actions[#observed.disk_actions + 1] = { kind = 'harness_rename', from = cfg.target, to = cfg.renamed }
    api.nvim_buf_set_name(buffers.target, cfg.renamed); observed.before.target = snapshot(buffers.target)
    assert(not adapter.status().ready, 'renamed frozen session remains ready')
    restart(); query('valueB', 'jumped', buffers.target)
  elseif scenario == 'configuration' or scenario == 'owner' or scenario == 'edge' then
    local next_manifest = vim.deepcopy(cfg.manifest_initial)
    if scenario == 'configuration' then next_manifest.compilations[2].source_roots = { 'other' }
    elseif scenario == 'owner' then
      next_manifest.compilations[2].id.project = 'other-owner'
      next_manifest.compilations[1].dependencies = { { project = 'other-owner', source_set = 'main' } }
    else next_manifest.compilations[1].dependencies = {} end
    manifest(next_manifest)
    query('valueB', 'jumped', buffers.target) -- Existing session deliberately remains frozen.
    restart(scenario == 'configuration' and { 'app', 'second' } or nil)
    query('valueB', scenario == 'edge' and 'empty' or 'jumped',
      scenario == 'configuration' and buffers.second or scenario == 'owner' and buffers.target or nil)
    if scenario == 'configuration' then
      assert(not vim.lsp.buf_is_attached(buffers.target, adapter.status().client_id), 'outside-owned buffer replayed')
    end
  elseif scenario == 'initialize_refusal' then
    local invalid = vim.deepcopy(cfg.manifest_initial)
    invalid.compilations[1].dependencies = { { project = 'absent', source_set = 'main' } }
    manifest(invalid); adapter.restart(); observed.expected_spawns = 2
    wait_for(function() return not adapter.status().active and type(adapter.status().error) == 'string' end,
      'native initialization refusal must become terminal', 3500)
    stopped(); observed.refused_status = adapter.status()
    assert(not adapter.status().ready and not adapter.status().restart_blocked, 'known refusal cannot recover')
    manifest(cfg.manifest_initial); restart(); query('valueB', 'jumped', buffers.target)
  elseif scenario == 'readiness_reentrant' then
    local old, attaches = adapter.status().client_id, 0
    observed.attach_ready = {}
    local ac = api.nvim_create_autocmd('LspAttach', { callback = function(event)
      if event.data.client_id ~= old then
        attaches = attaches + 1
        observed.attach_ready[#observed.attach_ready + 1] = adapter.status().ready
        local count = 0
        place('valueB')
        observed.attach_request = observed.attach_request or {}
        observed.attach_request[#observed.attach_request + 1] = adapter.definition({ on_result = function() count = count + 1 end }) or false
      end
    end })
    restart(); api.nvim_del_autocmd(ac); assert(attaches == 4, 'did not observe all replay attachments')
    for _, exposed in ipairs(observed.attach_ready) do assert(not exposed, 'ready exposed before complete replay at LspAttach') end
    for _, sent in ipairs(observed.attach_request) do assert(not sent, 'definition admitted during partial replay') end
    observed.attach_count = attaches; query('valueB', 'jumped', buffers.target); query('deepB', 'jumped', buffers.extra)
  elseif scenario == 'held_restarts' or scenario == 'reentrant_restart' or scenario == 'reentrant_stop' then
    mark('definition')
    local reentered = 0
    local pending = request('valueB', function()
      if scenario == 'reentrant_restart' then reentered = reentered + 1; adapter.restart()
      elseif scenario == 'reentrant_stop' then reentered = reentered + 1; adapter.stop() end
    end)
    local data = held('definition')
    assert(type(data.message.result) == 'table' and data.message.result.uri, 'held result must be real native target')
    observed.held_result = data.message.result
    place('valueB')
    adapter.restart()
    local result, count = pending()
    assert(result and result.status == 'stale' and count == 1, 'retirement did not settle callback synchronously exactly once')
    if scenario == 'held_restarts' then for _ = 1, 7 do adapter.restart() end end
    assert(not adapter.status().ready, 'replacement ready before observed exit')
    assert(clients() == 1, 'overlapping client created while old reply held')
    observed.settled_before_release = { status = result.status, count = count }
    local before_buf, before_pos = api.nvim_get_current_buf(), api.nvim_win_get_cursor(0)
    release(data)
    if scenario == 'reentrant_stop' then
      stopped(); pause(80); assert(not adapter.status().pending_start and not adapter.status().ready, 'stop lost to older restart')
    else
      observed.expected_spawns = 2; ready(); replay(2, { 'app', 'target', 'extra', 'inventory' })
    end
    pause(50); observed.callback_final_count = select(2, pending()); assert(select(2, pending()) == 1, 'late response settled callback twice')
    assert(api.nvim_get_current_buf() == before_buf and vim.deep_equal(api.nvim_win_get_cursor(0), before_pos), 'late old response navigated')
    if scenario ~= 'held_restarts' then assert(reentered == 1, 'reentrant callback count') end
    if scenario ~= 'reentrant_stop' then query('valueB', 'jumped', buffers.target) end
  elseif scenario == 'attach_load_edit' or scenario == 'attach_restart' or scenario == 'attach_stop' then
    local old, fired = adapter.status().client_id, 0
    if scenario == 'attach_load_edit' then
      api.nvim_buf_delete(buffers.inventory, { force = true }); buffers.inventory = nil
    end
    local ac = api.nvim_create_autocmd('LspAttach', { callback = function(event)
      if event.data.client_id ~= old and fired == 0 then
        fired = fired + 1
        observed.attach_initial_ready = adapter.status().ready
        if scenario == 'attach_restart' then adapter.restart()
        elseif scenario == 'attach_stop' then adapter.stop()
        else
          buffers.inventory = open(cfg.inventory)
          text(buffers.inventory, '// loaded and edited during replay\npackage lib; class Inventory {}')
          text(buffers.app, '// app edited during replay\n' .. cfg.app_text:gsub('valueA', 'valueB'):gsub('deepA', 'deepB'))
          observed.before.inventory, observed.before.app = snapshot(buffers.inventory), snapshot(buffers.app)
        end
      end
    end })
    adapter.restart()
    observed.expected_spawns = scenario == 'attach_restart' and 3 or 2
    if scenario == 'attach_stop' then
      wait_for(function() return fired == 1 end, 'stop inside attachment'); stopped()
      assert(not adapter.status().ready and not adapter.status().pending_start, 'old replay overrode reentrant stop')
    else
      ready(); replay(observed.expected_spawns, { 'app', 'target', 'extra', 'inventory' })
      query('valueB', 'jumped', buffers.target); query('deepB', 'jumped', buffers.extra)
    end
    api.nvim_del_autocmd(ac)
    assert(fired == 1 and not observed.attach_initial_ready, 'reentrant attachment readiness')
  elseif scenario == 'request_restart' or scenario == 'request_stop' then
    local fired = 0
    local ac = api.nvim_create_autocmd('LspRequest', { callback = function(event)
      if event.data.request.method == 'textDocument/definition' and event.data.request.type == 'pending' and fired == 0 then
        fired = fired + 1
        if scenario == 'request_restart' then adapter.restart() else adapter.stop() end
      end
    end })
    local pending = request('valueB')
    api.nvim_del_autocmd(ac)
    local result, count = pending()
    assert(fired == 1 and result and result.status == 'stale' and count == 1, 'LspRequest reentrant retirement missed callback')
    if scenario == 'request_restart' then
      observed.expected_spawns = 2; ready(); replay(2, { 'app', 'target', 'extra', 'inventory' })
      query('valueB', 'jumped', buffers.target)
    else stopped() end
    pause(50); assert(select(2, pending()) == 1, 'LspRequest duplicate callback')
    observed.request_callback = { status = result.status, count = select(2, pending()) }
  elseif scenario == 'explicit_cancel' then
    vim.fn.writefile({ 'send genuine request and cancellation in one transport write' }, cfg.control .. '/batch-cancel-next')
    local pending, cancel = request('valueB')
    cancel(); cancel()
    local result, count = pending()
    assert(result and result.status == 'cancelled' and count == 1, 'explicit cancellation did not settle locally once')
    wait_for(function()
      for _, line in ipairs(vim.fn.readfile(cfg.control .. '/protocol.jsonl')) do
        local e = vim.json.decode(line)
        if e.direction == 'server_to_client' and e.method == 'textDocument/definition'
          and e.message.error and e.message.error.code == -32800 then observed.native_cancelled = true; return true end
      end
    end, 'actual native RequestCancelled response')
    pause(50); assert(select(2, pending()) == 1, 'native cancellation duplicated callback')
    assert(api.nvim_get_current_buf() == buffers.app, 'cancelled request navigated')
    observed.cancel_callback = { status = result.status, count = select(2, pending()) }
    query('valueB', 'jumped', buffers.target)
  elseif scenario == 'exit_stop' or scenario == 'exit_restart' then
    if scenario == 'exit_stop' then
      adapter.restart(); wait_for(function() return #exits == 1 end, 'exit callback stop'); stopped(); pause(50)
      assert(not adapter.status().ready and not adapter.status().pending_start, 'on_exit stop did not win')
    else
      adapter.stop(); observed.expected_spawns = 2; ready(); replay(2, { 'app', 'target', 'extra', 'inventory' })
      query('valueB', 'jumped', buffers.target)
    end
  elseif scenario == 'observer_stability' then
    observed.observers = { adapter.status().observer_count }
    observed.observer_attachments = { vim.deepcopy(observer_attaches) }
    assert(type(observed.observers[1]) == 'number', 'missing stable observer count')
    for _ = 1, 5 do
      restart(); observed.observers[#observed.observers + 1] = adapter.status().observer_count
      assert(adapter.status().observer_count == observed.observers[1], 'untouched buffer observers grew')
      observed.observer_attachments[#observed.observer_attachments + 1] = vim.deepcopy(observer_attaches)
      assert(vim.deep_equal(observer_attaches, observed.observer_attachments[1]), 'actual buffer observer attachments grew')
    end
    local before_lines = observer_lines[buffers.target] or 0
    text(buffers.target, '// observer callback exercise\n' .. cfg.target_text:gsub('valueA', 'valueB'))
    observed.before.target = snapshot(buffers.target)
    observed.observer_mutation_callbacks = (observer_lines[buffers.target] or 0) - before_lines
    assert(observed.observer_mutation_callbacks == 1, 'one edit invoked accumulated old buffer observers')
    query('valueB', 'jumped', buffers.target)
  elseif initial_hold then
    if initialization_timeout then
      -- The deadline includes native/proxy startup. A timeout may correctly
      -- happen before this fixture receives a reply, so never wait for held
      -- response evidence before checking the product's fail-closed state.
      blocked()
      observed.initialize_timeout_stage = 'before_response'
      for _, line in ipairs(vim.fn.readfile(cfg.control .. '/protocol.jsonl')) do
        local e = vim.json.decode(line)
        if e.event == 'held' and e.kind == 'initialize' then
          observed.initialize_timeout_stage = 'held_response'
        end
      end
    else
      local data = held('initialize')
      assert(data.message.result and data.message.result.capabilities, 'held actual initialize success missing')
      assert(not adapter.status().ready, 'initialization hold exposed readiness')
      if scenario == 'initializing_stop' then
        adapter.stop(); adapter.stop(); pause(80); assert(clients() == 1, 'stop blindly terminated pending initialize')
        release(data); stopped(); assert(not adapter.status().pending_start, 'stop left queued restart')
      elseif scenario == 'initializing_restart' then
        for _ = 1, 6 do adapter.restart() end
        assert(clients() == 1, 'pending initialize restart overlap')
        release(data); observed.expected_spawns = 2; ready(); replay(2, { 'app', 'target', 'extra', 'inventory' })
        query('valueB', 'jumped', buffers.target); query('deepB', 'jumped', buffers.extra)
      else
        text(buffers.extra, '// edited while initializing\n' .. cfg.extra_text:gsub('deepA', 'deepB'))
        observed.before.extra = snapshot(buffers.extra)
        release(data); ready(); replay(1, { 'app', 'target', 'extra', 'inventory' })
        query('valueB', 'jumped', buffers.target); query('deepB', 'jumped', buffers.extra)
      end
    end
  elseif scenario == 'shutdown_timeout' then
    mark('shutdown'); adapter.restart(); local data = held('shutdown')
    assert(data.message.result == vim.NIL, 'shutdown reply not actual null')
    blocked()
  elseif scenario == 'abnormal_exit' then
    local events = vim.fn.readfile(cfg.control .. '/protocol.jsonl')
    local pid
    for _, line in ipairs(events) do local e = vim.json.decode(line); if e.event == 'spawn' then pid = e.proxy_pid end end
    vim.fn.writefile({ 'terminate owned native child' }, cfg.control .. '/terminate-native-' .. pid)
    blocked()
  else error('unknown scenario ' .. scenario) end
  observed.failure_stage = 'assert_preservation'
  unchanged(observed.before); observed.after = snapshots()
  adapter.stop(); adapter.stop(); stopped(); pause(30)
  observed.terminal_status = adapter.status()
  observed.terminal_phase = observed.terminal_status.phase or 'legacy'
  assert(not adapter.status().ready and not adapter.status().pending_start, 'not stopped at completion')
  observed.completed = true
  observed.failure_stage = 'completed'
end
local ok, failure = xpcall(run, debug.traceback)
if not ok then
  observed.failure = failure
  local status_ok, status = pcall(adapter.status)
  if status_ok then observed.failure_status = status end
  pcall(adapter.stop)
end
-- Restoration is the harness's deliberate disk action, including failure paths.
observed.before_restore = {}
for path in pairs(expected_disk) do observed.before_restore[path] = raw_read(path) or false end
for path in pairs(expected_disk) do if not cfg.original_disk[path] then pcall(uv.fs_unlink, path) end end
for path, original in pairs(cfg.original_disk) do raw_write(path, original) end
poll:stop(); poll:close()
observed.exits, observed.notifications = exits, notifications
vim.fn.writefile({ vim.json.encode({ scenario = cfg.scenario, ok = ok, observed = observed }) }, cfg.result)
if not ok then io.stderr:write(failure .. '\n'); vim.cmd('cquit 1') else vim.cmd('qa!') end
