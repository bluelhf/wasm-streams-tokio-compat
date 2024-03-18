use std::io::{Error, ErrorKind};
use js_sys::Object;
use js_sys::wasm_bindgen::{JsValue, UnwrapThrowExt};
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::{ReadableStreamGetReaderOptions};

/// Utilities taken from the `wasm-streams` crate.

pub(crate) fn js_to_string(js_value: &JsValue) -> Option<String> {
    js_value.as_string().or_else(|| {
        Object::try_from(js_value)
            .map(|js_object| js_object.to_string().as_string().unwrap_throw())
    })
}

pub(crate) fn js_to_io_error(js_value: &JsValue) -> Error {
    let message = js_to_string(js_value).unwrap_or_else(|| "Unknown error".to_string());
    Error::new(ErrorKind::Other, message)
}

#[wasm_bindgen]
extern "C" {
    /// Additional methods for [`ReadableStream`](web_sys::ReadableStream).
    #[wasm_bindgen(js_name = ReadableStream, typescript_type = "ReadableStream")]
    pub(crate) type ReadableStreamExt;

    #[wasm_bindgen(method, catch, js_class = ReadableStream, js_name = getReader)]
    pub(crate) fn try_get_reader_with_options(
        this: &ReadableStreamExt,
        options: &ReadableStreamGetReaderOptions,
    ) -> Result<Object, js_sys::Error>;
}