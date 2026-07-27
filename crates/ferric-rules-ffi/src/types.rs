//! C-facing value types, conversion helpers, and resource management.

use std::ffi::{c_void, CString};
use std::os::raw::c_char;
use std::ptr;

use std::ffi::CStr;

use ferric_rules_core::{ConflictResolutionStrategy, StringEncoding};
use ferric_rules_runtime::{Engine, EngineConfig, HaltReason};

use crate::error::{set_global_error, FerricError};

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

        let mut runtime_config =
            Self::from(ferric_rules_core::StringEncoding::from(string_encoding))
                .with_strategy(strategy.into());
        runtime_config.max_call_depth = config.max_call_depth;
        Ok(runtime_config)
    }
}

/// Convert a C-facing config into runtime `EngineConfig`.
pub(crate) fn engine_config_from_ffi(config: &FerricConfig) -> Result<EngineConfig, String> {
    EngineConfig::try_from(config)
}

/// C-facing fact type discriminant.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricFactType {
    Ordered = 0,
    Template = 1,
}

/// C-facing halt reason returned by `ferric_engine_run_ex`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FerricHaltReason {
    AgendaEmpty = 0,
    LimitReached = 1,
    HaltRequested = 2,
    ActionError = 3,
}

impl From<HaltReason> for FerricHaltReason {
    fn from(reason: HaltReason) -> Self {
        match reason {
            HaltReason::AgendaEmpty => Self::AgendaEmpty,
            HaltReason::LimitReached => Self::LimitReached,
            HaltReason::HaltRequested => Self::HaltRequested,
            HaltReason::ActionError => Self::ActionError,
        }
    }
}

/// C-facing value type discriminant.
///
/// Crosses the ABI as a raw `u32` (`FerricValue::value_type`), never as this
/// Rust enum: caller-populated memory is validated with [`Self::from_raw`] /
/// `TryFrom<u32>` before being interpreted, and unknown discriminants are
/// rejected with `FERRIC_ERROR_INVALID_ARGUMENT`.
///
/// Stable numeric values — new variants may be added but existing values
/// must never change.
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

impl FerricValueType {
    /// Integer discriminant used in `FerricValue::value_type`.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self as u32
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Void),
            1 => Some(Self::Integer),
            2 => Some(Self::Float),
            3 => Some(Self::Symbol),
            4 => Some(Self::String),
            5 => Some(Self::Multifield),
            6 => Some(Self::ExternalAddress),
            _ => None,
        }
    }
}

impl TryFrom<u32> for FerricValueType {
    type Error = String;

    fn try_from(raw: u32) -> Result<Self, Self::Error> {
        Self::from_raw(raw)
            .ok_or_else(|| format!("invalid value_type discriminant: {raw} (expected 0..=6)"))
    }
}

/// C-facing value representation.
///
/// ## Ownership
///
/// Ownership is contextual:
///
/// - Values returned by Ferric APIs and constructors are Ferric-owned.
///   Their non-null `string_ptr` and `multifield_ptr` fields must be released
///   with `ferric_value_free` (or the matching type-specific Ferric free API).
/// - Values passed to structured assertion APIs or
///   `ferric_value_multifield_copy` are borrowed for that call. Ferric neither
///   retains nor frees any part of the caller-provided tree.
/// - Only Ferric-owned values and arrays may be passed to
///   `ferric_value_free` and `ferric_value_array_free`.
/// - `external_pointer`: NOT owned by `FerricValue`. Lifetime is caller-managed.
///
/// ## Active Fields by Type
///
/// | `value_type` | Active fields |
/// |---|---|
/// | Void | (none) |
/// | Integer | `integer` |
/// | Float | `float` |
/// | Symbol | `string_ptr` |
/// | String | `string_ptr` |
/// | Multifield | `multifield_ptr`, `multifield_len` |
/// | ExternalAddress | `external_type_id`, `external_pointer` |
#[repr(C)]
pub struct FerricValue {
    /// Raw `FerricValueType` discriminant.
    ///
    /// Every API that reads a caller-populated `FerricValue` validates this
    /// field before interpreting it; values outside the documented
    /// `FerricValueType` range are rejected with
    /// `FERRIC_ERROR_INVALID_ARGUMENT`.
    pub value_type: u32,
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
            value_type: FerricValueType::Void.as_raw(),
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

/// Maximum recursively nested multifield levels accepted by the copy
/// constructor. This keeps both construction and the matching recursive free
/// path within a bounded stack depth.
const MAX_MULTIFIELD_COPY_DEPTH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BorrowedValueRange {
    start: usize,
    end: usize,
}

impl BorrowedValueRange {
    fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// RAII guard for Ferric-owned values that have not yet been transferred to a
/// caller-visible multifield. A validation failure at any later element drops
/// the guard and recursively releases every successful earlier copy.
struct OwnedFerricValues(Vec<FerricValue>);

impl OwnedFerricValues {
    fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }

