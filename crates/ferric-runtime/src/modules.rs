//! Module registry and focus stack for the CLIPS module system.
//!
//! Modules scope rule execution via the focus stack. Facts are global;
//! modules only affect which rules fire (based on focus) and which
//! templates/constructs are visible across module boundaries.

use std::collections::HashSet;
use ferric_parser::{ImportSpec, ModuleSpec};
use rustc_hash::FxHashMap as HashMap;

/// Simple module identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModuleId(pub u32);

/// A registered module in the engine.
#[derive(Clone, Debug)]
pub struct RuntimeModule {
    /// Module name (e.g., "MAIN", "SENSOR").
    pub name: String,
    /// Export specifications.
    pub exports: Vec<ModuleSpec>,
    /// Import specifications.
    pub imports: Vec<ImportSpec>,
}

/// The well-known MAIN module name.
pub const MAIN_MODULE_NAME: &str = "MAIN";

/// Module registry with focus stack.
///
/// Manages module definitions, import/export visibility, and the focus
/// stack that controls which module's rules fire during execution.
pub struct ModuleRegistry {
    modules: HashMap<ModuleId, RuntimeModule>,
    name_to_id: HashMap<Box<str>, ModuleId>,
    next_id: u32,
    /// The module that new constructs belong to.
    current_module: ModuleId,
    /// Focus stack for execution ordering (top = last element).
    focus_stack: Vec<ModuleId>,
    /// The MAIN module ID (stored for convenience).
    main_module_id: ModuleId,
}

impl ModuleRegistry {
    /// Create a new registry with the default MAIN module.
    ///
    /// MAIN exports `?ALL` and imports nothing.
    #[must_use]
    pub fn new() -> Self {
        let main_id = ModuleId(0);
        let main_module = RuntimeModule {
            name: MAIN_MODULE_NAME.to_string(),
            exports: vec![ModuleSpec::All],
            imports: vec![],
        };
        let mut modules = HashMap::default();
        modules.insert(main_id, main_module);
        let mut name_to_id = HashMap::default();
        name_to_id.insert(MAIN_MODULE_NAME.into(), main_id);

        Self {
            modules,
            name_to_id,
            next_id: 1,
            current_module: main_id,
            focus_stack: vec![main_id],
            main_module_id: main_id,
        }
    }

    /// Register a new module or update an existing one.
    ///
    /// Returns the module's ID. If a module with the same name already
    /// exists, its exports and imports are replaced.
    pub fn register(
        &mut self,
        name: &str,
        exports: Vec<ModuleSpec>,
        imports: Vec<ImportSpec>,
    ) -> ModuleId {
        if let Some(&existing_id) = self.name_to_id.get(name) {
            if let Some(module) = self.modules.get_mut(&existing_id) {
                module.exports = exports;
                module.imports = imports;
            }
            return existing_id;
        }

        let id = ModuleId(self.next_id);
        self.next_id += 1;
        let name_owned = name.to_string();
        let module = RuntimeModule {
            name: name_owned.clone(),
            exports,
            imports,
        };
        self.modules.insert(id, module);
        self.name_to_id.insert(name_owned.into_boxed_str(), id);
        id
    }

    /// Get a module by ID.
    #[must_use]
    pub fn get(&self, id: ModuleId) -> Option<&RuntimeModule> {
        self.modules.get(&id)
    }

    /// Get a module's ID by name.
    #[must_use]
    pub fn get_by_name(&self, name: &str) -> Option<ModuleId> {
        self.name_to_id.get(name).copied()
    }

    /// Get the MAIN module ID.
    #[must_use]
    pub fn main_module_id(&self) -> ModuleId {
        self.main_module_id
    }

    /// Get the current module (constructs defined now belong to this module).
    #[must_use]
    pub fn current_module(&self) -> ModuleId {
        self.current_module
    }

    /// Set the current module.
    pub fn set_current_module(&mut self, id: ModuleId) {
        self.current_module = id;
    }

    /// Push a module onto the focus stack.
    pub fn push_focus(&mut self, id: ModuleId) {
        self.focus_stack.push(id);
    }

    /// Replace the focus stack with a single module.
    pub fn set_focus(&mut self, id: ModuleId) {
        self.focus_stack.clear();
        self.focus_stack.push(id);
    }

