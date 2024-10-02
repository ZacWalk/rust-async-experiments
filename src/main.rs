use std::fs::File;
use std::future::Future;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use tokio::io::Result;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_IO_PENDING, HANDLE};
use windows::Win32::Storage::FileSystem::{
    ReadFile, ReadFileEx, WriteFile, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED,
};
use windows::Win32::System::Threading::CreateEventW;
use windows::Win32::System::IO::OVERLAPPED;

// Asynchronous file I/O wrapper for Windows
struct AsyncFile {
    file: File,
    overlapped: OVERLAPPED,
    waker: Option<Waker>,
}

impl AsyncFile {
    // Open a file asynchronously
    async fn open(path: &str, write_mode: bool) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(write_mode)
            .custom_flags(FILE_FLAG_OVERLAPPED.0)
            .open(path);

        if file.is_err() {
            return Err(std::io::Error::last_os_error().into());
        }

        let mut overlapped = OVERLAPPED::default();

        Ok(Self {
            file: file.unwrap(),
            overlapped,
            waker: None,
        })
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        AsyncFileRead { file: self, buf, reading: false }.await
    }

    // Close the file
    fn close(self) -> Result<()> {
        unsafe {
            if CloseHandle(HANDLE(self.file.as_raw_handle())).is_err() {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(())
    }
}

// Future for asynchronous file reading
struct AsyncFileRead<'a> {
    file: &'a mut AsyncFile,
    buf: &'a mut [u8],
    reading: bool,
}

unsafe extern "system" fn completion_callback(
    dwErrorCode: u32,
    dwNumberOfBytesTransfered: u32,
    lpOverlapped: *mut OVERLAPPED,
) {
    println!("completion_callback");

    if dwErrorCode == 0 {
        println!("Wake");

        // I/O operation completed successfully
        let overlapped = &mut *lpOverlapped;

        // Get the waker from the OVERLAPPED structure (you'll need to store it there)
        let waker: &Waker = std::mem::transmute(overlapped.hEvent);

        // Wake up the waker
        waker.wake_by_ref();
    } else {
        // Handle the error (e.g., log it)
        println!("Error in completion_callback: {}", dwErrorCode);
    }
}

impl<'a> Future for AsyncFileRead<'a> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let file = &mut *this.file;
        let buf_borrow = &mut this.buf;
        let overlapped_borrow = &mut file.overlapped as *mut _;

        if this.reading {
            // Handle the existing buffer;
            let bytes_read = unsafe { file.overlapped.InternalHigh };
            // Use GetOverlappedResult to determine if reading is complete
        }

        // Initiate asynchronous read operation
        println!("ReadFileEx");
        let result = unsafe {
            ReadFileEx(
                HANDLE(file.file.as_raw_handle()),
                Some(buf_borrow),
                overlapped_borrow,
                Some(completion_callback),
            )
        };

        if result.is_ok() {
            let bytes_read = unsafe { file.overlapped.InternalHigh };
            println!("ReadFileEx synchronous read  {}", bytes_read);
            Poll::Ready(Ok(bytes_read as usize))
        } else {
            let error = unsafe { GetLastError() };
            if error == ERROR_IO_PENDING {
                println!("pending");
                file.overlapped.hEvent = unsafe { std::mem::transmute(cx.waker()) };
                file.waker = Some(cx.waker().clone());
                this.reading = true;
                Poll::Pending
            } else {
                // Read operation failed
                Poll::Ready(Err(std::io::Error::from_raw_os_error(error.0 as i32).into()))
            }
        }
    }
}

// Future for asynchronous file writing

impl Drop for AsyncFile {
    fn drop(&mut self) {
        // Wake up any pending tasks when the file is dropped
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Open");
    let mut file = AsyncFile::open("C:/windows/explorer.exe", false).await?;

    println!("Read");
    let mut buf = Box::new([0u8; 1024 * 64]);
    let bytes_read = file.read(&mut (*buf)[..]).await?;
    println!("Read {} bytes", bytes_read);

    file.close()?;
    Ok(())
}
