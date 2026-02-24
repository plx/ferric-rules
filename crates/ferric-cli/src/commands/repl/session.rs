//! REPL session state and command handlers.

use std::io::Write;
use std::path::{Path, PathBuf};

use ferric_core::FactId;
use ferric_runtime::{Engine, EngineConfig, RunLimit};

use super::commands::WatchTarget;
use super::display;

/// Mutable session state for the REPL.
pub(crate) struct ReplSession {
    /// The rules engine instance.
    pub engine: Engine,
    /// Whether to trace fact assertions/retractions.
    pub watch_facts: bool,
    /// Whether to trace rule firings.
    pub watch_rules: bool,
    /// Files loaded during this session (for informational purposes).
    pub loaded_files: Vec<PathBuf>,
}

impl ReplSession {
    /// Create a new session with a default engine configuration.
    pub fn new() -> Self {
        Self {
            engine: Engine::new(EngineConfig::default()),
            watch_facts: false,
            watch_rules: false,
            loaded_files: Vec::new(),
        }
    }

    /// Load files at startup. Errors are printed but do not abort the session.
    pub fn preload_files(&mut self, files: &[PathBuf]) {
        for path in files {
            self.cmd_load(&path.to_string_lossy());
        }
    }

    // ---- Command Handlers ----

    pub fn cmd_reset(&mut self) {
        let before = if self.watch_facts {
            Some(self.snapshot_fact_ids())
        } else {
            None
        };
        if let Err(err) = self.engine.reset() {
            eprintln!("Error: {err}");
        }
        if let Some(before) = before {
            self.print_fact_diff(&before);
        }
    }

    pub fn cmd_clear(&mut self) {
        self.engine.clear();
    }

    pub fn cmd_run(&mut self, limit: Option<usize>) {
        let run_limit = match limit {
            Some(n) => RunLimit::Count(n),
            None => RunLimit::Unlimited,
        };

        let before = if self.watch_facts {
            Some(self.snapshot_fact_ids())
        } else {
            None
        };

        if self.watch_rules {
            self.run_with_trace(run_limit);
        } else {
            match self.engine.run(run_limit) {
                Ok(_result) => {
                    display::print_output(&mut self.engine);
                    for diag in self.engine.action_diagnostics() {
                        eprintln!("Warning: {diag}");
                    }
                    self.engine.clear_action_diagnostics();
                }
                Err(err) => {
                    eprintln!("Error: {err}");
                }
            }
        }

        if let Some(before) = before {
            self.print_fact_diff(&before);
        }
    }

    pub fn cmd_facts(&self) {
        display::print_facts(&self.engine);
    }

    pub fn cmd_agenda(&self) {
        println!("For a total of {} activations.", self.engine.agenda_len());
    }

    pub fn cmd_rules(&self) {
        let rules = self.engine.rules();
        if rules.is_empty() {
            println!("No rules defined.");
            return;
        }
        for (name, salience) in &rules {
            if *salience == 0 {
                println!("  {name}");
            } else {
                println!("  {name}  (salience {salience})");
            }
        }
        println!("For a total of {} rules.", rules.len());
    }

    pub fn cmd_load(&mut self, path: &str) {
        let file_path = Path::new(path);
        match self.engine.load_file(file_path) {
            Ok(result) => {
                self.loaded_files.push(file_path.to_path_buf());
                display::print_output(&mut self.engine);
                // Print a summary of what was loaded.
                let parts: Vec<String> = [
                    (result.rules.len(), "rule"),
                    (result.templates.len(), "template"),
                    (result.asserted_facts.len(), "fact"),
                    (result.functions.len(), "function"),
                    (result.globals.len(), "global"),
                ]
                .iter()
                .filter(|(count, _)| *count > 0)
                .map(|(count, kind)| {
                    if *count == 1 {
                        format!("{count} {kind}")
                    } else {
                        format!("{count} {kind}s")
                    }
                })
                .collect();
                if !parts.is_empty() {
                    println!("Loaded {}", parts.join(", "));
                }
                for warning in &result.warnings {
                    eprintln!("Warning: {warning}");
                }
            }
            Err(errors) => {
                for err in &errors {
                    eprintln!("{}", display::format_load_error(err));
                }
            }
        }
    }

    pub fn cmd_save(&self, path: &str) {
        match self.save_facts_to_file(Path::new(path)) {
            Ok(count) => println!("Saved {count} facts to {path}"),
            Err(err) => eprintln!("Error saving: {err}"),
        }
    }

