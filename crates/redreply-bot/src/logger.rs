use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use tokio::sync::broadcast;

/// 内存中保留的历史日志条数。
pub const LOG_HISTORY_LIMIT: usize = 200;

/// 日志级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// 一条日志记录。
#[derive(Clone, Debug, Serialize)]
pub struct LogRecord {
    pub time: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

/// 框架日志门面。
///
/// 与旧版实现不同，日志不直接落文件，而是：
/// - 保留最近 [`LOG_HISTORY_LIMIT`] 条内存历史（管理台/日志页读取）；
/// - 通过 broadcast 通道推送给订阅者（未来的 `/watch_log` WebSocket）。
pub struct Logger {
    tx: broadcast::Sender<LogRecord>,
    history: Mutex<VecDeque<LogRecord>>,
    limit: usize,
}

impl Logger {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            tx,
            history: Mutex::new(VecDeque::new()),
            limit: LOG_HISTORY_LIMIT,
        }
    }

    pub fn log(&self, level: LogLevel, target: &str, message: impl Into<String>) {
        let record = LogRecord {
            time: chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string(),
            level,
            target: target.to_string(),
            message: message.into(),
        };
        if let Ok(mut history) = self.history.lock() {
            history.push_back(record.clone());
            while history.len() > self.limit {
                history.pop_front();
            }
        }
        let _ = self.tx.send(record);
    }

    pub fn debug(&self, target: &str, message: impl Into<String>) {
        self.log(LogLevel::Debug, target, message);
    }

    pub fn info(&self, target: &str, message: impl Into<String>) {
        self.log(LogLevel::Info, target, message);
    }

    pub fn warn(&self, target: &str, message: impl Into<String>) {
        self.log(LogLevel::Warn, target, message);
    }

    pub fn error(&self, target: &str, message: impl Into<String>) {
        self.log(LogLevel::Error, target, message);
    }

    /// 订阅实时日志。
    pub fn subscribe(&self) -> broadcast::Receiver<LogRecord> {
        self.tx.subscribe()
    }

    /// 最近的历史日志（时间正序）。
    pub fn history(&self) -> Vec<LogRecord> {
        self.history
            .lock()
            .map(|history| history.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_newest_records() {
        let logger = Logger::new();
        logger.info("test", "第一条");
        logger.warn("test", "第二条");
        let history = logger.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].message, "第一条");
        assert_eq!(history[1].level, LogLevel::Warn);
    }
}
