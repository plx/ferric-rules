//! Tests for copy-to-buffer error APIs (Pass 006).

use crate::engine::{ferric_engine_free, ferric_engine_last_error_copy, ferric_engine_new};
use crate::error::{
    clear_global_error, ferric_last_error_global_copy, set_global_error, FerricError,
};
use std::os::raw::c_char;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read the first `n` bytes of a raw buffer as a Rust `&str`.
///
/// # Safety
///
/// `buf` must point to at least `n` bytes of initialised memory.
unsafe fn buf_as_str(buf: *const c_char, n: usize) -> &'static str {
    let slice = std::slice::from_raw_parts(buf.cast::<u8>(), n);
    std::str::from_utf8(slice).expect("buffer should contain valid UTF-8")
}

/// Allocate a zeroed byte buffer of `n` bytes on the heap.
fn make_buf(n: usize) -> Vec<u8> {
    vec![0u8; n]
}

// ---------------------------------------------------------------------------
// Tests for `ferric_last_error_global_copy`
// ---------------------------------------------------------------------------

#[test]
fn global_copy_null_out_len() {
    // out_len is null → InvalidArgument (nothing else is touched).
    set_global_error("irrelevant".to_string());
    unsafe {
        let mut buf = make_buf(64);
        let result = ferric_last_error_global_copy(
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(result, FerricError::InvalidArgument);
    }
    clear_global_error();
}

#[test]
fn global_copy_no_error_stored() {
    // No error in channel → NotFound, *out_len = 0.
    clear_global_error();
    unsafe {
        let mut buf = make_buf(64);
        let mut out_len: usize = 999;
        let result = ferric_last_error_global_copy(
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::NotFound);
        assert_eq!(out_len, 0);
    }
}

#[test]
fn global_copy_size_query() {
    // buf=null, buf_len=0 → Ok (size query), *out_len = message.len() + 1.
    let msg = "size query message";
    set_global_error(msg.to_string());
    unsafe {
        let mut out_len: usize = 0;
        let result = ferric_last_error_global_copy(std::ptr::null_mut(), 0, &mut out_len);
        assert_eq!(result, FerricError::Ok, "size query should return Ok");
        assert_eq!(
            out_len,
            msg.len() + 1,
            "*out_len should be message length + NUL"
        );
    }
    clear_global_error();
}

#[test]
fn global_copy_null_buf_nonzero_len() {
    // buf=null but buf_len=10 → InvalidArgument (nonsensical combination).
    let msg = "hello";
    set_global_error(msg.to_string());
    unsafe {
        let mut out_len: usize = 0;
        let result = ferric_last_error_global_copy(std::ptr::null_mut(), 10, &mut out_len);
        assert_eq!(result, FerricError::InvalidArgument);
        assert_eq!(
            out_len,
            msg.len() + 1,
            "*out_len should still report needed size"
        );
    }
    clear_global_error();
}

#[test]
fn global_copy_exact_fit() {
    // buf_len == needed → Ok, buffer contains full message + NUL terminator.
    let msg = "exact fit";
    let needed = msg.len() + 1;
    set_global_error(msg.to_string());
    unsafe {
        let mut buf = make_buf(needed);
        let mut out_len: usize = 0;
        let result =
            ferric_last_error_global_copy(buf.as_mut_ptr().cast::<c_char>(), needed, &mut out_len);
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out_len, needed);
        // Last byte is NUL, preceding bytes match the message.
        assert_eq!(buf[msg.len()], 0, "NUL terminator missing");
        assert_eq!(&buf[..msg.len()], msg.as_bytes());
    }
    clear_global_error();
}

#[test]
fn global_copy_oversized_buffer() {
    // buf_len > needed → Ok, *out_len = needed (not buf_len).
    let msg = "short";
    let needed = msg.len() + 1;
    set_global_error(msg.to_string());
    unsafe {
        let mut buf = make_buf(128);
        let mut out_len: usize = 0;
        let result = ferric_last_error_global_copy(
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::Ok);
        assert_eq!(
            out_len, needed,
            "*out_len should equal needed, not buf capacity"
        );
        assert_eq!(buf[msg.len()], 0, "NUL terminator missing");
        assert_eq!(&buf[..msg.len()], msg.as_bytes());
    }
    clear_global_error();
}

#[test]
fn global_copy_truncation() {
    // buf_len = 5, message is longer → BufferTooSmall, *out_len = needed,
    // buffer holds 4 bytes of the message then a NUL terminator.
    let msg = "truncated message here";
    let buf_len = 5usize;
    let needed = msg.len() + 1;
    set_global_error(msg.to_string());
    unsafe {
        let mut buf = make_buf(buf_len);
        let mut out_len: usize = 0;
        let result =
            ferric_last_error_global_copy(buf.as_mut_ptr().cast::<c_char>(), buf_len, &mut out_len);
        assert_eq!(result, FerricError::BufferTooSmall);
        assert_eq!(out_len, needed, "*out_len should report full needed size");
        // buf_len - 1 = 4 bytes copied, then NUL
        assert_eq!(&buf[..4], &msg.as_bytes()[..4]);
        assert_eq!(buf[4], 0, "NUL terminator missing");
        // Verify the copied portion is valid UTF-8 / matches expected prefix
        let prefix = buf_as_str(buf.as_ptr().cast::<c_char>(), 4);
        assert_eq!(prefix, &msg[..4]);
    }
    clear_global_error();
}

