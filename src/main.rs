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

    // 1. Load and execute Prelude first (if it exists)
    if Path::new(PRELUDE_PATH).exists() {
        println!("Loading prelude from {}...", PRELUDE_PATH);
        match fs::read_to_string(PRELUDE_PATH) {
            Ok(prelude_src) => match compiler.compile(&prelude_src) {
                Ok(prelude_bc) => {
                    vm.load_program(prelude_bc);
                    vm.run_until_halt();
                    println!("Prelude loaded successfully.");
                }
                Err(e) => {
                    eprintln!("Failed to compile prelude: {}", e);
                    return;
                }
            },
            Err(e) => {
                eprintln!("Failed to read prelude: {}", e);
                return;
            }
        }
    } else {
        println!("No prelude found at {}, skipping...", PRELUDE_PATH);
    }

    // 2. Load and execute the main script
    // All constants (OP.PUSH, isDigit, etc.) are now in the global scope!
    println!("Compiling {}...", filename);

    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read file {}: {}", filename, e);
            return;
        }
    };

    match compiler.compile(&source) {
        Ok(bytecode) => {
            println!("Loading bytecode into VM...");
            vm.load_program(bytecode);

            println!("Running VM...");
            vm.run_event_loop();
        }
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
        }
    }
}
