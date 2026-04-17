# Abstract Interpretation / Type Inference for Geoscript Analysis

## Goal

Build a forward type-propagation pass over the geoscript AST that infers types for all bindings, enabling richer diagnostics, hover info, and autocomplete than the current scope-only analysis provides. This system should also subsume and eventually replace the ad-hoc `build_example_val` / `pre_resolve_expr_type` machinery currently used in the optimizer.

## Completed Work (Layer 0)

### Type-level signature matching (geoscript crate)

Replaced most uses of `build_example_val` in the optimizer with three new public functions in `lib.rs` that perform purely type-level signature matching:

- **`match_signature_by_arg_types(sigs, positional_types, kwarg_types)`** — general-purpose: matches arg types against signatures, returns a `SignatureTypeMatch` with sig index, arg_refs, and return types. Mirrors the `get_args` logic but operates on `ArgType` instead of `Value`.
- **`match_binop_by_arg_types(fn_entry_ix, lhs_ty, rhs_ty)`** — binary operator matching.
- **`match_unop_by_arg_types(fn_entry_ix, arg_ty)`** — unary operator matching.

Rewrote callers in `ast.rs`:
- `pre_resolve_binop_def_ix` — now uses `match_binop_by_arg_types`
- `pre_resolve_expr_type` PrefixOp arm — now uses `match_unop_by_arg_types`
- `maybe_pre_resolve_builtin_call_signature` — now uses `match_signature_by_arg_types`

**`build_example_val` is still used** only for `StaticFieldAccess` and `FieldAccess` in `pre_resolve_expr_type`, which evaluate actual field access on dummy values. Replacing these requires a type-level field access table (see Layer 1 followup).

Also made `Value::get_type()` public.

### TypeWalker (geoscript_analysis crate)

New module `type_infer.rs` containing:

- **`AbstractType`** enum: `Concrete(ArgType)`, `Union(Vec<ArgType>)`, `Unknown`
- **`TypeEnv`**: scoped type environment with push/pop, maps `Sym → AbstractType` and records `SourceLoc → AbstractType` for hover lookups
- **`TypeWalker`**: forward pass over the AST, handles:
  - Literals → `Concrete(value.get_type())`
  - Identifier lookups → env lookup
  - Simple assignments (with type hint support)
  - Destructure assignments → Unknown per binding
  - Builtin function calls by name → signature matching → return type
  - Array/map/range literals → Sequence/Map/Sequence
  - Closures → walks body (params get types from hints or Unknown), closure itself typed as `Callable`
  - Blocks → walks body with scope push/pop
  - Everything else (BinOp, PrefixOp, FieldAccess, Conditional) → Unknown

### Hover integration

`hover()` in `lib.rs` now runs `type_infer::infer_program()` and appends inferred types to hover output:
- `(variable) m: mesh` instead of `(variable) m`
- `(parameter) x: vec3` for typed closure params
- References resolve through `SymbolRef.resolved_def` to find the definition's type

`analyze()` also runs inference for validation (no new diagnostics produced yet).

### What Layer 0 enables

| Capability | Status |
|---|---|
| Typed hover for variables assigned from builtins (`m = box()` → `m: mesh`) | Working |
| Typed hover for literals (`x = 42` → `x: int`) | Working |
| Type propagation through assignment chains (`m = box(); n = m` → `n: mesh`) | Working |
| Type hints on assignments used as authoritative type | Working |
| Typed hover for closure params with type annotations | Working |

---

## Completed Work (Phase A0/A — Backend restructure + walker merge)

The analysis crate was reorganized into focused modules (`hover.rs`, `completions.rs`, `goto.rs`, `diagnostics/{mod,undefined,call_args}.rs`, `format.rs`, `ty.rs`) and the previously-separate `ScopeWalker` and `TypeWalker` were merged into a single `analysis::AnalysisWalker` producing one `Analysis` struct in a single AST traversal.  `scope.rs` shrank to just the data types.

