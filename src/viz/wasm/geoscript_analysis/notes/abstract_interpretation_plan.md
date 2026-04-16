# Abstract Interpretation / Type Inference for Geoscript Analysis

## Goal

Build a forward type-propagation pass over the geoscript AST that infers types for all bindings, enabling richer diagnostics, hover info, and autocomplete than the current scope-only analysis provides. This system should also subsume and eventually replace the ad-hoc `build_example_val` / `pre_resolve_expr_type` machinery currently used in the optimizer.

## Current State of Type Resolution

### What exists today

**`pre_resolve_expr_type` (ast.rs ~2594)**
A recursive function that attempts to determine the `ArgType` of an expression during the optimization pass. It works within the context of `ScopeTracker`, which tracks values as `TrackedValue::Const(Value)`, `TrackedValue::Arg(ClosureArg)`, or `TrackedValue::Dyn { type_hint }`.

Current coverage:
- Literals → direct type from value
- Identifiers → looks up `ScopeTracker`, extracts type from const value or type hint
- Binary ops → resolves the builtin fn for the operator, looks up return type from the matched signature
- Prefix ops → same approach via `get_unop_return_ty`
- Range expressions → always `Sequence`
- Static/dynamic field access → calls `build_example_val()` to create a fake value, then *actually evaluates* the field access on it to determine the result type
- Function calls → only works for `FunctionCallTarget::Literal` (pre-resolved builtins), NOT for `FunctionCallTarget::Name` (unresolved calls)

**`build_example_val` (lib.rs ~1026)**
A hack (self-documented as such with a TODO) that creates a dummy `Value` for a given `ArgType` so that the existing runtime `get_args` function can be reused for type-level signature matching. Creates real `LinkedMesh`, `MeshHandle`, etc. objects just to satisfy type checks. This is expensive and fragile — it doesn't work for `ArgType::Numeric` (returns `None`) and creates heavyweight objects that are immediately thrown away.

**`ScopeTracker` (ast.rs ~2524)**
Used during the optimizer's const-folding pass. Tracks variables as one of:
- `Const(Value)` — fully evaluated at compile time
- `Arg(ClosureArg)` — closure parameter, has optional `type_hint: Option<TypeName>`
- `Dyn { type_hint: Option<TypeName> }` — dynamic value with optional type annotation

This is close to what an abstract type environment would look like, but it conflates "do we have the value?" with "do we know the type?". A variable can have a known type without having a known value.

**`TypeName` vs `ArgType`**
There's a TODO noting these should be de-duped (ast.rs line 28). `TypeName` is a simpler enum used in type hints. `ArgType` is the authoritative type enum with bitflag support and the full list of types (including `Any`). The abstract interpretation system should use `ArgType` (or a wrapper around it) as the canonical type representation.

**`Callable::get_return_type_hint` (lib.rs ~442)**
Returns the return type for a callable, but:
- Only works for `Builtin` with a `pre_resolved_signature`
- Returns `None` for `PartiallyAppliedFn` (with a TODO)
- Delegates to `Closure.return_type_hint` (which is set during construction, not inferred)

**`maybe_pre_resolve_builtin_call_signature` (ast.rs ~2777)**
Attempts to resolve which overloaded signature a builtin call will use based on argument types. Uses `build_example_val` to create dummy values, then calls the runtime `get_args`. This is the core "which signature matches?" logic that the abstract interpreter would replace.

### Key limitations of the current approach

1. **`FunctionCallTarget::Name` is never resolved** — `pre_resolve_expr_type` returns `None` for calls to named functions. This means any expression like `x = box()` that hasn't been const-folded yet has no type information.  This is a rare case in practice, though, since const-folding will usually resolve and uplift named function calls to `Value::Callable` literals in the AST before they're called later on.

2. **`build_example_val` is a dead end** — creating real values for type checking doesn't scale. It can't represent `Numeric` (float OR int), can't represent unions, and creates heavy objects.

