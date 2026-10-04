//! Shared shell execution for the REPL and procedural source files.

use std::path::{Path, PathBuf};

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

use super::commands::ReplCommand;
use super::display;
use crate::commands::common::{emit_error, emit_warning};

pub(crate) struct ReplSession {
    pub engine: Engine,
    /// `Some` for file execution; `None` for interactive diagnostics.
    json_mode: Option<bool>,
}

impl ReplSession {
    pub fn new() -> Self {
        Self::with_engine(Engine::new(EngineConfig::default()), None)
    }

    pub fn with_engine(mut engine: Engine, json_mode: Option<bool>) -> Self {
        engine.enable_output_events();
        Self { engine, json_mode }
    }

    #[cfg(feature = "serde")]
    pub fn from_snapshot(
        path: &Path,
        format: ferric_rules_runtime::serialization::SerializationFormat,
    ) -> Result<Self, String> {
        let engine = Engine::deserialize_from_file(path, format)
            .map_err(|e| format!("failed to restore {}: {e}", path.display()))?;
        Ok(Self::with_engine(engine, None))
    }

    pub fn preload_files(&mut self, files: &[PathBuf]) {
        for path in files {
            let _ = self.cmd_load(path, false);
        }
    }

    /// Execute a parsed command; `Ok(true)` requests shell termination.
    pub fn dispatch(&mut self, command: ReplCommand, echo: bool) -> Result<bool, ()> {
        match command {
            ReplCommand::Exit => return Ok(true),
            ReplCommand::Run { limit } => self.cmd_run(limit)?,
            ReplCommand::Facts => display::print_facts(&self.engine),
            ReplCommand::Agenda { module } => self.cmd_agenda(module.as_deref())?,
            ReplCommand::Rules => self.cmd_rules(),
            ReplCommand::Load { path } => self.cmd_load(Path::new(&path), echo)?,
            ReplCommand::Help => Self::cmd_help(),
            ReplCommand::Construct { source } => self.load_source(&source.located())?,
            ReplCommand::Eval { source } => self.cmd_eval(&source.located(), echo)?,
        }
        Ok(false)
    }

    pub fn error(&self, kind: &str, error: impl std::fmt::Display) {
        if let Some(json_mode) = self.json_mode {
            emit_error(json_mode, "run", kind, error);
        } else {
            eprintln!("Error: {error}");
        }
    }

    fn warning(&self, warning: impl std::fmt::Display) {
        if let Some(json_mode) = self.json_mode {
            emit_warning(json_mode, "run", "action_warning", warning);
        } else {
            eprintln!("Warning: {warning}");
        }
    }

    pub fn drain(&mut self) {
        display::print_output(&mut self.engine);
        for diagnostic in self.engine.action_diagnostics() {
            self.warning(diagnostic);
        }
        self.engine.clear_action_diagnostics();
    }

    pub fn load_source(&mut self, source: &str) -> Result<(), ()> {
        let result = self.engine.load_str(source);
        self.drain();
        self.report_load(result)
    }

    fn report_load(
        &self,
        result: Result<ferric_rules_runtime::LoadResult, Vec<ferric_rules_runtime::LoadError>>,
    ) -> Result<(), ()> {
        match result {
            Ok(result) => {
                for warning in result.warnings {
                    self.warning(warning);
                }
                Ok(())
            }
            Err(errors) => {
                for error in errors {
                    self.error("load_error", error);
                }
                Err(())
            }
        }
    }

    pub fn cmd_reset(&mut self) -> Result<(), ()> {
        self.drain();
        let result = self.engine.reset();
        self.drain();
        result.map_err(|error| self.error("runtime_error", format_args!("reset failed: {error}")))
    }

    pub fn cmd_run(&mut self, limit: Option<usize>) -> Result<(), ()> {
        let result = self
            .engine
            .run(limit.map_or(RunLimit::Unlimited, RunLimit::Count));
        self.drain();
        result
            .map(|_| ())
            .map_err(|error| self.error("runtime_error", error))
    }

    fn cmd_agenda(&self, module: Option<&str>) -> Result<(), ()> {
        let entries = if let Some(module) = module {
            self.engine
                .agenda_entries_in_module(module)
                .map_err(|error| self.error("runtime_error", error))?
        } else {
            self.engine.agenda_entries()
        };
        let mut previous_module = None;
        for entry in &entries {
            if module == Some("*") {
                if previous_module != Some(&entry.module_name) {
                    println!("{}:", entry.module_name);
                    previous_module = Some(&entry.module_name);
                }
                println!("   {}", entry.format_line());
            } else {
                println!("{}", entry.format_line());
            }
        }
        if !entries.is_empty() {
            println!(
                "For a total of {} activation{}.",
                entries.len(),
                if entries.len() == 1 { "" } else { "s" }
            );
        }
        Ok(())
    }

    fn cmd_rules(&self) {
        let rules = self.engine.rules();
        for (name, _) in &rules {
            println!("{name}");
        }
        if !rules.is_empty() {
            println!("For a total of {} rules.", rules.len());
        }
    }

    fn cmd_load(&mut self, path: &Path, echo: bool) -> Result<(), ()> {
        let result = self.engine.load_file(path);
        self.drain();
        self.report_load(result)?;
        if echo {
            println!("TRUE");
        }
        Ok(())
    }

    fn cmd_eval(&mut self, source: &str, echo: bool) -> Result<(), ()> {
        let result = self.engine.eval_str(source);
        self.drain();
        match result {
            Ok(value) => {
                if echo && !matches!(value, Value::Void) {
                    println!("{}", self.engine.format_value(&value));
                }
                Ok(())
            }
            Err(error) => {
                self.error("evaluation_error", error);
                Err(())
            }
        }
    }

    fn cmd_help() {
        println!(
            "Available commands:\n\
  (facts)                    List facts\n\
  (rules)                    List rules\n\
  (agenda [module])          List activations in firing order\n\
  (run [N])                  Run rules, optionally limited to N firings\n\
  (reset) / (clear)           Reset working memory / clear constructs\n\
  (load \"file\")             Load constructs\n\
  (save-facts \"file\")       Save facts\n\
  (load-facts \"file\")       Load facts\n\
  (watch facts|rules)         Enable mutation/firing traces\n\
  (unwatch facts|rules)       Disable traces\n\
  (help) / (exit)             Show help / exit\n\
Ordinary CLIPS expressions are evaluated and non-void results are printed.\n\
Keyboard: Ctrl+D exits, Ctrl+C cancels input, Tab completes commands."
        );
    }
}
