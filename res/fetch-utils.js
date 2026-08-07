// 简单的fetch工具函数，用于替换axios
// v4.2：新增 User-Agent 覆盖（浏览器 forbidden header → 经 /proxy 服务端代理）
// v4.1：修复 body 读取阶段 AbortError 未捕获
// v4.0：全面修复兼容层问题
//   [修复1] 支持 responseType: 'arraybuffer' | 'blob' | 'json' | 'text'（默认自动 JSON）
//   [修复2] 支持 config.headers 合并、config.params 拼接、字符串 body 透传
//   [修复3] nodeFs.writeFile/appendFile 支持 Uint8Array/ArrayBuffer 二进制（走 content_base64）
//   [修复4] 大文件自动分块写入（兼容服务端 1MB 上限，每块 < 768KB）
//   [修复5] 识别 /proxy、/fs 返回的 {retcode:-1} 错误并抛出；网络错误包装为可读信息
// v3.1：修复 __doFetch 无条件 response.json() 导致 HTML/文本/二进制响应解析报错的问题
// v3.0：相对路径（同源 API）直接请求；绝对 URL 自动走 /proxy 跨域（公网/内网/localhost/局域网）
//       + 内联 nodeFs（fs.promises 风格）+ nodeCp（child_process 风格），页面加载即自动可用
function __resolveUrl(url) {
    if (/^https?:\/\//i.test(url)) {
        return '/proxy?url=' + encodeURIComponent(url);
    }
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
    // [修复6] User-Agent 是浏览器 forbidden header（JS 无法设置），
    //         检测到后转成 X-Redreply-UA 由 /proxy 服务端代理覆盖（v4.2）
    var headers = {};
    var proxyUA = null;
    for (var h in (config.headers || {})) {
        if (Object.prototype.hasOwnProperty.call(config.headers, h)) {
            if (h.toLowerCase() === 'user-agent') { proxyUA = config.headers[h]; continue; }
            headers[h] = config.headers[h];
        }
    }
    if (proxyUA) headers['X-Redreply-UA'] = proxyUA;
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
    // [修复5a] 识别 retcode 错误（/proxy、/fs 约定）
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

// ===== nodeFs：fs.promises 风格（底层 /fs）=====
async function __fsCall(payload) {
    var r = await fetch('/fs', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
    });
    var j = await r.json();
    if (j.retcode !== 0) {
        throw new Error((j.data && j.data.message) || JSON.stringify(j));
    }
    return j.data;
}
// [修复3] 二进制 → base64（分块避免调用栈溢出）
function __toBase64(bytes) {
    var CHUNK = 0x8000;
    var binary = '';
    for (var i = 0; i < bytes.length; i += CHUNK) {
        binary += String.fromCharCode.apply(null, bytes.subarray(i, i + CHUNK));
    }
    return btoa(binary);
}
// 归一化写入数据：返回 { kind: 'text'|'base64', payload }
function __normalizeData(data) {
    if (typeof data === 'string') return { kind: 'text', payload: data };
    if (data instanceof ArrayBuffer) data = new Uint8Array(data);
    if (data instanceof Uint8Array) return { kind: 'base64', payload: __toBase64(data) };
    if (ArrayBuffer.isView(data)) return { kind: 'base64', payload: __toBase64(new Uint8Array(data.buffer, data.byteOffset, data.byteLength)) };
    return { kind: 'text', payload: String(data) };
}
// [修复4] 分块写入：第一块 write，后续 append（块大小 4 的倍数，兼容 base64）
var __FS_CHUNK = 786432; // 768KB
async function __writeFileChunked(path, data, mode) {
    var n = __normalizeData(data);
    if (n.payload.length <= __FS_CHUNK) {
        var p1 = { op: mode === 'append' ? 'append' : 'write', path: path };
        p1[n.kind === 'base64' ? 'content_base64' : 'content'] = n.payload;
        return __fsCall(p1);
    }
    var first = true;
    for (var i = 0; i < n.payload.length;) {
        var end = Math.min(i + __FS_CHUNK, n.payload.length);
        if (n.kind === 'text' && end < n.payload.length) {
            var lastCodeUnit = n.payload.charCodeAt(end - 1);
            var nextCodeUnit = n.payload.charCodeAt(end);
            if (lastCodeUnit >= 0xD800 && lastCodeUnit <= 0xDBFF
                && nextCodeUnit >= 0xDC00 && nextCodeUnit <= 0xDFFF) {
                end -= 1;
            }
        }
        var chunk = n.payload.substring(i, end);
        var p = { op: (mode === 'append' || !first) ? 'append' : 'write', path: path };
        p[n.kind === 'base64' ? 'content_base64' : 'content'] = chunk;
        await __fsCall(p);
        first = false;
        i = end;
    }
    return true;
}
window.nodeFs = {
    readFile: async function (path) { var d = await __fsCall({ op: 'read', path: path }); return d.text !== undefined ? d.text : d.base64; },
    writeFile: async function (path, data) { return __writeFileChunked(path, data, 'write'); },
    appendFile: async function (path, data) { return __writeFileChunked(path, data, 'append'); },
    readdir: async function (path) { var d = await __fsCall({ op: 'list', path: path || '.' }); return d.map(function (x) { return x.name; }); },
    mkdir: async function (path) { await __fsCall({ op: 'mkdir', path: path }); },
    rm: async function (path, opts) { await __fsCall({ op: (opts && opts.recursive) ? 'rmtree' : 'delete', path: path }); },
    exists: async function (path) { var d = await __fsCall({ op: 'exists', path: path }); return d === true; },
    stat: async function (path) { return await __fsCall({ op: 'stat', path: path }); }
};

// ===== nodeCp：child_process 风格（底层 /fs op:cmd）=====
window.nodeCp = {
    exec: async function (cmd) { var d = await __fsCall({ op: 'cmd', content: cmd }); return { stdout: d.output, stderr: '' }; },
    execFile: async function (program, args) {
        var quoted = (args || []).map(function (a) { return JSON.stringify(a); }).join(" ");
        var d = await __fsCall({ op: 'cmd', content: program + ' ' + quoted });
        return { stdout: d.output, stderr: '' };
    }
};