## Completed Work (Layer 1)

- `walk_binop`: pipeline (`lhs | f(args)` → `f(args, lhs)`, matching PartiallyAppliedFn semantics), arithmetic/comparison/logical via `match_binop_by_arg_types`, range/range-inclusive/map → `Sequence`.
- `walk_expr` PrefixOp arm: `match_unop_by_arg_types`.
- Field access:
  - StaticFieldAccess: type-level swizzle table for Vec2/Vec3 (1 char → Float, 2 → Vec2, 3 → Vec3).
  - FieldAccess (dynamic): integer index into Vec2/Vec3 → Float; string-literal index forwards to static.
- `BinOp::get_builtin_fn_name` and `PrefixOp::get_builtin_fn_name` made `pub` in the geoscript crate so the analysis walker can resolve op names without duplicating the table.

## Completed Work (Layer 2 — Partial application awareness)

`AbstractType` now carries a `PartiallyApplied(PartialApplication)` variant so partial
applications of builtins are first-class types instead of `Unknown`:

- `PartialApplication { name: String, bound_args: Vec<ArgType>, bound_kwargs: Vec<(Sym, ArgType)> }`
  in `ty.rs`.  `name` is the canonical builtin name (alias resolved at PAF-creation time).
- `infer_builtin_return` now returns `PartiallyApplied(...)` for any call whose args are a valid
  prefix of some signature, instead of `Unknown` + a suppressed diagnostic.
- New `resolve_paf_call` helper combines bound + new args (positional and kwarg) and re-matches
  against the underlying signatures.  Returns either the resolved return type, another `PAF`
  if still incomplete, or `Unknown` + a "no overload matches" diagnostic when the combined args
  don't fit even as a prefix.
- `walk_function_call`: when the call target is a shadowing local whose type is a PAF, dispatch
  to `resolve_paf_call` instead of returning `Unknown`.
- `walk_pipeline`: same dispatch in both `lhs | f(args)` (Call form) and `lhs | f` (Ident form).
- `format::format_partial_application` renders rich hover content: bound positional/kwarg args,
  plus per-overload remaining params with their descriptions and the call's return type.
- `hover.rs` now uses the rich PAF formatter when a variable definition or reference resolves
  to a `PartiallyApplied` type.

**New capabilities:**

| Capability | Example |
|---|---|
| PAF type tracking through assignment | `f = translate(vec3(1,0,0))` → `f: partial application of translate` |
| Calling a PAF resolves to its return type | `f = translate(v); m = f(box())` → `m: mesh` |
| Piping into a PAF resolves to its return type | `f = translate(v); m = box() \| f` → `m: mesh` |
| Wrong type piped into a PAF emits a diagnostic | `1 \| translate(vec3(1,0,0))` → "no overload of `translate` matches argument types (vec3, int)" |
| Hover on a PAF var shows bound + remaining params | `(variable) f` + bound `vec3` + remaining `mesh: mesh\|light` + returns `mesh` |
| Chained PAF pipelines fully resolve | `box() \| shift \| scale(2)` where shift is a PAF → `mesh` |

## Completed Work (Layer 3 — Closure type inference + Layer 4 basics)

`AbstractType` gained a `Callable(CallableType)` variant carrying per-param types and a return
type, so user-defined closures are now first-class in the type system:

- `CallableType { params: Vec<CallableParam>, return_type: Box<AbstractType> }` in `ty.rs`.
  `CallableParam { name: Option<String>, ty: AbstractType }` captures per-param info.
- `as_single_arg_type` on `Callable` reports `ArgType::Callable`, so closures satisfy callable
  arg slots in builtin signatures without breaking existing dispatch.
- `AbstractType::display_str` for `Callable` renders `fn(x: int, y: vec3) → int`-style
  signatures, wired directly into the existing variable-hover path.
- New `merge_types` union helper (in `ty.rs`) combines types from branch / exit points, with
  Unknown as absorbing and dedup on unions.

