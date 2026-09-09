//! Value conversion between Rust and Python.

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyFloat, PyInt, PyList, PyString, PyTuple};

use ferric_rules_runtime::{Engine, HostValue, Value, HOST_VALUE_MAX_DEPTH, HOST_VALUE_MAX_ITEMS};

/// Decode a byte lexeme with Python's checked UTF-8 decoder.
pub(crate) fn checked_text(py: Python<'_>, bytes: &[u8]) -> PyResult<String> {
    PyBytes::new(py, bytes)
        .call_method1("decode", ("utf-8",))?
        .extract()
}

fn lexeme_bytes(value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(text) = value.downcast::<PyString>() {
        // to_cow remains available with abi3-py39 and rejects unpaired surrogates.
        return Ok(text.to_cow()?.as_bytes().to_vec());
    }
    if let Ok(bytes) = value.downcast::<PyBytes>() {
        return Ok(bytes.as_bytes().to_vec());
    }
    Err(pyo3::exceptions::PyTypeError::new_err(
        "expected str or bytes",
    ))
}

/// A distinct CLIPS Symbol lexeme with lossless byte storage.
#[pyclass(name = "Symbol", module = "ferric")]
#[derive(Clone, Debug)]
pub struct Symbol {
    pub bytes: Vec<u8>,
}

#[pymethods]
impl Symbol {
    #[new]
    fn new(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            bytes: lexeme_bytes(value)?,
        })
    }

    /// Checked UTF-8 text; raises `UnicodeDecodeError` for arbitrary bytes.
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<String> {
        checked_text(py, &self.bytes)
    }

    /// An immutable copy of the exact lexeme bytes.
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.bytes)
    }

    fn __repr__(&self) -> String {
        match std::str::from_utf8(&self.bytes) {
            Ok(text) => format!("Symbol({text:?})"),
            Err(_) => format!("Symbol({:?})", self.bytes),
        }
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        checked_text(py, &self.bytes)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .downcast::<Symbol>()
            .is_ok_and(|other| self.bytes == other.borrow().bytes)
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        PyTuple::new(
            py,
            [
                "ferric.Symbol".into_pyobject(py)?.into_any(),
                PyBytes::new(py, &self.bytes).into_any(),
            ],
        )?
        .hash()
    }
}

/// A distinct CLIPS String lexeme with lossless byte storage.
#[pyclass(name = "String", module = "ferric")]
#[derive(Clone, Debug)]
pub struct ClipsString {
    pub bytes: Vec<u8>,
}

#[pymethods]
impl ClipsString {
    #[new]
    fn new(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            bytes: lexeme_bytes(value)?,
        })
    }

    /// Checked UTF-8 text; raises `UnicodeDecodeError` for arbitrary bytes.
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<String> {
        checked_text(py, &self.bytes)
    }

    /// An immutable copy of the exact lexeme bytes.
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.bytes)
    }

    fn __repr__(&self) -> String {
        match std::str::from_utf8(&self.bytes) {
            Ok(text) => format!("String({text:?})"),
            Err(_) => format!("String({:?})", self.bytes),
        }
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        checked_text(py, &self.bytes)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .downcast::<ClipsString>()
            .is_ok_and(|other| self.bytes == other.borrow().bytes)
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        PyTuple::new(
            py,
            [
                "ferric.String".into_pyobject(py)?.into_any(),
                PyBytes::new(py, &self.bytes).into_any(),
            ],
        )?
        .hash()
    }
}

/// A distinct CLIPS `InstanceName` lexeme with lossless byte storage.
#[pyclass(name = "InstanceName", module = "ferric")]
#[derive(Clone, Debug)]
pub struct InstanceName {
    pub bytes: Vec<u8>,
}