    /// Pop the top module from the focus stack.
    pub fn pop_focus(&mut self) -> Option<ModuleId> {
        self.focus_stack.pop()
    }

    /// Get the module at the top of the focus stack.
    #[must_use]
    pub fn current_focus(&self) -> Option<ModuleId> {
        self.focus_stack.last().copied()
    }

    /// Get the full focus stack (bottom to top).
    #[must_use]
    pub fn focus_stack(&self) -> &[ModuleId] {
        &self.focus_stack
    }

    /// Reset the focus stack to contain only MAIN and set current module to MAIN.
    pub fn reset_focus(&mut self) {
        self.focus_stack.clear();
        self.focus_stack.push(self.main_module_id);
        self.current_module = self.main_module_id;
    }

    /// Get a module's name by ID.
    #[must_use]
    pub fn module_name(&self, id: ModuleId) -> Option<&str> {
        self.modules.get(&id).map(|m| m.name.as_str())
    }

    fn specific_spec_matches(
        spec_construct_type: &str,
        names: &[String],
        construct_type: &str,
        construct_name: &str,
    ) -> bool {
        if spec_construct_type != construct_type {
            return false;
        }
        let mut has_none = false;
        let mut has_name_match = false;
        for name in names {
            match name.as_str() {
                "?ALL" => return true,
                "?NONE" => has_none = true,
                _ if name == construct_name => has_name_match = true,
                _ => {}
            }
        }
        if has_none {
            return false;
        }
        has_name_match
    }

    fn spec_matches(spec: &ModuleSpec, construct_type: &str, construct_name: &str) -> bool {
        match spec {
            ModuleSpec::All => true,
            ModuleSpec::None => false,
            ModuleSpec::Specific {
                construct_type: ct,
                names,
            } => Self::specific_spec_matches(ct, names, construct_type, construct_name),
        }
    }

    fn module_exports_construct(
        &self,
        module_id: ModuleId,
        construct_type: &str,
        construct_name: &str,
    ) -> bool {
        let Some(module) = self.modules.get(&module_id) else {
            return false;
        };

        // CLIPS default behavior: if no explicit export spec is provided,
        // the module behaves as though it exports ?ALL.
        if module.exports.is_empty() {
            return true;
        }

        module
            .exports
            .iter()
            .any(|spec| Self::spec_matches(spec, construct_type, construct_name))
    }

    fn is_construct_visible_recursive(
        &self,
        from_module: ModuleId,
        owning_module: ModuleId,
        construct_type: &str,
        construct_name: &str,
        visiting: &mut HashSet<(ModuleId, ModuleId)>,
    ) -> bool {
        if from_module == owning_module {
            return true;
        }

        if !visiting.insert((from_module, owning_module)) {
            return false;
        }

        let visible = self.modules.get(&from_module).is_some_and(|importer| {
            importer.imports.iter().any(|import| {
                let Some(&import_from_id) = self.name_to_id.get(import.module_name.as_str()) else {
                    return false;
                };

                if !Self::spec_matches(&import.spec, construct_type, construct_name) {
                    return false;
                }

                self.module_exports_construct(import_from_id, construct_type, construct_name)
                    && self.is_construct_visible_recursive(
                        import_from_id,
                        owning_module,
                        construct_type,
                        construct_name,
                        visiting,
                    )
            })
        });

        visiting.remove(&(from_module, owning_module));
        visible
    }

    /// Check if a construct is visible from `from_module`.
    ///
    /// A construct is visible if:
    /// 1. It belongs to `from_module` (always visible within own module), OR
    /// 2. The owning module exports it AND `from_module` imports it.
    #[must_use]
    pub fn is_construct_visible(
        &self,
        from_module: ModuleId,
        owning_module: ModuleId,
        construct_type: &str,
        construct_name: &str,
    ) -> bool {
        let mut visiting = HashSet::new();
        self.is_construct_visible_recursive(
            from_module,
            owning_module,
            construct_type,
            construct_name,
            &mut visiting,
        )
    }

