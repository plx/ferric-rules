//! Module registry and focus stack for the CLIPS module system.
//!
//! Modules scope rule execution via the focus stack. Facts are global;
//! modules only affect which rules fire (based on focus) and which
//! templates/constructs are visible across module boundaries.

use std::collections::HashMap;

use ferric_parser::{ImportSpec, ModuleSpec};

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
    name_to_id: HashMap<String, ModuleId>,
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
        let mut modules = HashMap::new();
        modules.insert(main_id, main_module);
        let mut name_to_id = HashMap::new();
        name_to_id.insert(MAIN_MODULE_NAME.to_string(), main_id);

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
        let module = RuntimeModule {
            name: name.to_string(),
            exports,
            imports,
        };
        self.modules.insert(id, module);
        self.name_to_id.insert(name.to_string(), id);
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
        if from_module == owning_module {
            return true;
        }

        let Some(owner) = self.modules.get(&owning_module) else {
            return false;
        };

        let exported = owner.exports.iter().any(|spec| match spec {
            ModuleSpec::All => true,
            ModuleSpec::None => false,
            ModuleSpec::Specific {
                construct_type: ct,
                names,
            } => ct == construct_type && names.iter().any(|n| n == construct_name),
        });

        if !exported {
            return false;
        }

        let Some(importer) = self.modules.get(&from_module) else {
            return false;
        };

        importer.imports.iter().any(|import| {
            let Some(&import_from_id) = self.name_to_id.get(&import.module_name) else {
                return false;
            };
            if import_from_id != owning_module {
                return false;
            }
            match &import.spec {
                ModuleSpec::All => true,
                ModuleSpec::None => false,
                ModuleSpec::Specific {
                    construct_type: ct,
                    names,
                } => ct == construct_type && names.iter().any(|n| n == construct_name),
            }
        })
    }

    /// Debug-only structural checks for module/focus bookkeeping.
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
                "focus stack contains unknown module id {:?}",
                module_id
            );
        }

        for (name, id) in &self.name_to_id {
            let module = self
                .modules
                .get(id)
                .unwrap_or_else(|| panic!("name_to_id points to unknown module id {:?}", id));
            assert_eq!(
                module.name, *name,
                "name_to_id key `{name}` does not match module.name `{}`",
                module.name
            );
        }

        for (id, module) in &self.modules {
            let mapped = self.name_to_id.get(&module.name);
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
        let span = ferric_parser::Span::new(
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
        );
        registry.register(
            "MAIN",
            vec![ModuleSpec::All],
            vec![ImportSpec {
                module_name: "SENSOR".to_string(),
                spec: ModuleSpec::All,
                span,
            }],
        );

        let main = registry.main_module_id();
        assert!(registry.is_construct_visible(main, sensor_id, "deftemplate", "reading"));
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
    fn debug_consistency_passes_on_valid_registry() {
        let mut registry = ModuleRegistry::new();
        let sensor_id = registry.register("SENSOR", vec![ModuleSpec::All], vec![]);
        registry.push_focus(sensor_id);
        registry.debug_assert_consistency();
    }
}