3. **No type propagation through assignments** — the optimizer's `ScopeTracker` propagates *values* (for const folding), not *types*. A variable assigned `box()` that can't be const-folded gets `Dyn { type_hint: None }`, losing the type.

4. **`PartiallyAppliedFn` has no return type** — the TODO at lib.rs:215 notes this. With abstract types, this becomes solvable: you'd track which args are bound and what remains.

5. **Closures' return types aren't inferred** — `return_type_hint` is only set if the user provides a type annotation. The body could be analyzed to infer it.

## Proposed Design

### Core type: `AbstractType`

```rust
/// A type-level representation of a geoscript value, used during abstract interpretation.
enum AbstractType {
    /// A single concrete type (Mesh, Vec3, Float, Int, etc.)
    Concrete(ArgType),

    /// A union of possible types (e.g. Float | Int for Numeric)
    Union(SmallVec<[ArgType; 2]>),

    /// A callable with known parameter types and return type.
    Callable {
        /// None = unknown params (opaque callable)
        params: Option<Vec<AbstractParam>>,
        return_type: Box<AbstractType>,
    },

    /// A partially applied function: some args bound, rest remain.
    PartiallyApplied {
        /// Reference to the underlying function (builtin name or closure)
        base: CallableRef,
        /// Types of already-bound positional args
        bound_positional: Vec<AbstractType>,
        /// Types of already-bound kwargs
        bound_kwargs: Vec<(String, AbstractType)>,
        /// Remaining parameters after removing bound ones
        remaining_params: Vec<AbstractParam>,
        /// Return type once fully applied
        return_type: Box<AbstractType>,
    },

    /// We can't determine the type.
    Unknown,
}

struct AbstractParam {
    name: String,
    accepted_types: SmallVec<[ArgType; 2]>,
    required: bool,
}

enum CallableRef {
    Builtin(usize),    // fn_entry_ix
    UserDefined(Sym),  // reference to a binding
}
```

This could probably live in the `geoscript_analysis` crate, not the core `geoscript` crate. It doesn't need to interact with runtime values at all.  However, if we end up getting rid of `build_example_val` entirely, it might be useful or necessary to use these abstract types in the const eval portion of the `geoscript` crate directly, so it might make sense for them to live in the main `geoscript` crate.

### The inference pass: `TypeWalker`

A forward pass over the AST, structurally similar to the existing `ScopeWalker`, that maintains a type environment `HashMap<Sym, AbstractType>`:

```
TypeEnv = HashMap<Sym, AbstractType>
```

The walker processes statements top-to-bottom:

1. **Simple assignment** `x = expr` → infer type of `expr`, store in env as `x: T`
2. **Destructure assignment** `[a, b] = expr` → if `expr: Sequence`, each binding gets `Unknown` (or could try to narrow if the sequence is a literal array with known element types)
3. **Expression statement** `expr` → infer type (for side-effect validation)
4. **Closures** `|x, y| body` → push a new scope with param types, walk body, infer return type
5. **Blocks** `{ ... }` → push scope, walk, pop; type is the type of the last expression

### Expression type inference

For each `Expr` variant, determine the `AbstractType`:

| Expr variant | Type inference strategy |
|---|---|
| `Literal` | Direct from `Value::get_type()` |
| `Ident` | Look up in `TypeEnv` |
| `Call` (named) | Look up function name in env or builtins, match arg types against signatures, return the matched signature's `return_type` |
| `Call` (literal/resolved) | Same as above but using `fn_entry_ix` directly |
| `BinOp::Pipeline` | Infer LHS type, prepend to RHS call's args, resolve RHS |
| `BinOp` (arithmetic etc.) | Look up the operator's builtin, match LHS/RHS types, get return type |
| `PrefixOp` | Same as BinOp but unary |
| `Range` | `Concrete(Sequence)` |
| `ArrayLiteral` | `Concrete(Sequence)` |
| `MapLiteral` | `Concrete(Map)` |
| `Closure` | `Callable { params, return_type }` where return type is inferred from body |
| `Conditional` | Union of then/else branch types |
| `Block` | Type of the last expression in the block |
| `StaticFieldAccess` | Requires knowing the LHS type and the field name. For known types like Vec3, field `.x` → Float. Could maintain a small table of known field types. |
| `FieldAccess` (dynamic) | Generally `Unknown` unless the field is a literal |

