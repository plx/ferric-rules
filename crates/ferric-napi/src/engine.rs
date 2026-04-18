//! Node.js Engine wrapper.

use std::path::PathBuf;

use napi::{Env, JsNull, JsObject, JsUnknown, Result};
use napi_derive::napi;

use ferric_core::FactId;
use ferric_runtime::config::EngineConfig;
use ferric_runtime::execution::RunLimit;
use ferric_runtime::Engine as FerricEngine;
use slotmap::{Key, KeyData};

use crate::config::{Encoding, Strategy};
use crate::error::{engine_error_to_napi, init_error_to_napi, load_errors_to_napi};
use crate::fact::fact_to_js;
use crate::result::{FiredRule, RuleInfo, RunResult};
use crate::value::{collect_object_keys, js_to_value, value_to_js};

/// Options for constructing an [`Engine`].
#[napi(object)]
pub struct EngineOptions {
    /// Conflict resolution strategy (default: Depth).
    pub strategy: Option<Strategy>,
    /// String encoding mode (default: Utf8).
    pub encoding: Option<Encoding>,
    /// Maximum call depth for user-defined functions (default: engine default).
    pub max_call_depth: Option<u32>,
}

/// Build a [`EngineConfig`] from optional options.
fn make_config(options: Option<EngineOptions>) -> EngineConfig {
    let mut config = EngineConfig::default();
    if let Some(opts) = options {
        if let Some(s) = opts.strategy {
            config.strategy = s.into();
        }
        if let Some(e) = opts.encoding {
            config.string_encoding = e.into();
        }
        if let Some(depth) = opts.max_call_depth {
            config.max_call_depth = depth as usize;
        }
    }
    config
}

/// The Ferric rules engine — Node.js binding.
///
/// This wraps a `ferric_runtime::Engine` and exposes it to JavaScript via
/// napi-rs.  Because napi-rs objects cannot cross V8 thread boundaries, no
/// thread-local registry is needed — the engine is stored directly in the
/// struct.
#[napi]
pub struct Engine {
    inner: Option<FerricEngine>,
}

impl Engine {
    fn engine(&self) -> Result<&FerricEngine> {
        self.inner
            .as_ref()
            .ok_or_else(|| napi::Error::from_reason("engine has been closed"))
    }

    fn engine_mut(&mut self) -> Result<&mut FerricEngine> {
        self.inner
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("engine has been closed"))
    }
}

/// Convert a fact ID (as f64 from JS) back to a [`FactId`].
fn fact_id_from_f64(id: f64) -> FactId {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    FactId::from(KeyData::from_ffi(id as u64))
}

#[napi]
impl Engine {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Create a new, empty engine.
    #[napi(constructor)]
    pub fn new(options: Option<EngineOptions>) -> Self {
        let config = make_config(options);
        Self {
            inner: Some(FerricEngine::new(config)),
        }
    }

    /// Create an engine from CLIPS source, loading and resetting in one step.
    ///
    /// Equivalent to constructing an engine, calling `load(source)`, then
    /// `reset()`.
    #[napi(factory)]
    pub fn from_source(source: String, options: Option<EngineOptions>) -> Result<Self> {
        let config = make_config(options);
        let engine =
            FerricEngine::with_rules_config(&source, config).map_err(init_error_to_napi)?;
        Ok(Self {
            inner: Some(engine),
        })
    }

    // -----------------------------------------------------------------------
    // Loading
    // -----------------------------------------------------------------------

    /// Load CLIPS source into the engine.
    #[napi]
    pub fn load(&mut self, source: String) -> Result<()> {
        let engine = self.engine_mut()?;
        engine.load_str(&source).map_err(load_errors_to_napi)?;
        Ok(())
    }

