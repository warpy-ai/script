# Script Self-Hosting Roadmap

This document outlines the path to full self-hosting where the Script compiler (`scriptc`) is written entirely in Script.

---

## Current State

```
┌─────────────────────────────────────────────────────────────┐
│                    Source Code (.tscl)                      │
└───────────────────────────┬─────────────────────────────────┘
                            │
            ┌───────────────┼───────────────┐
            ▼               ▼               ▼
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│ src/compiler/ │  │  bootstrap/   │  │  compiler/    │
│    (Rust)     │  │   (working)   │  │ (incomplete)  │
│               │  │               │  │               │
│ SWC Parser    │  │ Custom Lexer  │  │ Modular       │
│ → Bytecode    │  │ Custom Parser │  │ Structure     │
│ → IR → Native │  │ → Bytecode    │  │ → Bytecode    │
└───────┬───────┘  └───────┬───────┘  └───────┬───────┘
        │                  │                  │
        ▼                  ▼                  ▼
   ┌─────────┐        ┌─────────┐        ┌─────────┐
   │ Native  │        │   VM    │        │   VM    │
   │ Binary  │        │ (Rust)  │        │ (Rust)  │
   └─────────┘        └─────────┘        └─────────┘
```

### What Works Today

| Component | Location | Status |
|-----------|----------|--------|
| Rust Compiler | `src/compiler/` | ✅ Full: parse → bytecode → IR → native |
| Bootstrap Compiler | `bootstrap/*.tscl` | ✅ Working: parse → bytecode |
| Modular Compiler | `compiler/*.tscl` | ⚠️ Incomplete: parse only |
| VM | `src/vm/` | ✅ Full bytecode execution |
| JIT | `src/backend/cranelift.rs` | ✅ Cranelift codegen |
| AOT | `src/backend/llvm/` | ✅ LLVM with LTO |

---

## Phase 1: Foundation (Current)

**Goal**: Consolidate and stabilize the .tscl compiler infrastructure.

### Architecture

```
Source (.tscl) ──► bootstrap/*.tscl ──► Bytecode ──► Rust VM
                        │
                        └──► (reference implementation)

Source (.tscl) ──► src/compiler/ (Rust) ──► Native Binary
                        │
                        └──► (production builds)
```

### Tasks

- [x] Working lexer in `bootstrap/lexer.tscl`
- [x] Working parser in `bootstrap/parser.tscl`
- [x] IR generation in `bootstrap/ir.tscl`, `bootstrap/ir_builder.tscl`
- [x] Bytecode emission in `bootstrap/codegen.tscl`, `bootstrap/emitter.tscl`
- [ ] Port `bootstrap/` features to `compiler/` (modular structure)
- [ ] Ensure `compiler/` can parse the same syntax as `bootstrap/`
- [ ] Add missing expression/statement handling in `compiler/parser/`

### File Structure

```
bootstrap/                    # Working flat compiler (reference)
├── main.tscl                # CLI (273 lines)
├── types.tscl               # Type definitions (357 lines)
├── lexer.tscl               # Tokenization (335 lines)
├── parser.tscl              # AST generation (1,432 lines)
├── ir.tscl                  # IR types (619 lines)
├── ir_builder.tscl          # AST → IR (270 lines)
├── codegen.tscl             # IR → Bytecode (315 lines)
├── emitter.tscl             # Binary serialization (846 lines)
├── pipeline.tscl            # Orchestration (228 lines)
├── stdlib.tscl              # Runtime decls (248 lines)
└── utils.tscl               # Helpers (22 lines)
Total: ~4,945 lines

compiler/                     # Modular compiler (target)
├── main.tscl                # CLI entry (259 lines)
├── lexer/
│   ├── mod.tscl             # Tokenization (338 lines)
│   ├── token.tscl           # Token types (182 lines)
│   └── error.tscl           # Lexer errors (67 lines)
├── parser/
│   ├── mod.tscl             # Parser entry (42 lines)
│   ├── expr.tscl            # Expressions (448 lines)
│   ├── stmt.tscl            # Statements (639 lines)
│   └── error.tscl           # Parse errors (110 lines)
├── ast/
│   ├── mod.tscl             # AST entry (13 lines)
│   └── types.tscl           # AST types (352 lines)
├── ir/
│   ├── mod.tscl             # IR types (381 lines)
│   └── builder.tscl         # IR builder (198 lines)
├── codegen/
│   └── mod.tscl             # Bytecode gen (306 lines)
└── stdlib/
    └── builtins.tscl        # Builtins (159 lines)
Total: ~3,494 lines
```