#[pymethods]
impl InstanceName {
    #[new]
    fn new(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            bytes: lexeme_bytes(value)?,
        })
    }

    /// Checked UTF-8 text; raises `UnicodeDecodeError` for arbitrary bytes.
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<String> {
        checked_text(py, &self.bytes)
    }

    /// An immutable copy of the exact lexeme bytes.
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.bytes)
    }

    fn __repr__(&self) -> String {
        match std::str::from_utf8(&self.bytes) {
            Ok(text) => format!("InstanceName({text:?})"),
            Err(_) => format!("InstanceName({:?})", self.bytes),
        }
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        checked_text(py, &self.bytes)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .downcast::<InstanceName>()
            .is_ok_and(|other| self.bytes == other.borrow().bytes)
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        PyTuple::new(
            py,
            [
                "ferric.InstanceName".into_pyobject(py)?.into_any(),
                PyBytes::new(py, &self.bytes).into_any(),
            ],
        )?
        .hash()
    }
}

/// Convert a Rust `Value` to a Python object.
///
/// # Errors
///
/// Returns a `PyErr` if the Python object cannot be created.
pub fn value_to_python(py: Python<'_>, val: &Value, engine: &Engine) -> PyResult<PyObject> {
    match val {
        Value::Integer(i) => Ok(i.into_pyobject(py)?.into_any().unbind()),
        Value::Float(f) => Ok(f.into_pyobject(py)?.into_any().unbind()),
        Value::Symbol(sym) => {
            let s = engine.resolve_core_symbol_bytes(*sym).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err("unresolved engine symbol")
            })?;
            Ok(Symbol { bytes: s.to_vec() }
                .into_pyobject(py)?
                .into_any()
                .unbind())
        }
        Value::String(s) => Ok(ClipsString {
            bytes: s.as_bytes().to_vec(),
        }
        .into_pyobject(py)?
        .into_any()
        .unbind()),
        Value::InstanceName(name) => Ok(InstanceName {
            bytes: engine
                .resolve_core_symbol_bytes(name.as_symbol())
                .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("unresolved instance name"))?
                .to_vec(),
        }
        .into_pyobject(py)?
        .into_any()
        .unbind()),
        Value::Multifield(mf) => {
            let items: PyResult<Vec<PyObject>> = mf
                .as_slice()
                .iter()
                .map(|v| value_to_python(py, v, engine))
                .collect();
            Ok(PyList::new(py, items?)?.into_any().unbind())
        }
        Value::Void => Ok(py.None()),
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
        if let Ok(cs) = obj.downcast::<ClipsString>() {
            let val = cs.borrow().bytes.clone();
            let fs = engine
                .create_string_bytes(&val)
                .map_err(crate::error::engine_error_to_pyerr)?;
            return Ok(Value::String(fs).into());
        }

        if let Ok(sym) = obj.downcast::<Symbol>() {
            let val = sym.borrow().bytes.clone();
            return engine
                .symbol_value_bytes(&val)
                .map_err(crate::error::engine_error_to_pyerr);
        }

        if let Ok(name) = obj.downcast::<InstanceName>() {
            return engine
                .instance_name_value_bytes(&name.borrow().bytes)
                .map_err(crate::error::engine_error_to_pyerr);
        }

        // Check bool before int (bool is a subclass of int in Python)
        if let Ok(b) = obj.downcast::<PyBool>() {
            let sym_name = if b.is_true() { "TRUE" } else { "FALSE" };
            return engine
                .symbol_value(sym_name)
                .map_err(crate::error::engine_error_to_pyerr);
        }

        if let Ok(i) = obj.downcast::<PyInt>() {
            let val: i64 = i.extract()?;
            return Ok(Value::Integer(val).into());
        }

        if let Ok(f) = obj.downcast::<PyFloat>() {
            let val: f64 = f.extract()?;
            return Ok(Value::Float(val).into());
        }

        // Match the other embedding surfaces: host strings are CLIPS strings.
        if let Ok(s) = obj.downcast::<PyString>() {
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

        if let Ok(list) = obj.downcast::<PyList>() {
            let items: PyResult<Vec<HostValue>> = list
                .iter()
                .map(|item| self.convert_at_depth(&item, engine, depth + 1))
                .collect();
            return HostValue::multifield(items?).map_err(crate::error::engine_error_to_pyerr);
        }

        if let Ok(tuple) = obj.downcast::<PyTuple>() {
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