    /// Debug-only structural checks for module/focus bookkeeping.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        assert!(
            self.modules.contains_key(&self.main_module_id),
            "module registry missing MAIN module id {:?}",
            self.main_module_id
        );
        assert!(
            self.name_to_id.get(MAIN_MODULE_NAME) == Some(&self.main_module_id),
            "name_to_id missing MAIN -> {:?}",
            self.main_module_id
        );
        assert!(
            self.modules.contains_key(&self.current_module),
            "current_module {:?} missing from modules",
            self.current_module
        );
        assert!(
            !self.focus_stack.is_empty(),
            "focus stack must never be empty during consistency checks"
        );
        for &module_id in &self.focus_stack {
            assert!(
                self.modules.contains_key(&module_id),
                "focus stack contains unknown module id {module_id:?}"
            );
        }

        for (name, id) in &self.name_to_id {
            let module = self
                .modules
                .get(id)
                .unwrap_or_else(|| panic!("name_to_id points to unknown module id {id:?}"));
            assert_eq!(
                module.name.as_str(),
                name.as_ref(),
                "name_to_id key `{name}` does not match module.name `{}`",
                module.name
            );
        }

        for (id, module) in &self.modules {
            let mapped = self.name_to_id.get(module.name.as_str());
            assert_eq!(
                mapped,
                Some(id),
                "module `{}` id {:?} missing reverse mapping",
                module.name,
                id
            );
        }
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_span() -> ferric_parser::Span {
        ferric_parser::Span::new(
            ferric_parser::Position {
                offset: 0,
                line: 1,
                column: 1,
            },
            ferric_parser::Position {
                offset: 0,
                line: 1,
                column: 1,
            },
            ferric_parser::FileId(0),
        )
    }

    #[test]
    fn new_registry_has_main_module() {
        let registry = ModuleRegistry::new();
        let main_id = registry.main_module_id();
        let main = registry.get(main_id).unwrap();
        assert_eq!(main.name, "MAIN");
        assert_eq!(registry.current_module(), main_id);
        assert_eq!(registry.current_focus(), Some(main_id));
    }

    #[test]
    fn register_new_module() {
        let mut registry = ModuleRegistry::new();
        let id = registry.register("SENSOR", vec![ModuleSpec::All], vec![]);
        assert_ne!(id, registry.main_module_id());
        assert_eq!(registry.module_name(id), Some("SENSOR"));
        assert_eq!(registry.get_by_name("SENSOR"), Some(id));
    }

    #[test]
    fn register_existing_module_updates() {
        let mut registry = ModuleRegistry::new();
        let id1 = registry.register("SENSOR", vec![ModuleSpec::All], vec![]);
        let id2 = registry.register("SENSOR", vec![ModuleSpec::None], vec![]);
        assert_eq!(id1, id2);
        let module = registry.get(id1).unwrap();
        assert!(matches!(module.exports[0], ModuleSpec::None));
    }

    #[test]
    fn focus_stack_push_pop() {
        let mut registry = ModuleRegistry::new();
        let main_id = registry.main_module_id();
        let sensor_id = registry.register("SENSOR", vec![], vec![]);

        assert_eq!(registry.current_focus(), Some(main_id));

        registry.push_focus(sensor_id);
        assert_eq!(registry.current_focus(), Some(sensor_id));

        let popped = registry.pop_focus();
        assert_eq!(popped, Some(sensor_id));
        assert_eq!(registry.current_focus(), Some(main_id));
    }

    #[test]
    fn reset_focus_restores_main() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register("SENSOR", vec![], vec![]);
        registry.set_current_module(sensor_id);
        registry.push_focus(sensor_id);

        registry.reset_focus();
        assert_eq!(registry.current_module(), registry.main_module_id());
        assert_eq!(registry.current_focus(), Some(registry.main_module_id()));
    }

    #[test]
    fn set_focus_replaces_stack() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register("SENSOR", vec![], vec![]);
        registry.push_focus(sensor_id);
        registry.set_focus(sensor_id);
        assert_eq!(registry.focus_stack(), &[sensor_id]);
        assert_eq!(registry.current_focus(), Some(sensor_id));
    }

    #[test]
    fn visibility_same_module_always_visible() {
        let registry = ModuleRegistry::new();
        let main = registry.main_module_id();
        assert!(registry.is_construct_visible(main, main, "deftemplate", "person"));
    }

    #[test]
    fn visibility_exported_and_imported() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register(
            "SENSOR",
            vec![ModuleSpec::Specific {
                construct_type: "deftemplate".to_string(),
                names: vec!["reading".to_string()],
            }],
            vec![],
        );

        // Register MAIN with import from SENSOR
        registry.register(
            "MAIN",
            vec![ModuleSpec::All],
            vec![ImportSpec {
                module_name: "SENSOR".to_string(),
                spec: ModuleSpec::All,
                span: dummy_span(),
            }],
        );

        let main = registry.main_module_id();
        assert!(registry.is_construct_visible(main, sensor_id, "deftemplate", "reading"));
    }

    #[test]
    fn visibility_defaults_to_export_all_when_unspecified() {
        let mut registry = ModuleRegistry::new();
        let source_id = registry.register("SOURCE", vec![], vec![]);
        registry.register(
            "MAIN",
            vec![ModuleSpec::All],
            vec![ImportSpec {
                module_name: "SOURCE".to_string(),
                spec: ModuleSpec::All,
                span: dummy_span(),
            }],
        );

        let main = registry.main_module_id();
        assert!(registry.is_construct_visible(main, source_id, "deftemplate", "reading"));
    }

    #[test]
    fn visibility_supports_transitive_reexports() {
        let mut registry = ModuleRegistry::new();
        let source_id = registry.register(
            "SOURCE",
            vec![ModuleSpec::Specific {
                construct_type: "deftemplate".to_string(),
                names: vec!["reading".to_string()],
            }],
            vec![],
        );

        registry.register(
            "MID",
            vec![ModuleSpec::All],
            vec![ImportSpec {
                module_name: "SOURCE".to_string(),
                spec: ModuleSpec::All,
                span: dummy_span(),
            }],
        );

        let leaf_id = registry.register(
            "LEAF",
            vec![],
            vec![ImportSpec {
                module_name: "MID".to_string(),
                spec: ModuleSpec::All,
                span: dummy_span(),
            }],
        );

        assert!(registry.is_construct_visible(leaf_id, source_id, "deftemplate", "reading"));
    }

    #[test]
    fn visibility_not_exported_not_visible() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register("SENSOR", vec![ModuleSpec::None], vec![]);
        let main = registry.main_module_id();
        assert!(!registry.is_construct_visible(main, sensor_id, "deftemplate", "reading"));
    }

    #[test]
    fn visibility_exported_but_not_imported() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register("SENSOR", vec![ModuleSpec::All], vec![]);
        let main = registry.main_module_id();
        // MAIN has no imports from SENSOR
        assert!(!registry.is_construct_visible(main, sensor_id, "deftemplate", "reading"));
    }

    #[test]
    fn visibility_typed_export_all_matches_construct_type() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register(
            "SENSOR",
            vec![ModuleSpec::Specific {
                construct_type: "deftemplate".to_string(),
                names: vec!["?ALL".to_string()],
            }],
            vec![],
        );
        registry.register(
            "MAIN",
            vec![ModuleSpec::All],
            vec![ImportSpec {
                module_name: "SENSOR".to_string(),
                spec: ModuleSpec::All,
                span: dummy_span(),
            }],
        );

        let main = registry.main_module_id();
        assert!(registry.is_construct_visible(main, sensor_id, "deftemplate", "reading"));
        assert!(!registry.is_construct_visible(main, sensor_id, "defrule", "reading"));
    }

    #[test]
    fn visibility_typed_import_all_matches_construct_type() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register(
            "SENSOR",
            vec![ModuleSpec::Specific {
                construct_type: "deftemplate".to_string(),
                names: vec!["reading".to_string()],
            }],
            vec![],
        );
        registry.register(
            "MAIN",
            vec![ModuleSpec::All],
            vec![ImportSpec {
                module_name: "SENSOR".to_string(),
                spec: ModuleSpec::Specific {
                    construct_type: "deftemplate".to_string(),
                    names: vec!["?ALL".to_string()],
                },
                span: dummy_span(),
            }],
        );

        let main = registry.main_module_id();
        assert!(registry.is_construct_visible(main, sensor_id, "deftemplate", "reading"));
        assert!(!registry.is_construct_visible(main, sensor_id, "defrule", "reading"));
    }

    #[test]
    fn visibility_typed_none_blocks_visibility() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register(
            "SENSOR",
            vec![ModuleSpec::Specific {
                construct_type: "deftemplate".to_string(),
                names: vec!["?NONE".to_string()],
            }],
            vec![],
        );
        registry.register(
            "MAIN",
            vec![ModuleSpec::All],
            vec![ImportSpec {
                module_name: "SENSOR".to_string(),
                spec: ModuleSpec::Specific {
                    construct_type: "deftemplate".to_string(),
                    names: vec!["?ALL".to_string()],
                },
                span: dummy_span(),
            }],
        );

        let main = registry.main_module_id();
        assert!(!registry.is_construct_visible(main, sensor_id, "deftemplate", "reading"));
    }

    #[test]
    fn debug_consistency_passes_on_valid_registry() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register("SENSOR", vec![ModuleSpec::All], vec![]);
        registry.push_focus(sensor_id);
        registry.debug_assert_consistency();
    }

    // -----------------------------------------------------------------------
    // Property-based tests
    // -----------------------------------------------------------------------

    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        /// Pre-defined module name pool — operations index into this pool so we
        /// avoid open-ended string generation while still covering multi-module
        /// interactions.
        const MODULE_NAMES: &[&str] = &["MOD_A", "MOD_B", "MOD_C", "MOD_D", "MOD_E"];

        /// A single operation applied to both the real `ModuleRegistry` and the
        /// shadow model.
        #[derive(Clone, Debug)]
        enum Op {
            /// Register the module at `MODULE_NAMES[name_idx % len]`.
            Register { name_idx: usize },
            /// Push a previously-registered module onto the focus stack.
            /// Skipped if no modules have been registered beyond MAIN.
            PushFocus { module_idx: usize },
            /// Pop the top of the focus stack.
            /// Skipped when the stack would become empty (to keep consistency
            /// check valid — `debug_assert_consistency` requires a non-empty stack).
            PopFocus,
            /// Replace the focus stack with a single previously-registered module.
            SetFocus { module_idx: usize },
            /// Reset focus back to MAIN and set current module to MAIN.
            ResetFocus,
            /// Change the current (construct-owning) module to a registered one.
            SetCurrentModule { module_idx: usize },
        }

        fn op_strategy() -> impl Strategy<Value = Op> {
            prop_oneof![
                3 => any::<usize>().prop_map(|n| Op::Register { name_idx: n }),
                2 => any::<usize>().prop_map(|m| Op::PushFocus { module_idx: m }),
                1 => Just(Op::PopFocus),
                2 => any::<usize>().prop_map(|m| Op::SetFocus { module_idx: m }),
                1 => Just(Op::ResetFocus),
                2 => any::<usize>().prop_map(|m| Op::SetCurrentModule { module_idx: m }),
            ]
        }

        fn scenario_strategy() -> impl Strategy<Value = Vec<Op>> {
            prop::collection::vec(op_strategy(), 0..60)
        }

        /// Shadow model for `ModuleRegistry`: tracks what state we expect the
        /// real implementation to be in.
        struct Model {
            /// Maps module name → assigned `ModuleId`.  MAIN is always pre-populated.
            name_to_id: std::collections::HashMap<String, ModuleId>,
            /// Currently-registered module IDs in registration order.
            registered_ids: Vec<ModuleId>,
            /// Focus stack (last element = top).
            focus_stack: Vec<ModuleId>,
            /// Current module.
            current_module: ModuleId,
        }

        impl Model {
            fn new(main_id: ModuleId) -> Self {
                let mut name_to_id = std::collections::HashMap::new();
                name_to_id.insert("MAIN".to_string(), main_id);
                Self {
                    name_to_id,
                    registered_ids: vec![main_id],
                    focus_stack: vec![main_id],
                    current_module: main_id,
                }
            }

            /// Return the model's focus-top.
            fn current_focus(&self) -> Option<ModuleId> {
                self.focus_stack.last().copied()
            }

            /// Pick a registered module by index (wrapping).
            fn pick_registered(&self, idx: usize) -> Option<ModuleId> {
                if self.registered_ids.is_empty() {
                    return None;
                }
                Some(self.registered_ids[idx % self.registered_ids.len()])
            }
        }

        /// Apply one `Op` to both the real registry and the shadow model.
        ///
        /// Returns `true` if the op was applied and `false` if it was a no-op
        /// (e.g., a focus-manipulation op when no extra modules are registered).
        fn apply_op(reg: &mut ModuleRegistry, model: &mut Model, op: &Op) -> bool {
            match *op {
                Op::Register { name_idx } => {
                    let name = MODULE_NAMES[name_idx % MODULE_NAMES.len()];
                    let id = reg.register(name, vec![], vec![]);
                    // Mirror: if not yet seen, add to model.
                    model.name_to_id.entry(name.to_string()).or_insert_with(|| {
                        model.registered_ids.push(id);
                        id
                    });
                    true
                }
                Op::PushFocus { module_idx } => {
                    let Some(id) = model.pick_registered(module_idx) else {
                        return false;
                    };
                    reg.push_focus(id);
                    model.focus_stack.push(id);
                    true
                }
                Op::PopFocus => {
                    // Only pop when the stack has more than one element so it
                    // stays non-empty and debug_assert_consistency remains valid.
                    if model.focus_stack.len() <= 1 {
                        return false;
                    }
                    reg.pop_focus();
                    model.focus_stack.pop();
                    true
                }
                Op::SetFocus { module_idx } => {
                    let Some(id) = model.pick_registered(module_idx) else {
                        return false;
                    };
                    reg.set_focus(id);
                    model.focus_stack.clear();
                    model.focus_stack.push(id);
                    true
                }
                Op::ResetFocus => {
                    let main_id = reg.main_module_id();
                    reg.reset_focus();
                    model.focus_stack.clear();
                    model.focus_stack.push(main_id);
                    model.current_module = main_id;
                    true
                }
                Op::SetCurrentModule { module_idx } => {
                    let Some(id) = model.pick_registered(module_idx) else {
                        return false;
                    };
                    reg.set_current_module(id);
                    model.current_module = id;
                    true
                }
            }
        }

        proptest! {
            /// After every operation the internal consistency check passes.
            ///
            /// Invariant: `debug_assert_consistency` must never panic for any
            /// sequence of valid operations.
            #[test]
            fn arbitrary_ops_maintain_consistency(ops in scenario_strategy()) {
                let mut reg = ModuleRegistry::new();
                let main_id = reg.main_module_id();
                let mut model = Model::new(main_id);

                for op in &ops {
                    apply_op(&mut reg, &mut model, op);
                    // After every step the registry must be internally consistent.
                    reg.debug_assert_consistency();
                }
            }

            /// The shadow model stays in sync with the real implementation.
            ///
            /// Invariants verified after each step:
            /// - Every name in the model maps to the correct ModuleId in the registry.
            /// - The model's focus stack top matches `current_focus()`.
            /// - The model's current module matches `current_module()`.
            /// - MAIN always exists and is retrievable by name.
            #[test]
            fn model_matches_implementation(ops in scenario_strategy()) {
                let mut reg = ModuleRegistry::new();
                let main_id = reg.main_module_id();
                let mut model = Model::new(main_id);

                for op in &ops {
                    apply_op(&mut reg, &mut model, op);

                    // Name→ID and ID→name are bidirectional for all registered modules.
                    for (name, &expected_id) in &model.name_to_id {
                        let actual_id = reg.get_by_name(name);
                        prop_assert_eq!(
                            actual_id,
                            Some(expected_id),
                            "get_by_name({}) mismatch", name
                        );
                        let actual_name = reg.module_name(expected_id);
                        prop_assert_eq!(
                            actual_name,
                            Some(name.as_str()),
                            "module_name({:?}) mismatch", expected_id
                        );
                    }

                    // Focus stack top matches the model's expectation.
                    prop_assert_eq!(
                        reg.current_focus(),
                        model.current_focus(),
                        "current_focus mismatch"
                    );

                    // Current module matches the model.
                    prop_assert_eq!(
                        reg.current_module(),
                        model.current_module,
                        "current_module mismatch"
                    );

                    // MAIN always remains present and reachable by name.
                    prop_assert_eq!(
                        reg.get_by_name("MAIN"),
                        Some(main_id),
                        "MAIN must always exist"
                    );
                }
            }
        }
    }
}
