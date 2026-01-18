# tscl: Development Progress

A high-performance JavaScript-like scripting language pivoting from VM-based interpretation to native code execution.

## Architecture Evolution

### Original Architecture (VM-First)
```
tscl source → Rust compiler → Bytecode → Stack-based VM → CPU
```

### Target Architecture (Native-First)
```
tscl source → Compiler → SSA IR → Native backend (Cranelift/LLVM) → CPU
                  ↓
           Borrow checker
           Type inference
           Optimizations
```

The VM remains as a development tool for debugging, testing, and bootstrapping.

---

## Phase 0: Runtime Kernel Foundation ✅

**Goal:** Separate runtime primitives from execution engine.

### Files Created
| File | Purpose |
|------|---------|
| `src/runtime/mod.rs` | Module root for native runtime |
| `src/runtime/abi.rs` | NaN-boxed `TsclValue` for native interop |
| `src/runtime/heap.rs` | Bump allocator, native object layouts |
| `src/runtime/stubs.rs` | `extern "C"` functions callable from JIT/AOT |

### Runtime ABI
```rust
// NaN-boxing: 64-bit value packs type tag + payload in IEEE 754 NaN space
pub struct TsclValue { bits: u64 }

// Type tags embedded in quiet NaN
const TAG_BOOLEAN: u64   = 0x0001_0000_0000_0000;
const TAG_NULL: u64      = 0x0002_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x0003_0000_0000_0000;
const TAG_POINTER: u64   = 0x0000_0000_0000_0000;
```

### Runtime Stubs (20+)
- **Allocation:** `tscl_alloc_object`, `tscl_alloc_array`, `tscl_alloc_string`
- **Property access:** `tscl_get_prop`, `tscl_set_prop`, `tscl_get_element`, `tscl_set_element`
- **Arithmetic:** `tscl_add_any`, `tscl_sub_any`, `tscl_mul_any`, `tscl_div_any`, `tscl_mod_any`
- **Comparisons:** `tscl_eq_strict`, `tscl_lt`, `tscl_gt`, `tscl_not`, `tscl_neg`
- **Type ops:** `tscl_to_boolean`, `tscl_to_number`
- **I/O:** `tscl_console_log`, `tscl_call`

---

## Phase 1: SSA IR System ✅

**Goal:** Transform stack-based bytecode to register-based SSA form.

### Files Created
| File | Purpose |
|------|---------|
| `src/ir/mod.rs` | IR data structures, ownership system |
| `src/ir/lower.rs` | Bytecode → SSA lowering |
| `src/ir/typecheck.rs` | Flow-sensitive type inference |
| `src/ir/opt.rs` | DCE, constant folding, CSE, copy propagation |
| `src/ir/verify.rs` | IR validation, borrow checking |
| `src/ir/stubs.rs` | IR → runtime stub mapping |

### IR Design

#### Type System
```rust
pub enum IrType {
    Number,   // IEEE 754 f64
    String,   // Heap-allocated UTF-8
    Boolean,  // true/false
    Object,   // Heap-allocated object
    Array,    // Heap-allocated array
    Function, // Closure
    Any,      // Dynamic type
    Never,    // Bottom type
    Void,     // No value
}
```

#### Ownership System
```rust
pub enum Ownership {
    Owned,       // Value owned by this binding
    Moved,       // Value transferred (tombstone)
    BorrowedImm, // Read-only reference
    BorrowedMut, // Exclusive write access
    Captured,    // Captured by closure
}

pub enum StorageLocation {
    Stack,    // Fast, automatic cleanup
    Heap,     // GC managed
    Register, // Immediate, no address
}
```

#### IR Operations
```rust
pub enum IrOp {
    // Constants
    Const(ValueId, Literal),
    
    // Specialized arithmetic (fast path)
    AddNum(ValueId, ValueId, ValueId),
    SubNum(ValueId, ValueId, ValueId),
    MulNum(ValueId, ValueId, ValueId),
    
    // Dynamic arithmetic (needs runtime)
    AddAny(ValueId, ValueId, ValueId),
    SubAny(ValueId, ValueId, ValueId),
    
    // Control flow
    Jump(BlockId),
    Branch(ValueId, BlockId, BlockId),
    Return(Option<ValueId>),
    
    // ...40+ operations total
}
```

