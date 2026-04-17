# Layer 5 Phase B — Optimizer / Analysis Type System Unification

## Scope

This document plans the remaining work from Layer 5 of the abstract-interpretation
plan: making the geoscript optimizer consume the analysis crate's richer type
inference instead of its own `pre_resolve_expr_type` machinery.

Layer 5 Phase A (completed) already did the "easy wins" — removed the `TypeName`
enum, unified on `ArgType`, removed `build_example_val`, and shared field-access
type tables.  What remains is the structural refactor to have a single
inference pipeline.

This plan is intentionally detailed enough for an implementor to begin directly
without a second research pass.

---

## 1. Honest assessment — is Phase B worth doing?

Before the mechanics, the question worth asking up front:

After Phase A, **`pre_resolve_expr_type` and the analysis crate's type walker share
their three biggest pieces**:

- `match_signature_by_arg_types`, `match_binop_by_arg_types`, `match_unop_by_arg_types`
  (Layer 0 result — one implementation, used by both).
- `infer_static_field_access_ty` / `infer_dynamic_field_access_ty`
  (Phase A result — lives in `geoscript::ast`, wrapped by the analysis crate).
- `ArgType` itself (Phase A — one enum).

What the two pipelines *still* do independently:

| Concern | Optimizer (`pre_resolve_expr_type`) | Analysis (`AnalysisWalker`) |
|---|---|---|
| Literals → type | ✓ | ✓ |
| Ident lookup in scope | via `TrackedValue::type_hint` | via `scope_stack` of `AbstractType` |
| BinOp / PrefixOp return type | delegates to shared matchers | delegates to shared matchers |
| Field access | delegates to shared helpers | delegates to shared helpers |
| Builtin call return type | delegates to shared matchers | delegates to shared matchers |
| User-defined closures | `Some(ArgType::Callable)` (opaque) | `Callable(CallableType)` with per-param + return types |
| Partial applications | no concept | `PartiallyApplied(PartialApplication)` |
| Conditionals / Blocks | `None` (unknown) | `Union` of branches |
| Returns Union of types | no — flat `Option<ArgType>` | yes — `AbstractType::Union(...)` |

So after Phase A, the optimizer already gets the "boring" types right for free.
The *only* additional precision Phase B can deliver is: better tracking of
user-defined callables, partial applications, conditional/block result unions,
and closure return types.

**Does the optimizer actually need any of that?**  Looking at every optimizer
decision that consumes types (details in §3):

- Operator overload selection (`pre_resolve_binop_def_ix` → `match_binop_by_arg_types`):
  takes flat `ArgType` on both sides. **Unions wouldn't help** — the optimizer
  would still have to pick one overload or bail out.
- Float-associativity grouping (`expr_requires_float_assoc` at `optimizer.rs:720`):
  one-bit check, "is this Int or wider". Flat `ArgType` is sufficient.
- Recording `type_hint` on `TrackedValue::Arg/Dyn`: only consumed by
  `pre_resolve_expr_type` lookups. Flat `ArgType` is sufficient.
- Builtin call sig caching (`pre_resolved_signature` on AST nodes): consumes the
  matched sig index. Flat `ArgType` is sufficient.

**So the direct user-visible wins of Phase B are modest.** The *indirect* wins
are:

1. One inference walker to maintain and test, not two.
2. Optimizer could in principle recognize typed closure bodies and fold calls
   across them — but closure inlining already handles this through a different
   mechanism.
3. Closure-return-type inference *could* unlock new optimizer decisions if we
   chose to make PAF / user-callable dispatch more aggressive at optimize time
   (speculative).

**Recommendation from this audit:** Phase B is a maintainability / architectural
cleanup, not a capability unlock.  It is worth doing to retire
`pre_resolve_expr_type` and reduce the surface area, *provided* it can be done
without a significant Wasm-size cost to the `geoscript_repl` blob (currently
2.6 MB).  If the size cost exceeds ~100 KB it probably isn't worth it.

The rest of this document plans the refactor; §9 covers alternatives if we
decide Phase B isn't worth the cost.

---

## 2. Goals / Non-Goals

### Goals

