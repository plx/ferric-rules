//! User-defined function environment and global variable storage.
//!
//! This module provides:
//! - [`UserFunction`]: A registered user-defined function from `deffunction`.
//! - [`FunctionEnv`]: A registry of all user-defined functions.
//! - [`GlobalStore`]: Runtime storage for `defglobal` values.

use std::collections::HashMap;

use ferric_core::Value;
use ferric_parser::ActionExpr;

// ---------------------------------------------------------------------------
// User-defined functions
// ---------------------------------------------------------------------------

/// A registered user-defined function from a `deffunction` form.
#[derive(Clone, Debug)]
pub struct UserFunction {
    /// The function name.
    pub name: String,
    /// Regular parameter names (without `?` prefix).
    pub parameters: Vec<String>,
    /// Optional wildcard parameter name (without `$?` prefix), for variadic functions.
    pub wildcard_parameter: Option<String>,
    /// Function body expressions, evaluated in sequence; last value is returned.
    pub body: Vec<ActionExpr>,
}

/// Registry of all user-defined functions loaded into the engine.
#[derive(Clone, Debug, Default)]
pub struct FunctionEnv {
    pub(crate) functions: HashMap<String, UserFunction>,
}

impl FunctionEnv {
    /// Create a new, empty function environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a user-defined function, replacing any existing function with the same name.
    pub fn register(&mut self, func: UserFunction) {
        self.functions.insert(func.name.clone(), func);
    }

