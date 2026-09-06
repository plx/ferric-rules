//! Node.js Engine wrapper.

use std::cell::{RefCell, RefMut};
use std::path::PathBuf;

use napi::{Env, JsBigInt, JsNull, JsNumber, JsObject, JsUnknown, Result, ValueType};
use napi_derive::napi;

use ferric_rules_runtime::config::EngineConfig;
use ferric_rules_runtime::execution::RunLimit;
use ferric_rules_runtime::Engine as FerricEngine;
use ferric_rules_runtime::{FactHandle as FactId, HOST_VALUE_MAX_ITEMS};

use crate::config::{checked_u32, Encoding, Strategy};
use crate::error::{engine_error_to_napi, init_error_to_napi, load_errors_to_napi};
use crate::fact::fact_to_js;
use crate::result::{checked_count, FiredRule, RuleInfo, RunResult};
use crate::value::{collect_object_keys, js_to_owned, value_to_js};

/// Options for constructing an [`Engine`].
#[napi(object)]
pub struct EngineOptions {
    /// Conflict resolution strategy (default: Depth).
    #[napi(ts_type = "Strategy")]
    pub strategy: Option<f64>,
    /// String encoding mode (default: Utf8).
    #[napi(ts_type = "Encoding")]
    pub encoding: Option<f64>,
    /// Maximum call depth for user-defined functions (default: engine default).
    pub max_call_depth: Option<f64>,
}

/// Build a [`EngineConfig`] from optional options.
fn make_config(options: Option<EngineOptions>) -> Result<EngineConfig> {
    let mut config = EngineConfig::default();
    if let Some(opts) = options {
        if let Some(s) = opts.strategy {
            config.strategy = Strategy::try_from(s)?.into();
        }
        if let Some(e) = opts.encoding {
            config.string_encoding = Encoding::try_from(e)?.into();
        }
        if let Some(depth) = opts.max_call_depth {
            config.max_call_depth = checked_u32(depth, "maxCallDepth")? as usize;
        }
    }
    Ok(config)
}

fn checked_run_limit(limit: f64) -> Result<usize> {
    if !limit.is_finite()
        || limit.fract() != 0.0
        || !(0.0..=9_007_199_254_740_991.0).contains(&limit)
    {
        return Err(napi::Error::new(
            napi::Status::InvalidArg,
            "run limit must be a non-negative safe integer",
        ));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(limit as usize)
}

/// The Ferric rules engine — Node.js binding.
///
/// This wraps a `ferric_rules_runtime::Engine` and exposes it to JavaScript via
/// napi-rs.  Because napi-rs objects cannot cross V8 thread boundaries, no
/// thread-local registry is needed — the engine is stored directly in the
/// struct.
#[napi]
pub struct Engine {
    inner: RefCell<Option<FerricEngine>>,
}

impl Engine {
    fn into_js(self, env: Env) -> Result<JsObject> {
        // Use the registered Engine constructor. napi-rs's generated factory
        // instead uses JS `this`, which can be another native class; wrapping
        // an Engine pointer under that class would permit a wrong-type read.
        Ok(self.into_instance(env)?.as_object(env))
    }

    // Our napi exports use shared receivers only. napi-rs does not dynamically
    // guard generated &mut receivers against JS getters reentering the same object.
    // This reservation protects all calls, including reads and close, while
    // conversions can invoke JavaScript. No runtime reference escapes it.
    fn state(&self) -> Result<RefMut<'_, Option<FerricEngine>>> {
        self.inner.try_borrow_mut().map_err(|_| {
            napi::Error::from_reason("FerricRuntimeError: reentrant engine access is not allowed")
        })
    }

    fn engine(&self) -> Result<RefMut<'_, FerricEngine>> {
        RefMut::filter_map(self.state()?, Option::as_mut)
            .map_err(|_| napi::Error::from_reason("engine has been closed"))
    }

    fn engine_mut(&self) -> Result<RefMut<'_, FerricEngine>> {
        self.engine()
    }
}