### Bytecode → SSA Lowering

| Bytecode | SSA IR |
|----------|--------|
| `Push(v)` | `Const(r, v)` |
| `Add` | `AddAny(dst, a, b)` → specialized after type inference |
| `Load(name)` | `LoadLocal(dst, slot)` |
| `Jump(addr)` | `Jump(block)` |
| `JumpIfFalse(addr)` | `Branch(cond, true_block, false_block)` |
| `Call(n)` | `Call(dst, func, args)` |

### Type Inference & Specialization

Forward dataflow propagates concrete types:
```
// Before type inference:
v2 = add.any v0, v1   // v0: num, v1: num

// After type inference:  
v2 = add.num v0, v1   // Specialized to numeric add!
```

### Optimization Passes

1. **Dead Code Elimination (DCE)** - Remove unused operations
2. **Constant Folding** - Evaluate `1 + 2` → `3` at compile time
3. **Common Subexpression Elimination (CSE)** - Reuse computed values
4. **Copy Propagation** - Replace copies with sources
5. **Branch Simplification** - Convert constant branches to jumps
6. **Unreachable Block Elimination** - Remove dead code paths

### IR Verification

- **SSA validation** - Each value defined exactly once
- **Use-after-move detection** - No use of moved values
- **Control flow validation** - All jump targets exist
- **Borrow rule checking** - No overlapping mutable borrows

### IR → Stub Mapping

```rust
pub enum CompileStrategy {
    Inline(InlineOp),    // Direct machine instruction
    StubCall(StubCall),  // Runtime function call
    NoOp,                // No codegen needed
}

// Specialized ops compile to inline instructions
IrOp::AddNum → CompileStrategy::Inline(InlineOp::FAdd)
IrOp::SubNum → CompileStrategy::Inline(InlineOp::FSub)

// Dynamic ops require runtime stubs
IrOp::AddAny → CompileStrategy::StubCall("tscl_add_any")
IrOp::GetProp → CompileStrategy::StubCall("tscl_get_prop")
```

### CLI Command
```bash
# Dump SSA IR for a file
./target/release/script ir <filename>
```

Outputs:
1. Bytecode listing
2. SSA IR before optimization
3. SSA IR after type inference
4. SSA IR after optimization

---

## Phase 2B: Native Backend ✅

**Goal:** Generate native machine code from SSA IR using Cranelift.

### Files Created
| File | Purpose |
|------|---------|
| `src/backend/mod.rs` | Backend manager, target selection |
| `src/backend/layout.rs` | Memory layout calculation for structs/arrays |
| `src/backend/cranelift.rs` | IR → Cranelift IR translation |
| `src/backend/jit.rs` | JIT compilation and execution runtime |
| `src/backend/aot.rs` | AOT compilation scaffold (future) |

### Backend Architecture
```rust
pub enum BackendKind {
    CraneliftJit,  // JIT compilation (implemented)
    CraneliftAot,  // AOT compilation (future)
    Interpreter,   // Fall back to VM
}

pub enum OptLevel {
    None,         // Fastest compile
    Speed,        // Default for JIT
    SpeedAndSize, // Default for AOT
}
```

### Cranelift Integration
- **IR Translation:** Each `IrOp` maps to Cranelift instructions or stub calls
- **NaN-boxing:** All values are 64-bit, uniform representation
- **Specialized ops:** `AddNum`, `SubNum`, etc. → inline FP instructions
- **Dynamic ops:** `AddAny`, etc. → call runtime stubs (`tscl_*` functions)
- **ARM64 Support:** Configured for non-PIC, colocated libcalls

### JIT Runtime
```rust
pub struct JitRuntime {
    codegen: CraneliftCodegen,
    compiled_funcs: HashMap<String, *const u8>,
}

impl JitRuntime {
    pub fn compile(&mut self, module: &IrModule) -> Result<(), BackendError>;
    pub fn call_main(&self) -> Result<TsclValue, BackendError>;
    pub fn call_func(&self, name: &str, args: &[TsclValue]) -> Result<TsclValue, BackendError>;
}
```

