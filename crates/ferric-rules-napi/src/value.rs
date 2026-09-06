//! Value conversion between JavaScript and Rust.
//!
//! ## Type mapping
//!
//! | JavaScript         | CLIPS             |
//! |--------------------|-------------------|
//! | `null`/`undefined` | void              |
//! | `boolean`          | Symbol TRUE/FALSE |
//! | `number` (integer) | Integer           |
//! | `number` (float)   | Float             |
//! | `bigint`           | Integer           |
//! | `string`           | String (quoted)   |
//! | `FerricSymbol`     | Symbol            |
//! | `Array`            | Multifield        |

use napi::{
    Env, Error, JsBigInt, JsBoolean, JsNull, JsNumber, JsObject, JsString, JsUnknown,
    KeyCollectionMode, KeyConversion, KeyFilter, Result, Status, ValueType,
};
use napi_derive::napi;

use ferric_rules_runtime::{Engine, Multifield, Value};

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

/// Owned input staging: all JavaScript access finishes before a runtime
/// reference is borrowed. The caller retains the native object's reservation.
pub enum OwnedValue {
    Void,
    Integer(i64),
    Float(f64),
    Symbol(String),
    String(String),
    Multifield(Vec<Self>),
}

impl OwnedValue {
    pub fn into_runtime(self, engine: &mut Engine) -> Result<Value> {
        match self {
            Self::Void => Ok(Value::Void),
            Self::Integer(value) => Ok(Value::Integer(value)),
            Self::Float(value) => Ok(Value::Float(value)),
            Self::Symbol(value) => engine
                .intern_symbol(&value)
                .map(Value::Symbol)
                .map_err(engine_error_to_napi),
            Self::String(value) => engine
                .create_string(&value)
                .map(Value::String)
                .map_err(engine_error_to_napi),
            Self::Multifield(values) => {
                let values = values
                    .into_iter()
                    .map(|value| value.into_runtime(engine))
                    .collect::<Result<Multifield>>()?;
                Ok(Value::Multifield(Box::new(values)))
            }
        }
    }
}

/// Convert JS input into owned data, with bounded nesting and exact integers.
#[allow(clippy::only_used_in_recursion)]
pub fn js_to_owned(env: &Env, val: JsUnknown, depth: usize) -> Result<OwnedValue> {
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
                if depth >= 128 {
                    return Err(Error::new(
                        Status::InvalidArg,
                        "multifield nesting exceeds 128 levels (cyclic values are unsupported)",
                    ));
                }
                let len = obj.get_array_length()?;
                let mut values = Vec::new();
                for index in 0..len {
                    values.push(js_to_owned(env, obj.get_element(index)?, depth + 1)?);
                }
                return Ok(OwnedValue::Multifield(values));
            }
            // The loader marshals FerricSymbol to this private native-call
            // representation. Worker wire symbols are reconstructed first.
            if obj.has_own_property("__ferric_symbol")? && obj.has_own_property("value")? {
                let marker: JsUnknown = obj.get_named_property("__ferric_symbol")?;
                if marker.get_type()? == ValueType::Boolean {
                    let marker: JsBoolean = marker.try_into()?;
                    if marker.get_value()? {
                        let value: JsString = obj.get_named_property("value")?;
                        return Ok(OwnedValue::Symbol(value.into_utf8()?.as_str()?.to_owned()));
                    }
                }
            }
            Err(Error::new(
                Status::InvalidArg,
                "cannot convert object to CLIPS value; expected Array or a canonical FerricSymbol",
            ))
        }
        other => Err(Error::new(
            Status::InvalidArg,
            format!("unsupported JS value type: {other:?}"),
        )),
    }
}

/// Convert a Rust [`Value`] to a JavaScript value.
///
/// - `Value::Integer` (in safe range) → `number`; otherwise → `bigint`
/// - `Value::Float` → `number`
/// - `Value::Symbol` → `FerricSymbol` instance
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
            let name = engine.resolve_symbol(*sym).unwrap_or("<unknown>");
            // Construct a FerricSymbol class instance and return it as JsUnknown.
            let symbol = FerricSymbol {
                name: name.to_owned(),
            };
            let instance = symbol.into_instance(*env)?;
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
