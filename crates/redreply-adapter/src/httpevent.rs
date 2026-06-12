use std::collections::HashMap;

pub fn get_params_from_uri(uri: &hyper::Uri) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(query) = uri.query() {
        for (key, val) in url::form_urlencoded::parse(query.as_bytes()) {
            params.insert(key.into_owned(), val.into_owned());
        }
    }
    params
}

