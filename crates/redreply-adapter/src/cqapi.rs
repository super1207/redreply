use crate::AdapterResult;

pub fn cq_add_log(msg: &str) -> AdapterResult<()> {
    crate::host::log(msg);
    Ok(())
}

pub fn cq_add_log_w(msg: &str) -> AdapterResult<()> {
    crate::host::warn(msg);
    Ok(())
}

pub fn cq_get_app_directory1() -> AdapterResult<String> {
    crate::host::app_dir()
}