### Signature matching without `build_example_val`

The key improvement over the current approach. Instead of creating dummy values and calling `get_args`, implement a purely type-level signature matcher:

```rust
fn match_signature(
    sig: &FnSignature,
    positional_types: &[AbstractType],
    kwarg_types: &[(Sym, AbstractType)],
) -> Option<SignatureMatch> {
    // For each ArgDef in the signature, check if the provided type
    // is compatible with the accepted types (bitflag check).
    // Handle optional args with defaults.
    // Return the match result including the return type.
}
```

An `AbstractType::Concrete(ArgType::Float)` is compatible with an ArgDef that accepts `argtype_flags!(ArgType::Float)` or `argtype_flags!(ArgType::Numeric)`. An `AbstractType::Union([Float, Int])` would need to check if ALL members of the union are accepted.

This replaces `build_example_val` + `get_args` with a direct type-to-type check and avoids creating any runtime values.

One thing to note is that the logic used to map positional + keyword arguments to actual argument indices that `get_arg` handles isn't trivial (not incredibly complicated either though).  Maybe we could find a way to generalize or de-dupe `get_args` so the same logic can be shared for both abstract + literal execution.  Just something to keep in mind.  Maybe I'm overthinking that though and it's simpler than I'm making it out to be.

### Partial application handling

When a function call provides fewer positional args than required and no signature matches fully:

1. Find signatures where the provided args match the first N params
2. The remaining params become the `remaining_params` of a `PartiallyApplied` type
3. When that value is later called or piped into, resolve the remaining params

The pipeline operator `a | f(b, c)` becomes: infer type of `a`, then treat the call as `f(a, b, c)` — the LHS type is prepended to the positional args before signature matching.

### Integration with existing systems

**Replacing `pre_resolve_expr_type`**: The abstract interpreter's `infer_expr_type` would be a superset. The optimizer could eventually call into the analysis crate's type inference instead of its own ad-hoc version. But this isn't required initially — the analysis crate can have its own implementation that coexists with the optimizer's.

**Replacing `build_example_val`**: The type-level signature matcher eliminates this entirely. No dummy values needed.

**Replacing `ScopeTracker` for type purposes**: The analysis pass would maintain its own `TypeEnv`. The optimizer's `ScopeTracker` would remain for const-folding (it tracks actual values, which is still needed for optimization). Eventually, the optimizer could consume the analysis pass's type info to avoid redundant work.