    /// Load CLIPS source from a file at the given path.
    #[napi]
    pub fn load_file(&mut self, path: String) -> Result<()> {
        let engine = self.engine_mut()?;
        engine
            .load_file(&PathBuf::from(path))
            .map_err(load_errors_to_napi)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Fact operations
    // -----------------------------------------------------------------------

    /// Assert one or more facts from CLIPS syntax (e.g. `"(color red)"`).
    ///
    /// Returns an array of fact IDs (as `number`) for all asserted facts.
    #[napi]
    pub fn assert_string(&mut self, source: String) -> Result<Vec<f64>> {
        let engine = self.engine_mut()?;
        let wrapped = format!("(assert {source})");
        let result = engine.load_str(&wrapped).map_err(load_errors_to_napi)?;
        if result.asserted_facts.is_empty() {
            return Err(napi::Error::from_reason(
                "assert_string did not produce any facts",
            ));
        }
        Ok(result
            .asserted_facts
            .iter()
            .map(|fid| {
                #[allow(clippy::cast_precision_loss)]
                let n = fid.data().as_ffi() as f64;
                n
            })
            .collect())
    }

    /// Assert an ordered fact with the given relation name and field values.
    ///
    /// Field values may be `null`, `boolean`, `number`, `bigint`, `string`,
    /// `FerricSymbol`, or `Array`.
    #[napi(ts_args_type = "relation: string, ...fields: unknown[]")]
    pub fn assert_fact(
        &mut self,
        env: Env,
        relation: String,
        fields: Vec<JsUnknown>,
    ) -> Result<f64> {
        let engine = self.engine_mut()?;
        let mut values = Vec::with_capacity(fields.len());
        for item in fields {
            values.push(js_to_value(&env, item, engine)?);
        }
        let fid = engine
            .assert_ordered(&relation, values)
            .map_err(engine_error_to_napi)?;
        #[allow(clippy::cast_precision_loss)]
        Ok(fid.data().as_ffi() as f64)
    }

    /// Assert a template fact by template name and slot values.
    ///
    /// `slots` is a plain JS object whose keys are slot names and values are
    /// CLIPS values (`null`, `boolean`, `number`, `bigint`, `string`,
    /// `FerricSymbol`, or `Array`).
    #[napi]
    pub fn assert_template(
        &mut self,
        env: Env,
        template_name: String,
        slots: JsObject,
    ) -> Result<f64> {
        let engine = self.engine_mut()?;
        let keys = collect_object_keys(&slots)?;

        let mut names: Vec<String> = Vec::with_capacity(keys.len());
        let mut values = Vec::with_capacity(keys.len());

        for name in keys {
            let val: JsUnknown = slots.get_named_property_unchecked(&name)?;
            let rust_val = js_to_value(&env, val, engine)?;
            names.push(name);
            values.push(rust_val);
        }

        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let fid = engine
            .assert_template(&template_name, &name_refs, values)
            .map_err(engine_error_to_napi)?;
        #[allow(clippy::cast_precision_loss)]
        Ok(fid.data().as_ffi() as f64)
    }

    /// Retract a fact by its ID.
    #[napi]
    pub fn retract(&mut self, fact_id: f64) -> Result<()> {
        let engine = self.engine_mut()?;
        let fid = fact_id_from_f64(fact_id);
        engine.retract(fid).map_err(engine_error_to_napi)
    }

    /// Get a fact by its ID, or `null` if it does not exist.
    #[napi]
    pub fn get_fact(&self, env: Env, fact_id: f64) -> Result<JsUnknown> {
        let engine = self.engine()?;
        let fid = fact_id_from_f64(fact_id);
        let fact = engine.get_fact(fid).map_err(engine_error_to_napi)?;
        match fact {
            Some(f) => {
                let obj = fact_to_js(&env, fid, f, engine)?;
                Ok(obj.into_unknown())
            }
            None => env.get_null().map(JsNull::into_unknown),
        }
    }

    /// Return all user-visible facts as a JS array of fact objects.
    #[napi]
    pub fn facts(&self, env: Env) -> Result<JsObject> {
        let engine = self.engine()?;
        let iter = engine.facts().map_err(engine_error_to_napi)?;
        let facts_vec: Vec<(FactId, _)> = iter.collect();
        let mut arr = env.create_array_with_length(facts_vec.len())?;
        for (i, (fid, fact)) in facts_vec.iter().enumerate() {
            let obj = fact_to_js(&env, *fid, fact, engine)?;
            #[allow(clippy::cast_possible_truncation)]
            arr.set_element(i as u32, obj)?;
        }
        Ok(arr)
    }

    /// Find facts by relation name. Returns a JS array of fact objects.
    #[napi]
    pub fn find_facts(&self, env: Env, relation: String) -> Result<JsObject> {
        let engine = self.engine()?;
        let found = engine.find_facts(&relation).map_err(engine_error_to_napi)?;
        let mut arr = env.create_array_with_length(found.len())?;
        for (i, (fid, fact)) in found.iter().enumerate() {
            let obj = fact_to_js(&env, *fid, fact, engine)?;
            #[allow(clippy::cast_possible_truncation)]
            arr.set_element(i as u32, obj)?;
        }
        Ok(arr)
    }

    /// Get the value of a template fact slot by name.
    #[napi]
    pub fn get_fact_slot(&self, env: Env, fact_id: f64, slot_name: String) -> Result<JsUnknown> {
        let engine = self.engine()?;
        let fid = fact_id_from_f64(fact_id);
        let val = engine
            .get_fact_slot_by_name(fid, &slot_name)
            .map_err(engine_error_to_napi)?;
        value_to_js(&env, val, engine)
    }

    // -----------------------------------------------------------------------
    // Execution
    // -----------------------------------------------------------------------

    /// Run the engine, optionally limiting the number of rule firings.
    ///
    /// Returns a `RunResult` describing how many rules fired and why
    /// execution stopped.
    #[napi]
    pub fn run(&mut self, limit: Option<u32>) -> Result<RunResult> {
        let engine = self.engine_mut()?;
        let run_limit = match limit {
            Some(n) => RunLimit::Count(n as usize),
            None => RunLimit::Unlimited,
        };
        let result = engine.run(run_limit).map_err(engine_error_to_napi)?;
        Ok(result.into())
    }

    /// Fire a single rule activation. Returns a `FiredRule` or `null`.
    #[napi]
    pub fn step(&mut self) -> Result<Option<FiredRule>> {
        let engine = self.engine_mut()?;
        let result = engine.step().map_err(engine_error_to_napi)?;
        Ok(result.map(|fr| {
            let name = engine
                .rule_name(fr.rule_id)
                .unwrap_or("<unknown>")
                .to_string();
            FiredRule { rule_name: name }
        }))
    }

    /// Request the engine to halt after the current rule completes.
    #[napi]
    pub fn halt(&mut self) -> Result<()> {
        let engine = self.engine_mut()?;
        engine.halt();
        Ok(())
    }

    /// Reset the engine: clear facts and re-assert deffacts.
    #[napi]
    pub fn reset(&mut self) -> Result<()> {
        let engine = self.engine_mut()?;
        engine.reset().map_err(engine_error_to_napi)
    }

    /// Clear the engine: remove all rules, facts, templates, globals, etc.
    #[napi]
    pub fn clear(&mut self) -> Result<()> {
        let engine = self.engine_mut()?;
        engine.clear();
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Properties / getters
    // -----------------------------------------------------------------------

    /// Number of user-visible facts in working memory.
    #[napi(getter)]
    pub fn fact_count(&self) -> Result<u32> {
        let engine = self.engine()?;
        let count = engine.facts().map_err(engine_error_to_napi)?.count();
        #[allow(clippy::cast_possible_truncation)]
        Ok(count as u32)
    }

    /// Whether the engine is currently halted.
    #[napi(getter)]
    pub fn is_halted(&self) -> Result<bool> {
        Ok(self.engine()?.is_halted())
    }

    /// Number of pending activations on the agenda.
    #[napi(getter)]
    pub fn agenda_size(&self) -> Result<u32> {
        #[allow(clippy::cast_possible_truncation)]
        Ok(self.engine()?.agenda_len() as u32)
    }

    /// Name of the current module.
    #[napi(getter)]
    pub fn current_module(&self) -> Result<String> {
        Ok(self.engine()?.current_module().to_owned())
    }

    /// Top of the focus stack, or `null` if the focus stack is empty.
    #[napi(getter)]
    pub fn focus(&self) -> Result<Option<String>> {
        Ok(self.engine()?.get_focus().map(str::to_owned))
    }

    /// Full focus stack as an array of module names (bottom to top).
    #[napi(getter)]
    pub fn focus_stack(&self) -> Result<Vec<String>> {
        Ok(self
            .engine()?
            .get_focus_stack()
            .into_iter()
            .map(String::from)
            .collect())
    }

    /// Non-fatal action diagnostics from the most recent `run()`/`step()`.
    #[napi(getter)]
    pub fn diagnostics(&self) -> Result<Vec<String>> {
        Ok(self
            .engine()?
            .action_diagnostics()
            .iter()
            .map(ToString::to_string)
            .collect())
    }

    // -----------------------------------------------------------------------
    // Introspection
    // -----------------------------------------------------------------------

    /// List all registered rules with their names and salience values.
    #[napi]
    pub fn rules(&self) -> Result<Vec<RuleInfo>> {
        Ok(self
            .engine()?
            .rules()
            .into_iter()
            .map(|(name, salience)| RuleInfo {
                name: name.to_owned(),
                salience,
            })
            .collect())
    }

    /// List the names of all registered templates.
    #[napi]
    pub fn templates(&self) -> Result<Vec<String>> {
        Ok(self
            .engine()?
            .templates()
            .into_iter()
            .map(String::from)
            .collect())
    }

    /// List the names of all registered modules.
    #[napi]
    pub fn modules(&self) -> Result<Vec<String>> {
        Ok(self
            .engine()?
            .modules()
            .into_iter()
            .map(String::from)
            .collect())
    }

    /// Get the value of a global variable by name, or `null` if not set.
    #[napi]
    pub fn get_global(&self, env: Env, name: String) -> Result<JsUnknown> {
        let engine = self.engine()?;
        match engine.get_global(&name) {
            Some(val) => value_to_js(&env, val, engine),
            None => env.get_null().map(JsNull::into_unknown),
        }
    }

    // -----------------------------------------------------------------------
    // Focus control
    // -----------------------------------------------------------------------

    /// Set focus to a single module, replacing the previous focus stack.
    #[napi]
    pub fn set_focus(&mut self, module_name: String) -> Result<()> {
        self.engine_mut()?
            .set_focus(&module_name)
            .map_err(engine_error_to_napi)
    }

    /// Push a module onto the focus stack.
    #[napi]
    pub fn push_focus(&mut self, module_name: String) -> Result<()> {
        self.engine_mut()?
            .push_focus(&module_name)
            .map_err(engine_error_to_napi)
    }

    // -----------------------------------------------------------------------
    // I/O
    // -----------------------------------------------------------------------

    /// Get captured output for a channel (e.g. `"stdout"`).
    ///
    /// Returns `null` if the channel has no captured output.
    #[napi]
    pub fn get_output(&self, channel: String) -> Result<Option<String>> {
        Ok(self.engine()?.get_output(&channel).map(str::to_owned))
    }

    /// Clear captured output for a channel.
    #[napi]
    pub fn clear_output(&mut self, channel: String) -> Result<()> {
        self.engine_mut()?.clear_output_channel(&channel);
        Ok(())
    }

    /// Push a line of input for `read`/`readline` to consume.
    #[napi]
    pub fn push_input(&mut self, line: String) -> Result<()> {
        self.engine_mut()?.push_input(&line);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Diagnostics
    // -----------------------------------------------------------------------

    /// Clear accumulated action diagnostics.
    #[napi]
    pub fn clear_diagnostics(&mut self) -> Result<()> {
        self.engine_mut()?.clear_action_diagnostics();
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Serialization
    // -----------------------------------------------------------------------

    /// Serialize the engine state to a Node.js `Buffer`.
    #[cfg(feature = "serde")]
    #[napi]
    pub fn serialize(
        &self,
        format: Option<crate::config::Format>,
    ) -> Result<napi::bindgen_prelude::Buffer> {
        let engine = self.engine()?;
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let bytes = engine
            .serialize(fmt)
            .map_err(crate::error::serde_error_to_napi)?;
        Ok(napi::bindgen_prelude::Buffer::from(bytes))
    }

    /// Create an engine by deserializing from a Node.js `Buffer`.
    #[cfg(feature = "serde")]
    #[napi(factory)]
    pub fn from_snapshot(
        data: napi::bindgen_prelude::Buffer,
        format: Option<crate::config::Format>,
    ) -> Result<Self> {
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let engine = FerricEngine::deserialize(data.as_ref(), fmt)
            .map_err(crate::error::serde_error_to_napi)?;
        Ok(Self {
            inner: Some(engine),
        })
    }

    /// Create an engine by deserializing from a file.
    #[cfg(feature = "serde")]
    #[napi(factory)]
    pub fn from_snapshot_file(path: String, format: Option<crate::config::Format>) -> Result<Self> {
        let data = std::fs::read(&path)
            .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let engine =
            FerricEngine::deserialize(&data, fmt).map_err(crate::error::serde_error_to_napi)?;
        Ok(Self {
            inner: Some(engine),
        })
    }

    /// Save a serialized engine snapshot to a file.
    #[cfg(feature = "serde")]
    #[napi]
    pub fn save_snapshot(&self, path: String, format: Option<crate::config::Format>) -> Result<()> {
        let engine = self.engine()?;
        let fmt = format.unwrap_or(crate::config::Format::Bincode).into();
        let bytes = engine
            .serialize(fmt)
            .map_err(crate::error::serde_error_to_napi)?;
        std::fs::write(&path, &bytes)
            .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Explicitly close and destroy this engine.
    ///
    /// After calling `close()`, any further method calls will throw an error.
    /// This is idempotent — calling it multiple times is safe.
    #[napi]
    pub fn close(&mut self) -> Result<()> {
        self.inner.take();
        Ok(())
    }
}