Walker changes in `analysis.rs`:

- `ClosureReturnContext` stack tracks the in-progress closure's declared return type and
  accumulated exit types.  `Statement::Return` walks the value, records it in the innermost
  context, and validates against the declared type (when one exists), emitting a
  "return type mismatch" diagnostic at the returned expression's loc.
- `Expr::Closure` arm pushes a return context, walks the body while tracking the implicit
  tail-expression return type (last statement if it's `Statement::Expr`, otherwise Nil and
  flagged as unreachable when the tail is an explicit `return`).  Validates the implicit
  return against any declared hint.  Produces a `Callable(CallableType { ... })` instead of
  `Concrete(Callable)`.
- `Expr::Block` arm now returns the type of its last `Statement::Expr` (else `Nil`).
- `Expr::Conditional` arm returns the union of then / else-if / else branch types (+ `Nil`
  when no `else` is present).
- `walk_function_call` and both `walk_pipeline` branches now also dispatch through typed
  callable user vars: a call or pipe into a local whose type is `Callable(ct)` returns
  `ct.return_type` instead of `Unknown`.

**New capabilities:**

| Capability | Example |
|---|---|
| Closure return type inferred from body | `f = \|x: int\| x + 1` → `f: fn(x: int) → int` |
| Closure return type annotation honored | `f = \|x\|: int { x + 1 }` → return type is `int` |
| Return-type mismatch diagnostics | `\|x\|: int { return "s" }` → "return type mismatch: expected `int` but value has type `str`" |
| Implicit trailing expression validated | `\|x\|: int { "hello" }` → mismatch on the tail expression |
| Call sites of typed closures resolve | `m = f(3)` where `f: fn(int) → int` → `m: int` |
| Pipeline through typed closures resolves | `m = 3 \| f` → `m: int` |
| Block result types | `x = { a = 10; a + 1 }` → `x: int` |
| Conditional result types | `x = if c { 1 } else { "s" }` → `x: int \| str` |
| Hover on closure var shows `fn(...) → ret` | `(variable) f: fn(x: int) → int` |

Stray top-level `return` statements no longer crash or misfire diagnostics (the walker's
closure-return stack is empty outside closures, so Return becomes a no-op for typing).

## Completed Work (Focused-overload hover)

Function-call hover now renders just the matched signature (with detailed per-argument docs)
when type inference uniquely identifies the active overload, instead of showing every overload:

- `FunctionCallInfo` carries `matched_sig_ix: Option<usize>`.
- `infer_builtin_return` returns `(AbstractType, Option<usize>)`; both call sites in the walker
  thread the sig index back onto the pushed `FunctionCallInfo`.
- `format::format_builtin_hover_with_sig` renders one signature with arg name/type/default and
  per-arg description (`- **width**: \`num\` — Width along the X axis`).
- `hover.rs` processes `function_calls` before `refs` so call-site hovers (which know the matched
  sig) win over the generic builtin reference path; non-call references still get the
  all-overloads view.
- Falls back to the all-overloads view when no signature matches (e.g. partial application or
  Unknown arg types).

## Completed Work (Phase C — diagnostics)

`Analysis` now carries a `diagnostics: Vec<AnalysisDiagnostic>` populated during the walk:

- **Type-hint mismatch**: `x: mesh = "string"` emits an error at the RHS expression.  Uses bitflag-based compatibility so `x: num = 1` is accepted (Numeric flag matches Int/Float).
- **No matching overload at builtin call sites**: `box("string")` emits an error.  Suppressed when:
  - any arg type is Unknown (avoid false positives),
  - the call is a *valid prefix* of some signature (partial-application path), or
  - the function uses dynamic signatures (first arg has empty name).

The diagnostics module merges these with the existing arity / kwarg-name / undefined-variable checks.

## Remaining Work

### Layer 1: Pipeline operator + binary/unary ops + field access (DONE — see Completed Work above)

