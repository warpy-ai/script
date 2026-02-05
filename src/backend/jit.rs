//! JIT runtime for tscl
//!
//! This module provides the JIT compilation and execution runtime.
//! It manages compiled functions and provides the interface for executing
//! native code.

use std::collections::HashMap;

use super::cranelift::CraneliftCodegen;
use super::{BackendConfig, BackendError};
use crate::ir::IrModule;
use crate::runtime::abi::OtValue;

/// JIT runtime for executing compiled code
pub struct JitRuntime {
    /// The Cranelift code generator
    codegen: CraneliftCodegen,
    /// Cached function pointers by name
    compiled_funcs: HashMap<String, *const u8>,
}

impl JitRuntime {
    /// Create a new JIT runtime
    pub fn new(config: &BackendConfig) -> Result<Self, BackendError> {
        let codegen = CraneliftCodegen::new(config)?;

        Ok(Self {
            codegen,
            compiled_funcs: HashMap::new(),
        })
    }

    pub fn get_func_ptr(&self, name: &str) -> Option<*const u8> {
        self.codegen.get_func(name)
    }

    /// Compile an IR module
    pub fn compile(&mut self, module: &IrModule) -> Result<(), BackendError> {
        // Use the new compile_module method for proper inter-function call support
        self.codegen.compile_module(module)?;

        // Copy compiled function pointers
        for (name, ptr) in self.codegen.get_all_funcs() {
            self.compiled_funcs.insert(name, ptr);
        }

        Ok(())
    }

    /// Get a compiled function by name
    pub fn get_func(&self, name: &str) -> Option<*const u8> {
        self.compiled_funcs.get(name).copied()
    }

    /// Get all compiled functions
    pub fn get_all_funcs(&self) -> HashMap<String, *const u8> {
        self.compiled_funcs.clone()
    }

    /// Call the main function with no arguments
    pub fn call_main(&self) -> Result<OtValue, BackendError> {
        let ptr = self
            .get_func("main")
            .ok_or_else(|| BackendError::JitError("No 'main' function found".into()))?;

        // Cast to function pointer and call
        // Safety: We trust the compiled code is valid
        let main_fn: extern "C" fn() -> u64 = unsafe { std::mem::transmute(ptr) };
        let result = main_fn();

        Ok(OtValue::from_bits(result))
    }

    /// Call a named function with arguments
    pub fn call_func(&self, name: &str, args: &[OtValue]) -> Result<OtValue, BackendError> {
        let ptr = self
            .get_func(name)
            .ok_or_else(|| BackendError::JitError(format!("Function '{}' not found", name)))?;

        // For now, only support 0-3 arguments
        // A proper implementation would use libffi or similar
        let result = match args.len() {
            0 => {
                let f: extern "C" fn() -> u64 = unsafe { std::mem::transmute(ptr) };
                f()
            }
            1 => {
                let f: extern "C" fn(u64) -> u64 = unsafe { std::mem::transmute(ptr) };
                f(args[0].to_bits())
            }
            2 => {
                let f: extern "C" fn(u64, u64) -> u64 = unsafe { std::mem::transmute(ptr) };
                f(args[0].to_bits(), args[1].to_bits())
            }
            3 => {
                let f: extern "C" fn(u64, u64, u64) -> u64 = unsafe { std::mem::transmute(ptr) };
                f(args[0].to_bits(), args[1].to_bits(), args[2].to_bits())
            }
            _ => {
                return Err(BackendError::JitError(
                    "Too many arguments (max 3 supported)".into(),
                ));
            }
        };

        Ok(OtValue::from_bits(result))
    }

    /// Execute a simple numeric function for benchmarking
    ///
    /// This is a convenience method for testing numeric computations
    /// like fibonacci.
    pub fn call_numeric(&self, name: &str, arg: f64) -> Result<f64, BackendError> {
        let input = OtValue::number(arg);
        let result = self.call_func(name, &[input])?;

        result
            .as_number()
            .ok_or_else(|| BackendError::JitError("Expected numeric result".into()))
    }
}

