//! User-defined function environment and global variable storage.
//!
//! This module provides:
//! - [`UserFunction`]: A registered user-defined function from `deffunction`.
//! - [`FunctionEnv`]: A registry of all user-defined functions.
//! - [`GlobalStore`]: Runtime storage for `defglobal` values.

use ferric_core::Value;
use ferric_parser::ActionExpr;
use rustc_hash::FxHashMap as HashMap;

use crate::modules::ModuleId;

pub(crate) type LocalNameMap<T> = HashMap<Box<str>, T>;
pub(crate) type ModuleNameMap<T> = HashMap<ModuleId, LocalNameMap<T>>;

fn modules_for_name_from_keys<T>(entries: &ModuleNameMap<T>, name: &str) -> Vec<ModuleId> {
    entries
        .iter()
        .filter_map(|(&module_id, local_names)| local_names.contains_key(name).then_some(module_id))
        .collect()
}

pub(crate) fn get_module_entry<'a, T>(
    entries: &'a ModuleNameMap<T>,
    module: ModuleId,
    name: &str,
) -> Option<&'a T> {
    entries.get(&module)?.get(name)
}

pub(crate) fn contains_module_entry<T>(
    entries: &ModuleNameMap<T>,
    module: ModuleId,
    name: &str,
) -> bool {
    get_module_entry(entries, module, name).is_some()
}

pub(crate) fn insert_module_entry<T>(
    entries: &mut ModuleNameMap<T>,
    module: ModuleId,
    name: impl Into<Box<str>>,
    value: T,
) -> Option<T> {
    entries
        .entry(module)
        .or_default()
        .insert(name.into(), value)
}

pub(crate) fn get_or_insert_module_entry_with<'a, T, F>(
    entries: &'a mut ModuleNameMap<T>,
    module: ModuleId,
    name: &str,
    init: F,
) -> &'a mut T
where
    F: FnOnce() -> T,
{
    let local_names = entries.entry(module).or_default();
    if !local_names.contains_key(name) {
        local_names.insert(name.into(), init());
    }
    local_names
        .get_mut(name)
        .expect("module entry should exist after insertion check")
}

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
    pub(crate) functions: ModuleNameMap<UserFunction>,
}

impl FunctionEnv {
    /// Create a new, empty function environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a user-defined function in a module, replacing any existing
    /// definition in that same module with the same local name.
    pub fn register(&mut self, module: ModuleId, func: UserFunction) {
        insert_module_entry(&mut self.functions, module, func.name.clone(), func);
    }

    /// Look up a user-defined function by module and local name.
    #[must_use]
    pub fn get(&self, module: ModuleId, name: &str) -> Option<&UserFunction> {
        get_module_entry(&self.functions, module, name)
    }

    /// Check whether a function with this local name exists in the given module.
    #[must_use]
    pub fn contains(&self, module: ModuleId, name: &str) -> bool {
        contains_module_entry(&self.functions, module, name)
    }

    /// Return all module IDs that define a function with this local name.
    #[must_use]
    pub fn modules_for_name(&self, name: &str) -> Vec<ModuleId> {
        modules_for_name_from_keys(&self.functions, name)
    }

    /// Debug-only structural checks for function registry bookkeeping.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        for local_names in self.functions.values() {
            for (name, func) in local_names {
                assert_eq!(
                    func.name.as_str(),
                    name.as_ref(),
                    "function registry key `{name}` does not match function.name `{}`",
                    func.name
                );
            }
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
    values: ModuleNameMap<Value>,
}

impl GlobalStore {
    /// Create a new, empty global store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current value of a global variable by module and local name.
    ///
    /// Returns `None` if the variable has not been set.
    #[must_use]
    pub fn get(&self, module: ModuleId, name: &str) -> Option<&Value> {
        get_module_entry(&self.values, module, name)
    }

    /// Check whether a global has a value in the given module.
    #[must_use]
    pub fn contains(&self, module: ModuleId, name: &str) -> bool {
        contains_module_entry(&self.values, module, name)
    }

    /// Return all module IDs that define a global with this local name.
    #[must_use]
    pub fn modules_for_name(&self, name: &str) -> Vec<ModuleId> {
        modules_for_name_from_keys(&self.values, name)
    }

