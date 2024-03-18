//! Provides compatibility tools for converting between `wasm_streams` streams and Tokio `AsyncRead` and `AsyncWrite`.
//!
//! # Examples
//! ```rust
//! use tokio::io::AsyncReadExt;
//! use wasm_streams::ReadableStream;
//! use wasm_streams_tokio_compat::ReadCompat;
//!
//! async fn read(file: web_sys::File) {
//!     let mut stream: ReadableStream = ReadableStream::from_raw(file.stream());
//! 
//!     // We use compat_byob() for reading in bring-your-own-buffer (BYOB) mode, which allows for direct copying
//!     // from the stream's internal buffer to the user's buffer. If the stream does not support BYOB, this will fail.
//!     // In such cases, you can use `compat_default()` instead, which has the stream allocate the buffer for you.
//!     let mut reader = stream.compat_byob().unwrap();
//!
//!     // Now we can read from `reader` using regular Tokio methods, as it is an `AsyncRead`.
//!
//!     let mut buf = [0; 14];
//!     reader.read_exact(&mut buf).await.unwrap();
//!     assert_eq!(buf, *b"Hello, world!\n");
//! }
//! ```
//!
//! ```rust
//! use std::io;
//! use tokio::io::AsyncWriteExt;
//! use wasm_streams_tokio_compat::Compat;
//!
//! async fn write(raw_stream: web_sys::WritableStream, content: Box<[u8]>) -> io::Result<()> {
//!     let mut stream: wasm_streams::WritableStream = wasm_streams::WritableStream::from_raw(raw_stream);
//!     let mut writer = stream.compat();
//!
//!     // Now we can write to `writer` using regular Tokio methods, as it is an `AsyncWrite`.
//!     writer.write_all(&content).await
//! }
//! ```
mod util;

pub trait Compat<'stream> {
    type Compat;
    fn compat(&'stream mut self) -> Self::Compat;
}

pub trait ReadCompat<'stream> {
    type Compat;
    fn compat_default(&'stream mut self) -> Self::Compat;
    fn compat_byob(&'stream mut self) -> Self::Compat;
}

impl<'stream> ReadCompat<'stream> for wasm_streams::ReadableStream {
    type Compat = Result<IntoTokioRead<'stream>, js_sys::Error>;

    fn compat_default(&'stream mut self) -> Self::Compat {
        match self.try_get_reader() {
            Ok(reader) => Ok(IntoTokioRead::new(ReadableStreamReader::Default(reader), true)),
            Err(err) => Err(err),
        }
    }

    fn compat_byob(&'stream mut self) -> Self::Compat {
        match self.try_get_byob_reader() {
            Ok(reader) => Ok(IntoTokioRead::new(ReadableStreamReader::Byob(reader), true)),
            Err(err) => Err(err),
        }
    }
}



impl<'stream> Compat<'stream> for wasm_streams::WritableStream {
    type Compat = IntoTokioWrite<'stream>;

    fn compat(&'stream mut self) -> Self::Compat {
        IntoTokioWrite::new(self.get_writer(), true)
    }
}



use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use futures_util::FutureExt;
use js_sys::Uint8Array;
use js_sys::wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use wasm_streams::writable::WritableStreamDefaultWriter;
use crate::util::js_to_io_error;

#[derive(Debug)]
pub struct IntoTokioWrite<'stream> {
    writer: Option<WritableStreamDefaultWriter<'stream>>,
    write_fut: Option<JsFuture>,
    close_fut: Option<JsFuture>,
    cancel_on_drop: bool,
}

impl<'stream> IntoTokioWrite<'stream> {
    #[inline]
    #[must_use]
    fn new(writer: WritableStreamDefaultWriter, cancel_on_drop: bool) -> IntoTokioWrite {
        IntoTokioWrite {
            writer: Some(writer),
            write_fut: None,
            close_fut: None,
            cancel_on_drop,
        }
    }

    /// [Aborts](https://streams.spec.whatwg.org/#abort-a-writable-stream) the stream,
    /// signaling that the producer can no longer successfully write to the stream.
    /// # Errors
    /// If the stream is currently locked to a writer, then this returns an error.
    pub async fn abort(mut self) -> Result<(), JsValue> {
        match self.writer.take() {
            Some(mut writer) => writer.abort().await,
            None => Ok(()),
        }
    }

