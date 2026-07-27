//! `ferric version` command — print version information.

pub fn execute() -> i32 {
    println!("ferric {}", env!("CARGO_PKG_VERSION"));
    0
}