    fn push(&mut self, value: FerricValue) {
        self.0.push(value);
    }

    fn into_multifield(mut self) -> FerricValue {
        let values = std::mem::take(&mut self.0);
        let len = values.len();
        let multifield_ptr = if values.is_empty() {
            ptr::null_mut()
        } else {
            Box::into_raw(values.into_boxed_slice()).cast::<FerricValue>()
        };
        FerricValue {
            value_type: FerricValueType::Multifield.as_raw(),
            multifield_ptr,
            multifield_len: len,
            ..FerricValue::void()
        }
    }
}

impl Drop for OwnedFerricValues {
    fn drop(&mut self) {
        for value in &self.0 {
            // Values in this guard were constructed below and therefore have
            // valid tags and exclusively Ferric-owned recursive resources.
            let result = unsafe { free_value_resources(value) };
            debug_assert!(
                result.is_ok(),
                "internally constructed FerricValue must be freeable: {result:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Rust-to-C value conversion
// ---------------------------------------------------------------------------

use ferric_rules_core::Value;

/// Convert a Rust `Value` to a C-facing `FerricValue`.
///
/// Heap-allocates strings (for Symbol/String variants) and arrays (for Multifield).
/// The caller owns the resulting `FerricValue` and must free it with
/// `ferric_value_free` or the type-specific free functions.
pub(crate) fn value_to_ferric(value: &Value, engine: &Engine) -> FerricValue {
    match value {
        Value::Integer(i) => FerricValue {
            value_type: FerricValueType::Integer.as_raw(),
            integer: *i,
            ..FerricValue::void()
        },
        Value::Float(f) => FerricValue {
            value_type: FerricValueType::Float.as_raw(),
            float: *f,
            ..FerricValue::void()
        },
        Value::Symbol(sym) => {
            let name = engine.resolve_symbol(*sym).unwrap_or("<unknown>");
            let cstring = CString::new(name).unwrap_or_default();
            FerricValue {
                value_type: FerricValueType::Symbol.as_raw(),
                string_ptr: cstring.into_raw(),
                ..FerricValue::void()
            }
        }
        Value::String(s) => {
            let cstring = CString::new(s.as_str()).unwrap_or_default();
            FerricValue {
                value_type: FerricValueType::String.as_raw(),
                string_ptr: cstring.into_raw(),
                ..FerricValue::void()
            }
        }
        Value::Multifield(mf) => {
            let values: Vec<FerricValue> = mf.iter().map(|v| value_to_ferric(v, engine)).collect();
            OwnedFerricValues(values).into_multifield()
        }
        Value::ExternalAddress(ea) => FerricValue {
            value_type: FerricValueType::ExternalAddress.as_raw(),
            external_type_id: ea.type_id.0,
            external_pointer: ea.pointer,
            ..FerricValue::void()
        },
        Value::Void => FerricValue::void(),
    }
}

// ---------------------------------------------------------------------------
// C-to-Rust value conversion
// ---------------------------------------------------------------------------

/// Convert a C-facing `FerricValue` to a Rust `Value`.
///
/// For Symbol and String types, the `string_ptr` is read as a NUL-terminated
/// C string. For Symbol, the string is interned via the engine's symbol table.
///
/// The raw `value_type` discriminant is validated before interpretation;
/// unknown discriminants (including in nested multifield elements) produce an
/// `Err`, which callers surface as `FERRIC_ERROR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// - `fv` must be a valid `FerricValue` with active fields matching `value_type`.
/// - `string_ptr` (for Symbol/String) must be a valid NUL-terminated string.
/// - `multifield_ptr` (for Multifield) must point to `multifield_len` valid `FerricValue`s.
pub(crate) unsafe fn ferric_to_value(
    fv: &FerricValue,
    engine: &mut Engine,
) -> Result<Value, String> {
    match FerricValueType::try_from(fv.value_type)? {
        FerricValueType::Void => Ok(Value::Void),
        FerricValueType::Integer => Ok(Value::Integer(fv.integer)),
        FerricValueType::Float => Ok(Value::Float(fv.float)),
        FerricValueType::Symbol => {
            if fv.string_ptr.is_null() {
                return Err("symbol string_ptr is null".to_string());
            }
            let name = CStr::from_ptr(fv.string_ptr)
                .to_str()
                .map_err(|e| format!("symbol is not valid UTF-8: {e}"))?;
            let sym = engine.intern_symbol(name).map_err(|e| e.to_string())?;
            Ok(Value::Symbol(sym))
        }
        FerricValueType::String => {
            if fv.string_ptr.is_null() {
                return Err("string string_ptr is null".to_string());
            }
            let s = CStr::from_ptr(fv.string_ptr)
                .to_str()
                .map_err(|e| format!("string is not valid UTF-8: {e}"))?;
            let fs = engine.create_string(s).map_err(|e| e.to_string())?;
            Ok(Value::String(fs))
        }
        FerricValueType::Multifield => {
            if fv.multifield_len == 0 {
                return Ok(Value::Multifield(Box::new(
                    ferric_rules_core::Multifield::new(),
                )));
            }
            if fv.multifield_ptr.is_null() {
                return Err("multifield_ptr is null with non-zero length".to_string());
            }
            let mut mf = ferric_rules_core::Multifield::new();
            for i in 0..fv.multifield_len {
                let elem = &*fv.multifield_ptr.add(i);
                mf.push(ferric_to_value(elem, engine)?);
            }
            Ok(Value::Multifield(Box::new(mf)))
        }
        FerricValueType::ExternalAddress => {
            Err("ExternalAddress cannot be converted from FFI".to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// C API: Value construction helpers
// ---------------------------------------------------------------------------

/// Create an integer `FerricValue`.
#[no_mangle]
pub extern "C" fn ferric_value_integer(value: i64) -> FerricValue {
    FerricValue {
        value_type: FerricValueType::Integer.as_raw(),
        integer: value,
        ..FerricValue::void()
    }
}

/// Create a float `FerricValue`.
#[no_mangle]
pub extern "C" fn ferric_value_float(value: f64) -> FerricValue {
    FerricValue {
        value_type: FerricValueType::Float.as_raw(),
        float: value,
        ..FerricValue::void()
    }
}

/// Create a symbol `FerricValue` with a heap-copied string.
///
/// Returns a void value if `name` is null. The caller owns the
/// `string_ptr` and must free it with `ferric_value_free`.
///
/// # Safety
///
/// - `name` must be a valid NUL-terminated string, or null.
#[no_mangle]
pub unsafe extern "C" fn ferric_value_symbol(name: *const c_char) -> FerricValue {
    if name.is_null() {
        return FerricValue::void();
    }
    let cstr = CStr::from_ptr(name);
    let cstring = CString::new(cstr.to_bytes()).unwrap_or_default();
    FerricValue {
        value_type: FerricValueType::Symbol.as_raw(),
        string_ptr: cstring.into_raw(),
        ..FerricValue::void()
    }
}

/// Create a string `FerricValue` with a heap-copied string.
///
/// Returns a void value if `s` is null. The caller owns the
/// `string_ptr` and must free it with `ferric_value_free`.
///
/// # Safety
///
/// - `s` must be a valid NUL-terminated string, or null.
#[no_mangle]
pub unsafe extern "C" fn ferric_value_string(s: *const c_char) -> FerricValue {
    if s.is_null() {
        return FerricValue::void();
    }
    let cstr = CStr::from_ptr(s);
    let cstring = CString::new(cstr.to_bytes()).unwrap_or_default();
    FerricValue {
        value_type: FerricValueType::String.as_raw(),
        string_ptr: cstring.into_raw(),
        ..FerricValue::void()
    }
}

/// Create a void `FerricValue` with all fields zeroed/null.
#[no_mangle]
pub extern "C" fn ferric_value_void() -> FerricValue {
    FerricValue::void()
}

/// Deep-copy a borrowed array of values into one Ferric-owned multifield.
///
/// The input array and its complete nested value tree are borrowed for the
/// duration of this call. Ferric never retains or frees caller-provided
/// arrays or strings. On success, `*out_value` owns independent copies of
/// every Symbol, String, and Multifield allocation and must be released with
/// `ferric_value_free`. External-address payload pointers are copied shallowly
/// and remain caller-owned.
///
/// `elements == null` with `len == 0` constructs an empty multifield.
/// Unknown tags, null active string pointers, null non-empty multifield
/// pointers, cyclic or ancestor-overlapping array storage, and nesting deeper
/// than 128 levels return `FERRIC_ERROR_INVALID_ARGUMENT`. A null required
/// pointer returns `FERRIC_ERROR_NULL_POINTER`.
///
/// When `out_value` is non-null, it is overwritten with Void before any input
/// validation and remains Void on every failure. Every partial Ferric-owned
/// copy is released before an error is returned.
///
/// # Safety
///
/// - `out_value` must point to writable storage for one `FerricValue`, must
///   not overlap the borrowed input tree, and must not currently contain live
///   Ferric-owned resources.
/// - If `len > 0`, `elements` must point to `len` aligned, initialized
///   `FerricValue`s.
/// - Every active nested string pointer must address a NUL-terminated byte
///   string, and every active nested multifield pointer must address its
///   declared number of initialized `FerricValue`s.
/// - The complete input tree must remain readable and unchanged until this
///   function returns.
#[no_mangle]
pub unsafe extern "C" fn ferric_value_multifield_copy(
    elements: *const FerricValue,
    len: usize,
    out_value: *mut FerricValue,
) -> FerricError {
    if out_value.is_null() {
        return report_multifield_copy_error(FerricError::NullPointer, "out_value is null");
    }
    ptr::write(out_value, FerricValue::void());

    if elements.is_null() && len > 0 {
        return report_multifield_copy_error(
            FerricError::NullPointer,
            "elements is null with non-zero length",
        );
    }

    let mut active_ranges = Vec::new();
    match copy_borrowed_ferric_values(elements, len, 0, &mut active_ranges) {
        Ok(values) => {
            ptr::write(out_value, values.into_multifield());
            FerricError::Ok
        }
        Err(message) => report_multifield_copy_error(FerricError::InvalidArgument, &message),
    }
}

fn report_multifield_copy_error(code: FerricError, message: &str) -> FerricError {
    set_global_error(format!("ferric_value_multifield_copy: {message}"));
    code
}

fn borrowed_value_range(
    elements: *const FerricValue,
    len: usize,
) -> Result<Option<BorrowedValueRange>, String> {
    if len == 0 {
        return Ok(None);
    }
    if elements.is_null() {
        return Err("nested multifield pointer is null with non-zero length".to_string());
    }

    let start = elements as usize;
    if start % std::mem::align_of::<FerricValue>() != 0 {
        return Err("multifield pointer is not aligned for FerricValue".to_string());
    }
    let byte_len = len
        .checked_mul(std::mem::size_of::<FerricValue>())
        .filter(|size| isize::try_from(*size).is_ok())
        .ok_or_else(|| "multifield length exceeds the addressable value-array bound".to_string())?;
    let end = start
        .checked_add(byte_len)
        .ok_or_else(|| "multifield pointer range wraps the address space".to_string())?;
    Ok(Some(BorrowedValueRange { start, end }))
}

/// Copy one borrowed value tree into a wholly Ferric-owned value.
///
/// # Safety
///
/// `value` and every active pointer reachable from it must satisfy the public
/// copy constructor's safety contract.
unsafe fn copy_borrowed_ferric_value(
    value: &FerricValue,
    depth: usize,
    active_ranges: &mut Vec<BorrowedValueRange>,
) -> Result<FerricValue, String> {
    match FerricValueType::try_from(value.value_type)? {
        FerricValueType::Void => Ok(FerricValue::void()),
        FerricValueType::Integer => Ok(FerricValue {
            value_type: FerricValueType::Integer.as_raw(),
            integer: value.integer,
            ..FerricValue::void()
        }),
        FerricValueType::Float => Ok(FerricValue {
            value_type: FerricValueType::Float.as_raw(),
            float: value.float,
            ..FerricValue::void()
        }),
        FerricValueType::Symbol | FerricValueType::String => {
            if value.string_ptr.is_null() {
                return Err(format!(
                    "{} string_ptr is null",
                    if value.value_type == FerricValueType::Symbol.as_raw() {
                        "symbol"
                    } else {
                        "string"
                    }
                ));
            }
            let copied = CStr::from_ptr(value.string_ptr).to_owned();
            Ok(FerricValue {
                value_type: value.value_type,
                string_ptr: copied.into_raw(),
                ..FerricValue::void()
            })
        }
        FerricValueType::Multifield => copy_borrowed_ferric_values(
            value.multifield_ptr,
            value.multifield_len,
            depth + 1,
            active_ranges,
        )
        .map(OwnedFerricValues::into_multifield),
        FerricValueType::ExternalAddress => Ok(FerricValue {
            value_type: FerricValueType::ExternalAddress.as_raw(),
            external_type_id: value.external_type_id,
            external_pointer: value.external_pointer,
            ..FerricValue::void()
        }),
    }
}

/// Copy a borrowed value array while rejecting cycles and bounding recursive
/// construction/free depth.
///
/// # Safety
///
/// `elements` and all reachable active pointers must satisfy the public copy
/// constructor's safety contract.
unsafe fn copy_borrowed_ferric_values(
    elements: *const FerricValue,
    len: usize,
    depth: usize,
    active_ranges: &mut Vec<BorrowedValueRange>,
) -> Result<OwnedFerricValues, String> {
    if depth > MAX_MULTIFIELD_COPY_DEPTH {
        return Err(format!(
            "multifield nesting depth exceeds maximum of {MAX_MULTIFIELD_COPY_DEPTH}"
        ));
    }

    let Some(range) = borrowed_value_range(elements, len)? else {
        return Ok(OwnedFerricValues::with_capacity(0));
    };
    if active_ranges.iter().any(|active| range.overlaps(*active)) {
        return Err(
            "input contains cyclic or ancestor-overlapping multifield array storage".to_string(),
        );
    }

    active_ranges.push(range);
    let result = (|| {
        let mut copied = OwnedFerricValues::with_capacity(len);
        for index in 0..len {
            let value = &*elements.add(index);
            copied.push(copy_borrowed_ferric_value(value, depth, active_ranges)?);
        }
        Ok(copied)
    })();
    let popped = active_ranges.pop();
    debug_assert_eq!(popped, Some(range));
    result
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
/// Recursively frees owned strings and multifield arrays. Null pointers are
/// safely ignored and return `FERRIC_ERROR_OK`.
///
/// If the value (or any nested multifield element) carries an unknown
/// `value_type` discriminant, returns `FERRIC_ERROR_INVALID_ARGUMENT` and
/// records a diagnostic in the global error channel. The unknown-tagged
/// value's payload fields are never interpreted (nothing of it is freed),
/// but all sibling values with known discriminants and every owned
/// containing multifield array are still released.
///
/// # Safety
///
/// - `value` must point to a valid `FerricValue` or be null.
/// - Every recursively owned allocation must have Ferric provenance; borrowed
///   or foreign-allocated value trees must not be passed to this function.
/// - Any owned resources (`string_ptr`, `multifield_ptr`) must not have been freed already.
#[no_mangle]
pub unsafe extern "C" fn ferric_value_free(value: *mut FerricValue) -> FerricError {
    if value.is_null() {
        return FerricError::Ok;
    }
    let val = &*value;
    report_free_result(free_value_resources(val))
}

/// Free an array of `FerricValue`s and all their owned resources.
///
/// Frees each element's owned resources, then frees the array allocation.
/// Null pointers are safely ignored and return `FERRIC_ERROR_OK`.
///
/// If any element (or any nested multifield element) carries an unknown
/// `value_type` discriminant, returns `FERRIC_ERROR_INVALID_ARGUMENT` and
/// records a diagnostic in the global error channel. Unknown-tagged
/// elements' payload fields are never interpreted (nothing of them is
/// freed), but all elements with known discriminants and the array
/// allocation itself are still released.
///
/// # Safety
///
/// - `arr` must point to a contiguous array of `len` `FerricValue`s, or be null.
/// - The array and every recursively owned allocation must have been allocated
///   by Ferric; borrowed or foreign-allocated arrays must not be passed here.
#[no_mangle]
pub unsafe extern "C" fn ferric_value_array_free(arr: *mut FerricValue, len: usize) -> FerricError {
    if arr.is_null() || len == 0 {
        return FerricError::Ok;
    }
    // Free each element's owned resources, retaining the first invalid tag.
    let mut result = Ok(());
    for i in 0..len {
        let elem = &*arr.add(i);
        let elem_result = free_value_resources(elem);
        if result.is_ok() {
            result = elem_result;
        }
    }
    // Free the array allocation itself
    let slice = std::slice::from_raw_parts_mut(arr, len);
    drop(Box::from_raw(slice as *mut [FerricValue]));
    report_free_result(result)
}

/// Map a cleanup result to an FFI error code, storing any diagnostic in the
/// global error channel.
fn report_free_result(result: Result<(), String>) -> FerricError {
    match result {
        Ok(()) => FerricError::Ok,
        Err(msg) => {
            set_global_error(msg);
            FerricError::InvalidArgument
        }
    }
}

/// Internal: free owned resources inside a `FerricValue` without freeing the struct itself.
///
/// The raw `value_type` discriminant is validated before interpretation. An
/// unknown discriminant returns `Err` and frees nothing of that value: with
/// an invalid tag there is no way to know which fields are active, so leaking
/// any caller-supplied resources is preferred over interpreting arbitrary bit
/// patterns (potential wild frees). Cleanup still proceeds for all sibling
/// values with known discriminants and for the owned containing array; the
/// first invalid tag encountered is retained in the returned diagnostic.
///
/// # Safety
///
/// - `val` must point to a valid `FerricValue`.
/// - Any owned resources referenced by `val` must not have been freed already.
unsafe fn free_value_resources(val: &FerricValue) -> Result<(), String> {
    match FerricValueType::from_raw(val.value_type) {
        Some(FerricValueType::Symbol | FerricValueType::String) => {
            if !val.string_ptr.is_null() {
                drop(CString::from_raw(val.string_ptr));
            }
            Ok(())
        }
        Some(FerricValueType::Multifield) => {
            let mut result = Ok(());
            if !val.multifield_ptr.is_null() && val.multifield_len > 0 {
                for i in 0..val.multifield_len {
                    let elem = &*val.multifield_ptr.add(i);
                    let elem_result = free_value_resources(elem);
                    if result.is_ok() {
                        result = elem_result;
                    }
                }
                let slice = std::slice::from_raw_parts_mut(val.multifield_ptr, val.multifield_len);
                drop(Box::from_raw(slice as *mut [FerricValue]));
            }
            result
        }
        Some(
            FerricValueType::Void
            | FerricValueType::Integer
            | FerricValueType::Float
            | FerricValueType::ExternalAddress,
        ) => Ok(()),
        None => Err(format!(
            "cannot free value: invalid value_type discriminant: {} (expected 0..=6); \
             its owned resources were not freed",
            val.value_type
        )),
    }
}
