-- Original Apache-2.0 code. Optional Neovim 0.11.5 definition-only adapter.
-- Never downloads software, writes sources, imports builds, or starts Java.
local M = {}
local api, uv = vim.api, vim.uv
local configured, current, last_session, pending_start, lifecycle_error
local session_serial, intent_serial = 0, 0
local blocked, drive_scheduled = false, false
local observers, group = {}, nil
local drive, retire, fail_closed, replay

local function warn(message)
  vim.schedule(function() vim.notify('java-analyzer: ' .. message, vim.log.levels.WARN) end)
end

local function message(value)
  value = tostring(value)
  local last = math.min(#value, 2048)
  -- Native JSON errors are UTF-8. Do not split a path's final code point.
  while last > 0 do
    local next_byte = value:byte(last + 1)
    if not next_byte or next_byte < 128 or next_byte >= 192 then break end
    last = last - 1
  end
  return value:sub(1, last)
end

local function finish(request, status, result, err)
  if request.finished then return end
  request.finished = true
  request.session.pending[request.serial] = nil
  if request.on_result then
    local ok, failure = pcall(request.on_result, { status = status, result = result, error = err })
    if not ok then warn('result callback failed: ' .. message(failure)) end
  elseif status == 'error' then
    warn(err and err.message or 'definition request failed')
  end
end

local function cancel_request(request, explicit)
  if request.finished then return end
  if explicit then request.cancelled = true else request.obsolete = true end
  if not request.cancel_scheduled then
    request.cancel_scheduled = true
    -- on_lines runs under textlock. Invalidation is immediate; protocol work
    -- is scheduled so unrelated autocmds never run under textlock.
    vim.schedule(function()
      local client = request.session.client
      if request.id and client and client.requests[request.id] and not client:is_stopped() then
        client:cancel_request(request.id)
      end
    end)
  end
  if explicit then finish(request, 'cancelled') end
end

local function invalidate(session, settle)
  session.generation = session.generation + 1
  local requests = {}
  for _, request in pairs(session.pending) do
    request.obsolete = true
    requests[#requests + 1] = request
  end
  -- Mark every request before invoking any reentrant user callback.
  for _, request in ipairs(requests) do
    cancel_request(request, false)
    if settle then finish(request, request.cancelled and 'cancelled' or 'stale') end
  end
end

local function canonical(path)
  return uv.fs_realpath(path)
end

local function source_path(session, bufnr)
  if not api.nvim_buf_is_loaded(bufnr) or vim.bo[bufnr].buftype ~= '' then return nil end
  local path = canonical(api.nvim_buf_get_name(bufnr))
  if not path or path:sub(-5) ~= '.java' then return nil end
  for _, root in ipairs(session.roots) do
    if path:sub(1, #root + 1) == root .. '/' then return path end
  end
end

local function live(session)
  return current == session and session.active and not session.exit and not blocked
end

local function untrack(session, bufnr)
  if session.buffers[bufnr] then
    session.buffers[bufnr] = nil
    if live(session) then invalidate(session) end
  end
end

local function observe(bufnr)
  if observers[bufnr] then return true end
  local token = {}
  observers[bufnr] = token
  local function changed()
    local session = current
    if session and live(session) and session.buffers[bufnr] then invalidate(session) end
  end
  local ok = api.nvim_buf_attach(bufnr, false, {
    on_lines = changed,
    on_reload = changed,
    on_detach = function()
      if observers[bufnr] ~= token then return end
      observers[bufnr] = nil
      local session = current
      if session then untrack(session, bufnr) end
    end,
  })
  if not ok then observers[bufnr] = nil end
  return ok
end

local function attach(session, bufnr)
  if not live(session) or not session.client or not session.client.initialized then return false end
  local path = source_path(session, bufnr)
  if not path then
    if session.buffers[bufnr] then
      untrack(session, bufnr)
      if vim.lsp.buf_is_attached(bufnr, session.client.id) then
        vim.lsp.buf_detach_client(bufnr, session.client.id)
      end
    end
    return true
  end
  local old = session.buffers[bufnr]
  if old and old.path == path and vim.lsp.buf_is_attached(bufnr, session.client.id) then return true end
  if not observe(bufnr) then return false end
  session.open_serial = session.open_serial + 1
  local entry = { path = path, epoch = session.open_serial }
  session.buffers[bufnr] = entry
  invalidate(session)
  local was_ready = session.ready
  session.ready = false
  local ok, attached = pcall(vim.lsp.buf_attach_client, bufnr, session.client.id)
  if not ok or not attached then
    untrack(session, bufnr)
    return false
  end
  -- LspAttach can synchronously stop/restart, rename or detach this buffer.
  if live(session) then session.ready = was_ready end
  return live(session) and session.buffers[bufnr] == entry
    and source_path(session, bufnr) == path
    and vim.lsp.buf_is_attached(bufnr, session.client.id)
end

local function options(opts)
  local version = vim.version()
  assert(version.major == 0 and version.minor == 11 and version.patch == 5,
    'this preview adapter is validated for Neovim 0.11.5 only')
  assert(opts == nil or type(opts) == 'table', 'options must be a table')
  opts = vim.deepcopy(opts or configured or {})
  for _, name in ipairs({ 'binary', 'manifest' }) do
    if type(opts[name]) ~= 'string' or opts[name]:sub(1, 1) ~= '/' then
      error(name .. ' must be an explicit absolute path')
    end
    opts[name] = assert(canonical(opts[name]), name .. ' does not exist')
  end
  assert(vim.fn.executable(opts.binary) == 1, 'binary is not executable')
  local stat = assert(uv.fs_stat(opts.manifest))
  assert(stat.type == 'file' and stat.size <= 1048576, 'manifest must be a file of at most 1 MiB')
  local file = assert(io.open(opts.manifest, 'rb'))
  local data = file:read('*a')
  file:close()
  local manifest = vim.json.decode(data)
  assert(manifest.schema_version == 1 and type(manifest.compilations) == 'table', 'invalid manifest')
  opts.root = vim.fs.dirname(opts.manifest)
  opts.roots = {}
  for _, compilation in ipairs(manifest.compilations) do
    for _, root in ipairs(compilation.source_roots or {}) do
      local absolute = root:sub(1, 1) == '/' and root or opts.root .. '/' .. root
      local resolved = assert(canonical(absolute), 'source root does not exist: ' .. root)
      opts.roots[#opts.roots + 1] = resolved
    end
  end
  local deadlines = opts.lifecycle_timeouts
  if deadlines == nil then deadlines = {} end
  assert(type(deadlines) == 'table', 'lifecycle_timeouts must be a table')
  opts.lifecycle_timeouts = {}
  for name, default in pairs({ initialize_ms = 35000, shutdown_ms = 5000, force_ms = 2000 }) do
    local value = deadlines[name]
    if value == nil then value = default end
    assert(type(value) == 'number' and value == math.floor(value) and value >= 50 and value <= 120000,
      name .. ' must be an integer from 50 to 120000 milliseconds')
    opts.lifecycle_timeouts[name] = value
  end
  assert(opts.on_exit == nil or type(opts.on_exit) == 'function', 'on_exit must be a function')
  return opts
end

local function cancel_timer(session)
  if session.timer then
    session.timer:stop()
    session.timer:close()
    session.timer = nil
  end
end

local function timer(session, milliseconds, callback)
  cancel_timer(session)
  local created, handle = pcall(uv.new_timer)
  if not created or not handle then
    fail_closed(session, 'could not allocate a lifecycle timer; cleanup is uncertain')
    return
  end
  session.timer = handle
  local started, result = pcall(handle.start, handle, milliseconds, 0, vim.schedule_wrap(function()
    if session.timer ~= handle then return end
    cancel_timer(session)
    if current == session and not session.exit then callback() end
  end))
  if not started or result == nil then
    cancel_timer(session)
    fail_closed(session, 'could not start a lifecycle timer; cleanup is uncertain')
  end
end

local function schedule_drive()
  if drive_scheduled then return end
  drive_scheduled = true
  vim.schedule(function()
    drive_scheduled = false
    drive()
  end)
end

local function set_error(session, reason)
  reason = message(reason)
  session.error, lifecycle_error = reason, reason
  warn(reason)
end

fail_closed = function(session, reason)
  if current ~= session then return end
  blocked, pending_start = true, nil
  session.active, session.ready, session.phase, session.cleanup = false, false, 'blocked', 'uncertain'
  set_error(session, reason .. '; automatic restart blocked. Check remaining analyzer processes before starting a fresh Neovim session')
  invalidate(session, true)
  if session.exit or session.forced then return end
  session.forced = true
  if session.client then pcall(session.client.stop, session.client, true)
  elseif session.transport then pcall(session.transport.terminate) end
  timer(session, session.config.lifecycle_timeouts.force_ms, function()
    set_error(session, 'forced stop exit was not observed; cleanup remains uncertain. Check remaining analyzer processes before starting a fresh Neovim session')
  end)
end

retire = function(session)
  if current ~= session or session.exit then return end
  if not session.retiring then
    session.retiring = true
    session.active, session.ready = false, false
    session.phase, session.cleanup = blocked and 'blocked' or 'retiring', blocked and 'uncertain' or 'pending'
    invalidate(session, true)
  end
  schedule_drive()
end

local function retirement(session)
  if session.shutdown_sent or session.exit or blocked then return end
  if not session.init_settled then return end -- Keep the bounded initialization timer.
  session.shutdown_sent = true
  local ok, sent
  if session.refused then
    -- Native RequestFailed is emitted only after its inspection job completed.
    session.expected_exit = 1
    ok, sent = pcall(session.client.notify, session.client, 'exit')
  elseif session.client.initialized then
    session.expected_exit = 0
    ok = pcall(session.client.stop, session.client, false)
    sent = ok
  end
  if not ok or not sent then fail_closed(session, 'could not request safe retirement'); return end
  timer(session, session.config.lifecycle_timeouts.shutdown_ms, function()
    fail_closed(session, 'graceful retirement deadline exceeded')
  end)
end

local function failed_session(session, reason)
  set_error(session, reason)
  retire(session)
end

local function install_events()
  if group then return end
  group = api.nvim_create_augroup('java_analyzer_preview', { clear = true })
  api.nvim_create_autocmd({ 'BufReadPost', 'BufEnter' }, { group = group,
    callback = function(event)
      local session = current
      if session and live(session) and session.client and session.client.initialized then
        if not attach(session, event.buf) and live(session) then
          failed_session(session, 'could not attach a supported source buffer')
        end
      end
    end })
  api.nvim_create_autocmd('LspAttach', { group = group,
    callback = function(event)
      local session = current
      if session and live(session) and session.client and event.data.client_id == session.client.id
        and not session.buffers[event.buf] then
        if not attach(session, event.buf) and live(session) then
          failed_session(session, 'could not observe an attached source buffer')
        end
      end
    end })
  api.nvim_create_autocmd('LspDetach', { group = group,
    callback = function(event)
      local session = current
      if session and session.client and event.data.client_id == session.client.id then
        untrack(session, event.buf)
      end
    end })
  api.nvim_create_autocmd('BufFilePost', { group = group,
    callback = function(event)
      local session = current
      if session and live(session) and session.buffers[event.buf] then
        -- A changed buffer name changes the frozen source inventory.
        set_error(session, 'source path changed; restart after updating the project manifest')
        M.stop()
      end
    end })
end

replay = function(session)
  if not live(session) then return end
  session.phase, session.ready = 'replaying', false
  -- LspAttach can load more buffers. Rescan until stable, with explicit work
  -- limits against arbitrary plugins creating/detaching buffers recursively.
  for _ = 1, 8 do
    local count = 0
    for _, bufnr in ipairs(api.nvim_list_bufs()) do
      if not live(session) then return end
      if source_path(session, bufnr) then
        count = count + 1
        if count > 1024 or not attach(session, bufnr) then
          if live(session) then failed_session(session, 'buffer replay failed or exceeded 1024 sources') end
          return
        end
      end
    end
    if not live(session) then return end
    local complete = true
    for _, bufnr in ipairs(api.nvim_list_bufs()) do
      local path, entry = source_path(session, bufnr), session.buffers[bufnr]
      if path and (not entry or entry.path ~= path or not vim.lsp.buf_is_attached(bufnr, session.client.id)) then
        complete = false
        break
      end
    end
    if complete then
      if not session.client:notify('$/setTrace', { value = 'off' }) then
        fail_closed(session, 'could not flush replayed buffers')
      elseif live(session) then
        session.phase, session.ready = 'ready', true
      end
      return
    end
  end
  failed_session(session, 'buffer replay did not stabilize within eight passes')
end

local function exited(session, code, signal)
  if session.exit then return end
  session.exit = { code = code, signal = signal }
  session.active, session.ready = false, false
  cancel_timer(session)
  -- Our on_exit precedes Neovim's internal on_client_exit, which schedules
  -- detach and registry removal. Two turns let that cleanup finish first.
  vim.schedule(function() vim.schedule(function()
    if current ~= session then return end
    local safe = session.retiring and session.shutdown_sent and not session.forced
      and not blocked and signal == 0 and code == session.expected_exit
    if not safe then
      if not blocked then
        fail_closed(session, 'unexpected analyzer exit (' .. code .. ', signal ' .. signal .. ')')
      end
    else
      session.cleanup = 'safe'
      session.phase = session.error and 'failed' or 'idle'
      current, last_session = nil, session
      invalidate(session, true)
    end
    if session.config.on_exit then
      local ok, err = pcall(session.config.on_exit, code, signal)
      if not ok then warn('exit callback failed: ' .. message(err)) end
    end
    schedule_drive()
  end) end)
end

local function launch(config)
  session_serial = session_serial + 1
  local session = { active = true, ready = false, phase = 'initializing', cleanup = 'pending',
    generation = 0, open_serial = 0, serial = session_serial, request_serial = 0,
    pending = {}, buffers = {}, roots = config.roots, config = config }
  current, lifecycle_error = session, nil
  install_events()
  local ok, id = pcall(vim.lsp.start, {
    name = 'java-analyzer-preview',
    cmd = function(dispatchers)
      local rpc = vim.lsp.rpc.start({ config.binary, 'lsp', '--manifest', config.manifest }, dispatchers,
        { cwd = config.root, detached = false })
      session.transport = rpc
      -- Public transport factory: observe the actual initialize error, not
      -- Neovim's later scheduled assertion (outside rpc.lua's try_call).
      return {
        notify = rpc.notify, is_closing = rpc.is_closing, terminate = rpc.terminate,
        request = function(method, params, callback, notify_reply)
          if method ~= 'initialize' then return rpc.request(method, params, callback, notify_reply) end
          return rpc.request(method, params, function(err, result)
            if current ~= session or session.exit or blocked then return end
            session.init_settled = true
            cancel_timer(session)
            if err then
              if type(err) == 'table' and err.code == -32803 and type(err.message) == 'string' then
                session.refused = true
                failed_session(session, 'initialization refused: ' .. message(err.message))
              else
                fail_closed(session, 'unexpected initialization failure')
              end
              return
            end
            local success, failure = pcall(callback, err, result)
            if not success then fail_closed(session, 'initialization callback failed: ' .. message(failure)) end
          end, notify_reply)
        end,
      }
    end,
    cmd_cwd = config.root,
    root_dir = config.root,
    detached = false,
    flags = { debounce_text_changes = 150, exit_timeout = 2000 },
    capabilities = { general = { positionEncodings = { 'utf-16' } } },
    get_language_id = function() return 'java' end,
    on_init = function(client, response)
      session.client = client
      if current ~= session or session.exit or blocked then return end
      if session.retiring then schedule_drive(); return end
      local cap = response.capabilities
      if cap.positionEncoding ~= 'utf-16' or type(cap.textDocumentSync) ~= 'table'
        or cap.textDocumentSync.openClose ~= true or cap.textDocumentSync.change ~= 1
        or cap.definitionProvider ~= true then
        failed_session(session, 'server does not expose the required UTF-16/full-sync/definition capabilities')
        return
      end
      local success, failure = pcall(replay, session)
      if not success then fail_closed(session, 'buffer replay callback failed: ' .. message(failure)) end
    end,
    on_error = function(code, err)
      if current == session then
        -- Transport dispatch normally schedules this hook. Preserve immediate
        -- invalidation even if another documented dispatcher calls it fast.
        session.active, session.ready = false, false
        session.generation = session.generation + 1
        local reason = 'LSP transport/client error ' .. tostring(code) .. ': ' .. message(err)
        if vim.in_fast_event() then
          blocked, pending_start = true, nil
          vim.schedule(function() if current == session then fail_closed(session, reason) end end)
        else fail_closed(session, reason) end
      end
    end,
    on_exit = function(code, signal) exited(session, code, signal) end,
  }, { attach = false, reuse_client = function() return false end })
  if not ok or not id then
    -- If the transport factory started a process before startup failed, its
    -- outcome is uncertain. Do not guess that a missing client ID means exit.
    fail_closed(session, 'could not start java-analyzer: ' .. message(id))
    return
  end
  session.client = assert(vim.lsp.get_client_by_id(id))
  timer(session, config.lifecycle_timeouts.initialize_ms, function()
    fail_closed(session, 'initialization deadline exceeded')
  end)
end

drive = function()
  if blocked then return end
  if current then
    if current.retiring then retirement(current) end
    return
  end
  local intent = pending_start
  if not intent then return end
  pending_start = nil
  local ok, config = pcall(options, intent.options)
  if intent.serial ~= intent_serial then schedule_drive(); return end
  if not ok then
    lifecycle_error = 'configuration rejected: ' .. message(config)
    warn(lifecycle_error)
    return
  end
  launch(config)
end

--- Configure explicit paths without starting software.
function M.setup(opts)
  assert(not current and not pending_start, 'wait for the current session to exit before changing setup')
  assert(not blocked, 'cleanup is uncertain; start a fresh Neovim session after manual recovery')
  configured = options(opts)
  lifecycle_error = nil
  return M
end

local function start(opts, restart)
  if blocked then warn(lifecycle_error or 'cleanup is uncertain; automatic restart blocked'); return nil end
  if not restart and opts == nil and current and live(current) then
    return current.client and current.client.id
  end
  intent_serial = intent_serial + 1
  local requested
  if opts ~= nil then requested = vim.deepcopy(opts)
  else requested = vim.deepcopy(configured or {}) end
  configured = requested
  pending_start = { serial = intent_serial, options = requested }
  if current then retire(current) end
  schedule_drive()
  return nil
end

--- Schedule initialization, or coalesce a start after safe retirement.
function M.start(opts) return start(opts, false) end

--- Invalidate navigation now. Retain process ownership until actual exit.
function M.stop()
  intent_serial = intent_serial + 1
  pending_start = nil
  if current then retire(current) end
end

--- Retire then reinitialize/replay. Read the successor ID only after ready.
function M.restart(opts) return start(opts, true) end

function M.status()
  local s, count = current or last_session, 0
  for _ in pairs(observers) do count = count + 1 end
  return { active = current ~= nil and current.active or false,
    ready = current ~= nil and current.ready and current.client and current.client.initialized or false,
    client_id = current and current.client and current.client.id,
    session = s and s.serial, generation = s and s.generation,
    phase = blocked and 'blocked' or (current and current.phase or (lifecycle_error and 'failed' or 'idle')),
    error = lifecycle_error, pending_start = pending_start ~= nil, restart_blocked = blocked,
    cleanup = blocked and 'uncertain' or (current and current.cleanup or 'safe'), observer_count = count }
end

--- Safe definition navigation. Return request id and an explicit cancel function.
--- Optional on_result receives {status,result,error}; never changes server results.
function M.definition(opts)
  opts = opts or {}
  local session = current
  if not session or not session.active or not session.ready or not session.client.initialized then
    warn('server is not ready; call start and wait for initialization')
    return nil
  end
  local win, origin = api.nvim_get_current_win(), api.nvim_get_current_buf()
  if not attach(session, origin) or not live(session) or not session.ready then return nil end
  local entry = session.buffers[origin]
  if not entry then warn('current buffer is not an existing configured Java source'); return nil end
  for _, pending in pairs(session.pending) do cancel_request(pending, false) end
  session.request_serial = session.request_serial + 1
  local request = { session = session, generation = session.generation, epoch = entry.epoch,
    serial = session.request_serial, origin = origin, cursor = api.nvim_win_get_cursor(win),
    on_result = opts.on_result, ticks = {} }
  -- Public Client:request emits synchronous LspRequest before returning its ID.
  -- Register first so a reentrant stop/restart settles this request too.
  session.pending[request.serial] = request
  for buf in pairs(session.buffers) do request.ticks[buf] = api.nvim_buf_get_changedtick(buf) end
  local client = session.client
  -- Public API, verified specifically against pinned Neovim 0.11.5:
  -- Client:notify flushes pending changes for ALL attached buffers before it
  -- sends any non-didChange notification. Client:request alone flushes origin.
  -- $/setTrace is standard and harmless here; it is NOT an LSP barrier response.
  if not client:notify('$/setTrace', { value = 'off' }) then
    finish(request, 'error', nil, { message = 'could not synchronize buffers' })
    return nil
  end
  local params = vim.lsp.util.make_position_params(win, 'utf-16')
  local function is_current()
    if current ~= session or not session.active or session.client ~= client
      or request.obsolete or request.generation ~= session.generation
      or request.serial ~= session.request_serial then return false end
    local now = session.buffers[origin]
    if not now or now.epoch ~= request.epoch or not api.nvim_win_is_valid(win)
      or api.nvim_get_current_win() ~= win
      or api.nvim_win_get_buf(win) ~= origin
      or not vim.deep_equal(api.nvim_win_get_cursor(win), request.cursor) then return false end
    for buf, tick in pairs(request.ticks) do
      if not api.nvim_buf_is_loaded(buf) or api.nvim_buf_get_changedtick(buf) ~= tick then return false end
    end
    return true
  end
  local sent, id = client:request('textDocument/definition', params, function(err, result)
    if request.finished then return end
    if request.cancelled then finish(request, 'cancelled', result, err); return end
    if not is_current() then finish(request, 'stale', result, err); return end
    if err then
      finish(request, err.code == -32800 and 'cancelled' or 'error', result, err)
      return
    end
    if result == nil or result == vim.NIL or (vim.islist(result) and #result == 0) then
      finish(request, 'empty', result)
      return
    end
    local location = result
    if vim.islist(result) then
      if #result ~= 1 then
        finish(request, 'error', result, { message = 'ambiguous definition results are not selected' })
        return
      end
      location = result[1]
    end
    if type(location) ~= 'table' or type(location.uri) ~= 'string'
      or type(location.range) ~= 'table' then
      finish(request, 'error', result, { message = 'unsupported definition result' })
      return
    end
    -- Remove the pending request before opening a previously unloaded target.
    -- The resulting BufEnter can itself advance the workspace generation.
    session.pending[request.serial] = nil
    local ok, jumped = pcall(vim.lsp.util.show_document, location, 'utf-16', { focus = true })
    if ok and jumped then finish(request, 'jumped', result)
    else finish(request, 'error', result, { message = 'could not display definition' }) end
  end, origin)
  if not sent then finish(request, 'error', nil, { message = 'could not send definition' }); return nil end
  request.id = id
  return id, function() cancel_request(request, true) end
end

return M
