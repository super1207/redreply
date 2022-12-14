
fn main() {
    
    // 初始化日志
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();

    // 初始化资源
    redlang::initialize();

    // 调用插件菜单
    //redlang::menu_a();
    loop {
        let time_struct = core::time::Duration::from_secs(1);
        std::thread::sleep(time_struct);
    }
}