//! Value conversion between Rust and Python.

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyFloat, PyInt, PyList, PyString, PyTuple};

use ferric_rules_runtime::{Engine, HostValue, Value, HOST_VALUE_MAX_DEPTH, HOST_VALUE_MAX_ITEMS};

/// A CLIPS symbol value.
///
/// Wraps a Python string and converts to `Value::Symbol` on the Rust side.
/// Use this explicit marker for unquoted CLIPS symbols.
#[pyclass(from_py_object, name = "Symbol", module = "ferric")]
#[derive(Clone, Debug)]
pub struct Symbol {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl Symbol {
    #[new]
    fn new(value: String) -> Self {
        Self { value }
    }

    fn __repr__(&self) -> String {
        format!("Symbol({:?})", self.value)
    }

    fn __str__(&self) -> &str {
        &self.value
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        if let Ok(sym) = other.cast::<Symbol>() {
            return self.value == sym.borrow().value;
        }
        false
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        PyTuple::new(py, ["ferric.Symbol", &self.value])?.hash()
    }
}

/// A CLIPS instance name such as `[widget]`, holding the spelling without
/// brackets. Distinct from `Symbol`; Ferric has no object system.
#[pyclass(from_py_object, name = "InstanceName", module = "ferric")]
#[derive(Clone, Debug)]
pub struct InstanceName {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl InstanceName {
    #[new]
    fn new(value: String) -> Self {
        Self { value }
    }

    fn __repr__(&self) -> String {
        format!("InstanceName({:?})", self.value)
    }

    fn __str__(&self) -> String {
        format!("[{}]", self.value)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        if let Ok(name) = other.cast::<InstanceName>() {
            return self.value == name.borrow().value;
        }
        false
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        PyTuple::new(py, ["ferric.InstanceName", &self.value])?.hash()
    }
}

/// A CLIPS string value (distinct from a symbol).
///
/// Plain Python `str` also maps to a CLIPS string literal.
/// Returned values retain this wrapper to preserve their native type.
#[pyclass(from_py_object, name = "String", module = "ferric")]
#[derive(Clone, Debug)]
pub struct ClipsString {
    #[pyo3(get)]
    pub value: String,
}

#[pymethods]
impl ClipsString {
    #[new]
    fn new(value: String) -> Self {
        Self { value }
    }

    fn __repr__(&self) -> std::string::String {
        format!("String({:?})", self.value)
    }

    fn __str__(&self) -> &str {
        &self.value
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        if let Ok(cs) = other.cast::<ClipsString>() {
            return self.value == cs.borrow().value;
        }
        false
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        PyTuple::new(py, ["ferric.String", &self.value])?.hash()
    }
}

/// Convert a Rust `Value` to a Python object.
///
/// # Errors
///
/// Returns a `PyErr` if the Python object cannot be created or contains an
/// unsupported external identity or fact address, including inside multifields.
pub fn value_to_python(py: Python<'_>, val: &Value, engine: &Engine) -> PyResult<Py<PyAny>> {
    match val {
        Value::Integer(i) => Ok(i.into_pyobject(py)?.into_any().unbind()),
        Value::Float(f) => Ok(f.into_pyobject(py)?.into_any().unbind()),
        Value::Symbol(sym) => {
            let s = engine.resolve_core_symbol(*sym).unwrap_or("<unknown>");
            Ok(Symbol {
                value: s.to_owned(),
            }
            .into_pyobject(py)?
            .into_any()
            .unbind())
        }
        Value::InstanceName(name) => {
            let s = engine
                .resolve_core_symbol(name.as_symbol())
                .unwrap_or("<unknown>");
            Ok(InstanceName {
                value: s.to_owned(),
            }
            .into_pyobject(py)?
            .into_any()
            .unbind())
        }
        Value::String(s) => Ok(ClipsString {
            value: s.as_str().to_owned(),
        }
        .into_pyobject(py)?
        .into_any()
        .unbind()),
        Value::Multifield(mf) => {
            let items: PyResult<Vec<Py<PyAny>>> = mf
                .as_slice()
                .iter()
                .map(|v| value_to_python(py, v, engine))
                .collect();
            Ok(PyList::new(py, items?)?.into_any().unbind())
        }
        Value::Void => Ok(py.None()),
        Value::FactAddress(_) => Err(pyo3::exceptions::PyTypeError::new_err(
            "fact addresses are not supported by the Python binding",
        )),
        Value::ExternalAddress(_) => Err(pyo3::exceptions::PyTypeError::new_err(
            "host external identities are not supported by the Python binding",
        )),
    }
}

/// Convert a Python object to an engine-validated host value.
///
/// # Errors
///
/// Returns a `PyErr` if the Python object cannot be converted.
#[derive(Default)]
pub struct PythonValueBudget {
    items: usize,
}

impl PythonValueBudget {
    pub fn convert(&mut self, obj: &Bound<'_, PyAny>, engine: &mut Engine) -> PyResult<HostValue> {
        self.convert_at_depth(obj, engine, 0)
    }

