//! C-facing value types, conversion helpers, and resource management.

use std::ffi::{c_void, CString};
use std::os::raw::c_char;
use std::ptr;

use ferric_core::{ConflictResolutionStrategy, StringEncoding};
use ferric_runtime::{Engine, EngineConfig};

/// C-facing string-encoding configuration for `FerricConfig`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricStringEncoding {
    Ascii = 0,
    Utf8 = 1,
    AsciiSymbolsUtf8Strings = 2,
}

impl From<FerricStringEncoding> for StringEncoding {
    fn from(value: FerricStringEncoding) -> Self {
        match value {
            FerricStringEncoding::Ascii => Self::Ascii,
            FerricStringEncoding::Utf8 => Self::Utf8,
            FerricStringEncoding::AsciiSymbolsUtf8Strings => Self::AsciiSymbolsUtf8Strings,
        }
    }
}

impl FerricStringEncoding {
    /// Integer discriminant used in `FerricConfig`.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self as u32
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Ascii),
            1 => Some(Self::Utf8),
            2 => Some(Self::AsciiSymbolsUtf8Strings),
            _ => None,
        }
    }
}

impl TryFrom<u32> for FerricStringEncoding {
    type Error = String;

    fn try_from(raw: u32) -> Result<Self, Self::Error> {
        Self::from_raw(raw)
            .ok_or_else(|| format!("invalid string_encoding value: {raw} (expected 0..=2)"))
    }
}

/// C-facing conflict-resolution strategy for `FerricConfig`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricConflictStrategy {
    Depth = 0,
    Breadth = 1,
    Lex = 2,
    Mea = 3,
}

impl From<FerricConflictStrategy> for ConflictResolutionStrategy {
    fn from(value: FerricConflictStrategy) -> Self {
        match value {
            FerricConflictStrategy::Depth => Self::Depth,
            FerricConflictStrategy::Breadth => Self::Breadth,
            FerricConflictStrategy::Lex => Self::Lex,
            FerricConflictStrategy::Mea => Self::Mea,
        }
    }
}

impl FerricConflictStrategy {
    /// Integer discriminant used in `FerricConfig`.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self as u32
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Depth),
            1 => Some(Self::Breadth),
            2 => Some(Self::Lex),
            3 => Some(Self::Mea),
            _ => None,
        }
    }
}

impl TryFrom<u32> for FerricConflictStrategy {
    type Error = String;

    fn try_from(raw: u32) -> Result<Self, Self::Error> {
        Self::from_raw(raw).ok_or_else(|| format!("invalid strategy value: {raw} (expected 0..=3)"))
    }
}

/// C-facing engine configuration used by `ferric_engine_new_with_config`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FerricConfig {
    /// Raw `FerricStringEncoding` discriminant.
    pub string_encoding: u32,
    /// Raw `FerricConflictStrategy` discriminant.
    pub strategy: u32,
    pub max_call_depth: usize,
}

impl Default for FerricConfig {
    fn default() -> Self {
        Self {
            string_encoding: FerricStringEncoding::Utf8.as_raw(),
            strategy: FerricConflictStrategy::Depth.as_raw(),
            max_call_depth: 64,
        }
    }
}

impl TryFrom<&FerricConfig> for EngineConfig {
    type Error = String;

    fn try_from(config: &FerricConfig) -> Result<Self, Self::Error> {
        let string_encoding = FerricStringEncoding::try_from(config.string_encoding)?;
        let strategy = FerricConflictStrategy::try_from(config.strategy)?;

        Ok(Self {
            string_encoding: string_encoding.into(),
            strategy: strategy.into(),
            max_call_depth: config.max_call_depth,
        })
    }
}

/// Convert a C-facing config into runtime `EngineConfig`.
pub(crate) fn engine_config_from_ffi(config: &FerricConfig) -> Result<EngineConfig, String> {
    EngineConfig::try_from(config)
}

/// C-facing value type discriminant.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricValueType {
    Void = 0,
    Integer = 1,
    Float = 2,
    Symbol = 3,
    String = 4,
    Multifield = 5,
    ExternalAddress = 6,
}

/// C-facing value representation.
///
/// ## Ownership
///
/// - `string_ptr`: when non-null, is a heap-allocated NUL-terminated string.
///   The caller must free it with `ferric_string_free` or `ferric_value_free`.
/// - `multifield_ptr`: when non-null, is a heap-allocated array of `FerricValue`s.
///   The caller must free it with `ferric_value_free` (which recursively frees elements)
///   or `ferric_value_array_free`.
/// - `external_pointer`: NOT owned by `FerricValue`. Lifetime is caller-managed.
///
/// ## Active Fields by Type
///
/// | `value_type` | Active fields |
/// |---|---|
/// | Void | (none) |
/// | Integer | `integer` |
/// | Float | `float` |
/// | Symbol | `string_ptr` (owned) |
/// | String | `string_ptr` (owned) |
/// | Multifield | `multifield_ptr` (owned), `multifield_len` |
/// | ExternalAddress | `external_type_id`, `external_pointer` |
#[repr(C)]
pub struct FerricValue {
    pub value_type: FerricValueType,
    pub integer: i64,
    pub float: f64,
    pub string_ptr: *mut c_char,
    pub multifield_ptr: *mut FerricValue,
    pub multifield_len: usize,
    pub external_type_id: u32,
    pub external_pointer: *mut c_void,
}

