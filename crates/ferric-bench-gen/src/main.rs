//! Workload generator for comparative benchmarks.
//!
//! Generates `.clp` files that are valid in both CLIPS and ferric, suitable for
//! head-to-head performance comparison.  The generated workloads use only the
//! feature intersection of both engines (no `~?var` constraint syntax).
//!
//! ## Usage
//!
//! ```sh
//! ferric-bench-gen --output-dir target/bench-workloads
//! ```

use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Waltz generator (from crates/ferric/benches/waltz_bench.rs)
// ---------------------------------------------------------------------------

const JUNCTION_TYPES: [&str; 3] = ["L", "T", "fork"];

fn generate_waltz_source(n_junctions: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate edge (slot p1) (slot p2) (slot label (default unknown)))
(deftemplate junction (slot name) (slot type))

(deffacts scene\n",
    );

    for i in 0..n_junctions {
        let jtype = JUNCTION_TYPES[i % JUNCTION_TYPES.len()];
        writeln!(source, "    (junction (name j{i}) (type {jtype}))").unwrap();
    }

    for i in 0..n_junctions.saturating_sub(1) {
        writeln!(source, "    (edge (p1 j{i}) (p2 j{}))", i + 1).unwrap();
    }
    for i in (0..n_junctions.saturating_sub(2)).step_by(3) {
        writeln!(source, "    (edge (p1 j{i}) (p2 j{}))", i + 2).unwrap();
    }
    if n_junctions > 3 {
        writeln!(source, "    (edge (p1 j{}) (p2 j0))", n_junctions - 1).unwrap();
    }

    source.push_str(
        "    (phase label))

(defrule label-L-junction
    (declare (salience 10))
    (phase label)
    (junction (name ?j) (type L))
    ?e <- (edge (p1 ?j) (p2 ?) (label unknown))
    =>
    (modify ?e (label convex)))

(defrule label-T-junction
    (declare (salience 10))
    (phase label)
    (junction (name ?j) (type T))
    ?e <- (edge (p1 ?j) (p2 ?) (label unknown))
    =>
    (modify ?e (label boundary)))

(defrule label-fork-junction
    (declare (salience 10))
    (phase label)
    (junction (name ?j) (type fork))
    ?e <- (edge (p1 ?j) (p2 ?) (label unknown))
    =>
    (modify ?e (label concave)))

(defrule done-labeling
    (declare (salience -10))
    (phase label)
    (not (edge (label unknown)))
    =>
    (printout t \"Labeling complete\" crlf))
",
    );
    source
}

// ---------------------------------------------------------------------------
// Manners generator (from crates/ferric/benches/manners_bench.rs)
// ---------------------------------------------------------------------------

const HOBBIES: [&str; 4] = ["chess", "hiking", "cooking", "reading"];

fn generate_manners_source(n_guests: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate guest (slot name) (slot hobby))
(deftemplate seating (slot seat) (slot guest))
(deftemplate count (slot value))

(deffacts guests\n",
    );

    for i in 0..n_guests {
        let hobby = HOBBIES[i % HOBBIES.len()];
        writeln!(source, "    (guest (name g{i}) (hobby {hobby}))").unwrap();
    }

    source.push_str(
        "    (count (value 0))
    (phase assign))

(defrule assign-first-seat
    (declare (salience 40))
    (phase assign)
    (guest (name ?n) (hobby ?h))
    (count (value 0))
    =>
    (assert (seating (seat 1) (guest ?n)))
    (assert (count (value 1))))

(defrule assign-next-seat
    (declare (salience 30))
    (phase assign)
    ?c <- (count (value ?v))
    (seating (seat ?v) (guest ?prev))
    (guest (name ?prev) (hobby ?ph))
    (guest (name ?next) (hobby ?nh))
    (test (neq ?nh ?ph))
    (not (seating (seat ?) (guest ?next)))
    =>
    (retract ?c)
    (assert (seating (seat (+ ?v 1)) (guest ?next)))
    (assert (count (value (+ ?v 1)))))
",
    );
    source
}

// ---------------------------------------------------------------------------
// Join-width generator (from crates/ferric/benches/join_bench.rs)
// ---------------------------------------------------------------------------

fn generate_join_source(width: usize, n_keys: usize) -> String {
    let mut source = String::new();

    for w in 0..width {
        writeln!(source, "(deftemplate layer-{w} (slot key) (slot val))").unwrap();
    }
    writeln!(source, "(deftemplate result (slot key) (slot matched))").unwrap();
    source.push('\n');

    source.push_str("(deffacts data\n");
    for w in 0..width {
        for k in 0..n_keys {
            writeln!(source, "    (layer-{w} (key k{k}) (val v{w}-{k}))").unwrap();
        }
    }
    source.push_str(")\n\n");

    source.push_str("(defrule wide-join\n");
    for w in 0..width {
        writeln!(source, "    (layer-{w} (key ?k) (val ?v{w}))").unwrap();
    }
    source.push_str("    =>\n");
    source.push_str("    (assert (result (key ?k) (matched yes))))\n");

    source
}

// ---------------------------------------------------------------------------
// Churn generator (from crates/ferric/benches/churn_bench.rs)
// ---------------------------------------------------------------------------

