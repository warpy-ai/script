//! Cranelift code generation for tscl IR
//!
//! This module translates tscl SSA IR to Cranelift IR, which is then compiled
//! to native machine code. Key design decisions:
//!
//! - All values are 64-bit (NaN-boxed)
//! - Specialized ops (AddNum, etc.) compile to direct FP instructions
//! - Dynamic ops (AddAny, etc.) call runtime stubs
//! - Borrow ops are zero-cost (just pointer copies)

use cranelift::prelude::*;
use cranelift_codegen::ir::{FuncRef, StackSlot};
use cranelift_codegen::settings;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::HashMap;

use super::layout::VALUE_SIZE;
use super::{BackendConfig, BackendError};
use crate::ir::{BasicBlock, BlockId, IrFunction, IrOp, Literal, Terminator, ValueId};

/// Cranelift code generator
pub struct CraneliftCodegen {
    /// The JIT module being built
    module: JITModule,
    /// Codegen context (reused for each function)
    ctx: codegen::Context,
    /// Function builder context (reused)
    builder_ctx: FunctionBuilderContext,
    /// Declared runtime stubs (name -> FuncId)
    stubs: HashMap<String, FuncId>,
    /// Compiled function pointers
    compiled_funcs: HashMap<String, *const u8>,
    /// Backend configuration
    config: BackendConfig,
}

impl CraneliftCodegen {
    /// Create a new code generator
    pub fn new(config: &BackendConfig) -> Result<Self, BackendError> {
        // Create JIT builder with appropriate settings for the platform
        let mut flag_builder = settings::builder();
        
        // Disable PLT on aarch64 since it's not supported in cranelift-jit 0.113
        flag_builder.set("use_colocated_libcalls", "true").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        
        let isa_builder = cranelift_native::builder()
            .map_err(|e| BackendError::Cranelift(format!("Failed to create ISA builder: {}", e)))?;
        
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| BackendError::Cranelift(format!("Failed to create ISA: {}", e)))?;

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Register runtime stubs as symbols
        Self::register_runtime_symbols(&mut builder);

        let module = JITModule::new(builder);

        let ctx = module.make_context();

        // Note: Optimization level is configured via the ISA settings,
        // not on the Context directly. For now, we use defaults.