    fn convert_at_depth(
        &mut self,
        obj: &Bound<'_, PyAny>,
        engine: &mut Engine,
        depth: usize,
    ) -> PyResult<HostValue> {
        self.items += 1;
        if self.items > HOST_VALUE_MAX_ITEMS {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "host input exceeds 1000000 values",
            ));
        }
        if depth >= HOST_VALUE_MAX_DEPTH
            && (obj.is_instance_of::<PyList>() || obj.is_instance_of::<PyTuple>())
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "host multifield nesting exceeds 32 levels",
            ));
        }
        // Check marker types first: Symbol and ClipsString
        if let Ok(cs) = obj.cast::<ClipsString>() {
            let val = cs.borrow().value.clone();
            let fs = engine
                .create_string(&val)
                .map_err(crate::error::engine_error_to_pyerr)?;
            return Ok(Value::String(fs).into());
        }

        if let Ok(sym) = obj.cast::<Symbol>() {
            let val = sym.borrow().value.clone();
            return engine
                .symbol_value(&val)
                .map_err(crate::error::engine_error_to_pyerr);
        }

        if let Ok(name) = obj.cast::<InstanceName>() {
            let val = name.borrow().value.clone();
            return engine
                .instance_name_value(&val)
                .map_err(crate::error::engine_error_to_pyerr);
        }

        // Check bool before int (bool is a subclass of int in Python)
        if let Ok(b) = obj.cast::<PyBool>() {
            let sym_name = if b.is_true() { "TRUE" } else { "FALSE" };
            return engine
                .symbol_value(sym_name)
                .map_err(crate::error::engine_error_to_pyerr);
        }

        if let Ok(i) = obj.cast::<PyInt>() {
            let val: i64 = i.extract()?;
            return Ok(Value::Integer(val).into());
        }

        if let Ok(f) = obj.cast::<PyFloat>() {
            let val: f64 = f.extract()?;
            return Ok(Value::Float(val).into());
        }

        // Match the other embedding surfaces: host strings are CLIPS strings.
        if let Ok(s) = obj.cast::<PyString>() {
            let val: String = s.extract()?;
            let string = engine
                .create_string(&val)
                .map_err(crate::error::engine_error_to_pyerr)?;
            return Ok(Value::String(string).into());
        }

        if obj.is_none() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "None (void) cannot be stored in a fact, including inside a multifield",
            ));
        }

        if let Ok(list) = obj.cast::<PyList>() {
            let items: PyResult<Vec<HostValue>> = list
                .iter()
                .map(|item| self.convert_at_depth(&item, engine, depth + 1))
                .collect();
            return HostValue::multifield(items?).map_err(crate::error::engine_error_to_pyerr);
        }

        if let Ok(tuple) = obj.cast::<PyTuple>() {
            let items: PyResult<Vec<HostValue>> = tuple
                .iter()
                .map(|item| self.convert_at_depth(&item, engine, depth + 1))
                .collect();
            return HostValue::multifield(items?).map_err(crate::error::engine_error_to_pyerr);
        }

        Err(pyo3::exceptions::PyTypeError::new_err(format!(
            "cannot convert {} to a ferric Value",
            obj.get_type().name()?
        )))
    }
}