#[test]
fn global_copy_one_byte_buffer() {
    // buf_len = 1 → BufferTooSmall; buffer[0] = NUL terminator (0 bytes of message).
    let msg = "something";
    let needed = msg.len() + 1;
    set_global_error(msg.to_string());
    unsafe {
        let mut buf = make_buf(1);
        let mut out_len: usize = 0;
        let result =
            ferric_last_error_global_copy(buf.as_mut_ptr().cast::<c_char>(), 1, &mut out_len);
        assert_eq!(result, FerricError::BufferTooSmall);
        assert_eq!(out_len, needed);
        assert_eq!(buf[0], 0, "single-byte buffer should contain NUL");
    }
    clear_global_error();
}

#[test]
fn global_copy_zero_len_nonnull_buf() {
    // buf_len = 0 with a non-null buffer → BufferTooSmall (nothing written).
    let msg = "cannot fit";
    let needed = msg.len() + 1;
    set_global_error(msg.to_string());
    unsafe {
        // Allocate a real buffer but pass len=0 so the function must not touch it.
        let mut buf = make_buf(64);
        // Pre-fill with sentinel so we can detect any unwanted writes.
        buf.fill(0xAB);
        let mut out_len: usize = 0;
        let result =
            ferric_last_error_global_copy(buf.as_mut_ptr().cast::<c_char>(), 0, &mut out_len);
        assert_eq!(result, FerricError::BufferTooSmall);
        assert_eq!(out_len, needed);
        // Buffer must be untouched.
        assert!(
            buf.iter().all(|&b| b == 0xAB),
            "buffer should not be written when buf_len=0"
        );
    }
    clear_global_error();
}

#[test]
fn global_copy_after_clear() {
    // Set an error, clear it, then attempt copy → NotFound.
    set_global_error("will be cleared".to_string());
    clear_global_error();
    unsafe {
        let mut buf = make_buf(64);
        let mut out_len: usize = 999;
        let result = ferric_last_error_global_copy(
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::NotFound);
        assert_eq!(out_len, 0);
    }
}

// ---------------------------------------------------------------------------
// Tests for `ferric_engine_last_error_copy`
// ---------------------------------------------------------------------------

#[test]
fn engine_copy_null_out_len() {
    // out_len is null → InvalidArgument.
    unsafe {
        let engine = ferric_engine_new();
        let mut buf = make_buf(64);
        let result = ferric_engine_last_error_copy(
            engine,
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(result, FerricError::InvalidArgument);
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_copy_null_engine() {
    // Null engine pointer → NullPointer, *out_len = 0.
    unsafe {
        let mut out_len: usize = 999;
        let mut buf = make_buf(64);
        let result = ferric_engine_last_error_copy(
            std::ptr::null(),
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::NullPointer);
        assert_eq!(out_len, 0);
    }
}

#[test]
fn engine_copy_no_error() {
    // Fresh engine with no per-engine error set → NotFound, *out_len = 0.
    unsafe {
        let engine = ferric_engine_new();
        let mut buf = make_buf(64);
        let mut out_len: usize = 999;
        let result = ferric_engine_last_error_copy(
            engine,
            buf.as_mut_ptr().cast::<c_char>(),
            buf.len(),
            &mut out_len,
        );
        assert_eq!(result, FerricError::NotFound);
        assert_eq!(out_len, 0);
        ferric_engine_free(engine);
    }
}

#[test]
fn engine_copy_size_query() {
    // Set a per-engine error, perform size query (buf=null, buf_len=0) → Ok, *out_len = needed.
    let msg = "per-engine size query";
    unsafe {
        let engine = ferric_engine_new();
        // Set the per-engine error directly (pub(crate) field accessible within crate).
        (*engine).error_state.set(msg.to_string());

        let mut out_len: usize = 0;
        let result = ferric_engine_last_error_copy(engine, std::ptr::null_mut(), 0, &mut out_len);
        assert_eq!(result, FerricError::Ok, "size query should return Ok");
        assert_eq!(out_len, msg.len() + 1);

        ferric_engine_free(engine);
    }
}

#[test]
fn engine_copy_exact_fit() {
    // Set a per-engine error, copy with exactly the right buffer size → Ok.
    let msg = "engine error msg";
    let needed = msg.len() + 1;
    unsafe {
        let engine = ferric_engine_new();
        (*engine).error_state.set(msg.to_string());

        let mut buf = make_buf(needed);
        let mut out_len: usize = 0;
        let result = ferric_engine_last_error_copy(
            engine,
            buf.as_mut_ptr().cast::<c_char>(),
            needed,
            &mut out_len,
        );
        assert_eq!(result, FerricError::Ok);
        assert_eq!(out_len, needed);
        assert_eq!(buf[msg.len()], 0, "NUL terminator missing");
        assert_eq!(&buf[..msg.len()], msg.as_bytes());

        ferric_engine_free(engine);
    }
}

#[test]
fn engine_copy_truncation() {
    // Set a per-engine error, copy into a buffer that is too small → BufferTooSmall.
    let msg = "this engine error is too long to fit";
    let buf_len = 8usize;
    let needed = msg.len() + 1;
    unsafe {
        let engine = ferric_engine_new();
        (*engine).error_state.set(msg.to_string());

        let mut buf = make_buf(buf_len);
        let mut out_len: usize = 0;
        let result = ferric_engine_last_error_copy(
            engine,
            buf.as_mut_ptr().cast::<c_char>(),
            buf_len,
            &mut out_len,
        );
        assert_eq!(result, FerricError::BufferTooSmall);
        assert_eq!(out_len, needed, "*out_len should report full needed size");
        // buf_len - 1 = 7 message bytes then NUL
        assert_eq!(&buf[..buf_len - 1], &msg.as_bytes()[..buf_len - 1]);
        assert_eq!(buf[buf_len - 1], 0, "NUL terminator missing");

        ferric_engine_free(engine);
    }
}
