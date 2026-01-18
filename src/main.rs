mod backend;
mod compiler;
use compiler::Compiler;
mod ir;
mod loader;
mod runtime;
mod stdlib;
pub mod types;
mod vm;

use swc_ecma_parser::Syntax;

use crate::loader::BytecodeDecoder;
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
fn load_and_run_script(
    vm: &mut VM,
    compiler: &mut Compiler,
    path: &str,
    append: bool,
) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;

    // Determine syntax based on file extension
    let syntax = if path.ends_with(".ts") || path.ends_with(".tsx") {
        Some(Syntax::Typescript(Default::default()))
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript for .tscl files
        Some(Syntax::Typescript(Default::default()))
    };

    let bytecode = compiler
        .compile_with_syntax(&source, syntax)
        .map_err(|e| format!("Failed to compile {}: {}", path, e))?;
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

/// Load and run a pre-compiled bytecode file
fn run_binary_file(vm: &mut VM, path: &str) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|e| format!("Failed to read binary file {}: {}", path, e))?;

    let mut decoder = BytecodeDecoder::new(&bytes);

    match decoder.decode_all() {
        Ok(program) => {
            println!("Loaded {} instructions from binary file", program.len());
            // Debug: print each instruction
            for (i, op) in program.iter().enumerate() {
                println!("  [{}] {:?}", i, op);
            }
            // Debug: check if console is in global frame
            if let Some(frame) = vm.call_stack.first() {
                println!("Global frame has {} locals", frame.locals.len());
                if frame.locals.contains_key("console") {
                    println!("  - console: found");
                } else {
                    println!("  - console: NOT FOUND!");
                }
            }
            let offset = vm.append_program(program);
            println!("Running from offset {}...", offset);
            vm.run_event_loop();
            println!("Execution complete.");
            Ok(())
        }
        Err(e) => Err(format!("Failed to decode bytecode: {}", e)),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <command> [args...]", args[0]);
        eprintln!("Commands:");
        eprintln!("  check <filename>     Check a .tscl file for errors (for LSP)");
        eprintln!("  ir <filename>        Dump SSA IR for a .tscl file");
        eprintln!("  jit <filename>       Run a .tscl file with JIT compilation");
        eprintln!("  <filename>           Run a .tscl file (VM interpreter)");
        eprintln!("  --run-binary <file>  Run a bytecode file (.bc)");
        return;
    }

    let command = &args[1];

    // Handle "check" command for LSP diagnostics
    if command == "check" {
        if args.len() < 3 {
            eprintln!("Usage: {} check <filename>", args[0]);
            std::process::exit(1);
        }
        let filename = &args[2];
        check_file(filename);
        return;
    }

    // Handle "ir" command to dump SSA IR
    if command == "ir" {
        if args.len() < 3 {
            eprintln!("Usage: {} ir <filename>", args[0]);
            std::process::exit(1);
        }
        let filename = &args[2];
        dump_ir(filename);
        return;
    }

    // Handle "jit" command for JIT compilation
    if command == "jit" {
        if args.len() < 3 {
            eprintln!("Usage: {} jit <filename>", args[0]);
            std::process::exit(1);
        }
        let filename = &args[2];
        run_jit(filename);
        return;
    }

    let filename = command;

    // Check if we should run in binary mode
    let run_binary = args.iter().any(|a| a == "--run-binary")
        || filename.ends_with(".bc")
        || filename.ends_with(".tscl.bc");

    let mut vm = VM::new();
    let mut compiler = Compiler::new();

    // Setup standard library
    vm.setup_stdlib();

    // Binary mode: load and run pre-compiled bytecode directly
    if run_binary {
        println!("Running bytecode file: {}", filename);
        if let Err(e) = run_binary_file(&mut vm, filename) {
            eprintln!("{}", e);
        }
        return;
    }

    // 1. Load and run prelude first (if exists)
    // This sets up global constants (OP, TOKEN, TYPE) and utility functions
    if Path::new(PRELUDE_PATH).exists() {
        println!("Loading prelude...");
        if let Err(e) = load_and_run_script(&mut vm, &mut compiler, PRELUDE_PATH, false) {
            eprintln!("{}", e);
            return;
        }
    }

    // 2. Check if this is a bootstrap file that needs the compiler modules
    let is_bootstrap = filename.contains("bootstrap/");

    if is_bootstrap {
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

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        Some(Syntax::Typescript(Default::default()))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript for .tscl files
        Some(Syntax::Typescript(Default::default()))
    };

    match compiler.compile_with_syntax(&main_source, syntax) {
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

/// Dump SSA IR for a file
fn dump_ir(filename: &str) {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", filename, e);
            std::process::exit(1);
        }
    };

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        Some(Syntax::Typescript(Default::default()))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript for .tscl files
        Some(Syntax::Typescript(Default::default()))
    };

    let mut compiler = Compiler::new();
    let bytecode = match compiler.compile_with_syntax(&source, syntax) {
        Ok(bc) => bc,
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            std::process::exit(1);
        }
    };

    println!("=== Bytecode ({} instructions) ===", bytecode.len());
    for (i, op) in bytecode.iter().enumerate() {
        println!("  [{:4}] {:?}", i, op);
    }
    println!();

    // Lower to SSA IR
    match ir::lower::lower_module(&bytecode) {
        Ok(mut module) => {
            println!("=== SSA IR (before optimization) ===");
            println!("{}", module);

            // Run type inference and specialization
            ir::typecheck::typecheck_module(&mut module);
            println!("=== SSA IR (after type inference) ===");
            println!("{}", module);

            // Run optimizations
            ir::opt::optimize_module(&mut module);
            println!("=== SSA IR (after optimization) ===");
            println!("{}", module);
        }
        Err(e) => {
            eprintln!("IR lowering failed: {}", e);
            std::process::exit(1);
        }
    }
}

