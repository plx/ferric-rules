//! Value conversion between Rust and Python.

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyFloat, PyInt, PyList, PyString, PyTuple};

use ferric_runtime::{Engine, Multifield, Value};

/// Convert a Rust `Value` to a Python object.
pub fn value_to_python(py: Python<'_>, val: &Value, engine: &Engine) -> PyObject {
    match val {
        Value::Integer(i) => i.into_pyobject(py).expect("int conversion").into_any().unbind(),
        Value::Float(f) => f.into_pyobject(py).expect("float conversion").into_any().unbind(),
        Value::Symbol(sym) => {
            let s = engine.resolve_symbol(*sym).unwrap_or("<unknown>");
            s.into_pyobject(py).expect("str conversion").into_any().unbind()
        }
        Value::String(s) => s
            .as_str()
            .into_pyobject(py)
            .expect("str conversion")
            .into_any()
            .unbind(),
        Value::Multifield(mf) => {
            let items: Vec<PyObject> = mf
                .as_slice()
                .iter()
                .map(|v| value_to_python(py, v, engine))
                .collect();
            PyList::new(py, items)
                .expect("list conversion")
                .into_any()
                .unbind()
        }
        Value::Void | Value::ExternalAddress(_) => py.None(),
    }
}

/// Convert a Python object to a Rust `Value`.
///
/// # Errors
///
/// Returns a `PyErr` if the Python object cannot be converted.
pub fn python_to_value(obj: &Bound<'_, PyAny>, engine: &mut Engine) -> PyResult<Value> {
    // Check bool before int (bool is a subclass of int in Python)
    if let Ok(b) = obj.downcast::<PyBool>() {
        let sym_name = if b.is_true() { "TRUE" } else { "FALSE" };
        let sym = engine
            .intern_symbol(sym_name)
            .map_err(crate::error::engine_error_to_pyerr)?;
        return Ok(Value::Symbol(sym));
    }

    if let Ok(i) = obj.downcast::<PyInt>() {
        let val: i64 = i.extract()?;
        return Ok(Value::Integer(val));
    }

    if let Ok(f) = obj.downcast::<PyFloat>() {
        let val: f64 = f.extract()?;
        return Ok(Value::Float(val));
    }

    if let Ok(s) = obj.downcast::<PyString>() {
        let val: String = s.extract()?;
        let sym = engine
            .intern_symbol(&val)
            .map_err(crate::error::engine_error_to_pyerr)?;
        return Ok(Value::Symbol(sym));
    }

    if obj.is_none() {
        return Ok(Value::Void);
    }

    if let Ok(list) = obj.downcast::<PyList>() {
        let items: PyResult<Vec<Value>> = list
            .iter()
            .map(|item| python_to_value(&item, engine))
            .collect();
        let mf: Multifield = items?.into_iter().collect();
        return Ok(Value::Multifield(Box::new(mf)));
    }

    if let Ok(tuple) = obj.downcast::<PyTuple>() {
        let items: PyResult<Vec<Value>> = tuple
            .iter()
            .map(|item| python_to_value(&item, engine))
            .collect();
        let mf: Multifield = items?.into_iter().collect();
        return Ok(Value::Multifield(Box::new(mf)));
    }

    Err(pyo3::exceptions::PyTypeError::new_err(format!(
        "cannot convert {} to a ferric Value",
        obj.get_type().name()?
    )))
}
