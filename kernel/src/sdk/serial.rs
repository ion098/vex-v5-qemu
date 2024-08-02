//! USB Serial Communication

use core::{
    ffi::{c_char, VaList},
    fmt,
};

use crate::Mutex;
use heapless::spsc::Queue;
use semihosting::io::{stderr, Write};
use snafu::{OptionExt, Snafu};
use vexide_core::{
    io::{Cursor, Stdin},
    sync::LazyLock,
};

mod util {
    use core::cmp;
    use vexide_core::io::{Cursor, Write};

    use super::SerialError;

    /// Non-resizing write implementation from no_std_io
    #[inline]
    fn slice_write(
        pos_mut: &mut u64,
        slice: &mut [u8],
        buf: &[u8],
    ) -> vexide_core::io::Result<usize> {
        let pos = cmp::min(*pos_mut, slice.len() as u64);
        let amt = (&mut slice[(pos as usize)..]).write(buf)?;
        *pos_mut += amt as u64;
        Ok(amt)
    }

    /// Utility function to write to a cursor of fixed size
    pub fn cursor_write<const N: usize>(
        cursor: &mut Cursor<[u8; N]>,
        buf: &[u8],
    ) -> vexide_core::io::Result<usize> {
        let mut pos = cursor.position();
        let written = slice_write(&mut pos, cursor.get_mut(), buf)?;
        cursor.set_position(pos);
        Ok(written)
    }

