// 简单的fetch工具函数，用于替换axios
// v4.2：保留请求配置与超时处理
// v4.1：修复 body 读取阶段 AbortError 未捕获
// v4.0：全面修复兼容层问题
//   [修复1] 支持 responseType: 'arraybuffer' | 'blob' | 'json' | 'text'（默认自动 JSON）
//   [修复2] 支持 config.headers 合并、config.params 拼接、字符串 body 透传
//   [修复3] 识别命令接口返回的 {retcode:-1} 错误并抛出；网络错误包装为可读信息
// v3.1：修复 __doFetch 无条件 response.json() 导致 HTML/文本/二进制响应解析报错的问题
// v3.0：相对路径和绝对 URL 均由浏览器直接请求
//       + 内联 nodeCp（child_process 风格），页面加载即自动可用
function __resolveUrl(url) {
    return url;
}
function __toQuery(params) {
    var parts = [];
    for (var k in params) {
        if (Object.prototype.hasOwnProperty.call(params, k)) {
            var v = params[k];
            if (v !== undefined && v !== null) parts.push(encodeURIComponent(k) + '=' + encodeURIComponent(v));
        }
    }
    return parts.join('&');
}
async function __doFetch(url, method, data, config) {
    config = config || {};
    // [修复2a] params 拼接到 URL
    if (config.params) {
        var qs = __toQuery(config.params);
        if (qs) url = url + (url.indexOf('?') >= 0 ? '&' : '?') + qs;
    }
    // [修复2b] headers 合并（用户配置优先）
    var headers = {};
    for (var h in (config.headers || {})) {
        if (Object.prototype.hasOwnProperty.call(config.headers, h)) {
            headers[h] = config.headers[h];
        }
    }
    var opt = { method: method, headers: headers };
    // body：对象默认 JSON 序列化（axios 行为）；字符串/FormData/URLSearchParams 原样透传
    if (data !== undefined && data !== null) {
        if (typeof data === 'object' && !(data instanceof FormData) && !(data instanceof URLSearchParams) && !(data instanceof ArrayBuffer) && !ArrayBuffer.isView(data)) {
            if (!headers['Content-Type'] && !headers['content-type']) headers['Content-Type'] = 'application/json';
            opt.body = JSON.stringify(data);
        } else {
            opt.body = data;
        }
    }
    // [修复2c] timeout（AbortController 实现）
    var timeoutMs = config.timeout || 0;
    var controller = null;
    var timeoutId = null;
    var externalAbort = null;
    if (timeoutMs > 0 && config.signal) {
        controller = new AbortController();
        opt.signal = controller.signal;
        externalAbort = function () { controller.abort(); };
        if (config.signal.aborted) controller.abort();
        else config.signal.addEventListener('abort', externalAbort, { once: true });
        timeoutId = setTimeout(function () { controller.abort(); }, timeoutMs);
    } else if (timeoutMs > 0) {
        controller = new AbortController();
        opt.signal = controller.signal;
        timeoutId = setTimeout(function () { controller.abort(); }, timeoutMs);
    } else if (config.signal) {
        opt.signal = config.signal;
    }
    var response;
    var dataOut;
    try {
        response = await fetch(__resolveUrl(url), opt);
        if (!response.ok) { throw new Error('HTTP error! status: ' + response.status + ', url: ' + url); }
        // [修复1] responseType 支持（body 读取也在 try 内，abort 时统一处理）
        var rt = config.responseType;
        if (rt === 'arraybuffer') {
            dataOut = await response.arrayBuffer();
        } else if (rt === 'blob') {
            dataOut = await response.blob();
        } else if (rt === 'json') {
            dataOut = await response.json();
        } else if (rt === 'text') {
            dataOut = await response.text();
        } else {
            var text = await response.text();
            var json = null;
            try { json = text ? JSON.parse(text) : null; } catch (e) { json = text; }
            dataOut = json;
        }
    } catch (e) {
        if (e && e.name === 'AbortError') throw new Error('请求超时 (timeout ' + timeoutMs + 'ms): ' + url);
        // [修复5b] 网络层错误包装（Failed to fetch、body 读取失败等）
        throw new Error('网络请求失败: ' + url + ' (' + (e && e.message ? e.message : String(e)) + ')');
    } finally {
        if (timeoutId !== null) clearTimeout(timeoutId);
        if (externalAbort && config.signal) config.signal.removeEventListener('abort', externalAbort);
    }
    // [修复5a] 识别接口约定的 retcode 错误
    if (dataOut && typeof dataOut === 'object' && dataOut.retcode === -1 && dataOut.data && dataOut.data.message) {
        throw new Error(dataOut.data.message + ' (url: ' + url + ')');
    }
    // 模拟axios的响应格式
    return { data: dataOut, status: response.status, headers: response.headers, config: config };
}
const fetchUtils = {
    // GET请求
    get: function (url, config) { return __doFetch(url, 'GET', undefined, config); },
    // POST请求
    post: function (url, data, config) { return __doFetch(url, 'POST', data, config); },
    put: function (url, data, config) { return __doFetch(url, 'PUT', data, config); },
    "delete": function (url, config) { return __doFetch(url, 'DELETE', undefined, config); },
    patch: function (url, data, config) { return __doFetch(url, 'PATCH', data, config); }
};
// 为了兼容现有代码，创建一个axios兼容对象
const axios = fetchUtils;