- Retire `pre_resolve_expr_type` in `geoscript::ast`.
- Retire the duplicated `type_hint: Option<ArgType>` fields on
  `TrackedValue::Arg` / `TrackedValue::Dyn` (optimizer scope) in favor of a
  shared type environment.
- Have one implementation of "infer the type of an expression in context"
  used by both crates.
- Preserve current `geoscript_repl` Wasm blob size within a tight budget
  (target: ≤ +100 KB after LTO; hard fail: > +250 KB).
- Keep `geoscript_analysis` focused on IDE features (hover, goto, completions,
  diagnostics) layered on top of shared inference.

### Non-Goals

- Do not extend `AbstractType` with new variants (elements, refinement types,
  etc.). Scope is *consolidation*, not *expansion*.
- Do not refactor the optimizer's value-tracking (`TrackedValue::Const` for
  const folding). That is orthogonal and well-scoped already.
- Do not change `geoscript_analysis`'s public API surface
  (`AnalysisCtx::{analyze,hover,completions,goto_definition}`).
- Do not add new runtime dependencies.

---

## 3. Map of current type-info use in the optimizer

This is the inventory of sites we need to replace.  File paths are
`src/viz/wasm/geoscript/src/…`.

### 3.1 Optimizer consumers of `pre_resolve_expr_type`

1. **`optimizer.rs:720` — `expr_requires_float_assoc`**
   Used in `fold_associative_literal_chain` to decide whether an expression is
   strictly-integer or float-compatible. Drives whether associative reordering
   is legal.
   *After refactor:* `type_map.get(&expr_loc)` with a helper that classifies
   `AbstractType` → `"int"` / `"float-ish"` / `"other"`.

2. **`optimizer.rs:1798, 1907` — `optimize_statement` (Assignment/Export)**
   Computes `pre_resolved_ty` for the assignment RHS, then stores it on the
   `TrackedValue::Arg` / `TrackedValue::Dyn` inserted into the local scope.
   *After refactor:* lookup from `type_map`; don't store `type_hint` on
   tracked values at all (inference already knows it).

### 3.2 `pre_resolve_expr_type` internal recursion (self-calls)

These go away when the function itself is deleted:

- `ast.rs:1229, 1262, 1305, 1332` (closure param scope setup — inline_const_captures)
- `ast.rs:2516-2517` (inside `pre_resolve_binop_def_ix`)
- `ast.rs:2591, 2628, 2643, 2651-2652` (self-recursion in the match arms)
- `ast.rs:2728, 2736` (inside `maybe_pre_resolve_builtin_call_signature`)

### 3.3 `pre_resolve_binop_def_ix` (`ast.rs`) and `maybe_pre_resolve_builtin_call_signature` (`ast.rs`)

Two helpers that walk args, infer types, and match a signature. They return a
cached `(def_ix, _)` or `SignatureTypeMatch` that gets stored on AST nodes
(`BinOp.pre_resolved_def_ix`, `FunctionCall.pre_resolved_signature`).
*After refactor:* these stay, but take a `TypeMap` parameter (or consult a
passed-in type oracle) instead of recursing into `pre_resolve_expr_type`.

### 3.4 `ScopeTracker.type_hint` fields

`TrackedValue::Dyn { type_hint: Option<ArgType> }` and
`ClosureArg.type_hint: Option<ArgType>` (in `TrackedValue::Arg`).

Set in ~6 places during optimization; read only by `pre_resolve_expr_type`
(the `Ident` arm).  Once we replace the reader, these fields become dead and
can be deleted, shrinking `ScopeTracker` to "const values + closure args
(name/pattern only)".

Note: `ClosureArg.type_hint` also appears in the parser / AST because closures
carry declared types. Those AST-side `type_hint`s remain — only the
*optimizer-scope* mirrors go away.

---

## 4. Map of current analysis-crate inference shape

Summarized; full details in the audit.

- `AnalysisWalker` (analysis.rs, ~1,020 LOC) holds:
  - scope stack, builtin-syms set, closure-return stack → **type inference core**
  - defs / refs / unresolved_refs / function_calls / diagnostics → **IDE layer**
