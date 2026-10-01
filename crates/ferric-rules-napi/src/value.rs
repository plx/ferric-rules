//! Value conversion between JavaScript and Rust.
//!
//! ## Type mapping
//!
//! | JavaScript         | CLIPS             |
//! |--------------------|-------------------|
//! | `null`/`undefined` | rejected as fact input; void output only |
//! | `boolean`          | Symbol TRUE/FALSE |
//! | `number` (integer) | Integer           |
//! | `number` (float)   | Float             |
//! | `bigint`           | Integer           |
//! | `string`           | String (quoted)   |
//! | `FerricSymbol`     | Symbol            |
//! | `FerricInstanceName` | Instance name   |
//! | `Array`            | Multifield        |

use napi::{
    Env, Error, JsBigInt, JsBoolean, JsNull, JsNumber, JsObject, JsString, JsUnknown,
    KeyCollectionMode, KeyConversion, KeyFilter, Result, Status, ValueType,
};
use napi_derive::napi;

use ferric_rules_runtime::{Engine, HostValue, Value, HOST_VALUE_MAX_DEPTH};

use crate::error::engine_error_to_napi;

/// A CLIPS symbol value — distinct from a plain string.
///
/// In CLIPS, symbols and strings are different types. A JavaScript `string`
/// maps to a CLIPS `String` (quoted). To pass a CLIPS Symbol, wrap the name
/// in a `FerricSymbol`.
#[napi]
pub struct FerricSymbol {
    pub(crate) name: String,
}

#[napi]
impl FerricSymbol {
    /// Create a new CLIPS symbol with the given name.
    #[napi(constructor)]
    pub fn new(value: String) -> Self {
        Self { name: value }
    }

    /// The symbol name.
    #[napi(getter)]
    pub fn value(&self) -> &str {
        &self.name
    }

    /// Return the symbol name as a string.
    #[napi]
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.name.clone()
    }

    /// Return the symbol name (for JS `valueOf` protocol).
    #[napi]
    pub fn value_of(&self) -> String {
        self.name.clone()
    }
}

/// A CLIPS instance name such as `[widget]`, holding the spelling without
/// brackets. Distinct from `FerricSymbol`; Ferric has no object system.
#[napi]
pub struct FerricInstanceName {
    pub(crate) name: String,
}

#[napi]
impl FerricInstanceName {
    /// Create an instance name from its spelling without brackets.
    #[napi(constructor)]
    pub fn new(value: String) -> Self {
        Self { name: value }
    }

    /// The spelling without brackets.
    #[napi(getter)]
    pub fn value(&self) -> &str {
        &self.name
    }

    /// Return the CLIPS spelling, with brackets.
    #[napi]
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        format!("[{}]", self.name)
    }

    /// Return the spelling without brackets (for JS `valueOf` protocol).
    #[napi]
    pub fn value_of(&self) -> String {
        self.name.clone()
    }
}

/// Owned input staging: all JavaScript access finishes before a runtime
/// reference is borrowed. The caller retains the native object's reservation.
pub enum OwnedValue {
    Void,
    Integer(i64),
    Float(f64),
    Symbol(String),
    InstanceName(String),
    String(String),
    Multifield(Vec<Self>),
}

impl OwnedValue {
    pub fn into_runtime(self, engine: &mut Engine) -> Result<HostValue> {
        match self {
            Self::Void => Err(Error::new(
                Status::InvalidArg,
                "void cannot be stored in a fact",
            )),
            Self::Integer(value) => Ok(value.into()),
            Self::Float(value) => Ok(value.into()),
            Self::Symbol(value) => engine.symbol_value(&value).map_err(engine_error_to_napi),
            Self::InstanceName(value) => engine
                .instance_name_value(&value)
                .map_err(engine_error_to_napi),
            Self::String(value) => engine
                .create_string(&value)
                .map(HostValue::from)
                .map_err(engine_error_to_napi),
            Self::Multifield(values) => {
                let values = values
                    .into_iter()
                    .map(|value| value.into_runtime(engine))
                    .collect::<Result<Vec<_>>>()?;
                HostValue::multifield(values).map_err(engine_error_to_napi)
            }
        }
    }
}