/// Convert a JavaScript fact ID back to a [`FactId`]. Canonical IDs are
/// `bigint`, while safe non-negative integer `number` values remain accepted
/// for compatibility with the original Node API.
fn fact_id_from_js(id: JsUnknown) -> Result<FactId> {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0; // 2^53 - 1

    let raw = match id.get_type()? {
        ValueType::Number => {
            let js_number: JsNumber = id.try_into()?;
            let number = js_number.get_double()?;
            if !number.is_finite()
                || number.fract() != 0.0
                || !(0.0..=MAX_SAFE_INTEGER).contains(&number)
            {
                return Err(napi::Error::new(
                    napi::Status::InvalidArg,
                    "fact id number must be a non-negative safe integer; pass a bigint for 64-bit IDs",
                ));
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                number as u64
            }
        }
        ValueType::BigInt => {
            // SAFETY: We just confirmed the type is BigInt via get_type().
            let js_bigint: JsBigInt = unsafe { id.cast() };
            let (value, lossless) = js_bigint.get_u64()?;
            if !lossless {
                return Err(napi::Error::new(
                    napi::Status::InvalidArg,
                    "fact id bigint must be in the unsigned 64-bit range",
                ));
            }
            value
        }
        _ => {
            return Err(napi::Error::new(
                napi::Status::InvalidArg,
                "fact id must be a bigint or number",
            ));
        }
    };

    Ok(FactId::from_raw(raw))
}

#[napi]
impl Engine {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Create a new, empty engine.
    #[napi(constructor)]
    pub fn new(options: Option<EngineOptions>) -> Result<Self> {
        let config = make_config(options)?;
        Ok(Self {
            inner: RefCell::new(Some(FerricEngine::new(config))),
        })
    }

    /// Create an engine from CLIPS source, loading and resetting in one step.
    ///
    /// Equivalent to constructing an engine, calling `load(source)`, then
    /// `reset()`.
    #[napi(ts_return_type = "Engine")]
    pub fn from_source(
        env: Env,
        source: String,
        options: Option<EngineOptions>,
    ) -> Result<JsObject> {
        let config = make_config(options)?;
        let engine =
            FerricEngine::with_rules_config(&source, config).map_err(init_error_to_napi)?;
        Self {
            inner: RefCell::new(Some(engine)),
        }
        .into_js(env)
    }

    // -----------------------------------------------------------------------
    // Loading
    // -----------------------------------------------------------------------

    /// Load CLIPS source into the engine.
    #[napi]
    pub fn load(&self, source: String) -> Result<()> {
        let mut engine = self.engine_mut()?;
        engine.load_str(&source).map_err(load_errors_to_napi)?;
        Ok(())
    }

    /// Load CLIPS source from a file at the given path.
    #[napi]
    pub fn load_file(&self, path: String) -> Result<()> {
        let mut engine = self.engine_mut()?;
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
    /// Returns an array of fact IDs (as `bigint`) for all asserted facts.
    #[napi]
    pub fn assert_string(&self, source: String) -> Result<Vec<u64>> {
        let mut engine = self.engine_mut()?;
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
            .map(|fid| fid.as_raw())
            .collect())
    }