    /// [Aborts](https://streams.spec.whatwg.org/#abort-a-writable-stream) the stream,
    /// signaling that the producer can no longer successfully write to the stream.
    /// # Errors
    /// If the stream is currently locked to a writer, then this returns an error.
    pub async fn abort_with_reason(mut self, reason: &JsValue) -> Result<(), JsValue> {
        match self.writer.take() {
            Some(mut writer) => writer.abort_with_reason(reason).await,
            None => Ok(()),
        }
    }

    #[inline]
    fn discard_writer(mut self: Pin<&mut Self>) {
        self.writer = None;
    }
}

impl<'stream> tokio::io::AsyncWrite for IntoTokioWrite<'stream> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let ready_fut = match self.write_fut.as_mut() {
            Some(fut) => fut,
            None => match &self.writer {
                Some(writer) => {
                    let fut = JsFuture::from(writer.as_raw().write_with_chunk(&Uint8Array::from(buf)));
                    self.write_fut.insert(fut)
                }
                None => {
                    // Writer was already dropped
                    return Poll::Ready(Ok(0));
                }
            },
        };

        // Poll the ready future
        let js_result = futures_util::ready!(ready_fut.poll_unpin(cx));
        self.write_fut = None;

        // Ready future completed
        Poll::Ready(match js_result {
            Ok(js_value) => {
                debug_assert!(js_value.is_undefined());
                Ok(buf.len())
            }
            Err(js_value) => {
                // Error, drop writer
                self.discard_writer();
                Err(js_to_io_error(&js_value))
            }
        })
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let Some(write_fut) = self.write_fut.as_mut() else {
            // If we're not writing, then there's nothing to flush
            return Poll::Ready(Ok(()));
        };

        // Poll the write future
        let js_result = futures_util::ready!(write_fut.poll_unpin(cx));
        self.write_fut = None;

        // Write future completed
        Poll::Ready(match js_result {
            Ok(js_value) => {
                debug_assert!(js_value.is_undefined());
                Ok(())
            }
            Err(js_value) => {
                // Error, drop writer
                self.writer = None;
                Err(js_to_io_error(&js_value))
            }
        })
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let close_fut = match self.close_fut.as_mut() {
            Some(fut) => fut,
            None => match &self.writer {
                Some(writer) => {
                    // There is no pending close future
                    // Start closing the stream and create future from close promise
                    let fut = JsFuture::from(writer.as_raw().close());
                    self.close_fut.insert(fut)
                }
                None => {
                    // Writer was already dropped
                    // TODO Return error?
                    return Poll::Ready(Ok(()));
                }
            },
        };

        // Poll the close future
        let js_result = futures_util::ready!(close_fut.poll_unpin(cx));
        self.close_fut = None;

        // Close future completed
        self.writer = None;
        Poll::Ready(match js_result {
            Ok(js_value) => {
                debug_assert!(js_value.is_undefined());
                Ok(())
            }
            Err(js_value) => Err(js_to_io_error(&js_value)),
        })
    }
}

impl<'stream> Drop for IntoTokioWrite<'stream> {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            if let Some(writer) = self.writer.take() {
                let _ = writer.as_raw().abort();
            }
        }
    }
}

use std::task::ready;
use tokio::io::{AsyncRead, ReadBuf};
use js_sys::Object;
use js_sys::Reflect;
use js_sys::wasm_bindgen::JsCast;
use wasm_streams::readable::{ReadableStreamBYOBReader, ReadableStreamDefaultReader};

#[derive(Debug)]
pub(crate) enum ReadableStreamReader<'stream> {
    Default(ReadableStreamDefaultReader<'stream>),
    Byob(ReadableStreamBYOBReader<'stream>),
}

impl<'stream> ReadableStreamReader<'stream> {
    pub async fn cancel(&mut self) -> Result<(), JsValue> {
        match self {
            ReadableStreamReader::Default(default) => default.cancel().await,
            ReadableStreamReader::Byob(byob) => byob.cancel().await,
        }
    }

    pub async fn cancel_with_reason(&mut self, reason: &JsValue) -> Result<(), JsValue> {
        match self {
            ReadableStreamReader::Default(default) => default.cancel_with_reason(reason).await,
            ReadableStreamReader::Byob(byob) => byob.cancel_with_reason(reason).await,
        }
    }
}

#[must_use = "readers do nothing unless polled"]
#[derive(Debug)]
pub struct IntoTokioRead<'stream> {
    reader: Option<ReadableStreamReader<'stream>>,
    buffer: Option<Uint8Array>,
    fut: Option<JsFuture>,
    cancel_on_drop: bool,
}

