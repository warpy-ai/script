mod compiler;
use compiler::Compiler;
mod stdlib;
mod vm;

use crate::vm::VM;
use std::env;
use std::fs;
use std::path::Path;
#[cfg(test)]
mod tests;

/// Default path for the prelude file
const PRELUDE_PATH: &str = "std/prelude.tscl";

/// Bootstrap compiler files (loaded in order when running bootstrap tests)
const BOOTSTRAP_FILES: &[&str] = &[
    "bootstrap/lexer.tscl",
    "bootstrap/parser.tscl",
    "bootstrap/emitter.tscl",
];

/// Helper to load and run a script file
fn load_and_run_script(vm: &mut VM, compiler: &mut Compiler, path: &str, append: bool) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
    let bytecode = compiler.compile(&source).map_err(|e| format!("Failed to compile {}: {}", path, e))?;
    let bytecode_len = bytecode.len();

    if append {
        let offset = vm.append_program(bytecode);
        println!("  {} ({} ops at offset {})", path, bytecode_len, offset);
    } else {
        vm.load_program(bytecode);
        println!("  {} ({} ops)", path, bytecode_len);
    }

    vm.run_until_halt();
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        return;
    }
    let filename = &args[1];

    let mut vm = VM::new();
    let mut compiler = Compiler::new();

    // Setup standard library
    vm.setup_stdlib();

    // 1. Load and run prelude first (if exists)
    // This sets up global constants (OP, TOKEN, TYPE) and utility functions
    if Path::new(PRELUDE_PATH).exists() {
        println!("Loading prelude...");
        if let Err(e) = load_and_run_script(&mut vm, &mut compiler, PRELUDE_PATH, false) {
            eprintln!("{}", e);
            return;
        }
    }

    // 2. Check if this is a bootstrap test that needs the compiler modules
    let is_bootstrap_test = filename.contains("test_emitter") || filename.contains("bootstrap/test");

    if is_bootstrap_test {
        println!("Loading bootstrap compiler modules...");
        for bootstrap_file in BOOTSTRAP_FILES {
            if Path::new(bootstrap_file).exists() {
                if let Err(e) = load_and_run_script(&mut vm, &mut compiler, bootstrap_file, true) {
                    eprintln!("{}", e);
                    return;
                }
            } else {
                eprintln!("Warning: Bootstrap file not found: {}", bootstrap_file);
            }
        }
    }

    // 3. Load and run the main script
    println!("Loading main script: {}", filename);
    let main_source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", filename, e);
            return;
        }
    };

    match compiler.compile(&main_source) {
        Ok(main_bytecode) => {
            let offset = vm.append_program(main_bytecode);
            println!("Running from offset {}...", offset);
            vm.run_event_loop();
        }
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
        }
    }
}