- `Analysis` struct exposes `def_types: FxHashMap<SourceLoc, AbstractType>`,
  which is the pure-types artifact we care about.
- Scope, type inference, and diagnostic emission are *interleaved* in a single
  forward walk; they cannot be cheaply separated by restructuring the walker.
- Public API from `lib.rs`: `AnalysisCtx::{analyze, hover, completions, goto_definition}`.

---

## 5. Target architecture — three options

### Option A — Move the inference walker into `geoscript` wholesale

Move the analysis crate's `AbstractType`, `CallableType`, `PartialApplication`,
`TypeEnv`, and type-inference logic into a new `geoscript::type_infer` module.
The analysis crate continues to own defs/refs/diagnostics/hover, but its
walker now either:
- calls `geoscript::type_infer::infer_program` and overlays its own scope walk,
  **OR**
- extends the geoscript walker via a trait / callback hooks.

**Pros**
- Single inference core, zero duplication.
- Optimizer gets the rich `AbstractType` for free (even if it currently
  flattens most of it to `ArgType`).
- Future-proof: if the optimizer later wants typed-closure awareness, it's
  already there.

**Cons**
- **Wasm size:** `geoscript_repl` (2.6 MB) picks up the full walker plus the
  rich `AbstractType`. Rough estimate from the audit: +~600 LOC of inference
  core, +ty.rs (~170 LOC), hashmap of SourceLoc → AbstractType
  (runtime memory). Binary cost after LTO is the unknown — could be
  50 KB–300 KB.
- **Interleaving problem:** the walker's scope resolution is tangled with
  symbol tracking. Moving it wholesale either drags defs/refs/diagnostics
  along (which violates the goal), or requires surgically extracting ~600
  LOC of interleaved logic.
- Invalidation: optimizer mutates the AST. Precomputed
  `FxHashMap<SourceLoc, AbstractType>` becomes stale for rewritten nodes.

### Option B — Share a minimal inference core, keep rich inference in analysis

Extract only the *minimal* forward inference (essentially: what
`pre_resolve_expr_type` does today, but implemented as a SourceLoc → ArgType
map producer) into `geoscript::type_infer`. Keep PAF / Callable / Union
tracking in the analysis crate.

**Pros**
- Minimal Wasm cost to `geoscript_repl` (this is basically the same as the
  code it already carries in `pre_resolve_expr_type`, just restructured).
- Optimizer gets essentially the same precision it has today, but via a
  cleaner API.
- Analysis crate can keep its rich walker on top.

