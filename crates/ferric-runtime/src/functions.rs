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
#[derive(Clone, Debug)]
pub struct GlobalStore {
    values: ModuleNameMap<Value>,
    gensym_counter: i64,
    printout_events: Vec<(String, String)>,
}

impl Default for GlobalStore {
    fn default() -> Self {
        Self {
            values: HashMap::default(),
            gensym_counter: 1,
            printout_events: Vec::new(),
        }
    }
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

    /// Return the next `gensym` sequence number and increment internal state.
    pub fn next_gensym_counter(&mut self) -> i64 {
        let current = self.gensym_counter;
        self.gensym_counter = self.gensym_counter.saturating_add(1);
        current
    }

    /// Set the next value returned by `gensym`.
    pub fn set_gensym_counter(&mut self, value: i64) {
        self.gensym_counter = value.max(1);
    }

    /// Queue a deferred `printout` event emitted from expression evaluation.
    pub fn push_printout_event(&mut self, channel: String, text: String) {
        self.printout_events.push((channel, text));
    }

    /// Drain queued deferred `printout` events in FIFO order.
    pub fn take_printout_events(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.printout_events)
    }

    /// Clear all global variables (used during engine reset).
    pub fn clear(&mut self) {
        self.values.clear();
        self.printout_events.clear();
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
        assert!(
            self.gensym_counter >= 1,
            "gensym counter must remain positive"
        );
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
        let _ = store.next_gensym_counter();
        store.clear();

        assert!(store.get(main_module(), "a").is_none());
        assert!(store.get(main_module(), "b").is_none());
        assert_eq!(store.next_gensym_counter(), 2);
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

    #[test]
    fn global_store_gensym_counter_increments() {
        let mut store = GlobalStore::new();
        assert_eq!(store.next_gensym_counter(), 1);
        assert_eq!(store.next_gensym_counter(), 2);
        assert_eq!(store.next_gensym_counter(), 3);
    }

    #[test]
    fn global_store_set_gensym_counter_changes_next_value() {
        let mut store = GlobalStore::new();
        store.set_gensym_counter(10);
        assert_eq!(store.next_gensym_counter(), 10);
        assert_eq!(store.next_gensym_counter(), 11);
    }

    #[test]
    fn global_store_printout_events_roundtrip_and_drain() {
        let mut store = GlobalStore::new();
        store.push_printout_event("t".to_string(), "hello".to_string());
        store.push_printout_event("wtrace".to_string(), "trace".to_string());

        let events = store.take_printout_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], ("t".to_string(), "hello".to_string()));
        assert_eq!(events[1], ("wtrace".to_string(), "trace".to_string()));
        assert!(store.take_printout_events().is_empty());
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

    // -----------------------------------------------------------------------
    // Property tests: FunctionEnv & GlobalStore
    // -----------------------------------------------------------------------

    use proptest::prelude::*;

    /// Pre-defined pool of `ModuleId`s for property tests.
    fn prop_module_pool() -> [ModuleId; 3] {
        [ModuleId(0), ModuleId(1), ModuleId(2)]
    }

    /// Pre-defined pool of function/global names for property tests.
    const PROP_NAME_POOL: &[&str] = &["alpha", "beta", "gamma", "delta", "epsilon"];

    /// Build a minimal `UserFunction` with a given name and no body.
    fn make_user_func(name: &str) -> UserFunction {
        UserFunction {
            name: name.to_string(),
            parameters: vec![],
            wildcard_parameter: None,
            body: vec![],
        }
    }

    /// A single operation applied to `FunctionEnv` and the shadow model.
    #[derive(Clone, Debug)]
    enum FnOp {
        /// Register a user function at `modules[module_idx]` / `PROP_NAME_POOL[name_idx]`.
        Register { module_idx: usize, name_idx: usize },
        /// Query `get()` at `modules[module_idx]` / `PROP_NAME_POOL[name_idx]`.
        Get { module_idx: usize, name_idx: usize },
        /// Query `contains()` at `modules[module_idx]` / `PROP_NAME_POOL[name_idx]`.
        Contains { module_idx: usize, name_idx: usize },
    }

    fn fn_op_strategy() -> impl Strategy<Value = FnOp> {
        prop_oneof![
            3 => (any::<usize>(), any::<usize>())
                .prop_map(|(m, n)| FnOp::Register { module_idx: m, name_idx: n }),
            2 => (any::<usize>(), any::<usize>())
                .prop_map(|(m, n)| FnOp::Get { module_idx: m, name_idx: n }),
            2 => (any::<usize>(), any::<usize>())
                .prop_map(|(m, n)| FnOp::Contains { module_idx: m, name_idx: n }),
        ]
    }

    proptest! {
        /// Registering a function and retrieving it returns the same function.
        ///
        /// Invariants:
        /// - `get(module, name)` after `register` returns Some with the correct name.
        /// - `contains(module, name)` agrees with the shadow model's record.
        /// - `debug_assert_consistency` passes after every operation.
        #[test]
        fn function_env_register_get_contains_consistent(
            ops in prop::collection::vec(fn_op_strategy(), 0..60)
        ) {
            let modules = prop_module_pool();
            let mut env = FunctionEnv::new();
            // Shadow model: (module_id, name) → registered.
            let mut model: std::collections::HashMap<(ModuleId, String), bool> =
                std::collections::HashMap::new();

            for op in &ops {
                match *op {
                    FnOp::Register { module_idx, name_idx } => {
                        let module = modules[module_idx % modules.len()];
                        let name = PROP_NAME_POOL[name_idx % PROP_NAME_POOL.len()];
                        env.register(module, make_user_func(name));
                        model.insert((module, name.to_string()), true);
                    }
                    FnOp::Get { module_idx, name_idx } => {
                        let module = modules[module_idx % modules.len()];
                        let name = PROP_NAME_POOL[name_idx % PROP_NAME_POOL.len()];
                        let result = env.get(module, name);
                        let expected_present = model.contains_key(&(module, name.to_string()));
                        if expected_present {
                            // A registered function must be retrievable.
                            prop_assert!(
                                result.is_some(),
                                "get({module:?}, {name}) should return Some after register"
                            );
                            // The returned name must match the registration key.
                            prop_assert_eq!(
                                result.unwrap().name.as_str(),
                                name,
                                "retrieved function name must match registered name"
                            );
                        } else {
                            // An unregistered entry must return None.
                            prop_assert!(
                                result.is_none(),
                                "get({module:?}, {name}) should return None if not registered"
                            );
                        }
                    }
                    FnOp::Contains { module_idx, name_idx } => {
                        let module = modules[module_idx % modules.len()];
                        let name = PROP_NAME_POOL[name_idx % PROP_NAME_POOL.len()];
                        let actual = env.contains(module, name);
                        let expected = model.contains_key(&(module, name.to_string()));
                        // `contains` must agree with the shadow model.
                        prop_assert_eq!(
                            actual,
                            expected,
                            "contains({:?}, {}) mismatch",
                            module,
                            name
                        );
                    }
                }

                // After every step the registry must be internally consistent.
                env.debug_assert_consistency();
            }
        }

        /// Re-registering the same (module, name) pair replaces the old definition.
        ///
        /// Postcondition: the most-recently-registered function is the one retrieved.
        #[test]
        fn function_env_register_overwrites(
            module_idx in 0usize..3,
            name_idx in 0usize..5,
        ) {
            let modules = prop_module_pool();
            let module = modules[module_idx % modules.len()];
            let name = PROP_NAME_POOL[name_idx % PROP_NAME_POOL.len()];

            let mut env = FunctionEnv::new();
            // Register with one parameter.
            let mut first = make_user_func(name);
            first.parameters = vec!["x".to_string()];
            env.register(module, first);

            // Re-register with two parameters.
            let mut second = make_user_func(name);
            second.parameters = vec!["a".to_string(), "b".to_string()];
            env.register(module, second);

            // The latest registration must win.
            let retrieved = env.get(module, name).unwrap();
            prop_assert_eq!(
                retrieved.parameters.len(),
                2,
                "re-registered function must have the new parameter list"
            );

            env.debug_assert_consistency();
        }
    }

    // -----------------------------------------------------------------------
    // Property tests: GlobalStore
    // -----------------------------------------------------------------------

    /// A single operation applied to `GlobalStore` and the shadow model.
    #[derive(Clone, Debug)]
    enum GlobalOp {
        /// Set an integer value at `modules[module_idx]` / `PROP_NAME_POOL[name_idx]`.
        Set {
            module_idx: usize,
            name_idx: usize,
            value: i64,
        },
        /// Query `get()` for presence and value.
        Get { module_idx: usize, name_idx: usize },
        /// Advance gensym counter by one step.
        NextGensym,
        /// Set gensym counter to an arbitrary value (may be ≤ 0 to exercise floor).
        SetGensymCounter { value: i64 },
        /// Push a printout event drawn from small channel/text pools.
        PushPrintoutEvent { channel_idx: usize, text_idx: usize },
        /// Drain all queued printout events.
        TakePrintoutEvents,
        /// Clear the store.
        Clear,
    }

    const PROP_CHANNELS: &[&str] = &["t", "trace", "wdialog"];
    const PROP_TEXTS: &[&str] = &["hello", "world", "foo", "bar"];

    fn global_op_strategy() -> impl Strategy<Value = GlobalOp> {
        prop_oneof![
            3 => (any::<usize>(), any::<usize>(), any::<i64>())
                .prop_map(|(m, n, v)| GlobalOp::Set { module_idx: m, name_idx: n, value: v }),
            2 => (any::<usize>(), any::<usize>())
                .prop_map(|(m, n)| GlobalOp::Get { module_idx: m, name_idx: n }),
            2 => Just(GlobalOp::NextGensym),
            1 => any::<i64>().prop_map(|v| GlobalOp::SetGensymCounter { value: v }),
            2 => (any::<usize>(), any::<usize>())
                .prop_map(|(c, t)| GlobalOp::PushPrintoutEvent {
                    channel_idx: c,
                    text_idx: t,
                }),
            1 => Just(GlobalOp::TakePrintoutEvents),
            1 => Just(GlobalOp::Clear),
        ]
    }

    proptest! {
        /// Full shadow-model test for GlobalStore.
        ///
        /// Invariants verified after each step:
        /// - get after set returns the set integer value.
        /// - Unset keys return None from get().
        /// - next_gensym_counter returns the expected monotonically-advancing value.
        /// - set_gensym_counter floors at 1.
        /// - Printout events are drained in FIFO order; second drain returns empty.
        /// - clear removes values and events but preserves the gensym counter.
        /// - debug_assert_consistency passes at every step.
        #[test]
        fn global_store_shadow_model(
            ops in prop::collection::vec(global_op_strategy(), 0..80)
        ) {
            let modules = prop_module_pool();
            let mut store = GlobalStore::new();

            // Shadow model state.
            let mut model_values: std::collections::HashMap<(ModuleId, String), i64> =
                std::collections::HashMap::new();
            let mut model_gensym: i64 = 1;
            let mut model_events: std::collections::VecDeque<(String, String)> =
                std::collections::VecDeque::new();

            for op in &ops {
                match *op {
                    GlobalOp::Set { module_idx, name_idx, value } => {
                        let module = modules[module_idx % modules.len()];
                        let name = PROP_NAME_POOL[name_idx % PROP_NAME_POOL.len()];
                        store.set(module, name, Value::Integer(value));
                        model_values.insert((module, name.to_string()), value);
                    }
                    GlobalOp::Get { module_idx, name_idx } => {
                        let module = modules[module_idx % modules.len()];
                        let name = PROP_NAME_POOL[name_idx % PROP_NAME_POOL.len()];
                        let actual = store.get(module, name);
                        if let Some(&expected_val) =
                            model_values.get(&(module, name.to_string()))
                        {
                            // Key must be present and return the correct integer.
                            let val =
                                actual.expect("get should return Some for a set key");
                            prop_assert!(
                                val.structural_eq(&Value::Integer(expected_val)),
                                "get({module:?}, {name}) returned wrong value"
                            );
                        } else {
                            // Key must be absent.
                            prop_assert!(
                                actual.is_none(),
                                "get({module:?}, {name}) should return None for unset key"
                            );
                        }
                    }
                    GlobalOp::NextGensym => {
                        let returned = store.next_gensym_counter();
                        // Must return the model's expected counter value.
                        prop_assert_eq!(
                            returned,
                            model_gensym,
                            "next_gensym_counter must return model counter"
                        );
                        // Saturating-add mirrors the implementation.
                        model_gensym = model_gensym.saturating_add(1);
                    }
                    GlobalOp::SetGensymCounter { value } => {
                        store.set_gensym_counter(value);
                        // Implementation floors at 1.
                        model_gensym = value.max(1);
                    }
                    GlobalOp::PushPrintoutEvent { channel_idx, text_idx } => {
                        let channel =
                            PROP_CHANNELS[channel_idx % PROP_CHANNELS.len()].to_string();
                        let text = PROP_TEXTS[text_idx % PROP_TEXTS.len()].to_string();
                        store.push_printout_event(channel.clone(), text.clone());
                        model_events.push_back((channel, text));
                    }
                    GlobalOp::TakePrintoutEvents => {
                        let actual_events = store.take_printout_events();
                        // Must match the model queue in FIFO order.
                        let expected: Vec<_> = model_events.drain(..).collect();
                        prop_assert_eq!(
                            actual_events,
                            expected,
                            "take_printout_events must return events in FIFO order"
                        );
                        // After draining, a second take must return empty.
                        let again = store.take_printout_events();
                        prop_assert!(
                            again.is_empty(),
                            "second take_printout_events must return empty vec"
                        );
                    }
                    GlobalOp::Clear => {
                        store.clear();
                        // Values and events are removed; gensym counter is preserved.
                        model_values.clear();
                        model_events.clear();
                        // model_gensym intentionally NOT reset — clear preserves it.
                    }
                }

                // After every step the store must be internally consistent.
                store.debug_assert_consistency();
            }
        }

        /// Gensym counter increments strictly.
        ///
        /// Invariant: each successive call to `next_gensym_counter` returns a value
        /// strictly greater than the one before it.
        #[test]
        fn gensym_counter_strictly_increasing(n_steps in 1usize..50) {
            let mut store = GlobalStore::new();
            let mut prev = store.next_gensym_counter();
            for _ in 1..n_steps {
                let next = store.next_gensym_counter();
                prop_assert!(
                    next > prev,
                    "gensym counter must be strictly increasing: {prev} then {next}"
                );
                prev = next;
            }
        }

        /// `set_gensym_counter` floors at 1 for any non-positive input.
        ///
        /// Invariant: calling `set_gensym_counter(v)` with v ≤ 0 causes
        /// `next_gensym_counter()` to return 1.
        #[test]
        fn set_gensym_counter_floors_at_one(v in i64::MIN..=0i64) {
            let mut store = GlobalStore::new();
            store.set_gensym_counter(v);
            let result = store.next_gensym_counter();
            prop_assert_eq!(
                result,
                1,
                "set_gensym_counter({}) must floor at 1",
                v
            );
        }

        /// `clear` preserves the gensym counter.
        ///
        /// Invariant: the gensym counter value in effect just before `clear()`
        /// is returned correctly immediately after.
        #[test]
        fn clear_preserves_gensym_counter(advance in 1usize..20) {
            let mut store = GlobalStore::new();
            let mut expected_next: i64 = 1;
            for _ in 0..advance {
                store.next_gensym_counter();
                expected_next = expected_next.saturating_add(1);
            }
            store.clear();
            let after_clear = store.next_gensym_counter();
            prop_assert_eq!(
                after_clear,
                expected_next,
                "clear must not reset the gensym counter"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Property tests: GenericFunction & GenericRegistry
    // -----------------------------------------------------------------------

    /// Helper to build a minimal `RegisteredMethod` for testing.
    fn make_method(index: i32) -> RegisteredMethod {
        RegisteredMethod {
            index,
            parameters: vec!["x".to_string()],
            type_restrictions: vec![vec![]],
            wildcard_parameter: None,
            body: vec![],
        }
    }

    proptest! {
        /// Invariant: After inserting methods with arbitrary indices (0..100),
        /// the methods vec is always sorted in strictly ascending order by index.
        /// This verifies that `add_method` maintains the sorted invariant under
        /// any insertion order.
        #[test]
        fn methods_always_sorted(indices in prop::collection::vec(0i32..100, 1..20)) {
            let mut gf = GenericFunction::new("test".to_string());
            // Track which indices we've already added to avoid duplicates
            // (duplicate indices violate the strict-ascending invariant).
            let mut seen = std::collections::HashSet::new();
            for idx in &indices {
                if seen.insert(*idx) {
                    gf.add_method(make_method(*idx));
                }
            }
            // Invariant: methods are in strictly ascending index order.
            for w in gf.methods.windows(2) {
                prop_assert!(
                    w[0].index < w[1].index,
                    "methods not strictly sorted: {} then {}",
                    w[0].index, w[1].index
                );
            }
        }

        /// Invariant: Each successive call to `next_auto_index` returns a value
        /// strictly larger than the previous. Auto-index is monotonically increasing.
        #[test]
        fn next_auto_index_always_increases(n_calls in 1usize..20) {
            let mut gf = GenericFunction::new("test".to_string());
            let mut prev = gf.next_auto_index();
            for _ in 1..n_calls {
                let next = gf.next_auto_index();
                prop_assert!(
                    next > prev,
                    "next_auto_index did not increase: got {} after {}",
                    next, prev
                );
                prev = next;
            }
        }
    }

    /// Operations for the `GenericRegistry` shadow-model property test.
    #[derive(Clone, Debug)]
    #[allow(clippy::enum_variant_names)]
    enum GenericOp {
        /// Register a generic by (`module_idx`, `name_idx`) — no-op if already present.
        RegisterGeneric(usize, usize),
        /// Register a method with an explicit index.
        RegisterMethodExplicit(usize, usize, i32),
        /// Register a method with an auto-assigned index.
        RegisterMethodAuto(usize, usize),
    }

    fn generic_op_strategy() -> impl Strategy<Value = GenericOp> {
        prop_oneof![
            2 => (0usize..3, 0usize..5).prop_map(|(m, n)| GenericOp::RegisterGeneric(m, n)),
            3 => (0usize..3, 0usize..5, 1i32..50).prop_map(|(m, n, i)| GenericOp::RegisterMethodExplicit(m, n, i)),
            3 => (0usize..3, 0usize..5).prop_map(|(m, n)| GenericOp::RegisterMethodAuto(m, n)),
        ]
    }

    proptest! {
        /// Shadow-model invariant test for GenericRegistry.
        ///
        /// We execute a random sequence of register_generic / register_method
        /// operations against both a `GenericRegistry` and a shadow model
        /// (HashMap tracking which (module, name) entries exist and their method
        /// indices), then verify at every step that:
        ///   - register/get roundtrip is consistent
        ///   - method auto-creation via register_method works
        ///   - has_method_index agrees with the shadow model
        ///   - debug_assert_consistency() passes (sorted methods, name bookkeeping)
        #[test]
        fn generic_registry_shadow_model(
            ops in prop::collection::vec(generic_op_strategy(), 1..40)
        ) {
            // Pre-allocate 3 module ids and 5 generic names to keep the space small.
            let modules = [ModuleId(0), ModuleId(1), ModuleId(2)];
            let names = ["alpha", "beta", "gamma", "delta", "epsilon"];

            // Shadow model: tracks method indices per (module_idx, name) pair.
            // None means the generic has never been registered.
            let mut shadow: std::collections::HashMap<(usize, usize), std::collections::BTreeSet<i32>> =
                std::collections::HashMap::new();
            // Track the next auto-index per (module, name) key.
            let mut next_auto: std::collections::HashMap<(usize, usize), i32> =
                std::collections::HashMap::new();

            let mut reg = GenericRegistry::new();

            for op in &ops {
                match op {
                    GenericOp::RegisterGeneric(mi, ni) => {
                        let module = modules[*mi];
                        let name = names[*ni];
                        reg.register_generic(module, name);
                        // Shadow: ensure entry exists (no-op if already present).
                        shadow.entry((*mi, *ni)).or_default();
                    }
                    GenericOp::RegisterMethodExplicit(mi, ni, idx) => {
                        let module = modules[*mi];
                        let name = names[*ni];
                        // Only add if the index is not already present in the shadow
                        // (duplicate indices would violate the sorted-unique invariant).
                        let key = (*mi, *ni);
                        let indices = shadow.entry(key).or_default();
                        if !indices.contains(idx) {
                            indices.insert(*idx);
                            // Update next_auto to be at least idx+1.
                            let na = next_auto.entry(key).or_insert(1);
                            if *idx >= *na {
                                *na = *idx + 1;
                            }
                            reg.register_method(
                                module,
                                name,
                                Some(*idx),
                                vec!["x".to_string()],
                                vec![vec![]],
                                None,
                                vec![],
                            );
                        }
                    }
                    GenericOp::RegisterMethodAuto(mi, ni) => {
                        let module = modules[*mi];
                        let name = names[*ni];
                        let key = (*mi, *ni);
                        // Compute the auto-index from our shadow tracker.
                        let na = next_auto.entry(key).or_insert(1);
                        let auto_idx = *na;
                        *na += 1;
                        shadow.entry(key).or_default().insert(auto_idx);
                        reg.register_method(
                            module,
                            name,
                            None,
                            vec!["x".to_string()],
                            vec![vec![]],
                            None,
                            vec![],
                        );
                    }
                }

                // After every operation, verify invariants against the shadow model.

                // Structural consistency (sorted methods, name bookkeeping).
                reg.debug_assert_consistency();

                // Verify every entry in the shadow model is reflected in the registry.
                for ((mi, ni), expected_indices) in &shadow {
                    let module = modules[*mi];
                    let name = names[*ni];

                    // The generic must exist since we registered it.
                    prop_assert!(
                        reg.contains(module, name),
                        "registry missing generic ({}, {})",
                        *mi, *ni
                    );

                    // get() must return the same generic.
                    let gf = reg.get(module, name).unwrap();
                    prop_assert_eq!(
                        &gf.name, name,
                        "generic name mismatch for ({}, {})",
                        *mi, *ni
                    );

                    // has_method_index must agree with shadow for known indices.
                    for &idx in expected_indices {
                        prop_assert!(
                            reg.has_method_index(module, name, idx),
                            "has_method_index returned false for known index {} in ({}, {})",
                            idx, *mi, *ni
                        );
                    }
                }
            }
        }
    }
}