    /// Look up a user-defined function by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&UserFunction> {
        self.functions.get(name)
    }

    /// Check whether a function with this name exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// Debug-only structural checks for function registry bookkeeping.
    pub fn debug_assert_consistency(&self) {
        for (name, func) in &self.functions {
            assert_eq!(
                &func.name, name,
                "function registry key `{name}` does not match function.name `{}`",
                func.name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Global variable storage
// ---------------------------------------------------------------------------

/// Runtime storage for `defglobal` values.
///
/// Maps global variable names (without the `?*` and `*` delimiters) to their
/// current runtime values.
#[derive(Clone, Debug, Default)]
pub struct GlobalStore {
    values: HashMap<String, Value>,
}

impl GlobalStore {
    /// Create a new, empty global store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current value of a global variable by name.
    ///
    /// Returns `None` if the variable has not been set.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    /// Check whether a global has a value.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// Set the value of a global variable.
    ///
    /// If the variable was previously set, its value is replaced.
    pub fn set(&mut self, name: &str, value: Value) {
        self.values.insert(name.to_string(), value);
    }

    /// Clear all global variables (used during engine reset).
    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Debug-only structural checks for global store bookkeeping.
    pub fn debug_assert_consistency(&self) {
        for name in self.values.keys() {
            assert!(
                !name.is_empty(),
                "global store contains an empty-name entry"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Generic function and method dispatch
// ---------------------------------------------------------------------------

/// A registered method for a generic function.
#[derive(Clone, Debug)]
pub struct RegisteredMethod {
    /// Method index (determines dispatch priority; lower = tried first).
    pub index: i32,
    /// Regular parameter names (without `?` prefix).
    pub parameters: Vec<String>,
    /// Type restrictions per parameter (same length as `parameters`).
    /// Empty vec for a parameter means "any type".
    pub type_restrictions: Vec<Vec<String>>,
    /// Optional wildcard parameter name.
    pub wildcard_parameter: Option<String>,
    /// Method body expressions.
    pub body: Vec<ferric_parser::ActionExpr>,
}

/// A generic function with its collection of methods.
#[derive(Clone, Debug)]
pub struct GenericFunction {
    /// Generic function name.
    pub name: String,
    /// Methods sorted by index (ascending).
    pub methods: Vec<RegisteredMethod>,
    /// Next auto-assigned index.
    next_index: i32,
}

impl GenericFunction {
    /// Create a new empty generic function.
    pub fn new(name: String) -> Self {
        Self {
            name,
            methods: Vec::new(),
            next_index: 1,
        }
    }

    /// Add a method. Methods are kept sorted by index (ascending).
    pub fn add_method(&mut self, method: RegisteredMethod) {
        if method.index >= self.next_index {
            self.next_index = method.index + 1;
        }
        let pos = self.methods.partition_point(|m| m.index < method.index);
        self.methods.insert(pos, method);
    }

    /// Allocate the next auto index.
    pub fn next_auto_index(&mut self) -> i32 {
        let idx = self.next_index;
        self.next_index += 1;
        idx
    }
}

/// Registry of generic functions and their methods.
#[derive(Clone, Debug, Default)]
pub struct GenericRegistry {
    generics: HashMap<String, GenericFunction>,
}

impl GenericRegistry {
    /// Create a new, empty generic registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a generic function declaration. If already exists, this is a no-op.
    pub fn register_generic(&mut self, name: &str) {
        self.generics
            .entry(name.to_string())
            .or_insert_with(|| GenericFunction::new(name.to_string()));
    }

    /// Register a method. Auto-creates the generic if it doesn't exist.
    pub fn register_method(
        &mut self,
        name: &str,
        index: Option<i32>,
        parameters: Vec<String>,
        type_restrictions: Vec<Vec<String>>,
        wildcard_parameter: Option<String>,
        body: Vec<ferric_parser::ActionExpr>,
    ) {
        let generic = self
            .generics
            .entry(name.to_string())
            .or_insert_with(|| GenericFunction::new(name.to_string()));
        let actual_index = index.unwrap_or_else(|| generic.next_auto_index());
        generic.add_method(RegisteredMethod {
            index: actual_index,
            parameters,
            type_restrictions,
            wildcard_parameter,
            body,
        });
    }

    /// Look up a generic function by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&GenericFunction> {
        self.generics.get(name)
    }

    /// Check whether a generic with this name exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.generics.contains_key(name)
    }

    /// Check whether a generic already has a method with the given index.
    #[must_use]
    pub fn has_method_index(&self, name: &str, index: i32) -> bool {
        self.generics
            .get(name)
            .is_some_and(|g| g.methods.iter().any(|m| m.index == index))
    }

    /// Debug-only structural checks for generic/method bookkeeping.
    pub fn debug_assert_consistency(&self) {
        for (name, generic) in &self.generics {
            assert_eq!(
                &generic.name, name,
                "generic registry key `{name}` does not match generic.name `{}`",
                generic.name
            );
            for w in generic.methods.windows(2) {
                assert!(
                    w[0].index < w[1].index,
                    "generic `{name}` has non-increasing/duplicate method indices: {} then {}",
                    w[0].index,
                    w[1].index
                );
            }
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // FunctionEnv tests
    // -----------------------------------------------------------------------

    #[test]
    fn function_env_register_and_get() {
        let mut env = FunctionEnv::new();
        let func = UserFunction {
            name: "double".to_string(),
            parameters: vec!["x".to_string()],
            wildcard_parameter: None,
            body: vec![],
        };
        env.register(func);

        let retrieved = env.get("double");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "double");
        assert_eq!(retrieved.unwrap().parameters, vec!["x"]);
    }

    #[test]
    fn function_env_get_missing_returns_none() {
        let env = FunctionEnv::new();
        assert!(env.get("nonexistent").is_none());
    }

    #[test]
    fn function_env_contains_reports_presence() {
        let mut env = FunctionEnv::new();
        env.register(UserFunction {
            name: "f".to_string(),
            parameters: vec![],
            wildcard_parameter: None,
            body: vec![],
        });
        assert!(env.contains("f"));
        assert!(!env.contains("missing"));
    }

    #[test]
    fn function_env_register_overwrites_existing() {
        let mut env = FunctionEnv::new();
        let func1 = UserFunction {
            name: "f".to_string(),
            parameters: vec!["a".to_string()],
            wildcard_parameter: None,
            body: vec![],
        };
        let func2 = UserFunction {
            name: "f".to_string(),
            parameters: vec!["b".to_string(), "c".to_string()],
            wildcard_parameter: None,
            body: vec![],
        };
        env.register(func1);
        env.register(func2);

        let retrieved = env.get("f").unwrap();
        assert_eq!(retrieved.parameters, vec!["b", "c"]);
    }

    #[test]
    fn function_env_wildcard_parameter_stored() {
        let mut env = FunctionEnv::new();
        let func = UserFunction {
            name: "variadic".to_string(),
            parameters: vec!["first".to_string()],
            wildcard_parameter: Some("rest".to_string()),
            body: vec![],
        };
        env.register(func);

        let retrieved = env.get("variadic").unwrap();
        assert_eq!(retrieved.wildcard_parameter, Some("rest".to_string()));
    }

    // -----------------------------------------------------------------------
    // GlobalStore tests
    // -----------------------------------------------------------------------

    #[test]
    fn global_store_set_and_get() {
        let mut store = GlobalStore::new();
        store.set("count", Value::Integer(42));

        let val = store.get("count");
        assert!(val.is_some());
        assert!(val.unwrap().structural_eq(&Value::Integer(42)));
    }

    #[test]
    fn global_store_get_missing_returns_none() {
        let store = GlobalStore::new();
        assert!(store.get("missing").is_none());
    }

    #[test]
    fn global_store_set_overwrites_existing() {
        let mut store = GlobalStore::new();
        store.set("x", Value::Integer(1));
        store.set("x", Value::Integer(2));

        assert!(store.get("x").unwrap().structural_eq(&Value::Integer(2)));
    }

    #[test]
    fn global_store_clear_removes_all() {
        let mut store = GlobalStore::new();
        store.set("a", Value::Integer(1));
        store.set("b", Value::Integer(2));
        store.clear();

        assert!(store.get("a").is_none());
        assert!(store.get("b").is_none());
    }

    #[test]
    fn global_store_multiple_variables() {
        let mut store = GlobalStore::new();
        store.set("threshold", Value::Integer(50));
        store.set("counter", Value::Integer(0));

        assert!(store
            .get("threshold")
            .unwrap()
            .structural_eq(&Value::Integer(50)));
        assert!(store
            .get("counter")
            .unwrap()
            .structural_eq(&Value::Integer(0)));
    }

    #[test]
    fn global_store_contains_reports_presence() {
        let mut store = GlobalStore::new();
        store.set("x", Value::Integer(1));
        assert!(store.contains("x"));
        assert!(!store.contains("missing"));
    }

    // -----------------------------------------------------------------------
    // GenericRegistry tests
    // -----------------------------------------------------------------------

    #[test]
    fn generic_registry_register_and_get() {
        let mut reg = GenericRegistry::new();
        reg.register_generic("display");
        assert!(reg.get("display").is_some());
        assert_eq!(reg.get("display").unwrap().name, "display");
    }

    #[test]
    fn generic_registry_get_missing_returns_none() {
        let reg = GenericRegistry::new();
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn generic_registry_contains_reports_presence() {
        let mut reg = GenericRegistry::new();
        reg.register_generic("display");
        assert!(reg.contains("display"));
        assert!(!reg.contains("missing"));
    }

    #[test]
    fn generic_registry_method_auto_creates_generic() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            "format",
            None,
            vec!["x".into()],
            vec![vec!["INTEGER".into()]],
            None,
            vec![],
        );
        assert!(reg.get("format").is_some());
        assert_eq!(reg.get("format").unwrap().methods.len(), 1);
    }

    #[test]
    fn generic_registry_methods_sorted_by_index() {
        let mut reg = GenericRegistry::new();
        reg.register_method("f", Some(3), vec!["x".into()], vec![vec![]], None, vec![]);
        reg.register_method(
            "f",
            Some(1),
            vec!["x".into()],
            vec![vec!["INTEGER".into()]],
            None,
            vec![],
        );
        reg.register_method(
            "f",
            Some(2),
            vec!["x".into()],
            vec![vec!["FLOAT".into()]],
            None,
            vec![],
        );
        let methods = &reg.get("f").unwrap().methods;
        assert_eq!(methods[0].index, 1);
        assert_eq!(methods[1].index, 2);
        assert_eq!(methods[2].index, 3);
    }

    #[test]
    fn generic_registry_auto_index_increments() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            "f",
            None,
            vec!["x".into()],
            vec![vec!["INTEGER".into()]],
            None,
            vec![],
        );
        reg.register_method(
            "f",
            None,
            vec!["x".into()],
            vec![vec!["FLOAT".into()]],
            None,
            vec![],
        );
        let methods = &reg.get("f").unwrap().methods;
        assert_eq!(methods[0].index, 1);
        assert_eq!(methods[1].index, 2);
    }

    #[test]
    fn generic_registry_explicit_index_updates_next_auto() {
        let mut reg = GenericRegistry::new();
        reg.register_method("f", Some(10), vec!["x".into()], vec![vec![]], None, vec![]);
        reg.register_method("f", None, vec!["x".into()], vec![vec![]], None, vec![]);
        let methods = &reg.get("f").unwrap().methods;
        assert_eq!(methods[0].index, 10);
        assert_eq!(methods[1].index, 11);
    }

    #[test]
    fn generic_registry_method_with_wildcard() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            "f",
            None,
            vec!["x".into()],
            vec![vec![]],
            Some("rest".into()),
            vec![],
        );
        let method = &reg.get("f").unwrap().methods[0];
        assert_eq!(method.wildcard_parameter, Some("rest".into()));
    }

    #[test]
    fn generic_registry_has_method_index() {
        let mut reg = GenericRegistry::new();
        reg.register_method("f", Some(3), vec!["x".into()], vec![vec![]], None, vec![]);
        assert!(reg.has_method_index("f", 3));
        assert!(!reg.has_method_index("f", 2));
        assert!(!reg.has_method_index("missing", 3));
    }

    #[test]
    fn debug_consistency_checks_pass_for_valid_state() {
        let mut fenv = FunctionEnv::new();
        fenv.register(UserFunction {
            name: "f".to_string(),
            parameters: vec![],
            wildcard_parameter: None,
            body: vec![],
        });
        fenv.debug_assert_consistency();

        let mut globals = GlobalStore::new();
        globals.set("g", Value::Integer(1));
        globals.debug_assert_consistency();

        let mut reg = GenericRegistry::new();
        reg.register_method("m", Some(1), vec!["x".into()], vec![vec![]], None, vec![]);
        reg.register_method("m", Some(2), vec!["x".into()], vec![vec![]], None, vec![]);
        reg.debug_assert_consistency();
    }
}
