/*
 * redreply-compat.js  v1.0
 * 让 AI 生成的流行库风格代码在红色问答页面直接运行。
 *
 * 提供三个 API（与流行库语法一致，AI 易生成）：
 *   1. axios          —— axios 风格 HTTP，底层走 /proxy，支持跨域
 *   2. nodeFs         —— fs.promises 风格文件操作，底层走 /fs
 *   3. nodeCp         —— child_process 风格命令行，底层走 /fs op:cmd
 *
 * 给 AI 的提示词模板：
 *   "环境说明：axios 全局可用（已配置跨域代理）；nodeFs 提供 readFile/writeFile/
 *    readdir/mkdir/rm/exists/appendFile/stat（fs.promises 风格）；
 *    nodeCp.exec(cmd) 执行系统命令返回 Promise<{stdout,stderr}>。
 *    注意：Windows 下命令需带 cmd /c 前缀。"
 *
 * 用法：加载本文件即可（会自动改造页面自带的 axios/fetchUtils 支持跨域）。
 */
(function () {
  "use strict";

  // ================= 1. axios 跨域改造 =================
  // 页面自带 const axios = fetchUtils（词法绑定无法覆盖），只能改造其方法
  function proxyUrl(url) { return "/proxy?url=" + encodeURIComponent(url); }
  async function httpRequest(url, method, data) {
    const opt = { method: method, headers: { "Content-Type": "application/json" } };
    if (data !== undefined) opt.body = JSON.stringify(data);
    const resp = await fetch(proxyUrl(url), opt);
    if (!resp.ok) throw new Error("HTTP error! status: " + resp.status);
    const text = await resp.text();
    let json = null;
    try { json = text ? JSON.parse(text) : null; } catch (e) { json = text; }
    return { data: json, status: resp.status };
  }
  if (window.fetchUtils) {
    fetchUtils.get = function (url) { return httpRequest(url, "GET"); };
    fetchUtils.post = function (url, data) { return httpRequest(url, "POST", data); };
    fetchUtils.put = function (url, data) { return httpRequest(url, "PUT", data); };
    fetchUtils["delete"] = function (url) { return httpRequest(url, "DELETE"); };
    fetchUtils.patch = function (url, data) { return httpRequest(url, "PATCH", data); };
  }

  // ================= 2. /fs 基础调用 =================
  async function fsCall(payload) {
    const r = await fetch("/fs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    });
    const j = await r.json();
    if (j.retcode !== 0) throw new Error(JSON.stringify(j));
    return j.data;
  }

  // ================= 3. nodeFs：fs.promises 风格 =================
  window.nodeFs = {
    readFile: async function (path) {
      const d = await fsCall({ op: "read", path: path });
      return d.text !== undefined ? d.text : d.base64;
    },
    writeFile: async function (path, data) {
      await fsCall({ op: "write", path: path, content: String(data) });
    },
    appendFile: async function (path, data) {
      await fsCall({ op: "append", path: path, content: String(data) });
    },
    readdir: async function (path) {
      const d = await fsCall({ op: "list", path: path || "." });
      return d.map(function (x) { return x.name; });
    },
    mkdir: async function (path) {
      await fsCall({ op: "mkdir", path: path });
    },
    rm: async function (path, opts) {
      await fsCall({ op: (opts && opts.recursive) ? "rmtree" : "delete", path: path });
    },
    exists: async function (path) {
      const d = await fsCall({ op: "exists", path: path });
      return d === true;
    },
    stat: async function (path) {
      return await fsCall({ op: "stat", path: path });
    }
  };

  // ================= 4. nodeCp：child_process 风格 =================
  window.nodeCp = {
    exec: async function (cmd) {
      const d = await fsCall({ op: "cmd", content: cmd });
      return { stdout: d.output, stderr: "" };
    },
    execFile: async function (program, args) {
      const quoted = (args || []).map(function (a) { return JSON.stringify(a); }).join(" ");
      const d = await fsCall({ op: "cmd", content: program + " " + quoted });
      return { stdout: d.output, stderr: "" };
    }
  };

  // 标记已加载
  window.__redreplyCompat = "v1.0";
})();