**Cons**
- Still have two walkers (one minimal in geoscript, one rich in analysis).
- Duplicated scope-resolution logic (though it's ~100 LOC each).
- Doesn't actually deliver on Phase B's original promise of "one
  source of truth".

### Option C — Leave the optimizer alone, keep Phase A wins

Accept that Phase A got us 90% of the benefit (shared matchers + shared field
access + single ArgType enum), and stop here.  Document that
`pre_resolve_expr_type` intentionally lives in the geoscript crate and keep
the analysis walker independent.

**Pros**
- No refactor risk. No Wasm size hit.
- Preserves current clean crate boundaries.

**Cons**
- Two inference implementations that must stay in sync. In practice Phase A
  made them share their heaviest helpers, so "staying in sync" is mostly
  automatic, but the walker-shaped control flow is still duplicated.
- The "Single type system" row in the capabilities table stays red.

---

## 6. Recommended path — Option B, with an evolution path to A

After the audit, **Option B is the best balance** of the three.  Rationale:

- The optimizer's type-information needs (audit §3) are already covered by
  flat `ArgType`. Union / PAF / Callable would be ignored.
- Option A pays Wasm cost for capabilities the optimizer doesn't use.
- Option C doesn't retire `pre_resolve_expr_type`, which was the stated goal.

Option B reshapes `pre_resolve_expr_type` into a proper inference pass
(`infer_program`) producing `FxHashMap<SourceLoc, ArgType>`, retires the
stateful self-recursion, and leaves room for the analysis crate to layer its
richer `AbstractType` on top by consuming the same intermediate or running its
own walker.

**Evolution path:** if a future need emerges for the optimizer to reason about
typed closures or PAFs, we can widen the geoscript-side inference from
`ArgType` to `AbstractType` incrementally (essentially migrating toward
Option A) without throwing away this refactor.

---

## 7. Step-by-step migration plan (Option B)

Each step is a reviewable PR-sized chunk. Tests should pass after every step.

### Step 1 — Introduce `geoscript::type_infer` module (additive, no removals)

Create `src/viz/wasm/geoscript/src/type_infer.rs`.  Define:

```rust
pub struct TypeMap {
  pub by_loc: FxHashMap<SourceLoc, ArgType>,
  pub by_sym: FxHashMap<Sym, ArgType>,   // top-level bindings
}

pub fn infer_program(ctx: &EvalCtx, program: &Program) -> TypeMap;
pub fn infer_expr(ctx: &EvalCtx, env: &TypeEnv, expr: &Expr) -> Option<ArgType>;
```

Inside, port the logic of today's `pre_resolve_expr_type` but:

- Drive it with an explicit `TypeEnv` (scope stack of `FxHashMap<Sym, ArgType>`)
  instead of a borrowed `ScopeTracker`.
- Record an entry in `TypeMap::by_loc` for every `Expr` whose type we can
  infer, keyed by the expression's `SourceLoc`.
- Handle closures by recording param types from declared annotations (no body
  inference needed — we don't track closure return types here).
- Handle statements: Assignment binds `name` → inferred RHS type in the env.

The module should be feature-complete enough that `pre_resolve_expr_type`
becomes a thin shim: `type_infer::infer_expr(ctx, &env, expr)`.

**Validation:** add a `#[cfg(test)]` suite comparing `infer_expr` results
against `pre_resolve_expr_type` on the existing example programs. They should
agree on every expression (same `Option<ArgType>` return).

### Step 2 — Route optimizer through `type_infer`

Change `Optimizer` (or the `optimize_program` entrypoint) to call
`infer_program` once up front and hold the resulting `TypeMap`.

Replace each of the five `pre_resolve_expr_type` call sites identified in §3.1
with `type_map.by_loc.get(&expr.source_loc()).copied()`. Where the expression
is synthesized during optimization (no original SourceLoc), fall back to
`infer_expr` on-the-fly against a locally-maintained `TypeEnv`.

**Critical:** the optimizer mutates the AST. Two options:

- **7a — one-shot inference:** infer before optimizing; for newly-created
  nodes (constants folded out of chains, inlined closures), we mostly already
  know the result type from the computed `Value` (use `Value::get_type()`).
  For other synthesized nodes, call `infer_expr` on-demand.
- **7b — re-run after each pass:** simpler invalidation model, more expensive.

Recommend 7a; it matches current behavior where `pre_resolve_expr_type`
recomputes from context each time.

Delete `TrackedValue::Dyn.type_hint` and the `type_hint` field on
`ClosureArg` *as used by the optimizer* (retain the AST-side field).  Audit
`optimize_statement` and `inline_const_captures` for places that still set
these and remove them.

### Step 3 — Redirect `pre_resolve_binop_def_ix` and `maybe_pre_resolve_builtin_call_signature`

These two helpers need a way to ask "what type is this expression in this
scope".  Two API shapes, pick one:

- **Pass `&TypeMap` explicitly** — cleaner, forces callers to plumb it.
- **Thread-local / context-attached `TypeMap`** — less invasive, more implicit.

Recommend passing explicitly.  Both helpers are called from the optimizer's
BinOp / FunctionCall visitors, so `TypeMap` is in scope.

After this step, `pre_resolve_expr_type` has no callers *other than itself*
and the thin shim in Step 1.  It can be deleted.

### Step 4 — Delete `pre_resolve_expr_type`

Remove the function. Inline any remaining usages into `infer_expr`.
Also drop the now-dead `ScopeTracker::type_hint` paths and the `TrackedValue::Dyn`
variant's `type_hint` field.

`ScopeTracker` should simplify to:
```rust
pub(crate) enum TrackedValue {
  Const(Value),
  Arg(ClosureArg),
  Dyn,  // was: Dyn { type_hint: Option<ArgType> }
}
```

### Step 5 — Wire analysis crate to consume the shared core

The analysis crate's walker currently has its own scope + ArgType-lookup
logic.  Replace the `AnalysisWalker::walk_expr` identifier / literal / binop /
field-access arms' type-derivation with a call into
`geoscript::type_infer::infer_expr` — but only for `AbstractType::Concrete`
cases.  The walker continues to own:

- `PartiallyApplied` tracking (when a call doesn't fully saturate a builtin).
- `Callable` tracking (when a closure is assigned to a binding).
- `Union` tracking (conditional/block branch merging).
- All defs / refs / diagnostics emission.

The walker reads `infer_expr` as "what does the value-level view say?", then
refines or replaces with its richer view as appropriate.

**Concretely:** for each `walk_expr` match arm that currently computes a
`Concrete(ArgType)` directly from the geoscript helpers, refactor it to
delegate to `infer_expr`. Arms that need PAF / Callable / Union remain
specialized.

Expected code reduction in `analysis.rs`: ~50–150 lines.

### Step 6 — Update plan doc & capabilities table

Mark Layer 5 Phase B complete in `abstract_interpretation_plan.md`.
Remove the "Single type system" row from the deferred column.

---

## 8. Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `geoscript_repl` Wasm blob grows >250 KB | medium | high (blocker) | Measure after Step 1 with `wasm-opt` + LTO; if over budget, de-optimize (pull expensive arms behind feature flag) or revert |
| Optimizer produces different results after refactor | medium | high | Golden-test every optimizer output on the `test_example_programs` corpus; require byte-identical `Program` after optimize in a diff test |
| Stale `TypeMap` entries after AST mutation cause regressions | medium | medium | In Step 2, prefer `infer_expr` on-demand for synthesized nodes; add asserts that `TypeMap` lookups only happen on original nodes |
| `pre_resolve_expr_type` and the new `infer_expr` disagree on edge cases | low-medium | medium | Step 1's parity test suite catches this before we cut over |
| Analysis crate regresses hover / completions | medium | medium | Existing 50 analysis tests must continue to pass at every step; if any fails, stop and investigate |
| Scope semantics differ subtly between `ScopeTracker` and `TypeEnv` | low | medium | Port `ScopeTracker`'s exact lookup rules (parent chaining) into `TypeEnv`; add a targeted test case per lookup rule |

---

## 9. Alternatives if Phase B's cost outweighs benefit

If Step 1's Wasm-size measurement comes in over budget, or if the optimizer
refactor proves higher-risk than expected, these are the fallbacks:

- **9a — Ship Phase A and stop.** Document that "unify type systems" is
  explicitly out of scope and that the shared helpers (matchers + field-access)
  are the coordination point. The analysis crate keeps running its own walker.
- **9b — Do Step 1 only.** Extract `type_infer` as a lib module but keep
  `pre_resolve_expr_type` calling into it as a shim. No behavior change, but
  the new entry point becomes the place to add future shared logic. This
  costs effectively zero Wasm size and sets up incremental unification.
- **9c — Do Steps 1–4 but not Step 5.** Retire `pre_resolve_expr_type` and
  clean the optimizer, but leave the analysis walker independent. This
  captures the "single optimizer inference path" win without touching the
  analysis crate.

The plan above assumes Steps 1–6 all ship; 9a/b/c are stop-points if a step
exposes unacceptable cost.

---

## 10. Suggested sequencing

1. **Step 1** in one PR. Include the parity tests.
2. **Size audit.** Build `geoscript_repl_bg.wasm` before + after. If > 100 KB
   delta, pause and redesign (likely dropping to 9b). Otherwise proceed.
3. **Step 2** in one PR. Optimizer golden tests must pass unchanged.
4. **Steps 3 + 4** together in one PR (they co-change the same helpers).
5. **Step 5** in a separate, analysis-crate-focused PR.
6. **Step 6** alongside step 5 or just after.

Total estimated complexity: Steps 1–4 are the real work (~2–4 days of focused
effort). Step 5 is optional polish (~1 day). Step 6 is docs (~30 min).
