use time::UtcOffset;


fn main() {
    
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


    #[cfg(windows)]
    unsafe { 
        let mut lpdwprocesslistl = vec![0,0];
        let ptr = lpdwprocesslistl.as_mut_ptr();
        let is_in_cmd = windows_sys::Win32::System::Console::GetConsoleProcessList(ptr,2) > 1;
        if !is_in_cmd {
            let cur_process_id = windows_sys::Win32::System::Threading::GetCurrentProcessId();
            let desk_window = windows_sys::Win32::UI::WindowsAndMessaging::GetDesktopWindow();
            let mut window = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExA(desk_window,0,std::ptr::null(),std::ptr::null());
            while window != 0 {
                let mut process_id :u32 = 0;
                windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(window,&mut process_id);
                if process_id == cur_process_id {
                    //windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(window,windows_sys::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE,windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW as i32);
                    //windows_sys::Win32::UI::WindowsAndMessaging::SetWindowPos(window,0,0,0,0,0,windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOSIZE | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOZORDER | windows_sys::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED);
                    windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(window,windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE);
                }
                let window_next = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExA(desk_window,window,std::ptr::null(),std::ptr::null());
                windows_sys::Win32::Foundation::CloseHandle(window);
                window = window_next;
            }
            windows_sys::Win32::Foundation::CloseHandle(desk_window);
        }
        
    };

    #[cfg(windows)]
    let app = fltk::app::App::default().with_scheme(fltk::app::Scheme::Gtk);

    // 初始化图标（只有windows）才支持托盘图标
    #[cfg(windows)]
    let menu_channel = tray_icon::menu::MenuEvent::receiver();
    #[cfg(windows)]
    let tray_channel = tray_icon::TrayIconEvent::receiver();
    #[cfg(windows)]
    let show_web = tray_icon::menu::MenuItem::new("控制面板", true, None);
    #[cfg(windows)]
    let help_web = tray_icon::menu::MenuItem::new("帮助文档", true, None);
    #[cfg(windows)]
    let log_web = tray_icon::menu::MenuItem::new("查看日志", true, None);
    #[cfg(windows)]
    let dir_web = tray_icon::menu::MenuItem::new("软件目录", true, None);
    #[cfg(windows)]
    let quit = tray_icon::menu::MenuItem::new("退出软件", true, None);

    #[cfg(windows)]
    let _tray_icon = {
        let icon_data = redlang::Asset::get("res/favicon.ico").unwrap().data;
        let image = image::load_from_memory(&icon_data).unwrap().into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        let icon =tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to open icon");
        let tray_menu = tray_icon::menu::Menu::new();
        
        tray_menu.append_items(&[
            &show_web,
            &help_web,
            &log_web,
            &dir_web,
            &tray_icon::menu::PredefinedMenuItem::separator(),
            &quit
        ]).unwrap();
        tray_icon::TrayIconBuilder::new()
            .with_tooltip("欢迎使用红色问答")
            .with_icon(icon)
            .with_menu(Box::new(tray_menu))
            .build()
            .unwrap()
    };
    

    // 初始化资源 
    redlang::initialize();
    
    #[cfg(windows)]
    {
        use fltk::prelude::{WidgetExt, WindowExt};
        let mut wind = <fltk::window::Window as fltk::prelude::WidgetBase>::new(0, 0, 0, 0, "");
        fltk::prelude::GroupExt::end(&wind);
        wind.set_border(false);
        wind.show();
        wind.platform_hide();
        while app.wait() {
            if let Ok(event) = menu_channel.try_recv() {
                if event.id == show_web.id() {
                    let _err = redlang::show_ctrl_web();
                }else if event.id == quit.id() {
                    redlang::wait_for_quit();
                } else if event.id == help_web.id() {
                    let _err = redlang::show_help_web();
                } else if event.id == log_web.id() {
                    let _err = redlang::show_log_web();
                } else if event.id == dir_web.id() {
                    let _err = redlang::show_dir_web();
                }
            }
            if let Ok(event) = tray_channel.try_recv() {
                if event.click_type == tray_icon::ClickType::Double {
                    let _err = redlang::show_ctrl_web();
                }
                // println!("Tray event: {:?}", event);
            }

            let time_struct = core::time::Duration::from_millis(50);
            std::thread::sleep(time_struct);
        }
    }
   

    loop {
        let time_struct = core::time::Duration::from_secs(1);
        std::thread::sleep(time_struct);
    }
}