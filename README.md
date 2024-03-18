<img alt="cute kitten" src=".repo/kitten.png" align="right" height=200 style="border-radius: .5rem">

# wasm-streams-tokio-compat

<sup>**wasm-streams** **compat**ibility for **tokio**.</sup>

Provides compatibility tools for converting between [`wasm_streams`](https://docs.rs/wasm-streams/latest/wasm_streams/) streams and Tokio [`AsyncRead`](https://docs.rs/tokio/latest/tokio/io/trait.AsyncRead.html) and [`AsyncWrite`](https://docs.rs/tokio/latest/tokio/io/trait.AsyncWrite.html).

# Examples
```rust
use tokio::io::AsyncReadExt;
use wasm_streams::ReadableStream;
use wasm_streams_tokio_compat::ReadCompat;

async fn read(file: web_sys::File) {
    let mut stream: ReadableStream = ReadableStream::from_raw(file.stream());

    // We use compat_byob() for reading in bring-your-own-buffer (BYOB) mode, which allows for direct copying
    // from the stream's internal buffer to the user's buffer. If the stream does not support BYOB, this will fail.
    // In such cases, you can use `compat_default()` instead, which has the stream allocate the buffer for you.
    let mut reader = stream.compat_byob().unwrap();

    // Now we can read from `reader` using regular Tokio methods, as it is an `AsyncRead`.

    let mut buf = [0; 14];
    reader.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, *b"Hello, world!\n");
}
```

```rust
use std::io;
use tokio::io::AsyncWriteExt;
use wasm_streams_tokio_compat::Compat;

async fn write(raw_stream: web_sys::WritableStream, content: Box<[u8]>) -> io::Result<()> {
    let mut stream: wasm_streams::WritableStream = wasm_streams::WritableStream::from_raw(raw_stream);
    let mut writer = stream.compat();

    // Now we can write to `writer` using regular Tokio methods, as it is an `AsyncWrite`.
    writer.write_all(&content).await
}
```