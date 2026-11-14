use crate::AdapterResult;

pub use redreply_common::mytool::{
    cq_fix_image_file_in_arr, cq_params_encode, cq_text_encode, read_json_obj,
    read_json_obj_or_null, read_json_or_default, read_json_str,
};

pub mod all_to_silk {
    use crate::AdapterResult;

    pub fn all_to_silk(input: &[u8]) -> AdapterResult<Vec<u8>> {
        crate::host::all_to_silk(input)
    }
}

pub fn str_msg_to_arr(js: &serde_json::Value) -> AdapterResult<serde_json::Value> {
    redreply_common::mytool::str_msg_to_arr(js).map_err(|error| error.to_string().into())
}

pub fn json_to_cq_str(js: &serde_json::Value) -> AdapterResult<String> {
    redreply_common::mytool::json_to_cq_str(js).map_err(|error| error.to_string().into())
}