/// Check a file for errors without running it
fn check_file(filename: &str) {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}:1:1: Failed to read file: {}", filename, e);
            std::process::exit(1);
        }
    };

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        Some(Syntax::Typescript(Default::default()))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript for .tscl files
        Some(Syntax::Typescript(Default::default()))
    };

    let mut compiler = Compiler::new();
    match compiler.compile_with_syntax(&source, syntax) {
        Ok(_) => {
            // Success - no errors
            std::process::exit(0);
        }
        Err(e) => {
            // Parse error message to extract line/column if possible
            // Format: "Parsing error: ..." or "BORROW ERROR: ..." or "LIFETIME ERROR: ..."

            // Try to find the line number from the error
            let mut line_num = 1;
            let col_num = 1;

            // Check if error contains line information from SWC
            if e.contains("error at") || e.contains("line") {
                // Try to extract line number from error message
                // This is a simple heuristic - SWC errors might have different formats
                let lines: Vec<&str> = source.lines().collect();
                for (i, _line) in lines.iter().enumerate() {
                    if e.contains(&format!("line {}", i + 1))
                        || e.contains(&format!("{}:{}", filename, i + 1))
                    {
                        line_num = i + 1;
                        break;
                    }
                }
            }

            // Output in format: filename:line:col: message
            eprintln!("{}:{}:{}: {}", filename, line_num, col_num, e);
            std::process::exit(1);
        }
    }
}

/// Run a file using JIT compilation
fn run_jit(filename: &str) {
    use crate::backend::{BackendConfig, jit::JitRuntime};

    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", filename, e);
            std::process::exit(1);
        }
    };

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        Some(Syntax::Typescript(Default::default()))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript for .tscl files
        Some(Syntax::Typescript(Default::default()))
    };

    // Compile to bytecode
    let mut compiler = Compiler::new();
    let bytecode = match compiler.compile_with_syntax(&source, syntax) {
        Ok(bc) => bc,
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            std::process::exit(1);
        }
    };

    println!("=== Bytecode ({} instructions) ===", bytecode.len());
    for (i, op) in bytecode.iter().enumerate() {
        println!("[{:3}] {:?}", i, op);
    }

    // Lower to SSA IR
    let lowerer = ir::lower::Lowerer::new("main".to_string());
    match lowerer.lower(&bytecode) {
        Ok(mut func) => {
            // Create module
            let mut module = ir::IrModule::new();
            module.add_function(func);

            // Run type inference
            ir::typecheck::typecheck_module(&mut module);

            // Run optimizations
            ir::opt::optimize_module(&mut module);

            println!("\n=== SSA IR (optimized) ===");
            println!("{}", module);

            // JIT compile
            println!("\n=== JIT Compilation ===");
            let config = BackendConfig::default();
            let mut runtime = match JitRuntime::new(&config) {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create JIT runtime: {}", e);
                    std::process::exit(1);
                }
            };

            match runtime.compile(&module) {
                Ok(()) => {
                    println!("JIT compilation successful!");

                    // Try to call main
                    println!("\n=== Execution ===");
                    match runtime.call_main() {
                        Ok(result) => {
                            println!("Result: {:?}", result);
                        }
                        Err(e) => {
                            eprintln!("Execution failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("JIT compilation failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("IR lowering failed: {}", e);
            std::process::exit(1);
        }
    }
}