    /// Set the value of a global variable.
    ///
    /// If the variable was previously set, its value is replaced.
    pub fn set(&mut self, module: ModuleId, name: &str, value: Value) {
        insert_module_entry(&mut self.values, module, name, value);
    }

    /// Clear all global variables (used during engine reset).
    pub fn clear(&mut self) {
        self.values.clear();
    }

    /// Debug-only structural checks for global store bookkeeping.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        for local_names in self.values.values() {
            for name in local_names.keys() {
                assert!(
                    !name.is_empty(),
                    "global store contains an empty-name entry"
                );
            }
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
    generics: ModuleNameMap<GenericFunction>,
}

impl GenericRegistry {
    /// Create a new, empty generic registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a generic function declaration. If already exists, this is a no-op.
    pub fn register_generic(&mut self, module: ModuleId, name: &str) {
        let _ = get_or_insert_module_entry_with(&mut self.generics, module, name, || {
            GenericFunction::new(name.to_string())
        });
    }

    /// Register a method. Auto-creates the generic if it doesn't exist.
    #[allow(clippy::too_many_arguments)]
    pub fn register_method(
        &mut self,
        module: ModuleId,
        name: &str,
        index: Option<i32>,
        parameters: Vec<String>,
        type_restrictions: Vec<Vec<String>>,
        wildcard_parameter: Option<String>,
        body: Vec<ferric_parser::ActionExpr>,
    ) {
        let generic = get_or_insert_module_entry_with(&mut self.generics, module, name, || {
            GenericFunction::new(name.to_string())
        });
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
    pub fn get(&self, module: ModuleId, name: &str) -> Option<&GenericFunction> {
        get_module_entry(&self.generics, module, name)
    }

    /// Check whether a generic with this local name exists in the given module.
    #[must_use]
    pub fn contains(&self, module: ModuleId, name: &str) -> bool {
        contains_module_entry(&self.generics, module, name)
    }

    /// Return all module IDs that define a generic with this local name.
    #[must_use]
    pub fn modules_for_name(&self, name: &str) -> Vec<ModuleId> {
        modules_for_name_from_keys(&self.generics, name)
    }

    /// Check whether a generic already has a method with the given index.
    #[must_use]
    pub fn has_method_index(&self, module: ModuleId, name: &str, index: i32) -> bool {
        get_module_entry(&self.generics, module, name)
            .is_some_and(|g| g.methods.iter().any(|m| m.index == index))
    }

    /// Debug-only structural checks for generic/method bookkeeping.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        for local_names in self.generics.values() {
            for (name, generic) in local_names {
                assert_eq!(
                    generic.name.as_str(),
                    name.as_ref(),
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
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn main_module() -> ModuleId {
        ModuleId(0)
    }

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
        env.register(main_module(), func);

        let retrieved = env.get(main_module(), "double");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "double");
        assert_eq!(retrieved.unwrap().parameters, vec!["x"]);
    }

    #[test]
    fn function_env_get_missing_returns_none() {
        let env = FunctionEnv::new();
        assert!(env.get(main_module(), "nonexistent").is_none());
    }

    #[test]
    fn function_env_contains_reports_presence() {
        let mut env = FunctionEnv::new();
        env.register(
            main_module(),
            UserFunction {
                name: "f".to_string(),
                parameters: vec![],
                wildcard_parameter: None,
                body: vec![],
            },
        );
        assert!(env.contains(main_module(), "f"));
        assert!(!env.contains(main_module(), "missing"));
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
        env.register(main_module(), func1);
        env.register(main_module(), func2);

        let retrieved = env.get(main_module(), "f").unwrap();
        assert_eq!(retrieved.parameters, vec!["b", "c"]);
    }

    #[test]
    fn function_env_same_local_name_in_different_modules() {
        let mut env = FunctionEnv::new();
        let module_a = ModuleId(1);
        let module_b = ModuleId(2);
        env.register(
            module_a,
            UserFunction {
                name: "f".to_string(),
                parameters: vec![],
                wildcard_parameter: None,
                body: vec![],
            },
        );
        env.register(
            module_b,
            UserFunction {
                name: "f".to_string(),
                parameters: vec!["x".to_string()],
                wildcard_parameter: None,
                body: vec![],
            },
        );

        assert!(env.contains(module_a, "f"));
        assert!(env.contains(module_b, "f"));
        assert_eq!(env.get(module_a, "f").unwrap().parameters.len(), 0);
        assert_eq!(env.get(module_b, "f").unwrap().parameters.len(), 1);
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
        env.register(main_module(), func);

        let retrieved = env.get(main_module(), "variadic").unwrap();
        assert_eq!(retrieved.wildcard_parameter, Some("rest".to_string()));
    }

    // -----------------------------------------------------------------------
    // GlobalStore tests
    // -----------------------------------------------------------------------

    #[test]
    fn global_store_set_and_get() {
        let mut store = GlobalStore::new();
        store.set(main_module(), "count", Value::Integer(42));

        let val = store.get(main_module(), "count");
        assert!(val.is_some());
        assert!(val.unwrap().structural_eq(&Value::Integer(42)));
    }

    #[test]
    fn global_store_get_missing_returns_none() {
        let store = GlobalStore::new();
        assert!(store.get(main_module(), "missing").is_none());
    }

    #[test]
    fn global_store_set_overwrites_existing() {
        let mut store = GlobalStore::new();
        store.set(main_module(), "x", Value::Integer(1));
        store.set(main_module(), "x", Value::Integer(2));

        assert!(store
            .get(main_module(), "x")
            .unwrap()
            .structural_eq(&Value::Integer(2)));
    }

    #[test]
    fn global_store_clear_removes_all() {
        let mut store = GlobalStore::new();
        store.set(main_module(), "a", Value::Integer(1));
        store.set(main_module(), "b", Value::Integer(2));
        store.clear();

        assert!(store.get(main_module(), "a").is_none());
        assert!(store.get(main_module(), "b").is_none());
    }

    #[test]
    fn global_store_multiple_variables() {
        let mut store = GlobalStore::new();
        store.set(main_module(), "threshold", Value::Integer(50));
        store.set(main_module(), "counter", Value::Integer(0));

        assert!(store
            .get(main_module(), "threshold")
            .unwrap()
            .structural_eq(&Value::Integer(50)));
        assert!(store
            .get(main_module(), "counter")
            .unwrap()
            .structural_eq(&Value::Integer(0)));
    }

    #[test]
    fn global_store_contains_reports_presence() {
        let mut store = GlobalStore::new();
        store.set(main_module(), "x", Value::Integer(1));
        assert!(store.contains(main_module(), "x"));
        assert!(!store.contains(main_module(), "missing"));
    }

    #[test]
    fn global_store_same_local_name_in_different_modules() {
        let mut store = GlobalStore::new();
        let module_a = ModuleId(1);
        let module_b = ModuleId(2);
        store.set(module_a, "g", Value::Integer(1));
        store.set(module_b, "g", Value::Integer(2));
        assert!(store
            .get(module_a, "g")
            .unwrap()
            .structural_eq(&Value::Integer(1)));
        assert!(store
            .get(module_b, "g")
            .unwrap()
            .structural_eq(&Value::Integer(2)));
    }

    // -----------------------------------------------------------------------
    // GenericRegistry tests
    // -----------------------------------------------------------------------

    #[test]
    fn generic_registry_register_and_get() {
        let mut reg = GenericRegistry::new();
        reg.register_generic(main_module(), "display");
        assert!(reg.get(main_module(), "display").is_some());
        assert_eq!(reg.get(main_module(), "display").unwrap().name, "display");
    }

    #[test]
    fn generic_registry_get_missing_returns_none() {
        let reg = GenericRegistry::new();
        assert!(reg.get(main_module(), "nonexistent").is_none());
    }

    #[test]
    fn generic_registry_contains_reports_presence() {
        let mut reg = GenericRegistry::new();
        reg.register_generic(main_module(), "display");
        assert!(reg.contains(main_module(), "display"));
        assert!(!reg.contains(main_module(), "missing"));
    }

    #[test]
    fn generic_registry_method_auto_creates_generic() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            main_module(),
            "format",
            None,
            vec!["x".into()],
            vec![vec!["INTEGER".into()]],
            None,
            vec![],
        );
        assert!(reg.get(main_module(), "format").is_some());
        assert_eq!(reg.get(main_module(), "format").unwrap().methods.len(), 1);
    }

    #[test]
    fn generic_registry_methods_sorted_by_index() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            main_module(),
            "f",
            Some(3),
            vec!["x".into()],
            vec![vec![]],
            None,
            vec![],
        );
        reg.register_method(
            main_module(),
            "f",
            Some(1),
            vec!["x".into()],
            vec![vec!["INTEGER".into()]],
            None,
            vec![],
        );
        reg.register_method(
            main_module(),
            "f",
            Some(2),
            vec!["x".into()],
            vec![vec!["FLOAT".into()]],
            None,
            vec![],
        );
        let methods = &reg.get(main_module(), "f").unwrap().methods;
        assert_eq!(methods[0].index, 1);
        assert_eq!(methods[1].index, 2);
        assert_eq!(methods[2].index, 3);
    }

    #[test]
    fn generic_registry_auto_index_increments() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            main_module(),
            "f",
            None,
            vec!["x".into()],
            vec![vec!["INTEGER".into()]],
            None,
            vec![],
        );
        reg.register_method(
            main_module(),
            "f",
            None,
            vec!["x".into()],
            vec![vec!["FLOAT".into()]],
            None,
            vec![],
        );
        let methods = &reg.get(main_module(), "f").unwrap().methods;
        assert_eq!(methods[0].index, 1);
        assert_eq!(methods[1].index, 2);
    }

    #[test]
    fn generic_registry_explicit_index_updates_next_auto() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            main_module(),
            "f",
            Some(10),
            vec!["x".into()],
            vec![vec![]],
            None,
            vec![],
        );
        reg.register_method(
            main_module(),
            "f",
            None,
            vec!["x".into()],
            vec![vec![]],
            None,
            vec![],
        );
        let methods = &reg.get(main_module(), "f").unwrap().methods;
        assert_eq!(methods[0].index, 10);
        assert_eq!(methods[1].index, 11);
    }

    #[test]
    fn generic_registry_method_with_wildcard() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            main_module(),
            "f",
            None,
            vec!["x".into()],
            vec![vec![]],
            Some("rest".into()),
            vec![],
        );
        let method = &reg.get(main_module(), "f").unwrap().methods[0];
        assert_eq!(method.wildcard_parameter, Some("rest".into()));
    }

    #[test]
    fn generic_registry_has_method_index() {
        let mut reg = GenericRegistry::new();
        reg.register_method(
            main_module(),
            "f",
            Some(3),
            vec!["x".into()],
            vec![vec![]],
            None,
            vec![],
        );
        assert!(reg.has_method_index(main_module(), "f", 3));
        assert!(!reg.has_method_index(main_module(), "f", 2));
        assert!(!reg.has_method_index(main_module(), "missing", 3));
    }

    #[test]
    fn generic_registry_same_local_name_in_different_modules() {
        let mut reg = GenericRegistry::new();
        let module_a = ModuleId(1);
        let module_b = ModuleId(2);
        reg.register_generic(module_a, "g");
        reg.register_generic(module_b, "g");
        assert!(reg.get(module_a, "g").is_some());
        assert!(reg.get(module_b, "g").is_some());
    }

    #[test]
    fn debug_consistency_checks_pass_for_valid_state() {
        let mut fenv = FunctionEnv::new();
        fenv.register(
            main_module(),
            UserFunction {
                name: "f".to_string(),
                parameters: vec![],
                wildcard_parameter: None,
                body: vec![],
            },
        );
        fenv.debug_assert_consistency();

        let mut globals = GlobalStore::new();
        globals.set(main_module(), "g", Value::Integer(1));
        globals.debug_assert_consistency();

        let mut reg = GenericRegistry::new();
        reg.register_method(
            main_module(),
            "m",
            Some(1),
            vec!["x".into()],
            vec![vec![]],
            None,
            vec![],
        );
        reg.register_method(
            main_module(),
            "m",
            Some(2),
            vec!["x".into()],
            vec![vec![]],
            None,
            vec![],
        );
        reg.debug_assert_consistency();
    }
}
