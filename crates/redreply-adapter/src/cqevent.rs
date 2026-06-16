use crate::AdapterResult;

pub fn do_1207_event(event_json: &str) -> AdapterResult<()> {
    crate::host::dispatch_event(event_json)
}