    #[allow(clippy::unused_self)] // Method for consistency with other cmd_ handlers.
    pub fn cmd_help(&self) {
        println!(
            "\
Available commands:
  (facts)           List all facts in working memory
  (rules)           List all defined rules
  (agenda)          Show count of activations on the agenda
  (run)             Run the inference engine until complete
  (run N)           Run at most N rule firings
  (reset)           Reset engine (clear facts, re-assert deffacts)
  (clear)           Clear engine completely (remove everything)
  (load \"file\")     Load a CLIPS file
  (save \"file\")     Save current facts to a file
  (watch facts)     Enable fact assertion/retraction tracing
  (watch rules)     Enable rule firing tracing
  (unwatch facts)   Disable fact tracing
  (unwatch rules)   Disable rule tracing
  (help)            Show this help message
  (exit)            Exit the REPL

Keyboard shortcuts:
  Ctrl+D            Exit
  Ctrl+C            Cancel current input
  Ctrl+L            Clear screen
  Up/Down           Navigate history
  Tab               Complete command names"
        );
    }

    pub fn cmd_watch(&mut self, target: WatchTarget, enable: bool) {
        let (flag, name) = match target {
            WatchTarget::Facts => (&mut self.watch_facts, "facts"),
            WatchTarget::Rules => (&mut self.watch_rules, "rules"),
        };
        *flag = enable;
        if enable {
            println!("Watching {name}.");
        } else {
            println!("Unwatching {name}.");
        }
    }

    pub fn cmd_eval(&mut self, source: &str) {
        let before = if self.watch_facts {
            Some(self.snapshot_fact_ids())
        } else {
            None
        };

        match self.engine.load_str(source) {
            Ok(_) => {
                display::print_output(&mut self.engine);
            }
            Err(errors) => {
                for err in &errors {
                    eprintln!("{}", display::format_load_error(err));
                }
            }
        }

        if let Some(before) = before {
            self.print_fact_diff(&before);
        }
    }

    // ---- Watch Helpers ----

    /// Run the engine step-by-step, printing each rule firing.
    fn run_with_trace(&mut self, limit: RunLimit) {
        let max_steps = match limit {
            RunLimit::Unlimited => usize::MAX,
            RunLimit::Count(n) => n,
        };

        let mut fired_count = 0usize;
        for _ in 0..max_steps {
            match self.engine.step() {
                Ok(Some(fired)) => {
                    fired_count += 1;
                    let name = self
                        .engine
                        .rule_name(fired.rule_id)
                        .unwrap_or("<unknown>");
                    println!("FIRE  {fired_count}  {name}");
                    display::print_output(&mut self.engine);
                    for diag in self.engine.action_diagnostics() {
                        eprintln!("Warning: {diag}");
                    }
                    self.engine.clear_action_diagnostics();
                }
                Ok(None) => break, // Agenda empty.
                Err(err) => {
                    eprintln!("Error: {err}");
                    break;
                }
            }
        }
    }

    /// Snapshot all current fact IDs for diff tracking.
    fn snapshot_fact_ids(&self) -> Vec<FactId> {
        match self.engine.facts() {
            Ok(iter) => iter.map(|(id, _)| id).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Print facts that were added or removed compared to a previous snapshot.
    fn print_fact_diff(&self, before: &[FactId]) {
        let after = self.snapshot_fact_ids();

        // Facts added (in after but not in before).
        for &id in &after {
            if !before.contains(&id) {
                if let Ok(Some(fact)) = self.engine.get_fact(id) {
                    let id_num = {
                        use slotmap::Key as _;
                        id.data().as_ffi()
                    };
                    println!("==> f-{id_num}  {}", self.format_fact(fact));
                }
            }
        }

        // Facts removed (in before but not in after).
        for &id in before {
            if !after.contains(&id) {
                let id_num = {
                    use slotmap::Key as _;
                    id.data().as_ffi()
                };
                println!("<== f-{id_num}");
            }
        }
    }

    /// Format a fact for display in watch output.
    fn format_fact(&self, fact: &ferric_core::Fact) -> String {
        match fact {
            ferric_core::Fact::Ordered(o) => {
                let relation = self
                    .engine
                    .resolve_symbol(o.relation)
                    .unwrap_or("<unknown>");
                let fields: Vec<String> = o
                    .fields
                    .iter()
                    .map(|v| display::format_value(v, &self.engine))
                    .collect();
                if fields.is_empty() {
                    format!("({relation})")
                } else {
                    format!("({relation} {})", fields.join(" "))
                }
            }
            ferric_core::Fact::Template(t) => {
                let slots: Vec<String> = t
                    .slots
                    .iter()
                    .map(|v| display::format_value(v, &self.engine))
                    .collect();
                format!("(template-fact {})", slots.join(" "))
            }
        }
    }

    // ---- Save Helper ----

    /// Write all current facts to a file as CLIPS assert forms.
    fn save_facts_to_file(&self, path: &Path) -> Result<usize, std::io::Error> {
        let mut file = std::fs::File::create(path)?;
        let mut count = 0usize;

        match self.engine.facts() {
            Ok(iter) => {
                for (_id, fact) in iter {
                    let formatted = self.format_fact(fact);
                    writeln!(file, "(assert {formatted})")?;
                    count += 1;
                }
            }
            Err(err) => {
                return Err(std::io::Error::other(err.to_string()));
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_runtime::RunLimit;

    #[test]
    fn print_output_clears_t_channel() {
        let mut engine = Engine::new(EngineConfig::default());
        let src =
            r#"(defrule emit (initial-fact) => (printout t "hello" crlf) (printout stderr "err"))"#;
        assert!(engine.load_str(src).is_ok());
        assert!(engine.reset().is_ok());
        assert!(engine.run(RunLimit::Unlimited).is_ok());

        assert_eq!(engine.get_output("t"), Some("hello\n"));
        assert_eq!(engine.get_output("stderr"), Some("err"));

        display::print_output(&mut engine);

        assert!(engine.get_output("t").is_none());
        assert_eq!(engine.get_output("stderr"), Some("err"));
    }
}
