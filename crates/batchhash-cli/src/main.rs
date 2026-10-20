//! BatchHash developer CLI.
//!
//! This binary exists for CI smoke coverage and future debugging through CI-run
//! jobs. It is not a distribution artifact (see CONTRIBUTING.md).
//!
//! Commands are intentionally small in P0; more commands (hash/report/diff) land
//! as the engine crates stabilise.

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("help") | Some("--help") | Some("-h") => print_help(),
        Some("version") | Some("--version") | Some("-V") => {
            println!("{} {}", batchhash_types::TOOL_NAME, batchhash_types::TOOL_VERSION);
        }
        Some("algorithms") => print_algorithms(),
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!("BatchHash CLI (developer tool, not a distribution build)");
    println!();
    println!("USAGE:");
    println!("  batchhash <command>");
    println!();
    println!("COMMANDS:");
    println!("  version       Print tool name and version");
    println!("  algorithms    List the 16 registered hash algorithms");
    println!("  help          Show this help");
}

fn print_algorithms() {
    println!("{:<10} {:<14} {:>4} {:>5}  {}", "ID", "DISPLAY", "HEX", "CRYPTO", "SUFFIX");
    for spec in batchhash_types::REGISTRY.iter() {
        println!(
            "{:<10} {:<14} {:>4} {:>5}  {}",
            spec.stable_id,
            spec.display_name,
            spec.hex_len,
            if spec.cryptographic { "yes" } else { "no" },
            if spec.suffix_usable { "yes" } else { "no" }
        );
    }
}
