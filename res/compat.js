/*
 * redreply-compat.js  v1.0
 * 让 AI 生成的流行库风格代码在红色问答页面直接运行。
 *
 * 仅提供命令行 API，供看板娘 JS 工具调用：
 *   nodeCp         —— child_process 风格命令行
 *
 * 给 AI 的提示词模板：
 *   "环境说明：nodeCp.exec(cmd) 执行系统命令返回 Promise<{stdout,stderr}>。
 *    注意：Windows 下命令需带 cmd /c 前缀。"
 *
 * 用法：加载本文件即可安装 nodeCp。
 */
(function () {
  "use strict";

  // ================= 命令执行 =================
  async function commandCall(payload) {
    const r = await fetch("/run_command", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    });
    const j = await r.json();
    if (j.retcode !== 0) throw new Error(JSON.stringify(j));
    return j.data;
  }

  // ================= nodeCp：child_process 风格 =================
  window.nodeCp = {
    exec: async function (cmd) {
      const d = await commandCall({ content: cmd });
      return { stdout: d.output, stderr: "" };
    },
    execFile: async function (program, args) {
      const quoted = (args || []).map(function (a) { return JSON.stringify(a); }).join(" ");
      const d = await commandCall({ content: program + " " + quoted });
      return { stdout: d.output, stderr: "" };
    }
  };

  // 标记已加载
  window.__redreplyCompat = "v1.0";
})();
