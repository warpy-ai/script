---
sidebar_position: 1
title: Welcome to Script
description: Script is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler. Get started with Script today.
keywords:
  [
    script language,
    programming language,
    javascript alternative,
    native code,
    high performance,
  ]
---

# Welcome to Script

**Script** is a high-performance JavaScript-like scripting language with native code execution, featuring a self-hosting compiler and Rust-inspired memory safety.

```javascript
function fib(n) {
  if (n < 2) return n;
  return fib(n - 1) + fib(n - 2);
}

console.log(fib(35)); // Compiled to native code!
```

## Key Features

- **Native Execution** — SSA-based IR compiled to native code via Cranelift/LLVM
- **Link-Time Optimization** — ThinLTO and Full LTO for maximum performance
- **Standalone Binaries** — Self-contained executables with runtime stubs in LLVM IR
- **Memory Safety** — Ownership model with compile-time borrow checking
- **Self-Hosting Complete** — Compiler written in Script, generates LLVM IR → native binaries
- **Type Inference** — Hindley-Milner type analysis with generics
- **TypeScript Syntax** — Familiar syntax with full ES6+ and TypeScript features
- **Classes & Inheritance** — ES6 classes with extends, super(), and private fields
- **Error Handling** — try/catch/finally with exception propagation
- **Async/Await** — Native async functions with Promise support

## Quick Start

```bash
# Build
cargo build --release

# Run a script
./target/release/script myprogram.tscl

# Build to native binary (requires LLVM)
./target/release/script build myprogram.tscl --release -o myprogram

# Run the compiled binary
./myprogram
```

## What's Next?

- Read the [Getting Started](/docs/getting-started) guide
- Explore [Language Features](/docs/language-features)
- Learn about the [Architecture](/docs/architecture)
- Check the [Development Status](/docs/development-status)
