
fn main() {
    
    // 初始化日志
    let format = "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]";
    tracing_subscriber::fmt()
    .with_timer(tracing_subscriber::fmt::time::OffsetTime::new(
        time::UtcOffset::current_local_offset().unwrap(),
        time::format_description::parse(format).unwrap(),
    )).with_max_level(tracing::Level::INFO)
    .init();

    // 初始化资源 
    redlang::initialize();

    loop {
        let time_struct = core::time::Duration::from_secs(1);
        std::thread::sleep(time_struct);
    }
}