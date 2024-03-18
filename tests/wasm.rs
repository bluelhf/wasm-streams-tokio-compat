use std::sync::{Arc, Mutex};

#[wasm_bindgen]
pub struct ByteSource {
    #[wasm_bindgen(skip)]
    pub data: Vec<Vec<u8>>,
    pub is_byob: bool,
}

impl ByteSource {
    pub fn new(data: Vec<Vec<u8>>, is_byob: bool) -> Self {
        Self { data, is_byob }
    }
}

#[wasm_bindgen]
impl ByteSource {

    pub fn pull(&mut self, controller: web_sys::ReadableStreamDefaultController) {
        if self.data.is_empty() {
            let _ = controller.close();
            return;
        }
        
        controller.enqueue_with_chunk(&**Uint8Array::from(&self.data.remove(0)[..])).unwrap();
    }

    #[wasm_bindgen(getter = type)]
    pub fn get_type(&self) -> Option<String> {
        if self.is_byob { Some("bytes".to_string()) } else { None }
    }
}

impl TryInto<ReadableStream> for ByteSource {
    type Error = js_sys::Error;

    fn try_into(self) -> Result<ReadableStream, Self::Error> {
        Ok(ReadableStream::from_raw(web_sys::ReadableStream::new_with_underlying_source(&JsValue::from(self).into())?))
    }
}

#[wasm_bindgen]
pub struct ByteSink {
    #[wasm_bindgen(skip)]
    pub data: Arc<Mutex<Vec<u8>>>
}

#[wasm_bindgen]
impl ByteSink {
    pub fn write(&mut self, chunk: Uint8Array, _controller: web_sys::WritableStreamDefaultController) {
        let mut slice = vec![0; chunk.length() as usize];
        chunk.copy_to(&mut slice);
        self.data.try_lock().unwrap().extend(slice.into_iter());
    }
}

impl TryInto<WritableStream> for ByteSink {
    type Error = js_sys::Error;

    fn try_into(self) -> Result<WritableStream, Self::Error> {
        Ok(WritableStream::from_raw(web_sys::WritableStream::new_with_underlying_sink(&JsValue::from(self).into())?))
    }
}

use js_sys::{Uint8Array};
use js_sys::wasm_bindgen::JsValue;
use js_sys::wasm_bindgen::prelude::wasm_bindgen;
use wasm_streams::ReadableStream;
use wasm_streams::WritableStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use wasm_bindgen_test::wasm_bindgen_test;
use wasm_streams_tokio_compat::{Compat, ReadCompat};


#[wasm_bindgen_test]
async fn test_read_byob_on_byob() {
    let source = ByteSource::new(vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]], true);

    let mut stream: ReadableStream = source.try_into().unwrap();
    let mut reader = stream.compat_byob().unwrap();

    let mut buf = [0; 9];
    reader.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
}

#[wasm_bindgen_test]
async fn test_read_byob_on_default_fail() {
    let source = ByteSource::new(vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]], false);

    let mut stream: ReadableStream = source.try_into().unwrap();
    stream.compat_byob().unwrap_err();
}

#[wasm_bindgen_test]
async fn test_read_default_on_byob() {
    let source = ByteSource::new(vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]], true);

    let mut stream: ReadableStream = source.try_into().unwrap();
    let mut reader = stream.compat_default().unwrap();

    let mut buf = [0; 9];
    reader.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
}

#[wasm_bindgen_test]
async fn test_read_default_on_default() {
    let source = ByteSource::new(vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]], false);

    let mut stream: ReadableStream = source.try_into().unwrap();
    let mut reader = stream.compat_default().unwrap();

    let mut buf = [0; 9];
    reader.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
}

#[wasm_bindgen_test]
async fn test_write() {
    let original = Arc::new(Mutex::new(Vec::new()));
    let sink = ByteSink { data: original.clone() };

    let mut stream: WritableStream = sink.try_into().unwrap();
    let mut writer = stream.compat();
    writer.write_all(&[1, 2, 3, 4, 5, 6, 7, 8, 9]).await.unwrap();
    assert_eq!(original.try_lock().unwrap().as_slice(), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
}