        Ok(Self {
            module,
            ctx,
            builder_ctx: FunctionBuilderContext::new(),
            stubs: HashMap::new(),
            compiled_funcs: HashMap::new(),
            config: config.clone(),
        })
    }

    /// Register runtime stub functions as symbols for the JIT
    fn register_runtime_symbols(builder: &mut JITBuilder) {
        use crate::runtime::stubs::*;

        // Allocation stubs
        builder.symbol("tscl_alloc_object", tscl_alloc_object as *const u8);
        builder.symbol("tscl_alloc_array", tscl_alloc_array as *const u8);
        builder.symbol("tscl_alloc_string", tscl_alloc_string as *const u8);

        // Property access stubs
        builder.symbol("tscl_get_prop", tscl_get_prop as *const u8);
        builder.symbol("tscl_set_prop", tscl_set_prop as *const u8);
        builder.symbol("tscl_get_element", tscl_get_element as *const u8);
        builder.symbol("tscl_set_element", tscl_set_element as *const u8);

        // Dynamic arithmetic stubs
        builder.symbol("tscl_add_any", tscl_add_any as *const u8);
        builder.symbol("tscl_sub_any", tscl_sub_any as *const u8);
        builder.symbol("tscl_mul_any", tscl_mul_any as *const u8);
        builder.symbol("tscl_div_any", tscl_div_any as *const u8);
        builder.symbol("tscl_mod_any", tscl_mod_any as *const u8);

        // Comparison stubs
        builder.symbol("tscl_eq_strict", tscl_eq_strict as *const u8);
        builder.symbol("tscl_lt", tscl_lt as *const u8);
        builder.symbol("tscl_gt", tscl_gt as *const u8);
        builder.symbol("tscl_not", tscl_not as *const u8);
        builder.symbol("tscl_neg", tscl_neg as *const u8);

        // Type conversion stubs
        builder.symbol("tscl_to_boolean", tscl_to_boolean as *const u8);
        builder.symbol("tscl_to_number", tscl_to_number as *const u8);

        // Console/IO stubs
        builder.symbol("tscl_console_log", tscl_console_log as *const u8);
    }

    /// Declare a runtime stub function in the module
    fn declare_stub(&mut self, name: &str, arg_count: usize) -> Result<FuncId, BackendError> {
        if let Some(&id) = self.stubs.get(name) {
            return Ok(id);
        }

        // All stubs use u64 arguments and return u64
        let mut sig = self.module.make_signature();
        for _ in 0..arg_count {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));

        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| BackendError::Cranelift(format!("Failed to declare stub {}: {}", name, e)))?;

        self.stubs.insert(name.to_string(), id);
        Ok(id)
    }

    /// Compile a single function
    pub fn compile_function(&mut self, func: &IrFunction) -> Result<*const u8, BackendError> {
        // Clear previous context
        self.ctx.clear();

        // Create function signature
        let mut sig = self.module.make_signature();

        // Add parameters (all values are i64 / NaN-boxed)
        for _ in &func.params {
            sig.params.push(AbiParam::new(types::I64));
        }

        // Add return value
        sig.returns.push(AbiParam::new(types::I64));

        self.ctx.func.signature = sig.clone();

        // Declare the function
        let func_name = if func.name.is_empty() {
            "anonymous"
        } else {
            &func.name
        };

        let func_id = self
            .module
            .declare_function(func_name, Linkage::Export, &sig)
            .map_err(|e| {
                BackendError::Cranelift(format!("Failed to declare function {}: {}", func_name, e))
            })?;

        // Build the function body
        {
            let mut builder =
                FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);

            translate_function(&mut builder, &mut self.module, func)?;

            builder.finalize();
        }

        // Compile the function
        self.module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| {
                BackendError::Cranelift(format!("Failed to compile function {}: {}", func_name, e))
            })?;

        // Finalize and get the function pointer
        self.module.finalize_definitions().map_err(|e| {
            BackendError::Cranelift(format!("Failed to finalize: {}", e))
        })?;

        let ptr = self.module.get_finalized_function(func_id);
        self.compiled_funcs.insert(func_name.to_string(), ptr);

        Ok(ptr)
    }

    /// Get a compiled function by name
    pub fn get_func(&self, name: &str) -> Option<*const u8> {
        self.compiled_funcs.get(name).copied()
    }

    /// Get all compiled functions
    pub fn get_all_funcs(&self) -> HashMap<String, *const u8> {
        self.compiled_funcs.clone()
    }
}

/// Translate a function from tscl IR to Cranelift IR
fn translate_function(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ir_func: &IrFunction,
) -> Result<(), BackendError> {
    let mut ctx = TranslationContext {
        values: HashMap::new(),
        blocks: HashMap::new(),
        locals: Vec::new(),
        stubs: HashMap::new(),
    };

    // Create Cranelift blocks for each IR block
    for block in &ir_func.blocks {
        let cl_block = builder.create_block();
        ctx.blocks.insert(block.id, cl_block);
    }

    // Create stack slots for locals
    for _ in &ir_func.locals {
        let slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            VALUE_SIZE,
            0,
        ));
        ctx.locals.push(slot);
    }

    // Set up entry block with parameters
    let entry_block = ctx.blocks[&BlockId(0)];
    builder.switch_to_block(entry_block);
    builder.append_block_params_for_function_params(entry_block);

    // Map parameters to values
    let params = builder.block_params(entry_block).to_vec();
    for (i, param) in params.iter().enumerate() {
        ctx.values.insert(ValueId(i as u32), *param);
    }

    // Translate each block
    for (i, block) in ir_func.blocks.iter().enumerate() {
        if i > 0 {
            let cl_block = ctx.blocks[&block.id];
            builder.switch_to_block(cl_block);
        }

        translate_block(builder, module, &mut ctx, block)?;
    }

    // Seal all blocks
    builder.seal_all_blocks();

    Ok(())
}