// ===== nodeCp：child_process 风格（底层命令接口）=====
function __commandOptions(options) {
    var context = window.nodeCp && window.nodeCp.__live2dContext;
    return Object.assign({}, options || {}, context || {});
}
function __nodeCpArgs(options, callback) {
    if (typeof options === 'function') return { options: {}, callback: options };
    if (typeof callback === 'function') return { options: options || {}, callback: callback };
    return { options: options || {}, callback: null };
}
async function __commandCall(payload, options) {
    var r = await fetch('/run_command', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        signal: options && options.signal
    });
    var j = await r.json();
    if (j.retcode !== 0) {
        throw new Error((j.data && j.data.message) || JSON.stringify(j));
    }
    return j.data;
}
function __commandError(data, command) {
    if (data && data.exit_code === 0) return null;
    var stderr = data && typeof data.stderr === 'string' ? data.stderr : '';
    var error = new Error('Command failed: ' + command + (stderr ? '\n' + stderr : ''));
    if (data && data.exit_code !== undefined && data.exit_code !== null) error.code = data.exit_code;
    error.cmd = command;
    error.killed = false;
    error.signal = null;
    error.stdout = data && typeof data.stdout === 'string' ? data.stdout : '';
    error.stderr = stderr;
    return error;
}
function __nodeCpEmitter(target) {
    var listeners = Object.create(null);
    target.on = function (event, listener) {
        if (typeof listener === 'function') (listeners[event] || (listeners[event] = [])).push(listener);
        return target;
    };
    target.once = function (event, listener) {
        if (typeof listener !== 'function') return target;
        function onceListener() {
            target.removeListener(event, onceListener);
            listener.apply(target, arguments);
        }
        return target.on(event, onceListener);
    };
    target.removeListener = function (event, listener) {
        if (listeners[event]) listeners[event] = listeners[event].filter(function (item) { return item !== listener; });
        return target;
    };
    target.off = target.removeListener;
    target._emit = function (event) {
        var args = Array.prototype.slice.call(arguments, 1);
        (listeners[event] || []).slice().forEach(function (listener) { listener.apply(target, args); });
    };
    return target;
}
function __nodeCpStream() {
    var stream = __nodeCpEmitter({ readable: true, readableEnded: false });
    stream.setEncoding = function () { return stream; };
    return stream;
}
function __commandId() {
    if (window.crypto && typeof window.crypto.randomUUID === 'function') return window.crypto.randomUUID();
    return 'nodecp-' + Date.now() + '-' + Math.random().toString(16).slice(2);
}
function __runNodeCommand(payload, options, command, callback) {
    var controller = new AbortController();
    var child = __nodeCpEmitter({
        pid: undefined,
        connected: false,
        killed: false,
        exitCode: null,
        signalCode: null,
        stdout: __nodeCpStream(),
        stderr: __nodeCpStream()
    });
    payload.command_id = __commandId();
    var externalSignal = options && options.signal;
    var externalAbort = null;
    child.kill = function () {
        if (child.exitCode !== null || child.killed) return false;
        child.killed = true;
        controller.abort();
        void fetch('/run_command_cancel', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ generation: payload.command_id }),
            keepalive: true
        }).catch(function () {});
        return true;
    };
    if (externalSignal) {
        externalAbort = function () { child.kill(); };
        if (externalSignal.aborted) externalAbort();
        else externalSignal.addEventListener('abort', externalAbort, { once: true });
    }
    function finishStreams(stdout, stderr) {
        if (stdout) child.stdout._emit('data', stdout);
        if (stderr) child.stderr._emit('data', stderr);
        child.stdout.readableEnded = true;
        child.stderr.readableEnded = true;
        child.stdout._emit('end');
        child.stderr._emit('end');
        child.stdout._emit('close');
        child.stderr._emit('close');
    }
    __commandCall(payload, Object.assign({}, options, { signal: controller.signal })).then(function (data) {
        var stdout = typeof data.stdout === 'string' ? data.stdout : '';
        var stderr = typeof data.stderr === 'string' ? data.stderr : '';
        var error = __commandError(data, command);
        child.exitCode = data.exit_code === undefined ? null : data.exit_code;
        finishStreams(stdout, stderr);
        if (callback) callback(error, stdout, stderr);
        child._emit('exit', child.exitCode, null);
        child._emit('close', child.exitCode, null);
    }, function (error) {
        finishStreams('', '');
        if (callback) callback(error, '', '');
        child._emit('error', error);
        child._emit('close', child.exitCode, child.signalCode);
    }).finally(function () {
        if (externalSignal && externalAbort) externalSignal.removeEventListener('abort', externalAbort);
    });
    return child;
}
window.nodeCp = {
    exec: function (cmd, options, callback) {
        var args = __nodeCpArgs(options, callback);
        options = __commandOptions(args.options);
        return __runNodeCommand({
            kind: 'exec',
            command: String(cmd),
            encoding: options.encoding === undefined ? 'utf8' : options.encoding,
            generation: options.generation,
            cwd: options.cwd,
            env: options.env
        }, options, String(cmd), args.callback);
    },
    execFile: function (program, args, options, callback) {
        if (!Array.isArray(args)) {
            if (typeof callback !== 'function') {
                if (typeof options === 'function') callback = options;
                else if (typeof args === 'function') callback = args;
            }
            if (args && typeof args === 'object') options = args;
            else if (!options || typeof options === 'function') options = {};
            args = [];
        }
        var normalized = __nodeCpArgs(options, callback);
        options = __commandOptions(normalized.options);
        var command = String(program) + ' ' + args.map(function (a) { return String(a); }).join(' ');
        return __runNodeCommand({
            kind: 'execFile',
            program: String(program),
            args: args.map(function (a) { return String(a); }),
            shell: Boolean(options.shell),
            encoding: options.encoding === undefined ? 'utf8' : options.encoding,
            generation: options.generation,
            cwd: options.cwd,
            env: options.env
        }, options, command, normalized.callback);
    },
    __live2dContext: null
};