fn generate_churn_source(n_items: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate item (slot id) (slot status (default pending)))
(deftemplate phase (slot name))

(deffacts initial
    (phase (name run))\n",
    );

    for i in 0..n_items {
        writeln!(source, "    (item (id {i}) (status pending))").unwrap();
    }

    source.push_str(
        ")

(defrule process-item
    (declare (salience 10))
    (phase (name run))
    ?item <- (item (id ?id) (status pending))
    =>
    (modify ?item (status done)))

(defrule cleanup-item
    (declare (salience 5))
    (phase (name run))
    ?item <- (item (id ?id) (status done))
    =>
    (retract ?item))

(defrule all-done
    (declare (salience -10))
    (phase (name run))
    (not (item))
    =>
    (printout t \"All items processed\" crlf))
",
    );
    source
}

// ---------------------------------------------------------------------------
// Negation generator (from crates/ferric/benches/negation_bench.rs)
// ---------------------------------------------------------------------------

fn generate_negation_source(n_blockers: usize) -> String {
    let mut source = String::from(
        "\
(deftemplate signal (slot name))
(deftemplate blocker (slot name) (slot seq))
(deftemplate phase (slot name))

(deffacts setup
    (phase (name clear))
    (signal (name S))\n",
    );

    for i in 0..n_blockers {
        writeln!(source, "    (blocker (name S) (seq {i}))").unwrap();
    }

    source.push_str(
        ")

(defrule remove-blocker
    (declare (salience 10))
    (phase (name clear))
    ?b <- (blocker)
    =>
    (retract ?b))

(defrule signal-clear
    (declare (salience -10))
    (phase (name clear))
    (signal (name ?n))
    (not (blocker (name ?n)))
    =>
    (printout t \"Signal clear\" crlf))
",
    );
    source
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

struct Config {
    output_dir: PathBuf,
}

fn parse_args() -> Result<Config, String> {
    let args: Vec<String> = std::env::args().collect();
    let mut output_dir = None;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--output-dir" => {
                i += 1;
                output_dir = Some(PathBuf::from(
                    args.get(i).ok_or("--output-dir requires a value")?,
                ));
            }
            "-h" | "--help" => {
                eprintln!("Usage: ferric-bench-gen --output-dir <path>");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }

    Ok(Config {
        output_dir: output_dir.ok_or("--output-dir is required")?,
    })
}

fn write_workload(dir: &Path, name: &str, source: &str) -> std::io::Result<()> {
    let path = dir.join(format!("{name}.clp"));
    fs::write(&path, source)?;
    eprintln!("  wrote {}", path.display());
    Ok(())
}

fn main() {
    let config = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Usage: ferric-bench-gen --output-dir <path>");
            std::process::exit(2);
        }
    };

    if let Err(e) = fs::create_dir_all(&config.output_dir) {
        eprintln!("error: cannot create output directory: {e}");
        std::process::exit(1);
    }

    eprintln!("Generating comparative benchmark workloads:");

    let waltz_sizes = [5, 10, 20, 50, 100, 150, 200, 300, 500, 750, 1000];
    let manners_sizes = [8, 16, 32, 48, 64, 96, 128, 256, 512];
    let join_widths = [3, 5, 7, 9, 11, 13, 15, 17, 19, 21];
    let churn_sizes = [100, 250, 500, 1000, 2000, 5000, 10_000, 25_000, 50_000, 100_000];
    let negation_sizes = [50, 100, 200, 500, 1000, 2500, 5000, 10_000, 25_000, 50_000];

    for &n in &waltz_sizes {
        let source = generate_waltz_source(n);
        if let Err(e) = write_workload(&config.output_dir, &format!("waltz-{n}"), &source) {
            eprintln!("error writing waltz-{n}.clp: {e}");
            std::process::exit(1);
        }
    }

    for &n in &manners_sizes {
        let source = generate_manners_source(n);
        if let Err(e) = write_workload(&config.output_dir, &format!("manners-{n}"), &source) {
            eprintln!("error writing manners-{n}.clp: {e}");
            std::process::exit(1);
        }
    }

    let join_n_keys = 100;
    for &w in &join_widths {
        let source = generate_join_source(w, join_n_keys);
        if let Err(e) = write_workload(&config.output_dir, &format!("join-{w}"), &source) {
            eprintln!("error writing join-{w}.clp: {e}");
            std::process::exit(1);
        }
    }

    for &n in &churn_sizes {
        let source = generate_churn_source(n);
        if let Err(e) = write_workload(&config.output_dir, &format!("churn-{n}"), &source) {
            eprintln!("error writing churn-{n}.clp: {e}");
            std::process::exit(1);
        }
    }

    for &n in &negation_sizes {
        let source = generate_negation_source(n);
        if let Err(e) = write_workload(&config.output_dir, &format!("negation-{n}"), &source) {
            eprintln!("error writing negation-{n}.clp: {e}");
            std::process::exit(1);
        }
    }

    let total = waltz_sizes.len()
        + manners_sizes.len()
        + join_widths.len()
        + churn_sizes.len()
        + negation_sizes.len();
    eprintln!(
        "Done. {} workloads written to {}",
        total,
        config.output_dir.display()
    );
}