**Unifying `TypeName` and `ArgType`**: A natural cleanup to do alongside this work. `TypeName` can be converted to `ArgType` at parse time (it's a subset), and the `TypeName` enum can be removed.

## Layered Implementation Plan

### Layer 0: Foundation (do first)

- Define `AbstractType` enum and `TypeEnv` in `geoscript_analysis`
- Implement the type-level signature matcher (replaces `build_example_val` + `get_args` for type checking)
- Add a `type_infer` module with a `TypeWalker` that handles:
  - Literals
  - Simple assignments
  - Identifier lookups
  - Builtin function calls (named, not just pre-resolved literals)
- Wire the resulting `TypeEnv` into hover info (show `(variable) x: Mesh` instead of just `(variable) x`)
- This alone handles the majority of geoscript code since most expressions are assignments of builtin calls.

**Estimated scope**: ~300-500 lines of new Rust code, _potentially_ no changes to geoscript crate needed (earlier caveat applies).

### Layer 1: Pipeline operator + binary ops

- Handle `BinOp::Pipeline`: infer LHS type, prepend to RHS call
- Handle arithmetic/comparison operators via their builtin signatures
- This covers patterns like `box() | translate(vec3(1,0,0)) | scale(2)` — each step in the chain gets a type.

**Estimated scope**: ~100-200 lines, extends the existing `infer_expr_type` match arms.

### Layer 2: Partial application awareness

- Detect when a call provides fewer args than the minimum required
- Instead of `Unknown`, produce `PartiallyApplied { ... }` with the remaining params
- When a `PartiallyApplied` value is called, resolve against the remaining params
- Enables proper arg validation for partially applied functions (the current false-positive suppression could be replaced with accurate checking)

**Estimated scope**: ~200-300 lines. This is the most conceptually tricky layer.

### Layer 3: Closure type inference

- When encountering a closure, push param types into a new scope and walk the body
- Infer the return type from the body's final expression (or explicit `return` statements)
- Store the result as `AbstractType::Callable { params, return_type }`
- User-defined functions then have full type info, enabling call-site validation

**Estimated scope**: ~150-200 lines. Requires handling the scope push/pop correctly.

### Layer 4: Conditional narrowing + blocks

- `if/else` → union of branch types
- Blocks → type of last expression
- This is mostly mechanical — the types flow through naturally.

**Estimated scope**: ~50-100 lines.

### Layer 5: Optimizer integration (optional, later)

- Expose the analysis crate's type inference as a reusable API
- Have the optimizer consume it instead of `pre_resolve_expr_type` + `build_example_val`
- Remove `build_example_val` and the dummy-value approach
- Unify `TypeName` → `ArgType`
- This is a larger refactor with more risk since it touches the hot path.

## What this enables (by layer)

| Capability | Layer needed |
|---|---|
| Typed hover info (`x: Mesh`) | 0 |
| Type mismatch diagnostics at call sites | 0 |
| Full arg validation for direct builtin calls | 0 |
| Pipeline chain type tracking | 1 |
| Binary op type checking | 1 |
| Accurate partial application validation | 2 |
| Remaining-params display for PAFs | 2 |
| User-defined function return types | 3 |
| Call-site validation for user functions | 3 |
| Smart completions based on expected type (e.g. suggest `Mesh`-returning functions when a `Mesh` is expected) | 0+ |

## Key design decisions to make

1. **Where does `AbstractType` live?** Recommendation: `geoscript_analysis` crate. No dependency from geoscript core. The analysis pass imports the AST types and `ArgType` but nothing else from the runtime.

2. **Separate pass or merged with `ScopeWalker`?** Recommendation: initially a separate `TypeWalker` that takes `ScopeAnalysis` as input (for resolved refs). Merging with `ScopeWalker` could be done later if the double-walk is too expensive, but keeping them separate is cleaner and the cost is negligible for editor-sized files.

3. **How to handle `Unknown`?** Any operation on `Unknown` produces `Unknown` (it's absorbing). This prevents cascading false-positive errors. The system should only report errors when it has enough type info to be confident.

4. **Error reporting philosophy**: Only report type errors when ALL of:
   - The function is a known builtin (not shadowed)
   - ALL argument types are known (not `Unknown`)
   - NO signature matches the provided types
   This avoids false positives from incomplete inference.

## Relation to the existing analysis infrastructure

The abstract interpretation pass would slot in as a new stage in the analysis pipeline:

```
Source → Parse (Pest) → ScopeAnalysis (current) → TypeInference (new) → Diagnostics + Hover + Completions
```

The current scope analysis provides: defined symbols, resolved references, unresolved references, function call info. The type inference pass consumes this and adds: type assignments for all symbols, richer call-site validation, return type info.

The existing `hover()`, `completions()`, and `analyze()` methods in `AnalysisCtx` would be updated to use the type environment, enriching their output without changing their external API.