### Memory Layout
- **VALUE_SIZE:** 8 bytes (NaN-boxed)
- **VALUE_ALIGN:** 8 bytes
- **Struct layout:** Field offsets calculated with proper alignment
- **Frame layout:** Stack slots for locals + spill area

### CLI Command
```bash
# Run with JIT compilation
./target/release/script jit <filename>
```

### Implemented Operations
| Category | Operations |
|----------|------------|
| Constants | `Const` (numbers, booleans, null, undefined) |
| Arithmetic | `AddNum`, `SubNum`, `MulNum`, `DivNum`, `ModNum`, `NegNum` |
| Dynamic | `AddAny`, `SubAny`, `MulAny`, `DivAny`, `ModAny`, `NegAny` |
| Comparison | `Lt`, `LtEq`, `Gt`, `GtEq`, `EqStrict`, `NeStrict` |
| Logical | `Not`, `And`, `Or` |
| Variables | `LoadLocal`, `StoreLocal`, `LoadGlobal`, `StoreGlobal` |
| Objects | `NewObject`, `GetProp`, `SetProp`, `GetElement`, `SetElement` |
| Arrays | `NewArray`, `ArrayLen`, `ArrayPush` |
| Control | `Jump`, `Branch`, `Return` |
| Borrow | `Borrow`, `BorrowMut`, `Deref`, `DerefStore`, `EndBorrow` |
| Structs | `StructNew`, `StructGetField`, `StructSetField` |

### Future Work (Phase 2B-Beta/Gamma)
- [ ] Function calls (`Call`, `CallMethod`, `CallMono`)
- [ ] Closure creation (`MakeClosure`)
- [ ] Phi node handling for SSA merge points
- [ ] String literal allocation
- [ ] LLVM AOT backend
- [ ] Performance benchmarks vs VM

---

## Original VM System (Complete)

### Self-Hosting Bootstrap Compiler
- **Lexer** (`bootstrap/lexer.tscl`) - Tokenizes source into tokens
- **Parser** (`bootstrap/parser.tscl`) - Recursive descent parser producing AST
- **Emitter** (`bootstrap/emitter.tscl`) - Generates bytecode from AST using ByteStream
- **Two-Stage Loading** - Prelude loads first, then bootstrap modules, then main script
- **Bytecode Rebasing** - Appended bytecode has all addresses automatically adjusted

### Memory Management
- **Ownership Model** - Variables own their data; assigning objects moves ownership
- **Let vs Store Opcodes** - `Let` creates new bindings (shadowing), `Store` updates existing
- **Scoped Lifetimes** - Variables automatically freed when scope ends
- **Stack vs Heap** - Primitives on stack (copy), Objects/Arrays on heap (move)
- **Variable Lifting** - Captured variables lifted from stack to heap for closures

### Virtual Machine
- **Stack-based Architecture** - LIFO stack for expressions and operations
- **Call Stack & Frames** - Nested function calls with isolated local scopes
- **Heap Allocation** - Dynamic storage for Objects, Arrays, ByteStreams
- **Native Bridge** - Rust functions injected into JS environment
- **Event Loop** - Task queue with timer support (`setTimeout`)
- **Stack Overflow Protection** - Maximum call depth of 1000

### Closures & Functions
- **Function Declarations** - Named functions with parameters
- **Function Expressions** - Anonymous functions
- **Arrow Functions** - `(x) => x * 2` and `x => x * 2` syntax
- **Closures** - Capture outer scope variables via environment objects
- **Constructors** - `new` expressions with `this` binding

### Language Support
- **Variables** - `let` and `const` declarations
- **Objects** - Literals `{a: 1}`, property access `obj.a`, computed access `obj[key]`
- **Arrays** - Literals `[1, 2]`, indexed access `arr[0]`, methods (push, pop, etc.)
- **Control Flow** - `if`/`else`, `while`, `break`, `continue`
- **Operators** - Arithmetic (`+`, `-`, `*`, `/`, `%`), comparison, logical, unary (`!`, `-`)
- **String Methods** - `slice`, `charCodeAt`, `charAt`, `includes`, `trim`
- **Array Methods** - `push`, `pop`, `shift`, `unshift`, `splice`, `indexOf`, `includes`, `join`

