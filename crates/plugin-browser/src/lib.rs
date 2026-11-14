//! 浏览器插件：无头 Chromium（Windows 走 sidecar DLL，其他平台直接链接 crate），
//! 注册 `browser` 服务供脚本命令调用。

use std::sync::Arc;

use redlang::plugin_ports::{BrowserBridgeService, BrowserServiceHandle};
use redreply_bot::{Plugin, PluginContext, PluginMeta, Result};

/// 基于 `browserbridge` 的实现。
pub struct DefaultBrowserService {
    service: SidecarBrowser,
}

impl BrowserBridgeService for DefaultBrowserService {
    fn execute_json(&self, request: &str) -> std::result::Result<Vec<u8>, String> {
        let request: serde_json::Value =
            serde_json::from_str(request).map_err(|error| error.to_string())?;
        self.service
            .execute(&request)
            .map_err(|error| error.to_string())
    }
}

/// 浏览器插件。
pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta::new("browser", env!("CARGO_PKG_VERSION"))
            .description("无头浏览器：网页截图 / 获取内容")
            .provides("browser")
    }

    fn register(&self, ctx: &PluginContext) -> Result<()> {
        ctx.register_service(
            "browser",
            Arc::new(BrowserServiceHandle(Arc::new(DefaultBrowserService {
                service: SidecarBrowser::new(),
            }))),
        )
    }

    fn start(&self, ctx: &PluginContext) -> Result<()> {
        ctx.logger().info("browser", "浏览器插件已就绪");
        Ok(())
    }
}

#[cfg(not(windows))]
fn execute_json(request: &str) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    redreply_browser::execute_json(request).map_err(|message| std::io::Error::other(message).into())
}

pub struct SidecarBrowser {
    #[cfg(windows)]
    library: std::sync::Mutex<Option<std::sync::Arc<BrowserLibrary>>>,
}

impl SidecarBrowser {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            library: std::sync::Mutex::new(None),
        }
    }

    fn execute(
        &self,
        request: &serde_json::Value,
    ) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
        #[cfg(not(windows))]
        return execute_json(&request.to_string());

        #[cfg(windows)]
        {
            let library = {
                let mut library = self
                    .library
                    .lock()
                    .map_err(|_| std::io::Error::other("browser DLL lock poisoned"))?;
                if library.is_none() {
                    *library = Some(std::sync::Arc::new(BrowserLibrary::load()?));
                }
                library
                    .as_ref()
                    .ok_or_else(|| std::io::Error::other("browser DLL not loaded"))?
                    .clone()
            };
            library.execute(&request.to_string())
        }
    }
}

impl Default for SidecarBrowser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
#[repr(C)]
struct BrowserBuffer {
    ptr: *mut u8,
    len: usize,
}

#[cfg(windows)]
type ApiVersionFn = unsafe extern "C" fn() -> u32;
#[cfg(windows)]
type ExecuteFn = unsafe extern "C" fn(*const u8, usize, *mut BrowserBuffer) -> i32;
#[cfg(windows)]
type FreeFn = unsafe extern "C" fn(BrowserBuffer);

#[cfg(windows)]
struct BrowserLibrary {
    _library: libloading::Library,
    execute_fn: ExecuteFn,
    free_fn: FreeFn,
}

#[cfg(windows)]
impl BrowserLibrary {
    fn load() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .ok_or_else(|| std::io::Error::other("executable has no parent directory"))?;
        let path = directory.join("redreply_browser.dll");
        #[cfg(debug_assertions)]
        let path = if path.exists() {
            path
        } else {
            directory.join("deps").join("redreply_browser.dll")
        };

        unsafe {
            let library = libloading::Library::new(&path).map_err(|err| {
                std::io::Error::other(format!(
                    "failed to load browser DLL `{}`: {err}",
                    path.display()
                ))
            })?;
            let api_version: ApiVersionFn = *library.get(b"redreply_browser_api_version\0")?;
            let execute_fn: ExecuteFn = *library.get(b"redreply_browser_execute\0")?;
            let free_fn: FreeFn = *library.get(b"redreply_browser_free\0")?;

            let version = api_version();
            if version != 1 {
                return Err(std::io::Error::other(format!(
                    "unsupported browser DLL API version {version}, expected 1"
                ))
                .into());
            }

            Ok(Self {
                _library: library,
                execute_fn,
                free_fn,
            })
        }
    }

    fn execute(&self, request: &str) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut output = BrowserBuffer {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        let status =
            unsafe { (self.execute_fn)(request.as_ptr(), request.len(), &mut output as *mut _) };
        if output.ptr.is_null() {
            return Err(std::io::Error::other("browser DLL returned a null buffer").into());
        }

        let data = unsafe { std::slice::from_raw_parts(output.ptr, output.len).to_vec() };
        unsafe { (self.free_fn)(output) };

        if status == 0 {
            Ok(data)
        } else {
            Err(std::io::Error::other(String::from_utf8_lossy(&data).into_owned()).into())
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn loads_sidecar_and_returns_abi_error() {
        let browser = super::SidecarBrowser::new();
        let error = browser.execute(&serde_json::json!({})).unwrap_err();
        assert!(error.to_string().contains("invalid request"));
    }
}
