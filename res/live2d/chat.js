(function () {
    "use strict";

    const container = document.querySelector(".pio-container");
    const launcher = document.getElementById("pio-chat-launcher");
    const panel = document.getElementById("pio-chat");
    const closeButton = document.getElementById("pio-chat-close");
    const settingsButton = document.getElementById("pio-chat-settings-button");
    const compressHeaderButton = document.getElementById("pio-chat-compress-header");
    const chatForm = document.getElementById("pio-chat-form");
    const input = document.getElementById("pio-chat-input");
    function autoResizeInput() {
        input.style.height = "auto";
        input.style.height = input.scrollHeight + "px";
    }
    const sendButton = document.getElementById("pio-chat-send");
    const settingsForm = document.getElementById("pio-chat-settings");
    const settingsBack = document.getElementById("pio-chat-settings-back");
    const clearContextButton = document.getElementById("pio-chat-clear-context");
    const baseUrlInput = document.getElementById("pio-chat-base-url");
    const configInput = document.getElementById("pio-chat-config");
    const configAddInput = document.getElementById("pio-chat-config-add");
    const configAddButton = document.getElementById("pio-chat-config-add-btn");
    const configDelButton = document.getElementById("pio-chat-config-del-btn");
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

    // 看板娘显式状态机：业务状态与展示文案分离。
    const AGENT_STATE = Object.freeze({
        IDLE: "idle",
        THINKING: "thinking",
        CONTINUING: "continuing",
        EXECUTING_TOOL: "executing_tool",
        STOPPING: "stopping",
        COMPLETED: "completed",
        STOPPED: "stopped",
        FAILED: "failed"
    });
    const AGENT_STATE_TRANSITIONS = Object.freeze({
        idle: [AGENT_STATE.THINKING],
        thinking: [AGENT_STATE.CONTINUING, AGENT_STATE.EXECUTING_TOOL, AGENT_STATE.STOPPING, AGENT_STATE.COMPLETED, AGENT_STATE.STOPPED, AGENT_STATE.FAILED],
        continuing: [AGENT_STATE.EXECUTING_TOOL, AGENT_STATE.STOPPING, AGENT_STATE.COMPLETED, AGENT_STATE.STOPPED, AGENT_STATE.FAILED],
        executing_tool: [AGENT_STATE.CONTINUING, AGENT_STATE.STOPPING, AGENT_STATE.STOPPED, AGENT_STATE.FAILED],
        stopping: [AGENT_STATE.STOPPED, AGENT_STATE.FAILED],
        completed: [AGENT_STATE.THINKING, AGENT_STATE.FAILED],
        stopped: [AGENT_STATE.THINKING],
        failed: [AGENT_STATE.THINKING]
    });
    const BUSY_AGENT_STATES = new Set([
        AGENT_STATE.THINKING,
        AGENT_STATE.CONTINUING,
        AGENT_STATE.EXECUTING_TOOL,
        AGENT_STATE.STOPPING
    ]);

    function renderMarkdown(text) {
        const source = String(text == null ? "" : text);
        if (!window.marked || !window.DOMPurify) return null;
        const html = window.marked.parse(source, { breaks: true, gfm: true });
        return window.DOMPurify.sanitize(html, {
            USE_PROFILES: { html: true },
            FORBID_TAGS: ["style", "form", "iframe", "object", "embed"],
            FORBID_ATTR: ["style"]
        });
    }

    function setEntryContent(entry, text, markdown) {
        if (!entry) return;
        const rendered = markdown ? renderMarkdown(text) : null;
        if (rendered !== null) {
            entry.classList.add("pio-markdown");
            entry.innerHTML = rendered;
        } else {
            entry.classList.remove("pio-markdown");
            entry.textContent = String(text == null ? "" : text);
        }
    }

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
        agentState: AGENT_STATE.IDLE,
        toolStep: 0,
        configLoaded: false,
        contextUsed: 0,
        contextEstimated: true,
        contextLimit: 131072,
        stopRequested: false,
        controller: null,
        generation: null,
        cancelPromise: null,
        cancelRetryTimer: null,
        pendingCompress: false,
        pendingMessage: null,
        skipAutoCompress: false,
        configs: [],
        currentConfigName: ""
    };

    function stoppedError() {
        const error = new Error("已停止");
        error.stopped = true;
        return error;
    }

    function getAgentPhaseText() {
        switch (state.agentState) {
            case AGENT_STATE.THINKING: return "正在思考";
            case AGENT_STATE.CONTINUING: return "继续思考";
            case AGENT_STATE.EXECUTING_TOOL: return "正在执行第 " + state.toolStep + " 步";
            case AGENT_STATE.STOPPING: return "正在停止";
            case AGENT_STATE.COMPLETED: return "已完成";
            case AGENT_STATE.STOPPED: return "已停止";
            case AGENT_STATE.FAILED: return "执行失败";
            default: return "就绪";
        }
    }

    function isAgentBusy() {
        return BUSY_AGENT_STATES.has(state.agentState);
    }

    function renderAgentState() {
        const busy = isAgentBusy();
        const phase = getAgentPhaseText();
        agentPhaseText.textContent = phase;
        agentStatus.hidden = !busy && agentLog.childElementCount === 0;
        input.readOnly = busy;
        input.value = busy ? phase : "";
        sendButton.textContent = busy ? "■" : "↑";
        sendButton.classList.toggle("is-stopping", busy);
        sendButton.setAttribute("aria-label", busy ? "停止对话" : "发送消息");
        sendButton.title = busy ? "停止对话" : "发送消息";
        requestAnimationFrame(positionPanel);
    }

    function setAgentState(nextState, options) {
        const currentState = state.agentState;
        const knownStates = Object.values(AGENT_STATE);
        if (!knownStates.includes(nextState)) throw new Error("未知的看板娘状态: " + nextState);
        if (currentState !== nextState) {
            const allowed = AGENT_STATE_TRANSITIONS[currentState] || [];
            if (!allowed.includes(nextState)) throw new Error("非法的看板娘状态转换: " + currentState + " -> " + nextState);
            state.agentState = nextState;
        }
        state.toolStep = nextState === AGENT_STATE.EXECUTING_TOOL
            ? Math.max(1, Number(options && options.toolStep) || 1)
            : 0;
        renderAgentState();
    }

    renderAgentState();

    function formatTokens(tokens) {
        if (!Number.isFinite(tokens)) return "—";
        if (tokens >= 1048576) return (tokens / 1048576).toFixed(1) + "M";
        if (tokens >= 1024) return Math.round(tokens / 1024) + "k";
        return String(tokens);
    }

    function updateContextUsage() {
        if (!contextUsage) return;
        contextUsage.textContent = (state.contextEstimated ? "≈" : "")
            + formatTokens(state.contextUsed)
            + "/" + formatTokens(state.contextLimit);
        contextUsage.title = state.contextEstimated
            ? "当前为按字符数/4计算的估算值"
            : "当前为大模型接口返回的 token 用量";
    }

    function bubble(message, options) {
        if (window.pio && typeof window.pio.message === "function") {
            const messageOptions = typeof options === "number"
                ? { time: options }
                : Object.assign({}, options || { time: 5000 });
            let content = message;
            if (messageOptions.markdown) {
                const rendered = renderMarkdown(message);
                if (rendered !== null) {
                    content = rendered;
                    messageOptions.html = true;
                }
                delete messageOptions.markdown;
            }
            window.pio.message(content, messageOptions);
            if (messageOptions.html) {
                const dialogContent = container.querySelector(".pio-dialog-content");
                if (dialogContent) dialogContent.classList.add("pio-markdown");
            }
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

    function appendAgentStatus(label, text, className) {
        if (typeof text !== "string" || !text.trim()) return;
        if (!isAgentBusy()) agentStatus.hidden = false;
        const entry = document.createElement("div");
        entry.className = "pio-agent-entry" + (className ? " " + className : "");
        entry.dataset.label = label;
        entry.textContent = text.trim();
        agentLog.appendChild(entry);
        scrollAgentLog();
        requestAnimationFrame(positionPanel);
    }

    function resetAgentStatus() {
        agentLog.innerText = "";
    }

    function requestStop() {
        if (!isAgentBusy()) return;
        state.stopRequested = true;
        setAgentState(AGENT_STATE.STOPPING);
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
        return serialized.slice(0, MAX_TOOL_RESULT_CHARS) + "...[truncated]";
    }

    function serializeToolValue(value) {
        if (value === undefined) return "ok:true stdout:undefined stderr:";
        // plain string: output as-is, no JSON quotes
        if (typeof value === "string") return limitToolResult("ok:true stdout:" + value + " stderr:");
        // nodeCp.exec-style {stdout, stderr}: split fields directly, avoid nested JSON in stdout
        if (
            value && typeof value === "object" &&
            !Array.isArray(value) &&
            !(typeof Node !== "undefined" && value instanceof Node) &&
            (typeof value.stdout === "string" || typeof value.stderr === "string")
        ) {
            const stdout = value.stdout === undefined ? "" : String(value.stdout);
            const stderr = value.stderr === undefined ? "" : String(value.stderr);
            return limitToolResult("ok:true stdout:" + stdout + " stderr:" + stderr);
        }
        const seen = new WeakSet();
        try {
            let serialized = JSON.stringify(value, function (_key, item) {
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
            return limitToolResult("ok:true stdout:" + serialized + " stderr:");
        } catch (error) {
            let fallback;
            try {
                fallback = String(value);
            } catch (_stringError) {
                fallback = "[Unserializable value]";
            }
            return limitToolResult("ok:true stdout:" + fallback + " stderr: serialization_error:" + String(error));
        }
    }

    function serializeToolError(error) {
        const message = String(error && error.message ? error.message : error);
        return limitToolResult("ok:false stdout: stderr:" + message);
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
        const nodeCp = window.nodeCp;
        const previousCommandContext = nodeCp && nodeCp.__live2dContext;
        try {
            const signal = state.controller ? state.controller.signal : undefined;
            if (nodeCp) nodeCp.__live2dContext = { generation: state.generation, signal: signal };
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
        } finally {
            if (nodeCp) nodeCp.__live2dContext = previousCommandContext;
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
        if (!isAgentBusy()) agentStatus.hidden = false;
        const entry = document.createElement("div");
        entry.className = "pio-agent-entry";
        entry.dataset.label = label;
        setEntryContent(entry, text, false);
        agentLog.appendChild(entry);
        scrollAgentLog();
        return entry;
    }

    function updateAgentEntry(entry, text, markdown) {
        if (!entry) return;
        setEntryContent(entry, text, Boolean(markdown));
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
            let payload = { message: message, compress: state.pendingCompress || false };
            state.pendingCompress = false;
            let requestIndex = 0;
            // 本轮对话是否发生过工具调用（发生过则最终回答显示"大彻大悟"）
            let usedTool = false;
            while (true) {
                if (state.stopRequested) throw stoppedError();
                setAgentState(requestIndex === 0 ? AGENT_STATE.THINKING : AGENT_STATE.CONTINUING);
                let reasoningEntry = null;
                let contentEntry = null;
                let reasoningText = "";
                let contentText = "";
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
                            // 工具调用前可能带有说明正文，以服务端汇总后的内容刷新显示。
                            if (typeof outcome.content === "string" && contentEntry) {
                                contentText = outcome.content;
                                updateAgentEntry(contentEntry, contentText);
                            }
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
                        if (reasoningText.trim()) {
                            if (!reasoningEntry) reasoningEntry = appendAgentEntryLive("看板娘转圈圈", "");
                            updateAgentEntry(reasoningEntry, reasoningText);
                        }
                    } else if (event === "content") {
                        contentText += data.delta || "";
                        if (contentText.trim()) {
                            if (!contentEntry) contentEntry = appendAgentEntryLive(usedTool ? "看板娘大彻大悟" : "看板娘碎碎念", "");
                            updateAgentEntry(contentEntry, contentText);
                        }
                    } else if (event === "context_estimate") {
                        if (Number.isFinite(data.context_tokens)) {
                            state.contextUsed = data.context_tokens;
                            state.contextEstimated = true;
                            updateContextUsage();
                        }
                    } else if (event === "error") {
                        throw new Error(data.message || "看板娘请求失败");
                    }
                }
                if (!outcome) throw new Error("流式响应意外结束");
                if (Number.isFinite(outcome.context_tokens)) {
                    state.contextUsed = outcome.context_tokens;
                    state.contextEstimated = outcome.context_tokens_estimated !== false;
                }
                if (Number.isFinite(outcome.context_length)) state.contextLimit = outcome.context_length;
                updateContextUsage();
                if (outcome.type === "final") {
                    const reply = String(outcome.reply || "").trim();
                    if (!reply) throw new Error("看板娘没有返回内容");
                    state.generation = null;
                    setAgentState(AGENT_STATE.COMPLETED);
                    if (!contentEntry) {
                        contentEntry = appendAgentEntryLive(usedTool ? "看板娘大彻大悟" : "看板娘碎碎念", "");
                    }
                    updateAgentEntry(contentEntry, reply, true);
                    return reply;
                }
                if (outcome.type !== "tool_calls"
                    || !Array.isArray(outcome.calls)
                    || outcome.calls.length === 0) {
                    throw new Error("AI 返回了无效的工具步骤");
                }

                bubble("正在执行第 " + (requestIndex + 1) + " 步…", 120000);
                setAgentState(AGENT_STATE.EXECUTING_TOOL, { toolStep: requestIndex + 1 });
                const generation = String(outcome.generation || "").trim();
                if (!generation) throw new Error("工具步骤缺少对话代次");
                state.generation = generation;
                const toolResults = [];
                for (let callIndex = 0; callIndex < outcome.calls.length; callIndex += 1) {
                    if (state.stopRequested) throw stoppedError();
                    const call = outcome.calls[callIndex];
                    const codeText = call.javascript || call.source_code || "";
                    appendAgentStatus("看板娘搓搓小手", call.name + "\n" + codeText, "tool");
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

    // 发送一条对话消息：压缩按钮、自动压缩与用户手动发送共用。
    // 压缩请求在界面上与用户手动发送提示词完全一致（显示为一条“主人”消息）。
    async function sendMessage(message) {
        // 跳过自动压缩标记：压缩完成后续发的原消息不能再触发压缩（否则死循环）
        const skipAutoCompress = state.skipAutoCompress;
        if (skipAutoCompress) state.skipAutoCompress = false;

        if (!skipAutoCompress) {
            // 上下文使用率超过 75% 且当前不是压缩请求时：先可见地压缩，再发送原消息
            const threshold = Math.floor(state.contextLimit * 0.75);
            const shouldAutoCompress =
                !state.pendingCompress
                && state.contextLimit > 0
                && state.contextUsed > threshold
                && state.contextUsed > 0;
            if (shouldAutoCompress) {
                state.pendingMessage = message;
                state.pendingCompress = true;
                await sendMessage("总结下之前的对话");
                return; // 原消息续发由内层 finally 处理
            }
        }

        // 本次请求是否为压缩请求（runAgent 内部会清掉 pendingCompress）
        const isCompressRequest = state.pendingCompress;
        let stopped = false;

        clearBubble();
        resetAgentStatus();
        appendAgentStatus("主人", message, "user");
        setAgentState(AGENT_STATE.THINKING);
        settingsButton.disabled = true;
        bubble("正在思考…", 120000);

        try {
            const reply = await runAgent(message);
            bubble(reply, { persist: true, closeable: true, markdown: true });
        } catch (error) {
            if (error && error.stopped) {
                stopped = true;
                setAgentState(AGENT_STATE.STOPPED);
                appendAgentStatus("提示", "本次对话已停止", "result");
                bubble("已停止", 3000);
            } else {
                setAgentState(AGENT_STATE.FAILED);
                appendAgentStatus("错误", error && error.message ? error.message : String(error), "error");
                bubble(error && error.message ? error.message : "请求失败，请稍后重试", 8000);
            }
        } finally {
            autoResizeInput();
            settingsButton.disabled = false;
            // 压缩请求完成（未被停止）后：若有暂存的原消息，自动续发
            if (isCompressRequest && state.pendingMessage && !stopped) {
                const originalMessage = state.pendingMessage;
                state.pendingMessage = null;
                state.skipAutoCompress = true;
                void sendMessage(originalMessage);
            } else {
                if (isCompressRequest) state.pendingMessage = null;
            }
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
        if (compressHeaderButton) compressHeaderButton.hidden = show;
        agentStatus.hidden = show || (!isAgentBusy() && agentLog.childElementCount === 0);
        settingsButton.textContent = show ? "对话" : "设置";
        requestAnimationFrame(positionPanel);
        if (show) baseUrlInput.focus();
        else input.focus();
    }

    function fillConfigOptions(configs, selected) {
        if (!configInput) return;
        const list = Array.isArray(configs) ? configs : [];
        const source = list.length ? list : [selected];
        const seen = new Set();
        configInput.innerHTML = "";
        for (let i = 0; i < source.length; i += 1) {
            const value = String(source[i] == null ? "" : (typeof source[i] === "string" ? source[i] : (source[i].name || ""))).trim();
            if (!value || seen.has(value)) continue;
            seen.add(value);
            const option = document.createElement("option");
            option.value = value;
            option.textContent = value;
            configInput.appendChild(option);
        }
        const current = String(selected == null ? "" : selected).trim();
        if (current && seen.has(current)) {
            configInput.value = current;
        } else if (configInput.options.length) {
            configInput.options[0].selected = true;
        }
    }

    function applyCurrentConfigToInputs() {
        const current = state.configs.find(function (item) { return item.name === configInput.value; });
        baseUrlInput.value = (current && current.base_url) || "";
        apiKeyInput.value = (current && current.api_key) || "";
        modelInput.value = (current && current.model) || "";
        thinkingLevelInput.value = (current && current.thinking_level) || "high";
        const length = Number((current && current.context_length) || 131072);
        if (contextLengthInput) contextLengthInput.value = String(length);
        state.contextLimit = length;
        state.currentConfigName = configInput.value;
        updateContextUsage();
    }

    function applyConfig(config) {
        state.configLoaded = true;
        state.configs = Array.isArray(config.configs) ? config.configs : [];
        fillConfigOptions(state.configs, config.config);
        applyCurrentConfigToInputs();
        systemPromptInput.value = config.system_prompt || "";
        if (Number.isFinite(config.context_used)) state.contextUsed = config.context_used;
        state.contextEstimated = config.context_used_estimated !== false;
        updateContextUsage();
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
    if (compressHeaderButton) {
        compressHeaderButton.addEventListener("click", function () {
            if (isAgentBusy()) {
                bubble("正在对话中，请稍后再压缩", 4000);
                return;
            }
            // 压缩只由按钮触发：不篡改输入框，以与用户手动发送相同的方式
            // 发送压缩提示词（界面展示为一条“主人”消息）。
            state.pendingMessage = null;
            state.skipAutoCompress = false;
            state.pendingCompress = true;
            void sendMessage("总结下之前的对话");
        });
    }

    settingsButton.addEventListener("click", function () {
        const openingSettings = settingsForm.hidden;
        showSettings(openingSettings);
        if (openingSettings && !state.configLoaded) loadConfig();
    });
    settingsBack.addEventListener("click", function () {
        showSettings(false);
    });
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

    // [已修改] 移除了"点击外部自动关闭"逻辑，点击其它地方不会关闭对话框（2026-08-08）
    document.addEventListener("keydown", function (event) {
        if (event.key === "Escape" && !panel.hidden) closePanel();
    });
    input.addEventListener("input", function () {
        if (input.value.trim()) clearBubble();
        autoResizeInput();
    });

    input.addEventListener("keydown", function (event) {
        if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
            event.preventDefault();
            if (isAgentBusy()) return;
            chatForm.requestSubmit();
        }
    });
    sendButton.addEventListener("click", function (event) {
        if (!isAgentBusy()) return;
        event.preventDefault();
        requestStop();
    });
    window.addEventListener("resize", positionPanel);
    window.addEventListener("pagehide", function () {
        void cancelPendingGeneration().catch(function () {});
    });
    document.addEventListener("mouseup", positionPanel);

    chatForm.addEventListener("submit", async function (event) {
        event.preventDefault();
        if (isAgentBusy()) {
            return;
        }
        const message = input.value.trim();
        if (!message) {
            input.focus();
            return;
        }
        input.value = "";
        autoResizeInput();
        await sendMessage(message);
    });

    if (clearContextButton) {
        clearContextButton.addEventListener("click", async function () {
            if (isAgentBusy()) {
                bubble("请先停止当前对话，再清除上下文。", 5000);
                return;
            }
            if (!window.confirm("确定清除全部对话上下文吗？此操作无法撤销。")) return;

            const originalText = clearContextButton.textContent;
            clearContextButton.disabled = true;
            clearContextButton.textContent = "清除中…";
            try {
                const data = apiResult(await axios.post("/live2d_chat_clear", {}));
                state.contextUsed = data && Number.isFinite(data.context_tokens)
                    ? data.context_tokens
                    : 0;
                state.contextEstimated = true;
                updateContextUsage();
                clearBubble();
                bubble("上下文已清除。", 3000);
            } catch (error) {
                bubble(error && error.message ? error.message : "清除上下文失败", 8000);
            } finally {
                clearContextButton.disabled = false;
                clearContextButton.textContent = originalText;
            }
        });
    }

    function syncCurrentConfigFromInputs(name) {
        const targetName = name || state.currentConfigName || configInput.value;
        const current = state.configs.find(function (item) { return item.name === targetName; });
        if (!current) return;
        current.base_url = baseUrlInput.value.trim();
        current.api_key = apiKeyInput.value.trim();
        current.model = modelInput.value.trim();
        current.context_length = Number(contextLengthInput.value) || 131072;
        current.thinking_level = thinkingLevelInput.value;
    }

    if (configInput) {
        configInput.addEventListener("change", function () {
            syncCurrentConfigFromInputs(state.currentConfigName);
            applyCurrentConfigToInputs();
        });
    }

    if (configAddButton && configAddInput && configInput) {
        configAddButton.addEventListener("click", function () {
            const name = configAddInput.value.trim();
            if (!name) {
                bubble("请输入要添加的配置名称", 3000);
                return;
            }
            if (state.configs.some(function (item) { return item.name === name; })) {
                bubble("配置名称已存在", 3000);
                return;
            }
            syncCurrentConfigFromInputs(state.currentConfigName);
            state.configs.push({
                name: name,
                base_url: "",
                api_key: "",
                model: "",
                context_length: 131072,
                thinking_level: "high"
            });
            const option = document.createElement("option");
            option.value = name;
            option.textContent = name;
            configInput.appendChild(option);
            configInput.value = name;
            applyCurrentConfigToInputs();
            configAddInput.value = "";
            configAddInput.focus();
        });
        configAddInput.addEventListener("keydown", function (event) {
            if (event.key === "Enter") {
                event.preventDefault();
                configAddButton.click();
            }
        });
        if (configDelButton) {
            configDelButton.addEventListener("click", function () {
                if (configInput.options.length <= 1) {
                    bubble("至少需要保留一个配置", 3000);
                    return;
                }
                const name = configInput.value;
                const index = configInput.selectedIndex;
                if (index >= 0) {
                    state.configs = state.configs.filter(function (item) { return item.name !== name; });
                    configInput.remove(index);
                    applyCurrentConfigToInputs();
                }
            });
        }
    }

    settingsForm.addEventListener("submit", async function (event) {
        event.preventDefault();
        const submitButton = settingsForm.querySelector("button[type='submit']");
        submitButton.disabled = true;
        syncCurrentConfigFromInputs(state.currentConfigName);
        const payload = {
            config: configInput.value.trim(),
            configs: state.configs,
            system_prompt: systemPromptInput.value.trim()
        };

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
