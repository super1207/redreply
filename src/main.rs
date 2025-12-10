#![windows_subsystem = "windows"]

use time::UtcOffset;

#[cfg(windows)]
fn create_windows() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::collections::HashMap;

    fn create_new_window<T> (
        webviews: &mut HashMap<WindowId, (Window, wry::WebView)>,
        target: &tao::event_loop::EventLoopWindowTarget<T>,
        proxy: &EventLoopProxy<UserEvent>,
    )  -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config = redlang::read_config()?;
        let port = config.get("web_port").ok_or("无法获取web_port")?.as_u64().ok_or("无法获取web_port")?;
        create_specific_window(webviews, target, proxy, &format!("http://localhost:{port}"))?;
        Ok(())
    }

    fn create_specific_window<T>(
        webviews: &mut HashMap<WindowId, (Window, wry::WebView)>,
        target: &tao::event_loop::EventLoopWindowTarget<T>,
        proxy: &EventLoopProxy<UserEvent>,
        url: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let width = 1024.0;
        let height = 768.0;

        let new_window = WindowBuilder::new()
            .with_title("加载中...")
            .with_inner_size(LogicalSize::new(width, height))
            .with_visible(false)
            .build(target)?;

        if let Some(monitor) = new_window.current_monitor() {
            let screen_size = monitor.size();
            let window_size = new_window.outer_size();

            // 1. 计算绝对中心
            let center_x = (screen_size.width as i32 - window_size.width as i32) / 2;
            let center_y = (screen_size.height as i32 - window_size.height as i32) / 2;

            // 2. 生成伪随机偏移量 (利用当前时间的纳秒数)
            // 我们不需要引入 rand 库，用时间戳取模足够产生视觉上的随机感
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .subsec_nanos() as i32;

            // 设定偏移范围：例如 +/- 40 像素
            let range = 80;

            // 计算 X 轴偏移: (nanos % 80) 得到 0~79，减去 40 得到 -40~39
            let offset_x = (nanos % range) - (range / 2);

            // 计算 Y 轴偏移: 将 nanos 除以 100 再取模，避免 X 和 Y 线性相关
            let offset_y = ((nanos / 100) % range) - (range / 2);

            // 3. 应用偏移
            let final_x = center_x + offset_x;
            let final_y = center_y + offset_y;

            new_window.set_outer_position(PhysicalPosition::new(final_x, final_y));
        }

        new_window.set_visible(true);

        new_window.set_focus();              // 请求输入焦点
        new_window.set_always_on_top(true);  // 瞬间开启置顶，强制覆盖在所有窗口之上
        new_window.set_always_on_top(false); // 瞬间关闭置顶，让窗口行为恢复正常

        match create_webview(&new_window, url, proxy.clone()) {
            Ok(webview) => {
                webviews.insert(new_window.id(), (new_window, webview));
            }
            Err(e) => {
                let _ = redlang::cq_add_log_w(&format!("创建窗口失败: {:?}", e)).unwrap();
            }
        }
        Ok(())
    }

    fn create_webview(
        window: &Window,
        url: &str,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<wry::WebView, Box<dyn std::error::Error + Send + Sync>> {

        use std::path::Path;
        use wry::WebContext; 
        use redlang::get_tmp_dir;
        let tmp_dir = get_tmp_dir()?;
        let data_directory = Path::new(&tmp_dir);
        let mut web_context = WebContext::new(Some(data_directory.to_path_buf()));

        let builder = WebViewBuilder::new_with_web_context(&mut web_context);
        
        

        let window_id = window.id();
        let proxy_for_title = proxy.clone();
        let proxy_for_download_start = proxy.clone();
        let proxy_for_download_complete = proxy.clone();

        builder
            .with_url(url)
            .with_new_window_req_handler(move |request_url, _req| {
                let _ = proxy.send_event(UserEvent::NewWindow(request_url));
                NewWindowResponse::Deny
            })
            .with_document_title_changed_handler(move |title| {
                let _ = proxy_for_title.send_event(UserEvent::TitleChanged(window_id, title));
            })
            .with_download_started_handler(move |url, suggested_path| {
                // 从建议路径或URL中提取文件名
                let filename = suggested_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_else(|| {
                        // 从URL中提取文件名
                        url.split('/').last().unwrap_or("download")
                    });

                // 显示文件保存对话框
                if let Some(save_path) = rfd::FileDialog::new()
                    .set_file_name(filename)
                    .set_directory(dirs::download_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()))
                    .save_file()
                {
                    *suggested_path = save_path.clone();
                    
                    // 发送下载开始事件
                    let _ = proxy_for_download_start.send_event(UserEvent::DownloadStarted(
                        url.to_string(), 
                        save_path
                    ));
                    
                    // 返回true表示允许下载
                    true
                } else {
                    // 用户取消了保存对话框，不允许下载
                    false
                }
            })
            .with_download_completed_handler(move |url, path, success| {
                // 发送下载完成事件
                let _ = proxy_for_download_complete.send_event(UserEvent::DownloadCompleted(
                    url.to_string(),
                    path.clone(),
                    success
                ));
            })
            .build(window)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }

    use tao::{
        dpi::{LogicalSize, PhysicalPosition},
        event::{Event, WindowEvent},
        event_loop::{EventLoopBuilder, EventLoopProxy},
        window::{Window, WindowBuilder, WindowId},
    };
    use wry::{NewWindowResponse, WebViewBuilder};

    enum UserEvent {
        NewWindow(String),
        TitleChanged(WindowId, String),
        TrayIconEvent(tray_icon::TrayIconEvent),
        MenuEvent(tray_icon::menu::MenuEvent),
        DownloadStarted(String, std::path::PathBuf),
        DownloadCompleted(String, Option<std::path::PathBuf>, bool),
    }

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let mut webviews = std::collections::HashMap::new();

    // 初始化图标（只有windows）才支持托盘图标
    let show_web = tray_icon::menu::MenuItem::new("控制面板", true, None);
    let help_web = tray_icon::menu::MenuItem::new("帮助文档", true, None);
    let log_web = tray_icon::menu::MenuItem::new("查看日志", true, None);
    let dir_web = tray_icon::menu::MenuItem::new("软件目录", true, None);
    let debug_web = tray_icon::menu::MenuItem::new("红色调试", true, None);
    let quit = tray_icon::menu::MenuItem::new("退出软件", true, None);

    let tray_menu = tray_icon::menu::Menu::new();

    tray_menu.append_items(&[
        &show_web,
        &help_web,
        &log_web,
        &dir_web,
        &debug_web,
        &tray_icon::menu::PredefinedMenuItem::separator(),
        &quit,
    ])?;

    let _tray_icon = {
        let proxy_tray = proxy.clone();
        let proxy_menu = proxy.clone();

        tray_icon::TrayIconEvent::set_event_handler(Some(move |event| {
            let _ = proxy_tray.send_event(UserEvent::TrayIconEvent(event));
        }));

        tray_icon::menu::MenuEvent::set_event_handler(Some(move |event| {
            let _ = proxy_menu.send_event(UserEvent::MenuEvent(event));
        }));

        let icon_data = redlang::Asset::get("res/favicon.ico").unwrap().data;
        let image = image::load_from_memory(&icon_data).unwrap().into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        let icon = tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to open icon");

        let tray_icon_t = tray_icon::TrayIconBuilder::new()
            .with_tooltip("欢迎使用红色问答")
            .with_icon(icon)
            .with_menu(Box::new(tray_menu))
            .build()
            .ok();
        if let Some(tray) = &tray_icon_t {
            tray.set_show_menu_on_left_click(false);
        }
        tray_icon_t
    };

    event_loop.run(move |event, event_loop_window_target, control_flow| {
        *control_flow = tao::event_loop::ControlFlow::Wait;

        match event {
            tao::event::Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                webviews.remove(&window_id);
            }

            Event::UserEvent(user_event) => match user_event {
                UserEvent::TrayIconEvent(tray_event) => {
                    if let tray_icon::TrayIconEvent::DoubleClick {
                        button: tray_icon::MouseButton::Left,
                        ..
                    } = tray_event
                    {
                        if let Err(e) =  create_new_window(&mut webviews, event_loop_window_target, &proxy) {
                            use redlang::cq_add_log_w;
                            cq_add_log_w(&format!("Failed to create window on tray double click: {:?}", e)).unwrap();
                        }
                    }
                }

                UserEvent::MenuEvent(menu_event) => {
                    if menu_event.id == show_web.id() {
                        let _err = redlang::show_ctrl_web();
                    } else if menu_event.id == quit.id() {
                        redlang::wait_for_quit();
                    } else if menu_event.id == help_web.id() {
                        let _err = redlang::show_help_web();
                    } else if menu_event.id == log_web.id() {
                        let _err = redlang::show_log_web();
                    } else if menu_event.id == dir_web.id() {
                        let _err = redlang::show_dir_web();
                    } else if menu_event.id == debug_web.id() {
                        let _err = redlang::show_debug_web();
                    }
                }

                UserEvent::NewWindow(url) => {
                    if let Err(e) = create_specific_window(&mut webviews, event_loop_window_target, &proxy, &url) {
                        use redlang::cq_add_log_w;
                        cq_add_log_w(&format!("Failed to create specific window: {:?}", e)).unwrap();
                    }
                }

                UserEvent::TitleChanged(window_id, new_title) => {
                    if let Some((window, _)) = webviews.get(&window_id) {
                        window.set_title(&new_title);
                    }
                }

                UserEvent::DownloadStarted(url, path) => {
                    let _ = redlang::cq_add_log_w(&format!("开始下载: {} -> {:?}", url, path));
                }

                UserEvent::DownloadCompleted(url, path, success) => {
                    if success {
                        if let Some(path) = path {
                            let _ = redlang::cq_add_log_w(&format!("下载完成: {} -> {:?}", url, path));
                            
                            // 打开文件所在目录并选中刚下载的文件
                            #[cfg(windows)]
                            {
                                // 使用 explorer /select, 命令来选中文件
                                let _ = std::process::Command::new("explorer")
                                    .arg("/select,")
                                    .arg(&path)
                                    .spawn();
                            }
                        }
                    } else {
                        let _ = redlang::cq_add_log_w(&format!("下载失败: {}", url));
                    }
                }
            },
            _ => (),
        }
    });
}

fn main() {
    // 记录程序开始时间
    {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis().to_string();
        let mut lk = redlang::G_START_TIME.lock().unwrap();
        *lk = d;
    }
    
    // 初始化日志
    let format = "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]";

    // 获得utc偏移
    let utc_offset;
    if let Ok(v) = UtcOffset::current_local_offset() {
        utc_offset = v;
    } else {
        // 中国是东八区，所以这里写8 hour
        utc_offset = UtcOffset::from_hms(8,0,0).unwrap();
    }

    tracing_subscriber::fmt()
    .with_timer(tracing_subscriber::fmt::time::OffsetTime::new(
        utc_offset,
        time::format_description::parse(format).unwrap(),
    )).with_max_level(tracing::Level::INFO)
    .init();

    // 初始化资源 
    redlang::initialize();

    
    #[cfg(windows)]
    if let Err(e) = create_windows() {
        redlang::cq_add_log_w(&format!("load tray err:{:?}", e)).unwrap();
    }
    
   

    loop {
        let time_struct = core::time::Duration::from_secs(1);
        std::thread::sleep(time_struct);
    }
}
