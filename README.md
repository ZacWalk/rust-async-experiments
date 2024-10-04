# Rust asynchronous file I/O wrapper for Windows

This experiment confirms Windows Async IO is compatible with Rust's standard Async patterns. I used overlapped file IO. BindIoCompletionCallback is used to have a callback trigger the waker. This pattern also works with Windows sockets and http.sys.

```Rust
// 64K buffer on the stack but could also be on the heap via box.
// No heap allocations or buffer copying in this example.
let mut buf = [0u8; 1024 * 64];

// Callback allowing processing of data in buf.
let callback = |bytes_read: &[u8]| {
    println!("transferred {} bytes", bytes_read.len());
};

let file = AsyncFile::open_for_read("C:/windows/explorer.exe").await?;

// Reads the entire file in chunks based on the buffer size.
// Only the supplied buffer is used, meaning you must process the data in the callback. 
// The buffer is subsequently overwritten with the next chunk.
let bytes_read = file.read_all(&mut buf, callback).await?;

println!("Complete {} bytes", bytes_read);

file.close()?;
```

Don't use this as is. Just proof of concept. Needs a lot more testing and error checking.