/// Compiled function handle for type-safe calls
pub struct CompiledFunction {
    ptr: *const u8,
    arg_count: usize,
}

impl CompiledFunction {
    /// Create a new compiled function handle
    pub fn new(ptr: *const u8, arg_count: usize) -> Self {
        Self { ptr, arg_count }
    }

    /// Get the raw function pointer
    pub fn ptr(&self) -> *const u8 {
        self.ptr
    }

    /// Get the argument count
    pub fn arg_count(&self) -> usize {
        self.arg_count
    }

    /// Call with no arguments
    pub fn call0(&self) -> OtValue {
        assert_eq!(self.arg_count, 0);
        let f: extern "C" fn() -> u64 = unsafe { std::mem::transmute(self.ptr) };
        OtValue::from_bits(f())
    }

    /// Call with one argument
    pub fn call1(&self, a: OtValue) -> OtValue {
        assert_eq!(self.arg_count, 1);
        let f: extern "C" fn(u64) -> u64 = unsafe { std::mem::transmute(self.ptr) };
        OtValue::from_bits(f(a.to_bits()))
    }

    /// Call with two arguments
    pub fn call2(&self, a: OtValue, b: OtValue) -> OtValue {
        assert_eq!(self.arg_count, 2);
        let f: extern "C" fn(u64, u64) -> u64 = unsafe { std::mem::transmute(self.ptr) };
        OtValue::from_bits(f(a.to_bits(), b.to_bits()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IrFunction, IrModule, IrOp, IrType, Literal, Terminator, ValueId};

    #[test]
    fn test_jit_runtime_creation() {
        let config = BackendConfig::default();
        let runtime = JitRuntime::new(&config);
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_compile_simple_function() {
        let config = BackendConfig::default();
        let mut runtime = JitRuntime::new(&config).unwrap();

        // Create a simple function that returns a constant
        let mut func = IrFunction::new("test_const".to_string());
        let entry = func.alloc_block();

        // v0 = 42.0
        let v0 = func.alloc_value(IrType::Number);
        func.block_mut(entry)
            .push(IrOp::Const(v0, Literal::Number(42.0)));
        func.block_mut(entry)
            .terminate(Terminator::Return(Some(v0)));

        let mut module = IrModule::new();
        module.add_function(func);

        let result = runtime.compile(&module);
        assert!(result.is_ok());

        // Call the function
        let result = runtime.call_func("test_const", &[]);
        assert!(result.is_ok());

        let val = result.unwrap();
        assert_eq!(val.as_number(), Some(42.0));
    }

    #[test]
    fn test_compile_add_function() {
        let config = BackendConfig::default();
        let mut runtime = JitRuntime::new(&config).unwrap();

        // Create a function that adds two numbers
        let mut func = IrFunction::new("add_nums".to_string());
        func.params = vec![
            ("a".to_string(), IrType::Number),
            ("b".to_string(), IrType::Number),
        ];

        let entry = func.alloc_block();

        // Parameters are v0 and v1
        let v0 = ValueId(0);
        let v1 = ValueId(1);
        func.value_types.insert(v0, IrType::Number);
        func.value_types.insert(v1, IrType::Number);

        // v2 = v0 + v1
        let v2 = func.alloc_value(IrType::Number);
        func.block_mut(entry).push(IrOp::AddNum(v2, v0, v1));
        func.block_mut(entry)
            .terminate(Terminator::Return(Some(v2)));

        let mut module = IrModule::new();
        module.add_function(func);

        let result = runtime.compile(&module);
        assert!(result.is_ok());

        // Call the function with 3.0 + 4.0
        let a = OtValue::number(3.0);
        let b = OtValue::number(4.0);
        let result = runtime.call_func("add_nums", &[a, b]);
        assert!(result.is_ok());

        let val = result.unwrap();
        assert_eq!(val.as_number(), Some(7.0));
    }
}
