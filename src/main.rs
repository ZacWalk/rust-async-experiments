use std::fs::File;
use std::future::Future;
use std::io::{self, Result};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use windows::core::Error;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, HANDLE, STATUS_END_OF_FILE, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{ReadFile, FILE_FLAG_OVERLAPPED};
use windows::Win32::System::IO::{BindIoCompletionCallback, OVERLAPPED};

// Asynchronous file I/O wrapper for Windows
struct AsyncFile {
    file: File,
}

#[repr(C)]
pub struct OverlappedWrap {
    o: OVERLAPPED,
    len: u32,
    err: u32,
    waker: Option<Arc<Mutex<Waker>>>,
}

impl Default for OverlappedWrap {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlappedWrap {
    pub fn new() -> Self {
        OverlappedWrap {
            o: OVERLAPPED::default(),
            waker: None,
            err: 0,
            len: 0,
        }
    }
}

unsafe extern "system" fn private_callback(
    dwerrorcode: u32,
    dwnumberofbytestransfered: u32,
    lpoverlapped: *mut OVERLAPPED,
) {
    let wrap_ptr: *mut OverlappedWrap = lpoverlapped as *mut OverlappedWrap;
    let wrap: &mut OverlappedWrap = &mut *wrap_ptr;
    wrap.err = dwerrorcode;
    wrap.len = dwnumberofbytestransfered;
    wrap.waker.as_mut().unwrap().lock().unwrap().clone().wake();
}

impl AsyncFile {
    async fn open_for_read(path: &str) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OVERLAPPED.0)
            .open(path)?;

        unsafe {
            BindIoCompletionCallback(HANDLE(file.as_raw_handle()), Some(private_callback), 0)
        }?;

        Ok(Self { file })
    }

    async fn read(&mut self, buf: &mut [u8], callback: Option<Box<dyn Fn(usize)>>) -> Result<usize> {
        AsyncFileReadFuture {
            file: self,
            buf,
            overlapped: OverlappedWrap::default(),
            offset: 0,
            callback,
        }
        .await
    }

    fn close(self) -> Result<()> {
        unsafe {
            if CloseHandle(HANDLE(self.file.as_raw_handle())).is_err() {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(())
    }
}

struct AsyncFileReadFuture<'a> {
    file: &'a mut AsyncFile,
    buf: &'a mut [u8],
    overlapped: OverlappedWrap,
    offset: u64,
    callback: Option<Box<dyn Fn(usize)>>,
}

impl<'a> Future for AsyncFileReadFuture<'a> {
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let file = &mut *this.file;

        if this.overlapped.err == STATUS_END_OF_FILE.0 as u32 {
            // End of file
            return Poll::Ready(Ok(this.offset as usize));
        }

        let e = Error::from(WIN32_ERROR(this.overlapped.err));
        if e.code().is_err() {
            println!("Error {:x}", e.code().0);
            return Poll::Ready(Err(io::Error::from_raw_os_error(e.code().0 as i32)));
        }

        if this.overlapped.len != 0 {
            let bytes_transferred = this.overlapped.len;

            if let Some(callback) = this.callback.as_mut() {
                callback(bytes_transferred as usize);
            }
            
            this.offset += bytes_transferred as u64;
            this.overlapped.o.Anonymous.Anonymous.Offset = this.offset as u32;
            this.overlapped.o.Anonymous.Anonymous.OffsetHigh = (this.offset >> 32) as u32;
            this.overlapped.len = 0;
        }

        let mut bytes_read = 0;
        let result = unsafe {
            ReadFile(
                HANDLE(file.file.as_raw_handle()),
                Some(this.buf),
                Some(&mut bytes_read),
                Some(&mut this.overlapped.o),
            )
        };

        if result.is_ok() {
            if let Some(callback) = this.callback.as_mut() {
                callback(bytes_read as usize);
            }
            Poll::Ready(Ok(bytes_read as usize))
        } else {
            let error = unsafe { GetLastError() };
            if error == ERROR_IO_PENDING {
                this.overlapped.waker = Some(Arc::new(Mutex::new(cx.waker().clone())));
                Poll::Pending
            } else {
                // Read operation failed
                println!("Error {:x}", error.0);
                Poll::Ready(Err(io::Error::from_raw_os_error(error.0 as i32)))
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {

    let mut buf = Box::new([0u8; 1024 * 64]);

    // Called every time there is data to process in the supplied buffer.
    let callback = Box::new(|bytes_transferred: usize| {
        println!("transferred {} bytes", bytes_transferred);
    });

    let mut file = AsyncFile::open_for_read("C:/windows/explorer.exe").await?;
    let bytes_read = file.read(&mut (*buf)[..], Some(callback)).await?;

    println!("Complete {} bytes", bytes_read);

    file.close()?;
    Ok(())
}
