# tscl VM: Development Progress

A custom JavaScript-like scripting language with a stack-based VM in Rust, featuring a self-hosting bootstrap compiler.

## Core Architecture

```
┌─────────────┐    ┌─────────────────┐    ┌────────────────┐
│   Parsing   │───▶│  Borrow Checker │───▶│  Stack-based   │
│  (SWC AST)  │    │   (Middle-end)  │    │   VM (Back-end)│
└─────────────┘    └─────────────────┘    └────────────────┘
                            │
        ┌───────────────────┴───────────────────┐
        │         Bootstrap Compiler            │
        │    ┌─────────────────────────┐       │
        └───▶│  Lexer → Parser → Emitter │◀─────┘
             │     (Written in tscl)     │
             └─────────────────────────┘
```

## Completed Features

### 1. Self-Hosting Bootstrap Compiler
- **Lexer** (`bootstrap/lexer.tscl`) - Tokenizes source into tokens (identifiers, keywords, numbers, strings, operators, delimiters)
- **Parser** (`bootstrap/parser.tscl`) - Recursive descent parser producing AST nodes
- **Emitter** (`bootstrap/emitter.tscl`) - Generates bytecode from AST using ByteStream
- **Two-Stage Loading** - Prelude loads first, then bootstrap modules, then main script
- **Bytecode Rebasing** - Appended bytecode has all addresses automatically adjusted

### 2. Memory Management
- **Ownership Model** - Variables own their data; assigning objects moves ownership
- **Let vs Store Opcodes** - `Let` creates new bindings (proper shadowing), `Store` updates existing
- **Scoped Lifetimes** - Variables automatically freed when scope ends
- **Stack vs Heap** - Primitives on stack (copy), Objects/Arrays on heap (move)
- **Variable Lifting** - Captured variables lifted from stack to heap for closures

### 3. Virtual Machine
- **Stack-based Architecture** - LIFO stack for expressions and operations
- **Call Stack & Frames** - Nested function calls with isolated local scopes
- **Heap Allocation** - Dynamic storage for Objects, Arrays, ByteStreams
- **Native Bridge** - Rust functions injected into JS environment
- **Event Loop** - Task queue with timer support (`setTimeout`)
- **Stack Overflow Protection** - Maximum call depth of 1000

### 4. Closures & Functions
- **Function Declarations** - Named functions with parameters
- **Function Expressions** - Anonymous functions
- **Arrow Functions** - `(x) => x * 2` and `x => x * 2` syntax
- **Closures** - Capture outer scope variables via environment objects
- **Constructors** - `new` expressions with `this` binding

### 5. Language Support
- **Variables** - `let` and `const` declarations
- **Objects** - Literals `{a: 1}`, property access `obj.a`, computed access `obj[key]`
- **Arrays** - Literals `[1, 2]`, indexed access `arr[0]`, methods (push, pop, etc.)
- **Control Flow** - `if`/`else`, `while`, `break`, `continue`
- **Operators** - Arithmetic (`+`, `-`, `*`, `/`, `%`), comparison, logical, unary (`!`, `-`)
- **String Methods** - `slice`, `charCodeAt`, `charAt`, `includes`, `trim`
- **Array Methods** - `push`, `pop`, `shift`, `unshift`, `splice`, `indexOf`, `includes`, `join`

### 6. Standard Library
- **console.log** - Print values to stdout
- **setTimeout** - Schedule delayed execution
- **require** - Module loading (supports "fs")
- **fs.readFileSync** - Read file as string
- **fs.writeFileSync** - Write string to file
- **fs.writeBinaryFile** - Write binary data
- **ByteStream** - Binary data manipulation (create, writeU8, writeU32, writeF64, writeString, writeVarint, patchU32, length, toArray)
- **String.fromCharCode** - Create string from char code

## Bytecode Instruction Set

| OpCode              | Description                                      |
| ------------------- | ------------------------------------------------ |
| `Push(Value)`       | Push constant onto stack                         |
| `Let(Name)`         | Create new variable binding in current scope     |
| `Store(Name)`       | Update existing variable (searches all scopes)   |
| `Load(Name)`        | Push variable's value onto stack                 |
| `LoadThis`          | Push current `this` context                      |
| `NewObject`         | Allocate empty object on heap                    |
| `NewArray(Size)`    | Allocate array of given size                     |
| `SetProp(Key)`      | Set property on heap object                      |
| `GetProp(Key)`      | Get property from heap object                    |
| `StoreElement`      | Store value at array index                       |
| `LoadElement`       | Load value from array index                      |
| `Call(ArgCount)`    | Execute function with N arguments                |
| `CallMethod(N,A)`   | Call method on object                            |
| `Return`            | Return from function                             |
| `Jump(Addr)`        | Unconditional jump                               |
| `JumpIfFalse(Addr)` | Conditional branch                               |
| `MakeClosure(Addr)` | Create closure with captured environment         |
| `Construct(Args)`   | Construct new object instance                    |
| `Drop(Name)`        | Free variable and its heap data                  |
| `Dup`               | Duplicate top of stack                           |
| `Pop`               | Discard top of stack                             |
| `Add/Sub/Mul/Div`   | Arithmetic operations                            |
| `Mod`               | Modulo operation                                 |
| `Eq/EqEq/Ne/NeEq`   | Equality comparisons (strict and loose)          |
| `Lt/LtEq/Gt/GtEq`   | Comparison operations                            |
| `And/Or/Not`        | Logical operations                               |
| `Neg`               | Unary negation                                   |
| `Require`           | Load module                                      |
| `Halt`              | Stop execution                                   |

## Recent Fixes

### Variable Scoping Bug (Let vs Store)
- **Problem**: `let x = ...` in nested functions was updating outer scope's `x` instead of creating new binding
- **Cause**: VM's `Store` opcode searched all frames and updated first match
- **Solution**: Added `OpCode::Let` that always creates binding in current frame; compiler uses `Let` for declarations

### Bytecode Address Rebasing
- **Problem**: When appending bytecode, function addresses pointed to wrong locations
- **Cause**: Each compiled module starts addresses from 0
- **Solution**: `append_program` rebases Jump, JumpIfFalse, MakeClosure, and Function addresses

### Object Property Corruption
- **Problem**: AST node properties corrupted during recursive emit calls
- **Cause**: Reading `node.left` after `emit(node.left)` could return corrupted value
- **Solution**: Save node properties to local variables before any recursive calls

## Next Steps

- [ ] **For loops** - Implement `for` statement parsing and emission
- [ ] **Try/Catch** - Exception handling
- [ ] **Classes** - ES6 class syntax
- [ ] **Modules** - Import/export support
- [ ] **Async/Await** - Promise-based async syntax
- [ ] **Garbage Collection** - Reference counting for shared ownership
- [ ] **Source Maps** - Debug information in bytecode
- [ ] **REPL** - Interactive shell