impl<'stream> IntoTokioRead<'stream> {
    #[inline]
    fn new(reader: ReadableStreamReader, cancel_on_drop: bool) -> IntoTokioRead {
        IntoTokioRead {
            reader: Some(reader),
            buffer: None,
            fut: None,
            cancel_on_drop,
        }
    }

    /// [Cancels](https://streams.spec.whatwg.org/#cancel-a-readable-stream) the stream,
    /// signaling a loss of interest in the stream by a consumer.
    ///
    /// Equivalent to [`ReadableStream.cancel`](web_sys::ReadableStream::cancel).
    /// # Errors
    /// Returns an error if the stream is not a `ReadableStream` or if the stream is locked.
    pub async fn cancel(mut self) -> Result<(), JsValue> {
        match self.reader.take() {
            Some(mut reader) => reader.cancel().await,
            None => Ok(()),
        }
    }

    /// [Cancels](https://streams.spec.whatwg.org/#cancel-a-readable-stream) the stream,
    /// signaling a loss of interest in the stream by a consumer.
    ///
    /// Equivalent to [`ReadableStream.cancel_with_reason`](web_sys::ReadableStream::cancel_with_reason).
    /// # Errors
    /// Returns an error if the stream is not a `ReadableStream` or if the stream is locked.
    pub async fn cancel_with_reason(mut self, reason: &JsValue) -> Result<(), JsValue> {
        match self.reader.take() {
            Some(mut reader) => reader.cancel_with_reason(reason).await,
            None => Ok(()),
        }
    }

    #[inline]
    fn discard_reader(mut self: Pin<&mut Self>) {
        self.reader = None;
        self.buffer = None;
    }
}

impl<'stream> AsyncRead for IntoTokioRead<'stream> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let read_fut = if let Some(fut) = self.fut.as_mut() { fut} else {
            let buf_len = u32::try_from(buf.capacity()).unwrap_or(u32::MAX);
            let buffer = match self.buffer.take() {
                Some(buffer) if buffer.byte_length() >= buf_len => buffer,
                _ => Uint8Array::new_with_length(buf_len),
            };

            let buffer = buffer.subarray(0, buf_len).unchecked_into::<Object>();

            match &self.reader {
                Some(reader) => {
                    let fut = match reader {
                        ReadableStreamReader::Default(reader) => {
                            JsFuture::from(reader.as_raw().read())
                        }
                        ReadableStreamReader::Byob(reader) => {
                            JsFuture::from(reader.as_raw().read_with_array_buffer_view(&buffer))
                        }
                    };

                    self.fut.insert(fut)
                }
                None => return Poll::Ready(Ok(())),
            }
        };

        let js_result = ready!(read_fut.poll_unpin(cx));
        self.fut = None;

        Poll::Ready(match js_result {
            Ok(js_value) => {
                if Reflect::get(&js_value, &"done".into()).unwrap().is_truthy() {
                    self.discard_reader();
                    Ok(())
                } else {

                    let filled_view = Reflect::get(&js_value, &"value".into())
                        .unwrap().unchecked_into::<Uint8Array>();

                    let filled_len = filled_view.byte_length() as usize;
                    filled_view.copy_to(buf.initialize_unfilled_to(filled_len));
                    buf.advance(filled_len);

                    self.buffer = Some(Uint8Array::new(&filled_view.buffer()));
                    Ok(())
                }
            }
            Err(js_value) => {
                self.discard_reader();
                Err(js_to_io_error(&js_value))
            }
        })
    }
}

impl<'stream> Drop for IntoTokioRead<'stream> {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            if let Some(reader) = self.reader.take() {
                let _ = match reader {
                    ReadableStreamReader::Default(reader) => reader.as_raw().cancel(),
                    ReadableStreamReader::Byob(reader) => reader.as_raw().cancel()
                };
            }
        }
    }
}