---

## Phase 2: Feature Parity

**Goal**: Make `compiler/*.tscl` feature-complete with `bootstrap/` and add optimization passes.

### Architecture

```
Source (.tscl) ──► compiler/*.tscl ──► Bytecode ──► Rust VM
                        │
                        ├──► IR Verification
                        ├──► Type Inference  (NEW)
                        └──► Optimizations   (NEW)
```

### Tasks

- [ ] Complete parser in `compiler/parser/` to handle all syntax
- [ ] Add type inference pass (`compiler/passes/typecheck.tscl`)
- [ ] Add optimization passes (`compiler/passes/opt.tscl`)
  - [ ] Dead code elimination
  - [ ] Constant folding
  - [ ] Copy propagation
- [ ] Add borrow checker (`compiler/passes/borrow_ck.tscl`)
- [ ] Improve IR verification
- [ ] Match bytecode output with `bootstrap/`

### New Files Needed

```
compiler/
├── passes/                   # Compiler passes (NEW)
│   ├── mod.tscl             # Pass manager
│   ├── typecheck.tscl       # Type inference (~500 lines)
│   ├── opt.tscl             # Optimizations (~600 lines)
│   └── borrow_ck.tscl       # Ownership check (~400 lines)
└── ...
```

### Validation

```bash
# Both should produce identical bytecode
./target/release/script bootstrap/main.tscl test.tscl -o test1.tscb
./target/release/script compiler/main.tscl test.tscl -o test2.tscb
diff test1.tscb test2.tscb  # Should match
```

---

## Phase 3: Native Code Generation

**Goal**: Add native code generation to `compiler/*.tscl`, making it a full `scriptc`.

### Architecture

```
Source (.tscl) ──► compiler/*.tscl (scriptc) ──┬──► Bytecode ──► VM
                                               │
                                               ├──► x86-64 asm ──► Native
                                               │
                                               └──► ARM64 asm ──► Native
```

### Approach Options

#### Option A: Direct Assembly Generation
Write x86-64/ARM64 assembly directly from IR.

**Pros**: No dependencies, full control
**Cons**: Complex, two architectures to maintain

```
compiler/
├── backend/
│   ├── mod.tscl             # Backend interface
│   ├── x86_64.tscl          # x86-64 codegen (~2,000 lines)
│   ├── arm64.tscl           # ARM64 codegen (~2,000 lines)
│   └── elf.tscl             # ELF object writer (~500 lines)
```

#### Option B: C Backend
Generate C code, use system compiler.

**Pros**: Portable, leverages existing optimizers
**Cons**: Depends on C compiler, slower builds

```
compiler/
├── backend/
│   └── c.tscl               # C codegen (~1,000 lines)
```

#### Option C: LLVM IR Text
Generate LLVM IR text, use `llc` to compile.

**Pros**: Leverages LLVM optimizations
**Cons**: Depends on LLVM toolchain

```
compiler/
├── backend/
│   └── llvm_ir.tscl         # LLVM IR text gen (~1,500 lines)
```

### Recommended: Option A (Direct Assembly)

For true self-hosting without external dependencies:

```
compiler/
├── backend/
│   ├── mod.tscl             # Backend trait/interface
│   ├── regalloc.tscl        # Register allocation
│   ├── x86_64/
│   │   ├── mod.tscl         # x86-64 entry
│   │   ├── codegen.tscl     # Instruction selection
│   │   ├── asm.tscl         # Assembly emission
│   │   └── abi.tscl         # Calling convention
│   ├── arm64/
│   │   ├── mod.tscl         # ARM64 entry
│   │   ├── codegen.tscl     # Instruction selection
│   │   ├── asm.tscl         # Assembly emission
│   │   └── abi.tscl         # Calling convention
│   └── object/
│       ├── elf.tscl         # ELF writer (Linux)
│       └── macho.tscl       # Mach-O writer (macOS)
```

### Tasks

