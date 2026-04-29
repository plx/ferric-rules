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

use ferric_runtime::{Engine, Multifield, Value};

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

/// Convert a JavaScript value to a Rust [`Value`].
///
/// - `null`/`undefined` → `Value::Void`
/// - `boolean` → `Value::Symbol("TRUE")` or `Value::Symbol("FALSE")`
/// - `number` (integral) → `Value::Integer`
/// - `number` (fractional) → `Value::Float`
/// - `bigint` → `Value::Integer`
/// - `string` → `Value::String` (CLIPS quoted string)
/// - `FerricSymbol` → `Value::Symbol`
/// - `Array` → `Value::Multifield`
///
/// # Errors
///
/// Returns an error if the value type is not supported or if symbol/string
/// creation fails due to encoding constraints.
#[allow(clippy::only_used_in_recursion)]
pub fn js_to_value(env: &Env, val: JsUnknown, engine: &mut Engine) -> Result<Value> {
    match val.get_type()? {
        ValueType::Null | ValueType::Undefined => Ok(Value::Void),

        ValueType::Boolean => {
            let js_bool: JsBoolean = val.try_into()?;
            let b = js_bool.get_value()?;
            let sym_name = if b { "TRUE" } else { "FALSE" };
            let sid = engine
                .intern_symbol(sym_name)
                .map_err(engine_error_to_napi)?;
            Ok(Value::Symbol(sid))
        }

        ValueType::Number => {
            let js_num: JsNumber = val.try_into()?;
            let n: f64 = js_num.get_double()?;
            // If the number is a whole value within i64 range, treat as Integer.
            // i64::MAX rounds up to 2^63 as f64, so the upper bound must stay
            // strict to avoid saturating 2^63 into i64::MAX.
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            if n.fract() == 0.0 && n >= (i64::MIN as f64) && n < (i64::MAX as f64) {
                Ok(Value::Integer(n as i64))
            } else {
                Ok(Value::Float(n))
            }
        }

        ValueType::BigInt => {
            // SAFETY: We just confirmed the type is BigInt via get_type().
            let js_bigint: JsBigInt = unsafe { val.cast() };
            let (value, lossless) = js_bigint.get_i64()?;
            if !lossless {
                return Err(Error::new(
                    Status::InvalidArg,
                    "BigInt value is outside the signed 64-bit integer range",
                ));
            }
            Ok(Value::Integer(value))
        }

        ValueType::String => {
            let js_str: JsString = val.try_into()?;
            let s = js_str.into_utf8()?.as_str()?.to_owned();
            let fs = engine.create_string(&s).map_err(engine_error_to_napi)?;
            Ok(Value::String(fs))
        }

        ValueType::Object => {
            let obj: JsObject = val.try_into()?;

            // Check for Array first.
            if obj.is_array()? {
                let len = obj.get_array_length()?;
                let mut items = Vec::with_capacity(len as usize);
                for i in 0..len {
                    let elem: JsUnknown = obj.get_element(i)?;
                    items.push(js_to_value(env, elem, engine)?);
                }
                let mf: Multifield = items.into_iter().collect();
                return Ok(Value::Multifield(Box::new(mf)));
            }

            // Check for a tagged FerricSymbol marker object.  The JS
            // loader (`crates/ferric-napi/index.js::marshalValue`) converts
            // native FerricSymbol instances to plain objects of the form
            // `{ __ferric_symbol: true, value: "name" }` before the args
            // reach Rust, because napi-rs class instances lose their native
            // pointer when passed through `Vec<JsUnknown>` extraction.
            //
            // This is a *different* tagged format from the postMessage wire
            // form `{ __type: "FerricSymbol", value: string }` used between
            // the main thread and worker threads — see
            // `packages/ferric/src/wire.ts::WireSymbol`. The wire form never
            // reaches Rust directly.
            if obj.has_named_property("__ferric_symbol")? {
                let value_prop: JsString = obj.get_named_property("value")?;
                let sym_name = value_prop.into_utf8()?.as_str()?.to_owned();
                let sid = engine
                    .intern_symbol(&sym_name)
                    .map_err(engine_error_to_napi)?;
                return Ok(Value::Symbol(sid));
            }

            Err(Error::new(
                Status::InvalidArg,
                "cannot convert object to CLIPS value; expected Array or FerricSymbol",
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
/// - `Value::Void` / `Value::ExternalAddress` → `null`
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

        Value::Void | Value::ExternalAddress(_) => env.get_null().map(JsNull::into_unknown),
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