**What it does:**
- `BinOp::Pipeline`: infer LHS type, prepend to RHS call's args, resolve RHS via signature matching. This is how most geoscript code chains operations: `box() | translate(v) | scale(2)`.
- Arithmetic/comparison/logical operators: use `match_binop_by_arg_types` / `match_unop_by_arg_types` (already exist in the geoscript crate) to resolve return types.
- `BinOp::Map`: `seq | map(fn)` → `Sequence`
- `BinOp::Range` / `BinOp::RangeInclusive` → `Sequence`
- Static field access: build a type-level field lookup table (e.g. `Vec3.x → Float`, `Vec3.xy → Vec2`). This also allows removing the last `build_example_val` uses.

**What it unlocks:**

| Capability | Notes |
|---|---|
| Full pipeline chain type tracking | `box() \| translate(v) \| scale(2)` — every step shows `mesh` |
| Binary op result types | `1 + 2.0` → `float`, `vec3(1,0,0) + vec3(0,1,0)` → `vec3` |
| Unary op result types | `-x` where `x: int` → `int` |
| Field access types | `v.x` where `v: vec3` → `float` |
| Complete removal of `build_example_val` | The last two callers (StaticFieldAccess, FieldAccess) can be replaced |

**Estimated scope**: ~150-250 lines. The pipeline handling is the most interesting part — the binop/unop matching functions already exist.

### Layer 2: Partial application awareness (DONE — see Completed Work above)

### Layer 3: Closure type inference (DONE — see Completed Work above)

### Layer 4: Conditional narrowing + block types (basic cases DONE — see Completed Work above)

Layer 4 is implemented at the type level: blocks return their last expression's type, and
conditionals return the union of their branch types (+ `Nil` when no `else` is present).

Still open for future work (no concrete use case yet):
- Flow-sensitive narrowing inside `if cond { ... }` based on `cond` (e.g. `x != nil` branches).
- Arg-type narrowing into closure bodies used by higher-order builtins (`items | map(|x| ...)`
  — `x` should pick up the sequence's element type once sequences carry element-type info).

### Layer 5: Optimizer unification + cleanup

#### Phase A — Low-risk cleanups (DONE)

- **`TypeName` → `ArgType` unification.**  Removed the separate `TypeName` enum entirely; AST
  nodes (`type_hint`, `return_type_hint`) now carry `Option<ArgType>` directly.  Variants
  that were named differently (`Num`→`Numeric`, `Seq`→`Sequence`) were renamed in place and
  the `TypeName → ArgType` `Into` impls deleted.  All the `.into()` / `map(Into::into)` call
  sites that existed only to bridge the two enums are now gone.
- **Type-level field access helpers replace `build_example_val`.**  New `pub` helpers in
  `geoscript::ast`:
  - `infer_static_field_access_ty(lhs_ty: ArgType, field: &str) -> Option<ArgType>`
  - `infer_dynamic_field_access_ty(lhs_ty, field_expr, field_ty) -> Option<ArgType>`
  The three callers in `pre_resolve_expr_type` (StaticFieldAccess + two FieldAccess paths)
  now use the type-level helpers; `build_example_val` is deleted.  The analysis crate's
  `infer_static_field_access` / `infer_dynamic_field_access` are now thin `AbstractType`
  wrappers that delegate to the geoscript helpers, eliminating the duplication.

Phase A means the `build_example_val` TODO is gone, `TypeName` is gone, and the field-access
type tables are a single source of truth shared between the optimizer and the analysis walker.

#### Phase B — Full optimizer unification (DEFERRED)

**What it does:**
- Have the optimizer consume the analysis crate's type inference (the `AnalysisWalker`
  / `AbstractType` pipeline) instead of maintaining its own `pre_resolve_expr_type`.
- Remove `pre_resolve_expr_type` entirely; the optimizer's `ScopeTracker` would still track
  values for const-folding, but type info would come from the shared inference pass.