- [ ] Design backend interface (`compiler/backend/mod.tscl`)
- [ ] Implement register allocator
- [ ] Implement x86-64 instruction selection
- [ ] Implement x86-64 assembly emission
- [ ] Implement ELF/Mach-O object file writer
- [ ] Link runtime stubs
- [ ] Self-compile test: `scriptc` compiles itself

---

## Phase 4: Bootstrap Verification

**Goal**: Prove deterministic self-hosting through the bootstrap chain.

### Bootstrap Chain

```
┌─────────────────────────────────────────────────────────────┐
│  tscl₀ = src/compiler/ (Rust)                               │
│         Original compiler written in Rust                   │
└───────────────────────────┬─────────────────────────────────┘
                            │ compiles
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  tscl₁ = compiler/*.tscl compiled by tscl₀                  │
│         First native scriptc binary                         │
└───────────────────────────┬─────────────────────────────────┘
                            │ compiles
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  tscl₂ = compiler/*.tscl compiled by tscl₁                  │
│         Self-compiled scriptc binary                        │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  VERIFY: hash(tscl₁) == hash(tscl₂)                         │
│          Proves deterministic compilation                   │
└─────────────────────────────────────────────────────────────┘
```

### Verification Script

```bash
#!/bin/bash
# bootstrap_verify.sh

# Stage 1: Build scriptc with Rust compiler
./target/release/script build compiler/main.tscl -o scriptc1 --dist

# Stage 2: Build scriptc with scriptc1
./scriptc1 build compiler/main.tscl -o scriptc2 --dist

# Stage 3: Build scriptc with scriptc2
./scriptc2 build compiler/main.tscl -o scriptc3 --dist

# Verify: scriptc2 == scriptc3 (bit-for-bit)
if cmp -s scriptc2 scriptc3; then
    echo "✅ Bootstrap verification PASSED"
    echo "   scriptc2 and scriptc3 are identical"
    sha256sum scriptc2 scriptc3
else
    echo "❌ Bootstrap verification FAILED"
    echo "   scriptc2 and scriptc3 differ"
    exit 1
fi
```

### Tasks

- [ ] Ensure deterministic IR serialization
- [ ] Ensure deterministic bytecode generation
- [ ] Ensure deterministic native code generation
- [ ] Create bootstrap verification script
- [ ] CI integration for bootstrap verification

---

## Timeline Estimate

| Phase | Description | Estimated Effort |
|-------|-------------|------------------|
| Phase 1 | Foundation (current) | Done |
| Phase 2 | Feature parity | ~2,000 lines |
| Phase 3 | Native codegen | ~5,000-8,000 lines |
| Phase 4 | Bootstrap verify | ~500 lines + testing |

**Total new code**: ~7,500-10,500 lines of .tscl

---

## Success Criteria

### Phase 1 Complete When:
- [x] `bootstrap/` compiles .tscl to bytecode
- [ ] `compiler/` can parse same syntax as `bootstrap/`

### Phase 2 Complete When:
- [ ] `compiler/` produces identical bytecode to `bootstrap/`
- [ ] Type inference implemented
- [ ] Basic optimizations working

### Phase 3 Complete When:
- [ ] `compiler/` can produce native binaries
- [ ] Binaries run without Rust runtime
- [ ] `scriptc` can compile simple programs

### Phase 4 Complete When:
- [ ] `scriptc` can compile itself
- [ ] `hash(tscl₁) == hash(tscl₂)` verified
- [ ] Rust `src/compiler/` can be deprecated

---

## Migration Strategy

Once Phase 4 is complete:

1. **Keep Rust compiler** as reference/testing tool
2. **Primary compiler** becomes `scriptc` (compiler/*.tscl)
3. **Remove `bootstrap/`** (superseded by `compiler/`)
4. **Optional**: Remove `src/compiler/` entirely

### Final Structure

```
script/
├── compiler/                 # THE compiler (scriptc)
│   ├── main.tscl
│   ├── lexer/
│   ├── parser/
│   ├── ast/
│   ├── ir/
│   ├── passes/
│   ├── backend/
│   └── stdlib/
├── src/
│   ├── runtime/             # Keep: ABI, heap, stubs
│   │   ├── abi.rs
│   │   ├── heap.rs
│   │   └── stubs.rs
│   └── vm/                  # Keep: for debugging/testing
└── tests/
```

The Rust code becomes minimal runtime support, with the compiler fully in Script.