    pub fn cursor_write_all<const N: usize>(
        cursor: &mut Cursor<[u8; N]>,
        mut buf: &[u8],
    ) -> vexide_core::io::Result<()> {
        while !buf.is_empty() {
            match cursor_write(cursor, buf) {
                Ok(0) => {
                    return Err(SerialError::io_full_buffer());
                }
                Ok(n) => buf = &buf[n..],
                Err(ref e) if e.kind() == vexide_core::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

pub static SERIAL: LazyLock<Serial> = LazyLock::new(Serial::new);

#[derive(Debug, Snafu)]
pub enum SerialError {
    /// An invalid channel was provided.
    #[snafu(display("{channel} is not a valid serial channel"))]
    InvalidChannel {
        /// The invalid channel.
        channel: u32,
    },
    /// Stdio is not supported on this CPU architecture.
    StdioNotSupported,
    /// An I/O error occurred.
    #[snafu(display("An I/O error occurred: {inner}"))]
    Io { inner: vexide_core::io::Error },
    /// An I/O error occurred while flushing stdout.
    #[snafu(display("An I/O error occurred while flushing stdout: {inner}"))]
    Flush { inner: semihosting::io::Error },
}

impl SerialError {
    pub fn io_full_buffer() -> vexide_core::io::Error {
        vexide_core::io::Error::new(
            vexide_core::io::ErrorKind::WriteZero,
            "failed to write whole buffer",
        )
    }
}

type Result<T> = core::result::Result<T, SerialError>;

/// Serial driver based on ARM semihosting stdio operations.
pub struct Serial {
    stdout_buffer: Mutex<Cursor<[u8; vexide_core::io::Stdout::INTERNAL_BUFFER_SIZE]>>,
    // FIXME: capacity is actually N-1 instead of N
    // However using a non-power-of-two N is slower.
    stdin_buffer: Mutex<Queue<u8, { Stdin::STDIN_BUFFER_SIZE }>>,
}

impl Default for Serial {
    fn default() -> Self {
        Self::new()
    }
}

impl Serial {
    pub fn new() -> Self {
        Self {
            stdout_buffer: Mutex::new(Cursor::new(
                [0; vexide_core::io::Stdout::INTERNAL_BUFFER_SIZE],
            )),
            stdin_buffer: Mutex::new(Queue::new()),
        }
    }

    /// Write to the serial output buffer, returning the number of bytes written.
    pub fn write(&self, channel: u32, buffer: &[u8]) -> Result<usize> {
        match channel {
            1 => util::cursor_write(&mut self.stdout_buffer.lock(), buffer)
                .map_err(|inner| IoSnafu { inner }.build()),
            channel => InvalidChannelSnafu { channel }.fail(),
        }
    }

    /// Attempt to write all bytes to the serial output buffer, failing if the buffer becomes full.
    pub fn write_all(&self, channel: u32, buffer: &[u8]) -> Result<()> {
        match channel {
            1 => util::cursor_write_all(&mut self.stdout_buffer.lock(), buffer)
                .map_err(|inner| IoSnafu { inner }.build()),
            channel => InvalidChannelSnafu { channel }.fail(),
        }
    }

    /// Add bytes to the end of the serial input buffer, failing if the buffer becomes full.
    pub fn buffer_input(&self, channel: u32, buffer: &[u8]) -> Result<()> {
        match channel {
            1 => {
                let mut stdin_buffer = self.stdin_buffer.lock();
                for &byte in buffer {
                    stdin_buffer.enqueue(byte).map_err(|_| {
                        IoSnafu {
                            inner: SerialError::io_full_buffer(),
                        }
                        .build()
                    })?;
                }
                Ok(())
            }
            channel => InvalidChannelSnafu { channel }.fail(),
        }
    }

    /// Take the next byte from the serial input buffer if available.
    pub fn read_byte(&self, channel: u32) -> Result<Option<u8>> {
        match channel {
            1 => {
                let mut stdin_buffer = self.stdin_buffer.lock();
                Ok(stdin_buffer.dequeue())
            }
            channel => InvalidChannelSnafu { channel }.fail(),
        }
    }

    /// Peek the next byte from the serial input buffer if available.
    pub fn peek_byte(&self, channel: u32) -> Result<Option<u8>> {
        match channel {
            1 => {
                let stdin_buffer = self.stdin_buffer.lock();
                Ok(stdin_buffer.peek().copied())
            }
            channel => InvalidChannelSnafu { channel }.fail(),
        }
    }

    /// Get the number of free bytes in the serial output buffer.
    pub fn num_free_bytes(&self, channel: u32) -> Result<usize> {
        match channel {
            1 => Ok(vexide_core::io::Stdout::INTERNAL_BUFFER_SIZE
                - self.stdout_buffer.lock().position() as usize),
            channel => InvalidChannelSnafu { channel }.fail(),
        }
    }

    /// Flush the serial output buffer.
    pub fn flush(&self) -> Result<()> {
        let mut stdout_buffer = self.stdout_buffer.lock();
        if stdout_buffer.position() == 0 {
            return Ok(());
        }
        let mut stdout = stderr().ok().context(StdioNotSupportedSnafu)?;
        let old_buffer = core::mem::replace(
            &mut *stdout_buffer,
            Cursor::new([0; vexide_core::io::Stdout::INTERNAL_BUFFER_SIZE]),
        );
        let len = old_buffer.position() as usize;
        let bytes = &old_buffer.into_inner()[0..len];
        stdout
            .write_all(bytes)
            .map_err(|inner| FlushSnafu { inner }.build())?;
        Ok(())
    }
}

impl core::fmt::Write for &Serial {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_all(1, s.as_bytes())
            .map_err(|_| core::fmt::Error)?;
        Ok(())
    }
}

pub fn vexSerialWriteChar(channel: u32, c: u8) -> i32 {
    match SERIAL.write(channel, &[c]) {
        Ok(n) => i32::try_from(n).unwrap(),
        Err(_) => -1,
    }
}

/// # Safety
///
/// - `data` must be a valid pointer to a buffer of length `data_len`.
pub unsafe fn vexSerialWriteBuffer(channel: u32, data: *const u8, data_len: u32) -> i32 {
    let data = unsafe { core::slice::from_raw_parts(data, data_len as usize) };

    match SERIAL.write(channel, data) {
        Ok(n) => i32::try_from(n).unwrap(),
        Err(_) => -1,
    }
}
pub fn vexSerialReadChar(channel: u32) -> i32 {
    Default::default()
}
pub fn vexSerialPeekChar(channel: u32) -> i32 {
    Default::default()
}
pub fn vexSerialWriteFree(channel: u32) -> i32 {
    SERIAL
        .num_free_bytes(channel)
        .map(|n| i32::try_from(n).unwrap())
        .unwrap_or(-1)
}

/// # Safety
///
/// - `format` must be a valid printf format string for the given `args`
pub unsafe fn vex_vprintf(format: *const c_char, args: VaList<'_, '_>) -> i32 {
    // Safety: Caller guarantees `format` is a valid printf format string for the given `args`.
    unsafe {
        printf_compat::format(
            format as *const u8,
            args,
            printf_compat::output::fmt_write(&mut &*SERIAL),
        )
    }
}

/// # Safety
///
/// - `format` must be a valid printf format string for the given `args`
/// - `out` must be a valid pointer to a buffer of sufficient length
pub unsafe fn vex_vsprintf(out: *mut c_char, format: *const c_char, args: VaList<'_, '_>) -> i32 {
    struct PtrFmtWrite {
        ptr: *mut u8,
    }
    impl fmt::Write for PtrFmtWrite {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let len = s.len();
            // Safety: Caller guarantees that `self.ptr` is a valid pointer to a buffer of
            // sufficient length. `s` originates from kernel code and thus does not
            // overlap with `self.ptr`.
            unsafe {
                core::ptr::copy_nonoverlapping(s.as_ptr(), self.ptr, len);
                self.ptr = self.ptr.add(len);
            }
            Ok(())
        }
    }
    let mut writer = PtrFmtWrite {
        ptr: out as *mut u8,
    };
    unsafe {
        let res = printf_compat::format(
            format as *const u8,
            args,
            printf_compat::output::fmt_write(&mut writer),
        );
        // add null terminator to C-string
        writer.ptr.write(0);
        res
    }
}

/// # Safety
///
/// - `format` must be a valid printf format string for the given `args`
/// - `out` must be a valid pointer to a buffer of length `max_len` or more
pub unsafe fn vex_vsnprintf(
    out: *mut c_char,
    max_len: u32,
    format: *const c_char,
    args: VaList<'_, '_>,
) -> i32 {
    struct PtrFmtWrite {
        ptr: *mut u8,
        remaining_len: usize,
    }
    impl fmt::Write for PtrFmtWrite {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let len = s.len();
            let len = core::cmp::min(len, self.remaining_len);
            // if we run out of room, we write as much as we can and return an error
            let out_of_room = s.len() > self.remaining_len;

            // Safety: Caller guarantees that `self.ptr` is a valid pointer to a buffer of
            // sufficient length. `s` originates from kernel code and thus does not
            // overlap with `self.ptr`.
            unsafe {
                core::ptr::copy_nonoverlapping(s.as_ptr(), self.ptr, len);
                self.ptr = self.ptr.add(len);
                self.remaining_len -= len;
            }

            if out_of_room {
                Err(fmt::Error)
            } else {
                Ok(())
            }
        }
    }
    let mut writer = PtrFmtWrite {
        ptr: out as *mut u8,
        remaining_len: max_len as usize - 1, // reserve space for null terminator
    };
    let result = unsafe {
        printf_compat::format(
            format as *const u8,
            args,
            printf_compat::output::fmt_write(&mut writer),
        )
    };
    // add null terminator to C-string
    unsafe {
        writer.ptr.write(0);
    }
    result
}
