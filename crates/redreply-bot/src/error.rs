use std::fmt;

/// 框架统一错误类型。
#[derive(Debug)]
pub enum FrameworkError {
    /// 插件生命周期相关错误。
    Plugin(String),
    /// 服务注册表相关错误。
    Service(String),
    /// HTTP 服务相关错误。
    Http(String),
    /// 配置相关错误。
    Config(String),
    /// IO 错误。
    Io(std::io::Error),
    /// 运行时相关错误。
    Runtime(String),
}

impl FrameworkError {
    pub fn message(&self) -> &str {
        match self {
            FrameworkError::Plugin(message)
            | FrameworkError::Service(message)
            | FrameworkError::Http(message)
            | FrameworkError::Config(message)
            | FrameworkError::Runtime(message) => message,
            FrameworkError::Io(_) => "io error",
        }
    }
}

impl fmt::Display for FrameworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameworkError::Plugin(message) => write!(f, "插件错误：{message}"),
            FrameworkError::Service(message) => write!(f, "服务错误：{message}"),
            FrameworkError::Http(message) => write!(f, "HTTP 错误：{message}"),
            FrameworkError::Config(message) => write!(f, "配置错误：{message}"),
            FrameworkError::Io(error) => write!(f, "IO 错误：{error}"),
            FrameworkError::Runtime(message) => write!(f, "运行时错误：{message}"),
        }
    }
}

impl std::error::Error for FrameworkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FrameworkError::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FrameworkError {
    fn from(error: std::io::Error) -> Self {
        FrameworkError::Io(error)
    }
}

impl From<serde_json::Error> for FrameworkError {
    fn from(error: serde_json::Error) -> Self {
        FrameworkError::Config(error.to_string())
    }
}

/// 框架统一 Result。
pub type Result<T> = std::result::Result<T, FrameworkError>;
