//! 红色问答共享层：宿主与插件共用的工具与端口定义，不依赖宿主。

pub mod file_lock;
pub mod mytool;
pub mod plugin_ports;
pub mod proxy;

pub use file_lock::{add_file_lock, configure_conn_busy_timeout, del_file_lock};
