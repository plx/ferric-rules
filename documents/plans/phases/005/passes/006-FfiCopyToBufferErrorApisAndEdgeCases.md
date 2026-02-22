# Pass 006: FFI Copy-To-Buffer Error APIs And Edge Cases

## Objective

Implement and lock the copy-to-buffer error APIs with exact contract compliance for wrapper authors.

## Scope

- `ferric_last_error_global_copy` and `ferric_engine_last_error_copy` semantics.
- Query-size, truncation, invalid-argument, and no-error precedence behavior.
- Contract-focused test coverage.

## Tasks

1. Implement copy APIs with the documented no-error-first precedence (`FERRIC_ERROR_NOT_FOUND` + `*out_len = 0`).
2. Implement size-query path (`buf = NULL`, `buf_len = 0`) returning required size including NUL when an error exists.
3. Implement truncation behavior and `FERRIC_ERROR_BUFFER_TOO_SMALL` signaling with full-message length reporting.
4. Validate zero-length and one-byte-buffer edge cases (`buf_len == 0`, `buf_len == 1`) and pointer combinations.
5. Add exhaustive table-driven tests matching each documented branch in Section 11.4.1.

## Definition Of Done

- Copy-to-buffer APIs match the documented contract branch-for-branch.
- Wrapper authors can reliably detect truncation and retry using `out_len`.
- Edge-case behavior is regression-protected.

## Verification Commands

- `cargo test -p ferric-ffi copy_error`
- `cargo test -p ferric-ffi ffi_contract`
- `cargo check -p ferric-ffi`

## Handoff State

- FFI error reporting now supports robust wrapper-friendly buffer semantics.