/// Convert JS input into owned data, with bounded nesting and exact integers.
#[allow(clippy::only_used_in_recursion)]
pub fn js_to_owned(
    env: &Env,
    val: JsUnknown,
    depth: usize,
    remaining: &mut usize,
) -> Result<OwnedValue> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or_else(|| Error::new(Status::InvalidArg, "too many values in one assertion"))?;
    match val.get_type()? {
        ValueType::Null | ValueType::Undefined => Ok(OwnedValue::Void),
        ValueType::Boolean => {
            let value: JsBoolean = val.try_into()?;
            Ok(OwnedValue::Symbol(
                if value.get_value()? { "TRUE" } else { "FALSE" }.to_owned(),
            ))
        }
        ValueType::Number => {
            let value: JsNumber = val.try_into()?;
            let number = value.get_double()?;
            if number.fract() == 0.0 {
                if number.abs() > 9_007_199_254_740_991.0 {
                    return Err(Error::new(Status::InvalidArg, "integer number must be a safe integer; pass a bigint for signed 64-bit values"));
                }
                #[allow(clippy::cast_possible_truncation)]
                Ok(OwnedValue::Integer(number as i64))
            } else {
                Ok(OwnedValue::Float(number))
            }
        }
        ValueType::BigInt => {
            // SAFETY: get_type confirmed BigInt; lossless rejects narrowing.
            let value: JsBigInt = unsafe { val.cast() };
            let (value, lossless) = value.get_i64()?;
            if !lossless {
                return Err(Error::new(
                    Status::InvalidArg,
                    "BigInt value is outside the signed 64-bit integer range",
                ));
            }
            Ok(OwnedValue::Integer(value))
        }
        ValueType::String => {
            let value: JsString = val.try_into()?;
            Ok(OwnedValue::String(value.into_utf8()?.as_str()?.to_owned()))
        }
        ValueType::Object => {
            let obj: JsObject = val.try_into()?;
            if obj.is_array()? {
                if depth >= HOST_VALUE_MAX_DEPTH {
                    return Err(Error::new(
                        Status::InvalidArg,
                        "multifield nesting exceeds 32 levels (cyclic values are unsupported)",
                    ));
                }
                let len = obj.get_array_length()?;
                if len as usize > *remaining {
                    return Err(Error::new(
                        Status::InvalidArg,
                        "too many values in one assertion",
                    ));
                }
                let mut values = Vec::new();
                for index in 0..len {
                    values.push(js_to_owned(
                        env,
                        obj.get_element(index)?,
                        depth + 1,
                        remaining,
                    )?);
                }
                return Ok(OwnedValue::Multifield(values));
            }
            // The loader marshals FerricSymbol and FerricInstanceName to these
            // private native-call forms. Worker wire values are reconstructed first.
            if let Some(value) = marked_name(&obj, "__ferric_symbol")? {
                return Ok(OwnedValue::Symbol(value));
            }
            if let Some(value) = marked_name(&obj, "__ferric_instance_name")? {
                return Ok(OwnedValue::InstanceName(value));
            }
            Err(Error::new(
                Status::InvalidArg,
                "cannot convert object to CLIPS value; expected Array, a canonical FerricSymbol, or a FerricInstanceName",
            ))
        }
        other => Err(Error::new(
            Status::InvalidArg,
            format!("unsupported JS value type: {other:?}"),
        )),
    }
}

/// Read `{ <marker>: true, value: string }`, the loader's native-call form.
fn marked_name(obj: &JsObject, marker: &str) -> Result<Option<String>> {
    if !obj.has_own_property(marker)? || !obj.has_own_property("value")? {
        return Ok(None);
    }
    let flag: JsUnknown = obj.get_named_property(marker)?;
    if flag.get_type()? != ValueType::Boolean {
        return Ok(None);
    }
    let flag: JsBoolean = flag.try_into()?;
    if !flag.get_value()? {
        return Ok(None);
    }
    let value: JsString = obj.get_named_property("value")?;
    Ok(Some(value.into_utf8()?.as_str()?.to_owned()))
}

