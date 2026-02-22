# Pass 007: FFI Extended API Value And Query Surface

## Objective

Deliver the extended embedding surface beyond core run/assert/retract, including value marshalling and owned-resource helpers.

## Scope

- C-facing value/result representations.
- Additional query/call helpers needed for host integrations.
- Allocated-resource free helpers and ownership-safe conversions.

## Tasks

1. Introduce/complete C-facing value structs (`FerricValue`, `FerricValueArray`, IDs/config structs) and conversion utilities.
2. Implement extended query/call APIs needed for host integrations (fact/global reads, callable invocation, string/value extraction) with stable diagnostics.
3. Implement and document allocated-resource release helpers (`ferric_string_free`, `ferric_value_array_free`) and null-safe semantics.
4. Ensure module/global ambiguity and visibility errors remain runtime-authored (no adapter-level reinterpretation).
5. Add round-trip tests for value conversion, ownership boundaries, and failure diagnostics on invalid usage.

## Definition Of Done

- Extended FFI surface supports practical host-language wrapper work.
- Ownership-sensitive APIs are explicit, safe, and test-backed.
- Extended calls preserve Phase 4 module/global diagnostic contracts.

## Verification Commands

- `cargo test -p ferric-ffi value`
- `cargo test -p ferric-ffi ownership`
- `cargo check -p ferric-ffi`

## Handoff State

- FFI provides both core and extended operations with stable value/ownership behavior.
