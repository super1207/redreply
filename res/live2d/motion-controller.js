(function () {
    "use strict";

    const motionGroups = Object.freeze({
        neutral: null,
        happy: "happy",
        sad: "sad",
        shy: "flick_head",
        sleepy: "sleepy",
        surprised: "tap_body"
    });
    const originalLoadLive2d = window.loadlive2d;
    let currentModel = null;
    let pendingMotion = null;
    let retryTimer = null;

    if (typeof originalLoadLive2d !== "function") return;

    function isLive2dModel(candidate) {
        return candidate
            && typeof candidate.startRandomMotion === "function"
            && typeof candidate.startMotion === "function"
            && Object.prototype.hasOwnProperty.call(candidate, "initialized");
    }

    function playNow(label) {
        if (!Object.prototype.hasOwnProperty.call(motionGroups, label)) return false;
        const group = motionGroups[label];
        if (!group) return true;
        if (!currentModel || !currentModel.initialized || !currentModel.modelSetting) return false;
        if (currentModel.modelSetting.getMotionNum(group) < 1) return false;

        currentModel.startRandomMotion(group, 4);
        return true;
    }

    function flushPending(attempt) {
        clearTimeout(retryTimer);
        retryTimer = null;
        if (!pendingMotion) return;
        if (playNow(pendingMotion)) {
            pendingMotion = null;
            return;
        }
        if (attempt < 50) {
            retryTimer = setTimeout(function () {
                flushPending(attempt + 1);
            }, 100);
        } else {
            pendingMotion = null;
        }
    }

    // The minified Cubism 2 bundle keeps its model manager private. Capture the
    // model only during synchronous creation, then immediately restore push().
    window.loadlive2d = function () {
        const nativePush = Array.prototype.push;
        Array.prototype.push = function () {
            for (let index = 0; index < arguments.length; index += 1) {
                if (isLive2dModel(arguments[index])) currentModel = arguments[index];
            }
            return nativePush.apply(this, arguments);
        };

        try {
            return originalLoadLive2d.apply(this, arguments);
        } finally {
            Array.prototype.push = nativePush;
            if (pendingMotion) flushPending(0);
        }
    };

    window.playLive2dMotion = function (motion) {
        const label = typeof motion === "string" ? motion.trim().toLowerCase() : "neutral";
        if (!Object.prototype.hasOwnProperty.call(motionGroups, label)) return false;
        if (label === "neutral") {
            clearTimeout(retryTimer);
            retryTimer = null;
            pendingMotion = null;
            return true;
        }
        if (playNow(label)) return true;

        pendingMotion = label;
        flushPending(0);
        return true;
    };

    window.getLive2dModel = function () {
        return currentModel;
    };
}());