/// Convert a Rust [`Value`] to a JavaScript value.
///
/// - `Value::Integer` (in safe range) → `number`; otherwise → `bigint`
/// - `Value::Float` → `number`
/// - `Value::Symbol` → `FerricSymbol` instance
/// - `Value::InstanceName` → `FerricInstanceName` instance
/// - `Value::String` → `string`
/// - `Value::Multifield` → `Array`
/// - `Value::Void` → `null`
/// - `Value::ExternalAddress` → explicit unsupported-value error
///
/// # Errors
///
/// Returns an error if the JavaScript object cannot be created.
pub fn value_to_js(env: &Env, val: &Value, engine: &Engine) -> Result<JsUnknown> {
    match val {
        Value::Integer(i) => {
            // JS safe integer range: -(2^53-1) to 2^53-1
            const MAX_SAFE: i64 = (1i64 << 53) - 1;
            const MIN_SAFE: i64 = -MAX_SAFE;
            if *i >= MIN_SAFE && *i <= MAX_SAFE {
                env.create_int64(*i).map(JsNumber::into_unknown)
            } else {
                env.create_bigint_from_i64(*i)?.into_unknown()
            }
        }

        Value::Float(f) => env.create_double(*f).map(JsNumber::into_unknown),

        Value::Symbol(sym) => {
            let name = engine.resolve_core_symbol(*sym).unwrap_or("<unknown>");
            // Construct a FerricSymbol class instance and return it as JsUnknown.
            let symbol = FerricSymbol {
                name: name.to_owned(),
            };
            let instance = symbol.into_instance(*env)?;
            Ok(instance.as_object(*env).into_unknown())
        }

        Value::InstanceName(name) => {
            let name = engine
                .resolve_core_symbol(name.as_symbol())
                .unwrap_or("<unknown>");
            let instance = FerricInstanceName {
                name: name.to_owned(),
            }
            .into_instance(*env)?;
            Ok(instance.as_object(*env).into_unknown())
        }

        Value::String(s) => env.create_string(s.as_str()).map(JsString::into_unknown),

        Value::Multifield(mf) => {
            let mut arr = env.create_array_with_length(mf.len())?;
            for (i, v) in mf.as_slice().iter().enumerate() {
                let js_val = value_to_js(env, v, engine)?;
                #[allow(clippy::cast_possible_truncation)]
                arr.set_element(i as u32, js_val)?;
            }
            Ok(arr.into_unknown())
        }

        Value::Void => env.get_null().map(JsNull::into_unknown),
        Value::ExternalAddress(_) => Err(Error::new(
            Status::InvalidArg,
            "host external identities are not supported by the Node binding",
        )),
    }
}

/// Build a JS array from a Rust iterator of values.
///
/// # Errors
///
/// Returns an error if any element conversion fails.
pub fn values_to_js_array(env: &Env, values: &[Value], engine: &Engine) -> Result<JsObject> {
    let mut arr = env.create_array_with_length(values.len())?;
    for (i, v) in values.iter().enumerate() {
        let js_val = value_to_js(env, v, engine)?;
        #[allow(clippy::cast_possible_truncation)]
        arr.set_element(i as u32, js_val)?;
    }
    Ok(arr)
}

/// Iterate the own string keys of a `JsObject` and collect them.
///
/// # Errors
///
/// Returns an error if property name enumeration fails.
pub fn collect_object_keys(obj: &JsObject) -> Result<Vec<String>> {
    let keys = obj.get_all_property_names(
        KeyCollectionMode::OwnOnly,
        KeyFilter::AllProperties,
        KeyConversion::KeepNumbers,
    )?;
    let len = keys.get_array_length()?;
    let mut result = Vec::with_capacity(len as usize);
    for i in 0..len {
        let key: JsString = keys.get_element(i)?;
        result.push(key.into_utf8()?.as_str()?.to_owned());
    }
    Ok(result)
}
