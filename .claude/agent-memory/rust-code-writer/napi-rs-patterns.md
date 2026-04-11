---
name: napi-rs v2 API patterns
description: napi-rs v2.16 API patterns for ferric-napi crate — type conversions, Array, BigInt, etc.
type: reference
---

## napi-rs v2 key API facts

### rust-version
`napi-build 2.3.x` uses `cargo::` syntax in its build script, requiring Rust 1.77+.
Override in `ferric-napi/Cargo.toml`: `rust-version = "1.77"` (not workspace).

### Two Env APIs coexist
- **Old JS API** (`env.rs`): `Env::create_int64()` → `JsNumber`, `Env::create_string()` → `JsString`, `Env::create_array_with_length()` → `JsObject`, `Env::create_object()` → `JsObject`
- **Bindgen API** (`bindgen_runtime/`): `Env::create_array()` → `bindgen_prelude::Array` (different type)

Use the old API when you need `JsObject` with `.into_unknown()`.

### `into_unknown()` trait
The macro `impl_js_value_methods!` gives `into_unknown(self) -> JsUnknown` to:
`JsNull`, `JsBoolean`, `JsNumber`, `JsString`, `JsObject`, `JsBigInt` (returns `Result<JsUnknown>`), etc.
`JsBigInt::into_unknown()` → `Result<JsUnknown>` (call with `?`).

### Array creation (returning JsObject)
Use `env.create_array_with_length(n)?` (returns `JsObject`).
`JsObject` has `.set_element(idx: u32, val: T)` and `.into_unknown()`.

### BigInt casting
`JsBigInt` does NOT implement `TryFrom<JsUnknown>`.
Cast with `unsafe { val.cast::<JsBigInt>() }` after confirming `val.get_type() == ValueType::BigInt`.
`JsBigInt::get_i64()` → `Result<(i64, bool)>` where bool = lossless.

### Type coercions from JsUnknown
Most types: `let t: JsBoolean = val.try_into()?` (uses `TryFrom<JsUnknown>`).
Exception: `JsBigInt` — must use `unsafe { val.cast::<JsBigInt>() }`.

### ClassInstance<T>
`ClassInstance<T>` has `.as_object(env: Env) -> JsObject` (not `into_unknown` directly).
Chain: `symbol.into_instance(*env)?.as_object(*env).into_unknown()`.

### KeyCollectionMode / KeyFilter / KeyConversion
Used with `JsObject::get_all_property_names(mode, filter, conversion)`.
All three are in `napi::` (re-exported from `js_values::*`).
Pattern: `KeyCollectionMode::OwnOnly, KeyFilter::AllProperties, KeyConversion::KeepNumbers`

### bindgen_prelude::Array
Has `.set(idx: u32, val: T)` (not `set_element`).
Use `.coerce_to_object()` to get a `JsObject` if needed.
Implements `ToNapiValue` so can be passed to `JsObject::set`.

### inherent_to_string clippy lint
`pub fn to_string(&self)` on a struct triggers `clippy::inherent_to_string`.
Suppress with `#[allow(clippy::inherent_to_string)]` on the method — needed for napi-rs `#[napi]` methods
that must expose `toString()` to JS.

### FerricSymbol → JsUnknown pattern
```rust
let symbol = FerricSymbol { name: name.to_owned() };
let instance = symbol.into_instance(*env)?;
Ok(instance.as_object(*env).into_unknown())
```

### Null return
`env.get_null().map(JsNull::into_unknown)` — note no `?` after get_null(), just `.map()`
