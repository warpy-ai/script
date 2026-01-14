mod compiler;
use compiler::Compiler;
mod stdlib;
mod vm;

use crate::vm::VM;
use std::env;
use std::fs;
#[cfg(test)]
mod tests;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        return;
    }
    let filename = &args[1];

    let mut vm = VM::new();

    let mut tscl_compiler = Compiler::new();

    vm.setup_stdlib();

    let source = fs::read_to_string(filename).expect("Failed to read file");

    println!("Compiling...");

    match tscl_compiler.compile(&source) {
        Ok(bytecode) => {
            println!("Loading bytecode into VM...");
            vm.load_program(bytecode);

            println!("Bytecode generated successfully. Running VM...");
            vm.run_event_loop();
        }
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            return;
        }
    }
}
