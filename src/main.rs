#![windows_subsystem = "windows"]

use time::UtcOffset;

#[cfg(windows)]
fn create_windows() {
    let app = fltk::app::App::default().with_scheme(fltk::app::Scheme::Gtk);

    // 初始化图标（只有windows）才支持托盘图标
    let menu_channel = tray_icon::menu::MenuEvent::receiver();
    let tray_channel = tray_icon::TrayIconEvent::receiver();
    let show_web = tray_icon::menu::MenuItem::new("控制面板", true, None);
    let help_web = tray_icon::menu::MenuItem::new("帮助文档", true, None);
    let log_web = tray_icon::menu::MenuItem::new("查看日志", true, None);
    let dir_web = tray_icon::menu::MenuItem::new("软件目录", true, None);
    let quit = tray_icon::menu::MenuItem::new("退出软件", true, None);
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


    use fltk::button::*;
    use fltk::enums::*;

    use fltk::group::*;

    use fltk::prelude::*;

    use fltk::window::*;
    let mut wind = Window::new(446, 317, 1, 1, None);
    wind.set_color(Color::from_rgb(0, 0, 0));
    wind.resizable(&wind);
    wind.set_border(false);
    wind.set_callback(|e|{
        e.set_border(false);
        e.resize(e.x(), e.y(), 0, 0);
    });
    wind.set_label("锟斤拷??烫烫烫??屯屯屯?锘銝剜��	皜祈岫	撠舘⏛");
    let mut flex_win = Flex::new(10, 10, 100, 100, None);
    let flex_win_t = flex_win.clone();
    wind.resize_callback(move |_w,_x,_y,width,height|{
        flex_win_t.clone().resize(10, 10, width-20, height-20);
    });
    flex_win.set_type(FlexType::Column);
    let v = redlang::add_egg_click().unwrap();
    let button_title = format!("点我功德加一\n\n\n当前:{}",v);
    let mut btn = Button::new(0, 0, 100, 100, &*button_title);
    btn.set_label_color(Color::from_rgb(66, 134, 244));
    btn.set_color(Color::from_rgb(255, 255, 255));
    btn.visible_focus(false);
    btn.set_callback(move |w|{
        let v = redlang::add_egg_click().unwrap();
        let button_title = format!("点我功德加一\n\n\n当前:{}",v);
        w.set_label(&*button_title)
    });
    flex_win.end();
    wind.end();

    wind.show();
    
    wind.resize(wind.x(), wind.y(), 0, 0);
    
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
            match event {
                tray_icon::TrayIconEvent::Click { id: _, position: _, rect: _, button, button_state: _ } => {
                    match button {
                        tray_icon::MouseButton::Left => {
                            wind.set_border(true);
                            wind.resizable(&wind);
                            wind.resize(wind.x(), wind.y(),600, 400);
                        },
                        _ => {}
                    } 
                },
                _ => {}
            }
        }

        let time_struct = core::time::Duration::from_millis(50);
        std::thread::sleep(time_struct);
    }
}


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

    // 初始化资源 
    redlang::initialize();

    
    #[cfg(windows)]
    create_windows();
   

    loop {
        let time_struct = core::time::Duration::from_secs(1);
        std::thread::sleep(time_struct);
    }
}