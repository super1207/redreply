use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    time::Duration,
};

use headless_chrome::{
    protocol::cdp::Page::CaptureScreenshotFormatOption, types::Bounds, Browser, LaunchOptions,
};
use serde::Deserialize;

pub const API_VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum BrowserOperation {
    Screenshot,
    Content,
    InnerText,
}

#[derive(Deserialize)]
struct BrowserRequest {
    operation: BrowserOperation,
    path: String,
    selector: String,
    proxy: String,
    width: u32,
    height: u32,
    headless: bool,
    timeout_ms: u64,
    headers: HashMap<String, String>,
}

pub fn execute_json(request_json: &str) -> Result<Vec<u8>, String> {
    let request: BrowserRequest =
        serde_json::from_str(request_json).map_err(|err| format!("invalid request: {err}"))?;
    execute(request).map_err(|err| format!("{err:?}"))
}

fn execute(request: BrowserRequest) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let proxy_arg = (!request.proxy.is_empty())
        .then(|| OsString::from(format!("--proxy-server={}", request.proxy)));
    let args: Vec<&OsStr> = proxy_arg
        .as_ref()
        .map(|arg| vec![arg.as_os_str()])
        .unwrap_or_default();

    let options = LaunchOptions::default_builder()
        .window_size(Some((request.width, request.height)))
        .headless(request.headless)
        .devtools(false)
        .sandbox(false)
        .idle_browser_timeout(Duration::from_millis(request.timeout_ms))
        .args(args)
        .build()?;
    let browser = Browser::new(options)?;
    let tab = browser.new_tab()?;

    if !request.headers.is_empty() {
        let headers = request
            .headers
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        tab.set_extra_http_headers(headers)?;
    }

    tab.navigate_to(&request.path)?.wait_until_navigated()?;
    let html = tab.wait_for_element("html")?;

    if matches!(request.operation, BrowserOperation::Screenshot) {
        let box_model = html.get_box_model()?;
        tab.set_bounds(Bounds::Normal {
            left: Some(0),
            top: Some(0),
            width: Some(box_model.width),
            height: Some(box_model.height + 200.0),
        })?;
    }

    let element = if request.selector.is_empty() {
        html
    } else {
        tab.wait_for_element(&request.selector)?
    };

    match request.operation {
        BrowserOperation::Screenshot => Ok(tab.capture_screenshot(
            CaptureScreenshotFormatOption::Png,
            None,
            Some(element.get_box_model()?.content_viewport()),
            true,
        )?),
        BrowserOperation::Content => Ok(element.get_content()?.into_bytes()),
        BrowserOperation::InnerText => Ok(element.get_inner_text()?.into_bytes()),
    }
}

#[repr(C)]
pub struct BrowserBuffer {
    pub ptr: *mut u8,
    pub len: usize,
}

impl BrowserBuffer {
    fn from_vec(data: Vec<u8>) -> Self {
        let boxed = data.into_boxed_slice();
        let len = boxed.len();
        let ptr = Box::into_raw(boxed) as *mut u8;
        Self { ptr, len }
    }
}

#[no_mangle]
pub extern "C" fn redreply_browser_api_version() -> u32 {
    API_VERSION
}

#[no_mangle]
/// Execute a browser request through the C ABI.
///
/// # Safety
/// `request_ptr` must be null or point to `request_len` readable bytes, and
/// `output` must be null or point to writable `BrowserBuffer` storage.
pub unsafe extern "C" fn redreply_browser_execute(
    request_ptr: *const u8,
    request_len: usize,
    output: *mut BrowserBuffer,
) -> i32 {
    if request_ptr.is_null() || output.is_null() {
        return 1;
    }

    let result = std::panic::catch_unwind(|| {
        let request = std::slice::from_raw_parts(request_ptr, request_len);
        let request = std::str::from_utf8(request).map_err(|err| err.to_string())?;
        execute_json(request)
    });

    let (status, data) = match result {
        Ok(Ok(data)) => (0, data),
        Ok(Err(err)) => (1, err.into_bytes()),
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "browser DLL panicked".to_owned());
            (2, message.into_bytes())
        }
    };

    output.write(BrowserBuffer::from_vec(data));
    status
}

#[no_mangle]
/// Release a buffer returned by [`redreply_browser_execute`].
///
/// # Safety
/// `buffer` must have been returned by this crate and must not have already
/// been released.
pub unsafe extern "C" fn redreply_browser_free(buffer: BrowserBuffer) {
    if buffer.ptr.is_null() {
        return;
    }
    let slice = std::ptr::slice_from_raw_parts_mut(buffer.ptr, buffer.len);
    drop(Box::from_raw(slice));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_returns_invalid_request_error() {
        let request = b"{}";
        let mut output = BrowserBuffer {
            ptr: std::ptr::null_mut(),
            len: 0,
        };
        let status = unsafe {
            redreply_browser_execute(request.as_ptr(), request.len(), &mut output as *mut _)
        };
        assert_eq!(status, 1);
        assert!(!output.ptr.is_null());
        unsafe { redreply_browser_free(output) };
    }
}