    /// Assert an ordered fact with the given relation name and field values.
    ///
    /// Field values may be `null`, `boolean`, `number`, `bigint`, `string`,
    /// `FerricSymbol`, or `Array`.
    #[napi(ts_args_type = "relation: string, ...fields: unknown[]")]
    pub fn assert_fact(&self, env: Env, relation: String, fields: Vec<JsUnknown>) -> Result<u64> {
        let mut state = self.state()?;
        if state.is_none() {
            return Err(napi::Error::from_reason("engine has been closed"));
        }
        let mut remaining = HOST_VALUE_MAX_ITEMS;
        if fields.len() > remaining {
            return Err(napi::Error::from_reason("too many values in one assertion"));
        }
        let staged = fields
            .into_iter()
            .map(|item| js_to_owned(&env, item, 0, &mut remaining))
            .collect::<Result<Vec<_>>>()?;
        let engine = state
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("engine has been closed"))?;
        let values = staged
            .into_iter()
            .map(|value| value.into_runtime(engine))
            .collect::<Result<Vec<_>>>()?;
        let fid = engine
            .assert_ordered(&relation, values)
            .map_err(engine_error_to_napi)?;
        Ok(fid.as_raw())
    }

    /// Assert a template fact by template name and slot values.
    ///
    /// `slots` is a plain JS object whose keys are slot names and values are
    /// CLIPS values (`null`, `boolean`, `number`, `bigint`, `string`,
    /// `FerricSymbol`, or `Array`).
    #[napi]
    pub fn assert_template(&self, env: Env, template_name: String, slots: JsObject) -> Result<u64> {
        let mut state = self.state()?;
        if state.is_none() {
            return Err(napi::Error::from_reason("engine has been closed"));
        }
        let names = collect_object_keys(&slots)?;
        let mut remaining = HOST_VALUE_MAX_ITEMS;
        if names.len() > remaining {
            return Err(napi::Error::from_reason("too many values in one assertion"));
        }
        let staged = names
            .iter()
            .map(|name| {
                let value: JsUnknown = slots.get_named_property_unchecked(name)?;
                js_to_owned(&env, value, 0, &mut remaining)
            })
            .collect::<Result<Vec<_>>>()?;
        let engine = state
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("engine has been closed"))?;
        let values = staged
            .into_iter()
            .map(|value| value.into_runtime(engine))
            .collect::<Result<Vec<_>>>()?;

        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let fid = engine
            .assert_template(&template_name, &name_refs, values)
            .map_err(engine_error_to_napi)?;
        Ok(fid.as_raw())
    }

    /// Retract a fact by its ID. Canonical IDs are `bigint`; safe legacy
    /// `number` IDs are also accepted.
    #[napi(ts_args_type = "factId: bigint | number")]
    pub fn retract(&self, fact_id: JsUnknown) -> Result<()> {
        let fid = fact_id_from_js(fact_id)?;
        let mut engine = self.engine_mut()?;
        engine.retract(fid).map_err(engine_error_to_napi)
    }

    /// Get a fact by its ID, or `null` if it does not exist. Canonical IDs are
    /// `bigint`; safe legacy `number` IDs are also accepted.
    #[napi(ts_args_type = "factId: bigint | number")]
    pub fn get_fact(&self, env: Env, fact_id: JsUnknown) -> Result<JsUnknown> {
        let fid = fact_id_from_js(fact_id)?;
        let engine = self.engine()?;
        let fact = engine.get_fact(fid).map_err(engine_error_to_napi)?;
        match fact {
            Some(f) => {
                let obj = fact_to_js(&env, fid, f, &engine)?;
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
            let obj = fact_to_js(&env, *fid, fact, &engine)?;
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
            let obj = fact_to_js(&env, *fid, fact, &engine)?;
            #[allow(clippy::cast_possible_truncation)]
            arr.set_element(i as u32, obj)?;
        }
        Ok(arr)
    }

    /// Get the value of a template fact slot by name. Canonical IDs are
    /// `bigint`; safe legacy `number` IDs are also accepted.
    #[napi(ts_args_type = "factId: bigint | number, slotName: string")]
    pub fn get_fact_slot(
        &self,
        env: Env,
        fact_id: JsUnknown,
        slot_name: String,
    ) -> Result<JsUnknown> {
        let fid = fact_id_from_js(fact_id)?;
        let engine = self.engine()?;
        let val = engine
            .get_fact_slot_by_name(fid, &slot_name)
            .map_err(engine_error_to_napi)?;
        value_to_js(&env, val, &engine)
    }

    // -----------------------------------------------------------------------
    // Execution
    // -----------------------------------------------------------------------

    /// Run the engine, optionally limiting the number of rule firings.
    ///
    /// Returns a `RunResult` describing how many rules fired and why
    /// execution stopped.
    #[napi]
    pub fn run(&self, limit: Option<f64>) -> Result<RunResult> {
        let mut engine = self.engine_mut()?;
        let run_limit = match limit {
            Some(n) => RunLimit::Count(checked_run_limit(n)?),
            None => RunLimit::Unlimited,
        };
        let result = engine.run(run_limit).map_err(engine_error_to_napi)?;
        result.try_into()
    }

    /// Internal worker chunk. Module initialization moves this receiver-checked
    /// native function off the prototype before exposing the addon to JS.
    #[doc(hidden)]
    #[napi(js_name = "__continueRun", skip_typescript)]
    pub fn continue_run(&self, limit: u32) -> Result<RunResult> {
        let mut engine = self.engine_mut()?;
        let result = engine
            .continue_run(RunLimit::Count(limit as usize))
            .map_err(engine_error_to_napi)?;
        result.try_into()
    }

    /// Fire a single rule activation. Returns a `FiredRule` or `null`.
    #[napi]
    pub fn step(&self) -> Result<Option<FiredRule>> {
        let mut engine = self.engine_mut()?;
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
    pub fn halt(&self) -> Result<()> {
        let mut engine = self.engine_mut()?;
        engine.halt();
        Ok(())
    }

    /// Reset the engine: clear facts and re-assert deffacts.
    #[napi]
    pub fn reset(&self) -> Result<()> {
        let mut engine = self.engine_mut()?;
        engine.reset().map_err(engine_error_to_napi)
    }

    /// Clear the engine: remove all rules, facts, templates, globals, etc.
    #[napi]
    pub fn clear(&self) -> Result<()> {
        let mut engine = self.engine_mut()?;
        engine.clear();
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Properties / getters
    // -----------------------------------------------------------------------

    /// Number of user-visible facts in working memory.
    #[napi(getter)]
    pub fn fact_count(&self) -> Result<f64> {
        let engine = self.engine()?;
        let count = engine.fact_count();
        checked_count(count, "fact count")
    }

    /// Whether the engine is currently halted.
    #[napi(getter)]
    pub fn is_halted(&self) -> Result<bool> {
        Ok(self.engine()?.is_halted())
    }

    /// Number of pending activations on the agenda.
    #[napi(getter)]
    pub fn agenda_size(&self) -> Result<f64> {
        checked_count(self.engine()?.agenda_len(), "agenda size")
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

    /// Action evaluation diagnostics from the most recent `run()`/`step()`.
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
            Some(val) => value_to_js(&env, val, &engine),
            None => env.get_null().map(JsNull::into_unknown),
        }
    }

    // -----------------------------------------------------------------------
    // Focus control
    // -----------------------------------------------------------------------

    /// Set focus to a single module, replacing the previous focus stack.
    #[napi]
    pub fn set_focus(&self, module_name: String) -> Result<()> {
        self.engine_mut()?
            .set_focus(&module_name)
            .map_err(engine_error_to_napi)
    }

    /// Push a module onto the focus stack.
    #[napi]
    pub fn push_focus(&self, module_name: String) -> Result<()> {
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
    pub fn clear_output(&self, channel: String) -> Result<()> {
        self.engine_mut()?.clear_output_channel(&channel);
        Ok(())
    }

    /// Push a line of input for `read`/`readline` to consume.
    #[napi]
    pub fn push_input(&self, line: String) -> Result<()> {
        self.engine_mut()?.push_input(&line);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Diagnostics
    // -----------------------------------------------------------------------

    /// Clear accumulated action diagnostics.
    #[napi]
    pub fn clear_diagnostics(&self) -> Result<()> {
        self.engine_mut()?.clear_action_diagnostics();
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
    pub fn close(&self) -> Result<()> {
        self.state()?.take();
        Ok(())
    }
}

// Gate the whole napi impl so disabled methods are also absent from its
// generated registration helpers. Per-method cfg leaves dangling helpers.
#[cfg(feature = "serde")]
#[napi]
impl Engine {
    // -----------------------------------------------------------------------
    // Serialization
    // -----------------------------------------------------------------------

    /// Serialize the engine state to a Node.js `Buffer`.
    #[napi]
    pub fn serialize(
        &self,
        #[napi(ts_arg_type = "Format | undefined | null")] format: Option<f64>,
    ) -> Result<napi::bindgen_prelude::Buffer> {
        let engine = self.engine()?;
        let fmt = crate::config::snapshot_format(format)?;
        let bytes = engine
            .serialize(fmt)
            .map_err(crate::error::serde_error_to_napi)?;
        Ok(napi::bindgen_prelude::Buffer::from(bytes))
    }

    /// Create an engine by deserializing from a Node.js `Buffer`.
    #[napi(ts_return_type = "Engine")]
    pub fn from_snapshot(
        env: Env,
        data: napi::bindgen_prelude::Buffer,
        #[napi(ts_arg_type = "Format | undefined | null")] format: Option<f64>,
    ) -> Result<JsObject> {
        let fmt = crate::config::snapshot_format(format)?;
        let engine = FerricEngine::deserialize(data.as_ref(), fmt)
            .map_err(crate::error::serde_error_to_napi)?;
        Self {
            inner: RefCell::new(Some(engine)),
        }
        .into_js(env)
    }

    /// Create an engine by deserializing from a file.
    #[napi(ts_return_type = "Engine")]
    pub fn from_snapshot_file(
        env: Env,
        path: String,
        #[napi(ts_arg_type = "Format | undefined | null")] format: Option<f64>,
    ) -> Result<JsObject> {
        let fmt = crate::config::snapshot_format(format)?;
        let engine = FerricEngine::deserialize_from_file(std::path::Path::new(&path), fmt)
            .map_err(|error| match error {
                ferric_rules_runtime::SnapshotFileError::Io(error) => crate::error::io_error_to_napi(error),
                ferric_rules_runtime::SnapshotFileError::Serialization(error) => crate::error::serde_error_to_napi(error),
            })?;
        Self {
            inner: RefCell::new(Some(engine)),
        }
        .into_js(env)
    }

    /// Save a serialized engine snapshot to a file.
    #[napi]
    pub fn save_snapshot(
        &self,
        path: String,
        #[napi(ts_arg_type = "Format | undefined | null")] format: Option<f64>,
    ) -> Result<()> {
        let engine = self.engine()?;
        let fmt = crate::config::snapshot_format(format)?;
        let bytes = engine
            .serialize(fmt)
            .map_err(crate::error::serde_error_to_napi)?;
        std::fs::write(&path, &bytes).map_err(crate::error::io_error_to_napi)?;
        Ok(())
    }
}