impl FerricValue {
    /// Create a void value with all fields zeroed/null.
    #[must_use]
    pub const fn void() -> Self {
        Self {
            value_type: FerricValueType::Void,
            integer: 0,
            float: 0.0,
            string_ptr: ptr::null_mut(),
            multifield_ptr: ptr::null_mut(),
            multifield_len: 0,
            external_type_id: 0,
            external_pointer: ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// Rust-to-C value conversion
// ---------------------------------------------------------------------------

use ferric_core::Value;

/// Convert a Rust `Value` to a C-facing `FerricValue`.
///
/// Heap-allocates strings (for Symbol/String variants) and arrays (for Multifield).
/// The caller owns the resulting `FerricValue` and must free it with
/// `ferric_value_free` or the type-specific free functions.
pub(crate) fn value_to_ferric(value: &Value, engine: &Engine) -> FerricValue {
    match value {
        Value::Integer(i) => FerricValue {
            value_type: FerricValueType::Integer,
            integer: *i,
            ..FerricValue::void()
        },
        Value::Float(f) => FerricValue {
            value_type: FerricValueType::Float,
            float: *f,
            ..FerricValue::void()
        },
        Value::Symbol(sym) => {
            let name = engine.resolve_symbol(*sym).unwrap_or("<unknown>");
            let cstring = CString::new(name).unwrap_or_default();
            FerricValue {
                value_type: FerricValueType::Symbol,
                string_ptr: cstring.into_raw(),
                ..FerricValue::void()
            }
        }
        Value::String(s) => {
            let cstring = CString::new(s.as_str()).unwrap_or_default();
            FerricValue {
                value_type: FerricValueType::String,
                string_ptr: cstring.into_raw(),
                ..FerricValue::void()
            }
        }
        Value::Multifield(mf) => {
            let values: Vec<FerricValue> = mf.iter().map(|v| value_to_ferric(v, engine)).collect();
            let len = values.len();
            let ptr = if values.is_empty() {
                ptr::null_mut()
            } else {
                let boxed = values.into_boxed_slice();
                let raw = Box::into_raw(boxed);
                raw.cast::<FerricValue>()
            };
            FerricValue {
                value_type: FerricValueType::Multifield,
                multifield_ptr: ptr,
                multifield_len: len,
                ..FerricValue::void()
            }
        }
        Value::ExternalAddress(ea) => FerricValue {
            value_type: FerricValueType::ExternalAddress,
            external_type_id: ea.type_id.0,
            external_pointer: ea.pointer,
            ..FerricValue::void()
        },
        Value::Void => FerricValue::void(),
    }
}

// ---------------------------------------------------------------------------
// C API: Resource management
// ---------------------------------------------------------------------------

/// Free a heap-allocated C string returned by the FFI.
///
/// Null pointers are safely ignored.
///
/// # Safety
///
/// - `ptr` must be a pointer returned by an FFI function or null.
/// - The pointer must not have been freed already.
#[no_mangle]
pub unsafe extern "C" fn ferric_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// Free a `FerricValue` and its owned resources.
///
/// Recursively frees owned strings and multifield arrays.
/// Null pointers are safely ignored.
///
/// # Safety
///
/// - `value` must point to a valid `FerricValue` or be null.
/// - Any owned resources (`string_ptr`, `multifield_ptr`) must not have been freed already.
#[no_mangle]
pub unsafe extern "C" fn ferric_value_free(value: *mut FerricValue) {
    if value.is_null() {
        return;
    }
    let val = &*value;
    free_value_resources(val);
}

/// Free an array of `FerricValue`s and all their owned resources.
///
/// Frees each element's owned resources, then frees the array allocation.
/// Null pointers are safely ignored.
///
/// # Safety
///
/// - `arr` must point to a contiguous array of `len` `FerricValue`s, or be null.
/// - The array must have been allocated by the FFI.
#[no_mangle]
pub unsafe extern "C" fn ferric_value_array_free(arr: *mut FerricValue, len: usize) {
    if arr.is_null() || len == 0 {
        return;
    }
    // Free each element's owned resources
    for i in 0..len {
        let elem = &*arr.add(i);
        free_value_resources(elem);
    }
    // Free the array allocation itself
    let slice = std::slice::from_raw_parts_mut(arr, len);
    drop(Box::from_raw(slice as *mut [FerricValue]));
}

/// Internal: free owned resources inside a `FerricValue` without freeing the struct itself.
///
/// # Safety
///
/// - `val` must point to a valid `FerricValue`.
/// - Any owned resources referenced by `val` must not have been freed already.
unsafe fn free_value_resources(val: &FerricValue) {
    match val.value_type {
        FerricValueType::Symbol | FerricValueType::String => {
            if !val.string_ptr.is_null() {
                drop(CString::from_raw(val.string_ptr));
            }
        }
        FerricValueType::Multifield => {
            if !val.multifield_ptr.is_null() && val.multifield_len > 0 {
                for i in 0..val.multifield_len {
                    let elem = &*val.multifield_ptr.add(i);
                    free_value_resources(elem);
                }
                let slice = std::slice::from_raw_parts_mut(val.multifield_ptr, val.multifield_len);
                drop(Box::from_raw(slice as *mut [FerricValue]));
            }
        }
        FerricValueType::Void
        | FerricValueType::Integer
        | FerricValueType::Float
        | FerricValueType::ExternalAddress => {}
    }
}