/// Translation context holding state during function translation
struct TranslationContext {
    /// Map from IR ValueId to Cranelift Value
    values: HashMap<ValueId, Value>,
    /// Map from IR BlockId to Cranelift Block
    blocks: HashMap<BlockId, Block>,
    /// Local variable storage (stack slots)
    locals: Vec<StackSlot>,
    /// Declared stubs in this function
    stubs: HashMap<String, FuncRef>,
}

/// Translate a single basic block
fn translate_block(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    block: &BasicBlock,
) -> Result<(), BackendError> {
    // Translate each operation
    for op in &block.ops {
        translate_op(builder, module, ctx, op)?;
    }

    // Translate terminator
    translate_terminator(builder, ctx, &block.terminator)?;

    Ok(())
}

/// Translate a single IR operation
fn translate_op(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    op: &IrOp,
) -> Result<(), BackendError> {
    match op {
        // === Constants ===
        IrOp::Const(dst, lit) => {
            let val = translate_literal(builder, lit);
            ctx.values.insert(*dst, val);
        }

        // === Specialized Numeric Operations (inline) ===
        IrOp::AddNum(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            // Bitcast to f64, add, bitcast back
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let result = builder.ins().fadd(fa, fb);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::SubNum(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let result = builder.ins().fsub(fa, fb);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::MulNum(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let result = builder.ins().fmul(fa, fb);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::DivNum(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let result = builder.ins().fdiv(fa, fb);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::ModNum(dst, a, b) => {
            // Cranelift doesn't have fmod, so we compute a - floor(a/b) * b
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let div = builder.ins().fdiv(fa, fb);
            let floored = builder.ins().floor(div);
            let prod = builder.ins().fmul(floored, fb);
            let result = builder.ins().fsub(fa, prod);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::NegNum(dst, a) => {
            let va = get_value(ctx, *a)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let result = builder.ins().fneg(fa);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        // === Dynamic Operations (call stubs) ===
        IrOp::AddAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_add_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::SubAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_sub_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::MulAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_mul_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::DivAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_div_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::ModAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_mod_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::NegAny(dst, a) => {
            let result = call_stub(builder, module, ctx, "tscl_neg", &[*a])?;
            ctx.values.insert(*dst, result);
        }

        // === Comparison Operations ===
        IrOp::Lt(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let cmp = builder.ins().fcmp(FloatCC::LessThan, fa, fb);
            // Convert bool to NaN-boxed boolean
            let result = bool_to_tscl_value(builder, cmp);
            ctx.values.insert(*dst, result);
        }

        IrOp::LtEq(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let cmp = builder.ins().fcmp(FloatCC::LessThanOrEqual, fa, fb);
            let result = bool_to_tscl_value(builder, cmp);
            ctx.values.insert(*dst, result);
        }

        IrOp::Gt(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let cmp = builder.ins().fcmp(FloatCC::GreaterThan, fa, fb);
            let result = bool_to_tscl_value(builder, cmp);
            ctx.values.insert(*dst, result);
        }

        IrOp::GtEq(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let cmp = builder.ins().fcmp(FloatCC::GreaterThanOrEqual, fa, fb);
            let result = bool_to_tscl_value(builder, cmp);
            ctx.values.insert(*dst, result);
        }

        IrOp::EqStrict(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_eq_strict", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::NeStrict(dst, a, b) => {
            // Call eq_strict then negate
            let eq_result = call_stub(builder, module, ctx, "tscl_eq_strict", &[*a, *b])?;
            let result = call_stub_with_values(builder, module, ctx, "tscl_not", &[eq_result])?;
            ctx.values.insert(*dst, result);
        }

        // === Logical Operations ===
        IrOp::Not(dst, a) => {
            let result = call_stub(builder, module, ctx, "tscl_not", &[*a])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::And(dst, a, _b) | IrOp::Or(dst, a, _b) => {
            // For now, simple evaluation (not short-circuit)
            // Short-circuit would require control flow
            let va = get_value(ctx, *a)?;
            // Return first truthy/falsy value (simplified)
            ctx.values.insert(*dst, va);
        }

        // === Local Variable Operations ===
        IrOp::LoadLocal(dst, slot) => {
            let stack_slot = ctx.locals.get(*slot as usize).ok_or_else(|| {
                BackendError::Cranelift(format!("Invalid local slot: {}", slot))
            })?;
            let val = builder.ins().stack_load(types::I64, *stack_slot, 0);
            ctx.values.insert(*dst, val);
        }

        IrOp::StoreLocal(slot, src) => {
            let val = get_value(ctx, *src)?;
            let stack_slot = ctx.locals.get(*slot as usize).ok_or_else(|| {
                BackendError::Cranelift(format!("Invalid local slot: {}", slot))
            })?;
            builder.ins().stack_store(val, *stack_slot, 0);
        }

        // === Global Variable Operations ===
        IrOp::LoadGlobal(dst, _name) => {
            // TODO: Implement global variable access
            // For now, return undefined
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        IrOp::StoreGlobal(_name, _src) => {
            // TODO: Implement global variable store
        }

        // === Object Operations ===
        IrOp::NewObject(dst) => {
            let result = call_stub_no_args(builder, module, ctx, "tscl_alloc_object")?;
            ctx.values.insert(*dst, result);
        }

        IrOp::GetProp(dst, obj, _name) => {
            // TODO: Pass property name as string pointer
            let result = call_stub(builder, module, ctx, "tscl_get_prop", &[*obj])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::SetProp(obj, _name, val) => {
            // TODO: Pass property name as string pointer
            call_stub(builder, module, ctx, "tscl_set_prop", &[*obj, *val])?;
        }

        IrOp::GetElement(dst, obj, idx) => {
            let result = call_stub(builder, module, ctx, "tscl_get_element", &[*obj, *idx])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::SetElement(obj, idx, val) => {
            call_stub(builder, module, ctx, "tscl_set_element", &[*obj, *idx, *val])?;
        }

        // === Array Operations ===
        IrOp::NewArray(dst) => {
            let capacity = builder.ins().iconst(types::I64, 8);
            let result = call_stub_with_values(builder, module, ctx, "tscl_alloc_array", &[capacity])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::ArrayLen(dst, arr) => {
            // Get length property
            let result = call_stub(builder, module, ctx, "tscl_get_prop", &[*arr])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::ArrayPush(arr, val) => {
            // TODO: Implement proper array push
            call_stub(builder, module, ctx, "tscl_set_element", &[*arr, *val])?;
        }

        // === Copy/Move Operations ===
        IrOp::Copy(dst, src) | IrOp::Move(dst, src) | IrOp::Borrow(dst, src) | IrOp::BorrowMut(dst, src) => {
            let val = get_value(ctx, *src)?;
            ctx.values.insert(*dst, val);
        }

        IrOp::Clone(dst, src) => {
            // For now, just copy (proper clone would allocate)
            let val = get_value(ctx, *src)?;
            ctx.values.insert(*dst, val);
        }

        IrOp::Deref(dst, src) => {
            // Dereference: load through pointer
            let ptr = get_value(ctx, *src)?;
            let val = builder.ins().load(types::I64, MemFlags::new(), ptr, 0);
            ctx.values.insert(*dst, val);
        }

        IrOp::DerefStore(dst, src) => {
            // Store through pointer
            let ptr = get_value(ctx, *dst)?;
            let val = get_value(ctx, *src)?;
            builder.ins().store(MemFlags::new(), val, ptr, 0);
        }

        IrOp::EndBorrow(_) => {
            // No-op at runtime (borrow checking is compile-time)
        }

        // === Type Operations ===
        IrOp::TypeCheck(dst, _val, _ty) => {
            // Type checks are compile-time in typed code
            let one = builder.ins().iconst(types::I8, 1);
            let true_val = bool_to_tscl_value(builder, one);
            ctx.values.insert(*dst, true_val);
        }

        IrOp::TypeGuard(dst, val, _ty) => {
            // Just pass through the value
            let v = get_value(ctx, *val)?;
            ctx.values.insert(*dst, v);
        }

        IrOp::ToBool(dst, val) => {
            let result = call_stub(builder, module, ctx, "tscl_to_boolean", &[*val])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::ToNum(dst, val) => {
            let result = call_stub(builder, module, ctx, "tscl_to_number", &[*val])?;
            ctx.values.insert(*dst, result);
        }

        // === Phi Functions ===
        IrOp::Phi(dst, _entries) => {
            // Phi nodes are handled by Cranelift's SSA construction
            // For now, just use undefined
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        // === Function Operations ===
        IrOp::Call(dst, _func, _args) => {
            // TODO: Implement function calls
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        IrOp::CallMethod(dst, _obj, _name, _args) => {
            // TODO: Implement method calls
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        IrOp::CallMono(dst, _mono_id, _args) => {
            // TODO: Implement monomorphized calls
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        IrOp::MakeClosure(dst, _addr, _env) => {
            // TODO: Implement closure creation
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        IrOp::LoadThis(dst) => {
            // Load 'this' from first parameter or undefined
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        // === Struct Operations ===
        IrOp::StructNew(dst, _struct_id) => {
            let result = call_stub_no_args(builder, module, ctx, "tscl_alloc_object")?;
            ctx.values.insert(*dst, result);
        }

        IrOp::StructGetField(dst, src, _field_id) => {
            // TODO: Use field offset for direct access
            let val = get_value(ctx, *src)?;
            ctx.values.insert(*dst, val);
        }

        IrOp::StructSetField(dst, _field_id, val) => {
            // TODO: Use field offset for direct access
            let _ = get_value(ctx, *dst)?;
            let _ = get_value(ctx, *val)?;
        }

        IrOp::StructGetFieldNamed(dst, src, _name) => {
            let val = get_value(ctx, *src)?;
            ctx.values.insert(*dst, val);
        }

        IrOp::StructSetFieldNamed(dst, _name, val) => {
            let _ = get_value(ctx, *dst)?;
            let _ = get_value(ctx, *val)?;
        }
    }

    Ok(())
}

/// Translate a block terminator
fn translate_terminator(
    builder: &mut FunctionBuilder,
    ctx: &TranslationContext,
    term: &Terminator,
) -> Result<(), BackendError> {
    match term {
        Terminator::Jump(target) => {
            let block = ctx.blocks[target];
            builder.ins().jump(block, &[]);
        }

        Terminator::Branch(cond, true_block, false_block) => {
            let cond_val = get_value(ctx, *cond)?;
            // Check if truthy (not 0, not NaN, not undefined, etc.)
            // For simplicity, check if the boolean bit is set
            let is_truthy = tscl_value_to_bool(builder, cond_val);

            let true_bl = ctx.blocks[true_block];
            let false_bl = ctx.blocks[false_block];

            builder.ins().brif(is_truthy, true_bl, &[], false_bl, &[]);
        }

        Terminator::Return(val) => {
            let ret_val = match val {
                Some(v) => get_value(ctx, *v)?,
                None => translate_literal(builder, &Literal::Undefined),
            };
            builder.ins().return_(&[ret_val]);
        }

        Terminator::Unreachable => {
            builder.ins().trap(TrapCode::user(0).unwrap());
        }
    }

    Ok(())
}

/// Translate a literal to a Cranelift value
fn translate_literal(builder: &mut FunctionBuilder, lit: &Literal) -> Value {
    match lit {
        Literal::Number(n) => {
            let bits = n.to_bits();
            builder.ins().iconst(types::I64, bits as i64)
        }
        Literal::Boolean(b) => {
            // NaN-boxed boolean: QNAN | TAG_BOOLEAN | (b as u64)
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            const TAG_BOOLEAN: u64 = 0x0001_0000_0000_0000;
            let bits = QNAN | TAG_BOOLEAN | (*b as u64);
            builder.ins().iconst(types::I64, bits as i64)
        }
        Literal::Null => {
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            const TAG_NULL: u64 = 0x0002_0000_0000_0000;
            let bits = QNAN | TAG_NULL;
            builder.ins().iconst(types::I64, bits as i64)
        }
        Literal::Undefined => {
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            const TAG_UNDEFINED: u64 = 0x0003_0000_0000_0000;
            let bits = QNAN | TAG_UNDEFINED;
            builder.ins().iconst(types::I64, bits as i64)
        }
        Literal::String(_s) => {
            // TODO: Allocate string and return pointer
            // For now, return undefined
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            const TAG_UNDEFINED: u64 = 0x0003_0000_0000_0000;
            let bits = QNAN | TAG_UNDEFINED;
            builder.ins().iconst(types::I64, bits as i64)
        }
    }
}

/// Get a Cranelift value for an IR value
fn get_value(ctx: &TranslationContext, id: ValueId) -> Result<Value, BackendError> {
    ctx.values
        .get(&id)
        .copied()
        .ok_or_else(|| BackendError::Cranelift(format!("Undefined value: {:?}", id)))
}

/// Call a runtime stub with IR value IDs as arguments
fn call_stub(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    name: &str,
    args: &[ValueId],
) -> Result<Value, BackendError> {
    let arg_values: Vec<Value> = args
        .iter()
        .map(|id| get_value(ctx, *id))
        .collect::<Result<_, _>>()?;

    call_stub_with_values(builder, module, ctx, name, &arg_values)
}

/// Call a runtime stub with Cranelift values as arguments
fn call_stub_with_values(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    name: &str,
    args: &[Value],
) -> Result<Value, BackendError> {
    // Get or declare the stub
    let func_ref = if let Some(&f) = ctx.stubs.get(name) {
        f
    } else {
        // Declare the function
        let mut sig = module.make_signature();
        for _ in args {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));

        let func_id = module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| BackendError::Cranelift(format!("Failed to declare stub {}: {}", name, e)))?;

        let func_ref = module.declare_func_in_func(func_id, builder.func);
        ctx.stubs.insert(name.to_string(), func_ref);
        func_ref
    };

    let call = builder.ins().call(func_ref, args);
    let result = builder.inst_results(call)[0];
    Ok(result)
}

/// Call a runtime stub with no arguments
fn call_stub_no_args(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    name: &str,
) -> Result<Value, BackendError> {
    call_stub_with_values(builder, module, ctx, name, &[])
}

/// Convert a Cranelift boolean to a NaN-boxed boolean
fn bool_to_tscl_value(builder: &mut FunctionBuilder, b: Value) -> Value {
    const QNAN: u64 = 0x7FFC_0000_0000_0000;
    const TAG_BOOLEAN: u64 = 0x0001_0000_0000_0000;
    let base = builder.ins().iconst(types::I64, (QNAN | TAG_BOOLEAN) as i64);
    let b_i64 = builder.ins().uextend(types::I64, b);
    builder.ins().bor(base, b_i64)
}

/// Convert a NaN-boxed boolean to a Cranelift boolean
fn tscl_value_to_bool(builder: &mut FunctionBuilder, val: Value) -> Value {
    // Check if the low bit is set (for booleans)
    // This is a simplified check - proper impl would check type tag
    let one = builder.ins().iconst(types::I64, 1);
    let masked = builder.ins().band(val, one);
    builder.ins().icmp_imm(IntCC::NotEqual, masked, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codegen_creation() {
        let config = BackendConfig::default();
        let codegen = CraneliftCodegen::new(&config);
        assert!(codegen.is_ok());
    }
}
