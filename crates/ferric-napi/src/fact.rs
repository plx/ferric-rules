//! Fact construction for the Node.js binding.

use napi::{Env, JsObject, Result};
use napi_derive::napi;

use ferric_core::{Fact as CoreFact, FactId};
use ferric_runtime::Engine;
use slotmap::Key;

use crate::value::{value_to_js, values_to_js_array};

/// Fact type discriminator.
#[napi]
pub enum FactType {
    /// An ordered (positional) fact.
    Ordered = 0,
    /// A template (named-slot) fact.
    Template = 1,
}

/// Build a JavaScript object representing a fact snapshot.
///
/// The resulting object has the shape:
/// ```ts
/// {
///   id: number,
///   type: FactType,
///   // ordered facts:
///   relation?: string,
///   fields?: ClipsValue[],
///   // template facts:
///   templateName?: string,
///   slots?: Record<string, ClipsValue>,
/// }
/// ```
///
/// # Errors
///
/// Returns an error if any JS value creation fails.
pub fn fact_to_js(
    env: &Env,
    fact_id: FactId,
    fact: &CoreFact,
    engine: &Engine,
) -> Result<JsObject> {
    let mut obj = env.create_object()?;

    // Fact ID as a JS number (u64 → f64; fact IDs will not exceed 2^53).
    #[allow(clippy::cast_precision_loss)]
    let id_num = fact_id.data().as_ffi() as f64;
    obj.set("id", env.create_double(id_num)?)?;

    match fact {
        CoreFact::Ordered(ordered) => {
            obj.set("type", FactType::Ordered as u32)?;
            let relation = engine
                .resolve_symbol(ordered.relation)
                .unwrap_or("<unknown>");
            obj.set("relation", env.create_string(relation)?)?;

            let fields = values_to_js_array(env, &ordered.fields, engine)?;
            obj.set("fields", fields)?;
        }

        CoreFact::Template(template) => {
            obj.set("type", FactType::Template as u32)?;
            let tmpl_name = engine
                .template_name_by_id(template.template_id)
                .unwrap_or("<unknown>");
            obj.set("templateName", env.create_string(tmpl_name)?)?;

            let fields = values_to_js_array(env, &template.slots, engine)?;
            obj.set("fields", fields)?;

            // Build slots object if slot names are available.
            if let Some(slot_names) = engine.template_slot_names_by_id(template.template_id) {
                let mut slots = env.create_object()?;
                for (name, val) in slot_names.iter().zip(template.slots.iter()) {
                    let js_val = value_to_js(env, val, engine)?;
                    slots.set(name, js_val)?;
                }
                obj.set("slots", slots)?;
            }
        }
    }

    Ok(obj)
}
