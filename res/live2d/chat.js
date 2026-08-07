(function () {
    "use strict";

    const container = document.querySelector(".pio-container");
    const launcher = document.getElementById("pio-chat-launcher");
    const panel = document.getElementById("pio-chat");
    const closeButton = document.getElementById("pio-chat-close");
    const settingsButton = document.getElementById("pio-chat-settings-button");
    const chatForm = document.getElementById("pio-chat-form");
    const input = document.getElementById("pio-chat-input");
    const sendButton = document.getElementById("pio-chat-send");
    const settingsForm = document.getElementById("pio-chat-settings");
    const settingsBack = document.getElementById("pio-chat-settings-back");
    const baseUrlInput = document.getElementById("pio-chat-base-url");
    const modelInput = document.getElementById("pio-chat-model");
    const apiKeyInput = document.getElementById("pio-chat-api-key");
    const systemPromptInput = document.getElementById("pio-chat-system-prompt");
    const thinkingLevelInput = document.getElementById("pio-chat-thinking-level");
    const contextLengthInput = document.getElementById("pio-chat-context-length");
    const apiKeyToggle = document.getElementById("pio-api-key-toggle");
    const agentStatus = document.getElementById("pio-agent-status");
    const agentPhaseText = document.getElementById("pio-agent-phase-text");
    const agentLog = document.getElementById("pio-agent-log");
    const contextUsage = document.getElementById("pio-context-usage");
    const dialog = container && container.querySelector(".pio-dialog");
    const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
    const JAVASCRIPT_TOOL_NAME = "execute_javascript";
    const MAX_TOOL_RESULT_CHARS = 10000;

    if (!container || !launcher || !panel || !chatForm || !settingsForm) return;

        // ── 手动调整对话框大小（拖拽右下角手柄，尺寸记忆）──
    const RESIZE_STORAGE_KEY = "pio-chat-size";
    const resizeHandle = document.createElement("div");
    resizeHandle.className = "pio-resize-handle";
    resizeHandle.setAttribute("aria-hidden", "true");
    panel.appendChild(resizeHandle);

    function applyPanelSize(w, h) {
        if (w) panel.style.width = w + "px";
        if (h) panel.style.height = h + "px";
    }

    try {
        const saved = JSON.parse(localStorage.getItem(RESIZE_STORAGE_KEY) || "null");
        if (saved && saved.w > 260 && saved.h > 160) applyPanelSize(saved.w, saved.h);
    } catch (e) {}

    resizeHandle.addEventListener("mousedown", function (e) {
        e.preventDefault();
        e.stopPropagation();
        const startX = e.clientX;
        const startY = e.clientY;
        const startW = panel.offsetWidth;
        const startH = panel.offsetHeight;
        const maxW = window.innerWidth - 24;
        const maxH = window.innerHeight - 24;
        function onMove(ev) {
            const w = Math.min(maxW, Math.max(260, startW + (ev.clientX - startX)));
            const h = Math.min(maxH, Math.max(160, startH + (ev.clientY - startY)));
            applyPanelSize(w, h);
        }
        function onUp() {
            document.removeEventListener("mousemove", onMove);
            document.removeEventListener("mouseup", onUp);
            try {
                localStorage.setItem(RESIZE_STORAGE_KEY, JSON.stringify({
                    w: panel.offsetWidth,
                    h: panel.offsetHeight
                }));
            } catch (e) {}
        }
        document.addEventListener("mousemove", onMove);
        document.addEventListener("mouseup", onUp);
    });

const state = {
        sending: false,
        configLoaded: false,
        contextUsed: 0,
        contextLimit: 131072,
        stopRequested: false,
        controller: null,
        generation: null,
        cancelPromise: null,
        cancelRetryTimer: null,
        phase: "就绪"
    };

    function stoppedError() {
        const error = new Error("已停止");
        error.stopped = true;
        return error;
    }

    function renderStatusInput() {
        if (!state.sending || !input) return;
        input.value = state.phase;
    }

    function formatTokens(tokens) {
        if (!Number.isFinite(tokens)) return "—";
        if (tokens >= 1048576) return (tokens / 1048576).toFixed(1) + "M";
        if (tokens >= 1024) return Math.round(tokens / 1024) + "k";
        return String(tokens);
    }

    function updateContextUsage() {
        if (!contextUsage) return;
        contextUsage.textContent = formatTokens(state.contextUsed)
            + "/" + formatTokens(state.contextLimit);
    }

    function bubble(message, options) {
        if (window.pio && typeof window.pio.message === "function") {
            const messageOptions = typeof options === "number"
                ? { time: options }
                : (options || { time: 5000 });
            window.pio.message(message, messageOptions);
        }
    }

    function apiResult(response) {
        const payload = response && response.data;
        if (!payload || payload.retcode !== 0) {
            const message = payload && payload.data && payload.data.message;
            throw new Error(message || "请求失败，请稍后重试");
        }
        return payload.data;
    }

    function setAgentPhase(phase) {
        state.phase = phase;
        agentPhaseText.textContent = phase;
        if (!state.sending) agentStatus.hidden = false;
        renderStatusInput();
        requestAnimationFrame(positionPanel);
    }

    // ── 智能滚动：用户未主动向上翻阅时，任何新内容（含阶段切换）都自动跟随到底部 ──
    const AUTO_SCROLL_MARGIN = 48;
    let userScrolledUp = false;
    function shouldAutoScroll() {
        if (!agentStatus) return true;
        return agentStatus.scrollHeight - agentStatus.scrollTop - agentStatus.clientHeight
            < AUTO_SCROLL_MARGIN;
    }
    function scrollAgentLog() {
        // 阶段切换时内容可能瞬间增高（如追加长代码），此时 scrollTop 未变但距底
        // 已超阈值，因此改用"用户是否主动翻上去"判断，而非实时位置
        if (userScrolledUp) return;
        agentStatus.scrollTop = agentStatus.scrollHeight;
    }
    if (agentStatus) {
        agentStatus.addEventListener("scroll", function () {
            userScrolledUp = !shouldAutoScroll();
        });
    }

    // 流式工具参数渲染：JSON 完整则正常解析美化；不完整则容错提取
    // code/javascript 字段值（未闭合也渲染），并解码 JSON 转义符
    function renderStreamingArguments(text) {
        if (!text) return "";
        try {
            const parsed = JSON.parse(text);
            if (parsed && typeof parsed === "object") {
                if (typeof parsed.code === "string") return parsed.code;
                if (typeof parsed.javascript === "string") return parsed.javascript;
                return JSON.stringify(parsed, null, 2);
            }
            return JSON.stringify(parsed, null, 2);
        } catch (e) { /* 未完整，走容错 */ }
        const m = text.match(/"code"\s*:\s*"([\s\S]*)$|"javascript"\s*:\s*"([\s\S]*)$/);
        if (m) {
            let val = m[1] !== undefined ? m[1] : (m[2] !== undefined ? m[2] : "");
            const closeIdx = val.search(/"[\s,}]/);
            if (closeIdx >= 0) val = val.slice(0, closeIdx);
            return val
                .replace(/\\n/g, "\n")
                .replace(/\\t/g, "\t")
                .replace(/\\"/g, '"')
                .replace(/\\\\/g, "\\");
        }
        return text;
    }

    function appendAgentStatus(label, text, className) {
        if (typeof text !== "string" || !text.trim()) return;
        if (!state.sending) agentStatus.hidden = false;
        const entry = document.createElement("div");
        entry.className = "pio-agent-entry" + (className ? " " + className : "");
        entry.dataset.label = label;
        entry.textContent = text.trim();
        agentLog.appendChild(entry);
        scrollAgentLog();
        renderStatusInput();
        requestAnimationFrame(positionPanel);
    }

    function resetAgentStatus() {
        state.phase = "正在思考";
        agentLog.innerText = "";
        setAgentPhase("正在思考");
    }

    function enterStatusMode() {
        state.sending = true;
        input.readOnly = true;
        sendButton.textContent = "■";
        sendButton.classList.add("is-stopping");
        sendButton.setAttribute("aria-label", "停止对话");
        sendButton.title = "停止对话";
        renderStatusInput();
    }

    function leaveStatusMode() {
        state.sending = false;
        input.readOnly = false;
        sendButton.textContent = "↑";
        sendButton.classList.remove("is-stopping");
        sendButton.setAttribute("aria-label", "发送消息");
        sendButton.title = "发送消息";
        agentStatus.hidden = agentLog.childElementCount === 0;
    }

    function requestStop() {
        if (!state.sending) return;
        state.stopRequested = true;
        setAgentPhase("正在停止");
        if (state.controller) state.controller.abort();
        void cancelPendingGeneration().catch(function () {});
        appendAgentStatus("提示", "已请求停止", "result");
    }

    async function cancelPendingGeneration() {
        if (state.cancelPromise) return state.cancelPromise;
        const generation = state.generation;
        if (!generation) return;
        const request = (async function () {
            const controller = new AbortController();
            const timeout = setTimeout(function () { controller.abort(); }, 10000);
            try {
                const response = await fetch("/live2d_chat_cancel", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ generation: generation }),
                    keepalive: true,
                    signal: controller.signal
                });
                const result = await response.json();
                if (!response.ok || !result || result.retcode !== 0) {
                    throw new Error(result && result.data && result.data.message
                        ? result.data.message
                        : "取消工具执行失败");
                }
                if (state.generation === generation) state.generation = null;
                if (state.cancelRetryTimer) {
                    clearTimeout(state.cancelRetryTimer);
                    state.cancelRetryTimer = null;
                }
            } finally {
                clearTimeout(timeout);
            }
        }());
        state.cancelPromise = request;
        try {
            await request;
        } catch (error) {
            console.error("Cancel pending Live2D tools failed:", error);
            if (state.generation === generation && !state.cancelRetryTimer) {
                state.cancelRetryTimer = setTimeout(function () {
                    state.cancelRetryTimer = null;
                    void cancelPendingGeneration().catch(function () {});
                }, 1500);
            }
            throw error;
        } finally {
            if (state.cancelPromise === request) state.cancelPromise = null;
        }
    }

    function limitToolResult(serialized) {
        if (serialized.length <= MAX_TOOL_RESULT_CHARS) return serialized;
        return JSON.stringify({
            truncated: true,
            preview: serialized.slice(0, MAX_TOOL_RESULT_CHARS / 2)
        });
    }

    function serializeToolValue(value) {
        if (value === undefined) return JSON.stringify({ ok: true, value: "undefined" });
        const seen = new WeakSet();
        try {
            let serialized = JSON.stringify({ ok: true, value: value }, function (_key, item) {
                if (["bigint", "function", "symbol"].includes(typeof item)) return String(item);
                if (item && typeof item === "object") {
                    if (seen.has(item)) return "[Circular]";
                    seen.add(item);
                    if (typeof Node !== "undefined" && item instanceof Node) {
                        return {
                            node: item.nodeName,
                            text: String(item.textContent || "").slice(0, MAX_TOOL_RESULT_CHARS)
                        };
                    }
                }
                return item;
            });
            return limitToolResult(serialized);
        } catch (error) {
            let fallback;
            try {
                fallback = String(value);
            } catch (_stringError) {
                fallback = "[Unserializable value]";
            }
            return limitToolResult(JSON.stringify({
                ok: true,
                value: fallback,
                serialization_error: String(error)
            }));
        }
    }

    function serializeToolError(error) {
        const message = String(error && error.message ? error.message : error);
        return limitToolResult(JSON.stringify({ ok: false, error: message }));
    }

    async function executeJavascriptTool(call) {
        if (!call || !call.id) {
            throw new Error("AI 返回了无效的工具调用");
        }
        if (call.name !== JAVASCRIPT_TOOL_NAME || typeof call.javascript !== "string") {
            // 未知工具：与后端 AgentToolCall::Unknown 的容忍策略一致，
            // 返回失败结果而不中断整个对话流程
            return {
                id: call.id,
                result: serializeToolError(new Error("未知工具: " + (call.name || "无名称")))
            };
        }
        try {
            const signal = state.controller ? state.controller.signal : undefined;
            const run = new AsyncFunction("signal", "\"use strict\";\n" + call.javascript);
            const execution = run(signal);
            let abortHandler = null;
            const aborted = new Promise(function (_resolve, reject) {
                if (!signal) return;
                abortHandler = function () {
                    const error = new Error("工具执行已停止");
                    error.name = "AbortError";
                    reject(error);
                };
                if (signal.aborted) abortHandler();
                else signal.addEventListener("abort", abortHandler, { once: true });
            });
            let result;
            try {
                result = signal ? await Promise.race([execution, aborted]) : await execution;
            } finally {
                if (signal && abortHandler) signal.removeEventListener("abort", abortHandler);
            }
            return { id: call.id, result: serializeToolValue(result) };
        } catch (error) {
            console.error("AI JavaScript tool failed:", error);
            return { id: call.id, result: serializeToolError(error) };
        }
    }

    // ── 流式（SSE）支持 ──
    async function* parseSse(response) {
        const reader = response.body.getReader();
        const decoder = new TextDecoder("utf-8");
        let buffer = "";
        while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            buffer += decoder.decode(value, { stream: true });
            let sep;
            while ((sep = buffer.indexOf("\n\n")) >= 0) {
                const block = buffer.slice(0, sep);
                buffer = buffer.slice(sep + 2);
                let event = "message";
                let data = "";
                for (const line of block.split("\n")) {
                    if (line.startsWith("event:")) event = line.slice(6).trim();
                    else if (line.startsWith("data:")) data += line.slice(5).trim();
                }
                if (data) {
                    let parsed;
                    try {
                        parsed = JSON.parse(data);
                    } catch (e) {
                        continue;
                    }
                    yield { event, data: parsed };
                }
            }
        }
    }

    // 创建/更新一条实时滚动输出的 Agent 记录
    function appendAgentEntryLive(label, text) {
        if (!state.sending) agentStatus.hidden = false;
        const entry = document.createElement("div");
        entry.className = "pio-agent-entry";
        entry.dataset.label = label;
        entry.textContent = text;
        agentLog.appendChild(entry);
        scrollAgentLog();
        return entry;
    }

    function updateAgentEntry(entry, text) {
        if (!entry) return;
        entry.textContent = text;
        scrollAgentLog();
        requestAnimationFrame(positionPanel);
    }

    async function postAgentStream(payload, signal) {
        const response = await fetch("/live2d_chat_stream", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
            signal: signal
        });
        const contentType = response.headers.get("content-type") || "";
        if (!contentType.includes("text/event-stream")) {
            let message = "请求失败: " + response.status;
            try {
                const data = await response.json();
                if (data && data.data && data.data.message) {
                    message = data.data.message;
                } else if (data && data.data) {
                    message = String(data.data);
                }
            } catch (e) {}
            throw new Error(message);
        }
        if (!response.ok) {
            throw new Error("请求失败: " + response.status);
        }
        return parseSse(response);
    }

    async function runAgent(message) {
        const controller = new AbortController();
        state.stopRequested = false;
        state.controller = controller;
        try {
            let payload = { message: message };
            let requestIndex = 0;
            // 本轮对话是否发生过工具调用（发生过则最终回答显示"大彻大悟"）
            let usedTool = false;
            while (true) {
                if (state.stopRequested) throw stoppedError();
                setAgentPhase(requestIndex === 0 ? "正在思考" : "继续思考");
                let reasoningEntry = null;
                let contentEntry = null;
                let reasoningText = "";
                let contentText = "";
                const toolCallEntries = {};
                let outcome = null;
                let events;
                try {
                    events = await postAgentStream(payload, controller.signal);
                } catch (error) {
                    if (state.stopRequested || (error && error.name === "AbortError")) {
                        throw stoppedError();
                    }
                    throw error;
                }
                for await (const { event, data } of events) {
                    if (event === "done") {
                        outcome = data;
                        if (outcome && outcome.type === "tool_calls") {
                            usedTool = true;
                            const deliveredGeneration = String(outcome.generation || "").trim();
                            if (!deliveredGeneration) throw new Error("工具步骤缺少对话代次");
                            state.generation = deliveredGeneration;
                        }
                        if (state.stopRequested) {
                            try { await cancelPendingGeneration(); } catch (_cancelError) {}
                            throw stoppedError();
                        }
                        break;
                    }
                    if (state.stopRequested) throw stoppedError();
                    if (event === "reasoning") {
                        reasoningText += data.delta || "";
                        if (!reasoningEntry) reasoningEntry = appendAgentEntryLive("看板娘转圈圈", "");
                        updateAgentEntry(reasoningEntry, reasoningText);
                    } else if (event === "content") {
                        contentText += data.delta || "";
                        if (!contentEntry) contentEntry = appendAgentEntryLive(usedTool ? "看板娘大彻大悟" : "看板娘碎碎念", "");
                        updateAgentEntry(contentEntry, contentText);
                    } else if (event === "tool_call") {
                        // 工具调用流式生成：实时显示函数名与参数逐字生成过程
                        // 注意：不能依赖 appendAgentStatus 的返回值（它不 return entry），
                        // 必须手动创建元素并缓存引用，否则每个事件都会新建一条目
                        const tIndex = Number(data.index || 0);
                        const tName = String(data.name || "");
                        const tArgs = String(data.arguments || "");
                        // 流式美化：JSON 完整则解析，不完整则容错提取 code/javascript 字段
                        const prettyArgs = renderStreamingArguments(tArgs);
                        let tEntry = toolCallEntries[tIndex];
                        if (!tEntry) {
                            tEntry = document.createElement("div");
                            tEntry.className = "pio-agent-entry tool";
                            tEntry.dataset.label = "看板娘搓搓小手" + (tName ? " · " + tName : "");
                            tEntry.textContent = (tName ? tName + "\n" : "") + prettyArgs;
                            agentLog.appendChild(tEntry);
                            toolCallEntries[tIndex] = tEntry;
                            scrollAgentLog();
                            renderStatusInput();
                            requestAnimationFrame(positionPanel);
                        } else {
                            tEntry.dataset.label = "看板娘搓搓小手" + (tName ? " · " + tName : "");
                            tEntry.textContent = (tName ? tName + "\n" : "") + prettyArgs;
                            scrollAgentLog();
                        }
                    } else if (event === "error") {
                        throw new Error(data.message || "看板娘请求失败");
                    }
                }
                if (!outcome) throw new Error("流式响应意外结束");
                if (Number.isFinite(outcome.context_tokens)) state.contextUsed = outcome.context_tokens;
                if (Number.isFinite(outcome.context_length)) state.contextLimit = outcome.context_length;
                updateContextUsage();
                if (outcome.type === "final") {
                    const reply = String(outcome.reply || "").trim();
                    if (!reply) throw new Error("看板娘没有返回内容");
                    state.generation = null;
                    setAgentPhase("已完成");
                    return reply;
                }
                if (outcome.type !== "tool_calls"
                    || !Array.isArray(outcome.calls)
                    || outcome.calls.length === 0) {
                    throw new Error("AI 返回了无效的工具步骤");
                }

                bubble("正在执行第 " + (requestIndex + 1) + " 步…", 120000);
                setAgentPhase("正在执行第 " + (requestIndex + 1) + " 步");
                const generation = String(outcome.generation || "").trim();
                if (!generation) throw new Error("工具步骤缺少对话代次");
                state.generation = generation;
                const toolResults = [];
                for (let callIndex = 0; callIndex < outcome.calls.length; callIndex += 1) {
                    if (state.stopRequested) throw stoppedError();
                    const call = outcome.calls[callIndex];
                    const codeText = call.javascript || "";
                    const streamEntry = toolCallEntries[callIndex];
                    if (streamEntry) {
                        // 流式条目已存在：更新为完整信息（label 去掉"生成中"）
                        streamEntry.dataset.label = "看板娘搓搓小手" + (call.name ? " · " + call.name : "");
                        streamEntry.textContent = codeText;
                        scrollAgentLog();
                    } else {
                        appendAgentStatus("看板娘搓搓小手", call.name + "\n" + codeText, "tool");
                    }
                    const toolResult = await executeJavascriptTool(call);
                    if (state.stopRequested) throw stoppedError();
                    toolResults.push(toolResult);
                    appendAgentStatus("看板娘战利品+1", toolResult.result, "result");
                }
                payload = {
                    generation: generation,
                    tool_results: toolResults
                };
                requestIndex += 1;
            }
        } catch (error) {
            if (state.generation) void cancelPendingGeneration().catch(function () {});
            if (state.stopRequested || (error && error.name === "AbortError")) {
                throw stoppedError();
            }
            throw error;
        } finally {
            state.controller = null;
        }
    }

    function clearBubble() {
        if (window.pio && typeof window.pio.clearMessage === "function") {
            window.pio.clearMessage();
        }
    }

    function positionPanel() {
        if (panel.hidden) return;
        const modelRect = container.getBoundingClientRect();
        const panelRect = panel.getBoundingClientRect();
        const gap = 10;
        const roomLeft = modelRect.left;
        const roomRight = window.innerWidth - modelRect.right;
        let left;
        let placeAbove = false;

        if (roomLeft >= panelRect.width + gap) {
            left = modelRect.left - panelRect.width - gap;
        } else if (roomRight >= panelRect.width + gap) {
            left = modelRect.right + gap;
        } else {
            left = Math.max(12, Math.min(window.innerWidth - panelRect.width - 12, modelRect.left));
            placeAbove = true;
        }

        const preferredTop = placeAbove
            ? modelRect.top - panelRect.height - gap
            : modelRect.bottom - panelRect.height;
        const top = Math.max(12, Math.min(window.innerHeight - panelRect.height - 12, preferredTop));
        panel.style.left = left + "px";
        panel.style.top = top + "px";
    }

    function showSettings(show) {
        settingsForm.hidden = !show;
        chatForm.hidden = show;
        agentStatus.hidden = show || (!state.sending && agentLog.childElementCount === 0);
        settingsButton.textContent = show ? "对话" : "设置";
        requestAnimationFrame(positionPanel);
        if (show) baseUrlInput.focus();
        else input.focus();
    }

    function applyConfig(config) {
        state.configLoaded = true;
        baseUrlInput.value = config.base_url || "";
        modelInput.value = config.model || "";
        systemPromptInput.value = config.system_prompt || "";
        thinkingLevelInput.value = config.thinking_level || "high";
        if (contextLengthInput) {
            contextLengthInput.value = String(config.context_length || 131072);
        }
        state.contextLimit = Number(config.context_length) || 131072;
        if (Number.isFinite(config.context_used)) state.contextUsed = config.context_used;
        updateContextUsage();
        apiKeyInput.value = config.api_key || "";
        apiKeyInput.type = "password";
        if (apiKeyToggle) apiKeyToggle.textContent = "👁";
    }

    async function loadConfig() {
        try {
            applyConfig(apiResult(await axios.get("/live2d_chat_config")));
        } catch (error) {
            bubble(error && error.message ? error.message : "读取设置失败", 8000);
        }
    }

    function openPanel() {
        input.value = "";
        showSettings(false);
        panel.hidden = false;
        launcher.setAttribute("aria-expanded", "true");
        requestAnimationFrame(function () {
            positionPanel();
            input.focus();
        });
    }

    function closePanel() {
        panel.hidden = true;
        input.value = "";
        launcher.setAttribute("aria-expanded", "false");
    }

    launcher.addEventListener("click", function () {
        panel.hidden ? openPanel() : closePanel();
    });
    closeButton.addEventListener("click", closePanel);
    settingsButton.addEventListener("click", function () {
        const openingSettings = settingsForm.hidden;
        showSettings(openingSettings);
        if (openingSettings && !state.configLoaded) loadConfig();
    });
    settingsBack.addEventListener("click", function () {
        showSettings(false);
    });
    if (document.getElementById("pio-chat-compress")) {
        document.getElementById("pio-chat-compress").addEventListener("click", async function () {
            if (state.sending) { bubble("正在对话中，请稍后再压缩", 4000); return; }
            const btn = this;
            btn.disabled = true;
            btn.textContent = "压缩中…";
            try {
                const data = apiResult(await axios.post("/live2d_chat_compress", {}));
                const count = Number.isFinite(data && data.message_count) ? data.message_count : 0;
                bubble("对话已压缩 ✓（剩余 " + count + " 条消息）", 5000);
                if (Number.isFinite(data && data.context_tokens)) state.contextUsed = data.context_tokens;
                if (Number.isFinite(data && data.context_length)) state.contextLimit = data.context_length;
                updateContextUsage();
            } catch (error) {
                bubble(error && error.message ? error.message : "压缩失败", 8000);
            } finally {
                btn.disabled = false;
                btn.textContent = "压缩对话";
            }
        });
    }

    if (apiKeyToggle) {
        apiKeyToggle.addEventListener("click", function () {
            const isPassword = apiKeyInput.type === "password";
            apiKeyInput.type = isPassword ? "text" : "password";
            apiKeyToggle.textContent = isPassword ? "🙈" : "👁";
            apiKeyInput.focus();
        });
    }

    [launcher, panel, dialog].filter(Boolean).forEach(function (element) {
        element.addEventListener("mousedown", function (event) {
            event.stopPropagation();
        });
        element.addEventListener("touchstart", function (event) {
            event.stopPropagation();
        }, { passive: true });
    });

    document.addEventListener("mousedown", function () {
        if (!panel.hidden) closePanel();
    });
    document.addEventListener("keydown", function (event) {
        if (event.key === "Escape" && !panel.hidden) closePanel();
    });
    input.addEventListener("input", function () {
        if (input.value.trim()) clearBubble();
    });
    window.addEventListener("resize", positionPanel);
    window.addEventListener("pagehide", function () {
        void cancelPendingGeneration().catch(function () {});
    });
    document.addEventListener("mouseup", positionPanel);

    chatForm.addEventListener("submit", async function (event) {
        event.preventDefault();
        if (state.sending) {
            requestStop();
            return;
        }

        const message = input.value.trim();
        if (!message) {
            input.focus();
            return;
        }

        clearBubble();
        input.value = "";
        resetAgentStatus();
        appendAgentStatus("主人", message, "user");
        enterStatusMode();
        settingsButton.disabled = true;
        bubble("正在思考…", 120000);

        try {
            const reply = await runAgent(message);
            bubble(reply, { persist: true, closeable: true });
        } catch (error) {
            if (error && error.stopped) {
                setAgentPhase("已停止");
                appendAgentStatus("提示", "本次对话已停止", "result");
                bubble("已停止", 3000);
            } else {
                setAgentPhase("执行失败");
                appendAgentStatus("错误", error && error.message ? error.message : String(error), "error");
                bubble(error && error.message ? error.message : "请求失败，请稍后重试", 8000);
            }
        } finally {
            leaveStatusMode();
            input.value = "";
            settingsButton.disabled = false;
        }
    });

    settingsForm.addEventListener("submit", async function (event) {
        event.preventDefault();
        const submitButton = settingsForm.querySelector("button[type='submit']");
        submitButton.disabled = true;
        const payload = {
            enabled: true,
            base_url: baseUrlInput.value.trim(),
            model: modelInput.value.trim(),
            system_prompt: systemPromptInput.value.trim(),
            thinking_level: thinkingLevelInput.value,
            context_length: contextLengthInput
                ? Number(contextLengthInput.value)
                : 131072,
            timeout_seconds: 60
        };
        // 仅当用户输入了新 Key（非占位符）时才提交
        const keyVal = apiKeyInput.value.trim();
        if (keyVal) payload.api_key = keyVal;
        else payload.clear_api_key = true;

        try {
            applyConfig(apiResult(await axios.post("/live2d_chat_config", payload)));
            showSettings(false);
            bubble("对话设置已保存。", 3000);
        } catch (error) {
            bubble(error && error.message ? error.message : "保存设置失败", 8000);
        } finally {
            submitButton.disabled = false;
        }
    });

    updateContextUsage();
    loadConfig();
}());