**Why it's deferred:** this touches the optimizer's hot path and needs a separate design
pass to decide whether the analysis crate's pipeline (which currently owns `Expr`-level
walking, diagnostics emission, and hover/goto state) should be split so the optimizer can
pull just the type-inference piece.  Benefits are modest now that Phase A has removed the
shared-machinery pain points — no more `TypeName` drift, no more `build_example_val`.

**What it would unlock:**

| Capability | Notes |
|---|---|
| Single source of truth for types | No more divergence between optimizer and analysis type inference |
| Richer optimizer type info | Optimizer could benefit from Callable / PAF / Union types it currently can't represent |

---

## Capabilities by layer (summary)

| Capability | Layer |
|---|---|
| Typed hover for builtins + literals + assignment chains | 0 (done) |
| Typed hover for closure params with annotations | 0 (done) |
| `build_example_val` eliminated for signature matching | 0 (done) |
| Pipeline chain type tracking | 1 |
| Binary/unary op result types | 1 |
| Field access types (`v.x` → float) | 1 |
| Complete `build_example_val` removal | 1 |
| Accurate partial application validation | 2 |
| Remaining-params display for PAFs | 2 |
| Pipeline type checking (wrong type piped in) | 2 |
| User-defined function return type inference | 3 (done) |
| Call-site validation for user functions | 3 (done — return-type side; arg-type validation still future) |
| Conditional/block result types | 4 (done — basic cases) |
| `TypeName` → `ArgType` cleanup | 5A (done) |
| Complete `build_example_val` removal | 5A (done) |
| Single type system (optimizer + analysis unified) | 5B (deferred) |
| Smart completions based on expected type | 1+ (incremental) |
| Type mismatch diagnostics at call sites | 1+ (incremental, as coverage grows) |

## Architecture (current state)

```
geoscript crate
├── lib.rs
│   ├── ArgType                        ← single canonical type enum (TypeName removed)
│   ├── match_signature_by_arg_types() ← shared type-level matcher
│   ├── match_binop_by_arg_types()     ← shared
│   └── match_unop_by_arg_types()      ← shared
└── ast.rs
    ├── infer_static_field_access_ty()  ← pub, type-level field access (shared)
    ├── infer_dynamic_field_access_ty() ← pub, ditto
    ├── pre_resolve_expr_type()         ← uses shared matchers + type-level field helpers
    └── maybe_pre_resolve_builtin_call_signature()  ← uses shared matchers

geoscript_analysis crate
├── analysis.rs
│   ├── AnalysisWalker (single pass: scope + type inference + diagnostics)
│   ├── infer_static_field_access() ← delegates to geoscript helper
│   └── infer_dynamic_field_access() ← delegates to geoscript helper
├── ty.rs
│   ├── AbstractType { Concrete, Union, PartiallyApplied, Callable, Unknown }
│   ├── CallableType / CallableParam
│   └── merge_types()
├── hover.rs / completions.rs / goto.rs / format.rs / diagnostics/
└── scope.rs       ← pure data types for scope symbols
```

## Key design decisions (resolved)

1. **Where does `AbstractType` live?** → `geoscript_analysis` crate. The shared type-level matchers live in the `geoscript` crate and operate on `ArgType` directly. `AbstractType` wraps these for the analysis crate's richer needs (Union, Unknown).

2. **Separate pass or merged with ScopeWalker?** → Separate `TypeWalker`. Runs after `ScopeAnalysis::build()`. Cost is negligible for editor-sized files.

3. **How to handle `Unknown`?** → Absorbing: any operation involving Unknown produces Unknown. Signature matching treats Unknown args as "can't match" (returns Unknown rather than a false match).

4. **Error reporting philosophy** → Only report type errors when the function is a known builtin, all arg types are known (not Unknown), and no signature matches. Not yet implemented as diagnostics — waiting for more coverage.

5. **Optimizer integration** → Shared type-level matching functions live in geoscript crate, used by both the optimizer's `pre_resolve_expr_type` and the analysis crate's `TypeWalker`. Full unification deferred to Layer 5.