### Standard Library
- **console.log** - Print values to stdout
- **setTimeout** - Schedule delayed execution
- **require** - Module loading (supports "fs")
- **fs.readFileSync** - Read file as string
- **fs.writeFileSync** - Write string to file
- **fs.writeBinaryFile** - Write binary data
- **ByteStream** - Binary data manipulation

---

## Bytecode Instruction Set

| OpCode | Description |
|--------|-------------|
| `Push(Value)` | Push constant onto stack |
| `Let(Name)` | Create new variable binding in current scope |
| `Store(Name)` | Update existing variable (searches all scopes) |
| `Load(Name)` | Push variable's value onto stack |
| `StoreLocal(idx)` | Store to indexed local slot |
| `LoadLocal(idx)` | Load from indexed local slot |
| `LoadThis` | Push current `this` context |
| `NewObject` | Allocate empty object on heap |
| `NewArray(Size)` | Allocate array of given size |
| `SetProp(Key)` | Set property on heap object |
| `GetProp(Key)` | Get property from heap object |
| `StoreElement` | Store value at array index |
| `LoadElement` | Load value from array index |
| `Call(ArgCount)` | Execute function with N arguments |
| `CallMethod(N,A)` | Call method on object |
| `Return` | Return from function |
| `Jump(Addr)` | Unconditional jump |
| `JumpIfFalse(Addr)` | Conditional branch |
| `MakeClosure(Addr)` | Create closure with captured environment |
| `Construct(Args)` | Construct new object instance |
| `Drop(Name)` | Free variable and its heap data |
| `Dup` | Duplicate top of stack |
| `Pop` | Discard top of stack |
| `Add/Sub/Mul/Div` | Arithmetic operations |
| `Mod` | Modulo operation |
| `Eq/EqEq/Ne/NeEq` | Equality comparisons |
| `Lt/LtEq/Gt/GtEq` | Comparison operations |
| `And/Or/Not` | Logical operations |
| `Neg` | Unary negation |
| `Require` | Load module |
| `Halt` | Stop execution |

---

## Performance Targets

| Benchmark | Node.js | Bun | Target tscl |
|-----------|---------|-----|-------------|
| HTTP hello world | 100k rps | 200k rps | 250k rps |
| JSON parse | 1x | 1.5x | 2x |
| fib(35) | 50ms | 30ms | 20ms |
| Startup | 30ms | 10ms | 5ms |

---

## Test Results

```
91 tests passed, 0 failed
```

All tests cover:
- IR lowering (simple, conditional, loops, function calls, variables)
- Type inference and specialization
- Constant folding
- Dead code elimination
- CSE
- IR verification (SSA, undefined values, control flow, ownership)
- Runtime stubs
- Heap allocation
- NaN-boxing
- Original VM functionality
- Borrow checker
- Closures and async
- **Backend:** Cranelift codegen creation
- **Backend:** JIT runtime creation
- **Backend:** Function compilation (constants, arithmetic)
- **Backend:** Memory layout calculation
- **Backend:** AOT target detection

---

## Next Steps

### Phase 2B-Beta: Complete Native Backend
- [x] Integrate Cranelift as JIT backend
- [x] Implement basic codegen (constants, arithmetic, locals)
- [ ] Implement function calls and closures
- [ ] Implement phi node handling
- [ ] Add tiered compilation (interpreter → JIT → optimizing JIT)
- [ ] Performance benchmarks vs VM

### Phase 2B-Gamma: AOT & Optimization
- [ ] LLVM backend for AOT compilation
- [ ] Link-time optimization (LTO)
- [ ] Standalone binary generation

### Phase 3: Type Annotations
- [ ] Optional type syntax: `let x: number = 42`
- [ ] Function signatures: `function add(a: number, b: number): number`
- [ ] Array types: `let arr: string[] = ["a", "b"]`
- [ ] Gradual typing with `--strict` mode

### Phase 4: Self-Hosting Migration
- [ ] Emit SSA IR from bootstrap compiler
- [ ] Compile bootstrap compiler natively
- [ ] Full self-hosting: tscl compiles itself to native code

### Other
- [ ] For loops
- [ ] Try/catch
- [ ] ES6 classes
- [ ] Import/export modules
- [ ] Async/await
- [ ] Garbage collection
- [ ] Source maps
- [ ] REPL
