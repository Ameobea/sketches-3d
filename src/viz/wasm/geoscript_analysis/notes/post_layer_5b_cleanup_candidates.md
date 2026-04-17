# Post-Layer-5b Cleanup Candidates

Written after the Layer 5b unification refactor (`pre_resolve_expr_type` retired, shared `infer_expr` adopted) plus the three-level `TrackedValue` cleanup (Levels 1–3 in `/.claude/plans/synchronous-splashing-meteor.md`). This is a strategic map of what that refactor surfaced — not a plan. Each item is a question worth answering before doing the work.

---

## 1. Do `TrackedValue::Arg` and `TrackedValue::Dyn` still need to be distinct?

This is the headline question. Two pieces of evidence from the Level 3 diff say they may not:

- `build_type_env`: the `Arg` and `Dyn` match arms merged into one after Level 3. Both now read from `ScopeTracker::types`; both fall back to `AbstractType::Unknown`.
- The closure-scope pass-through at `optimizer.rs:1415` similarly merged — `Arg | Dyn` take the same branch.

That's two independent pieces of code where the distinction collapsed naturally under pressure. When variants behave identically at consumption, the distinction is being held up only by the *write* side.

**Where they still diverge (as of Level 3):**
- `get_dyn_type` classifies expressions into `DynType::{Const, Arg, Dyn}` by recursive traversal.
- `analyze_const_captures` / `inline_const_captures` branch on `DynType::Arg` vs `DynType::{Const,Dyn}`: the `Arg` branch writes a bare `TrackedValue::Arg` (type info discarded), while the `Dyn` branch runs `infer_expr` and calls `set_with_type`. So `Arg`-classified bindings silently lose type information that could be recovered.
- The optimizer's Assignment / Export paths run inference for both branches and use `set_with_type` on both — so they're already symmetrized there.

**Questions to answer before acting:**
1. Does any downstream consumer actually *care* whether a binding originated from a closure arg vs a de-constified local? After Level 3, the places I can find that still branch on this are `get_dyn_type` itself (self-perpetuating) and maybe one or two optimizer heuristics.
2. If we collapse `Arg | Dyn` into a single `TrackedValue::NonConst`, does `get_dyn_type` also collapse? That would eliminate `DynType::Arg` and a recursive traversal.
3. The `Arg` branch in `analyze_const_captures` / `inline_const_captures` discarding type info looks like an accidental asymmetry — is there a semantic reason, or is it leftover from before the shared `infer_expr` existed?

If the answers are "no meaningful distinction" + "accidental asymmetry", the right move is to fold `Arg` into `Dyn` entirely, delete `get_dyn_type`'s Arg-tracking, and run `infer_expr` uniformly.

---

## 2. `DynType` enum itself

Related to #1. Level 2 already collapsed `DynType::Const | DynType::Dyn` into the same code path everywhere I touched. If #1 also eliminates the `Arg` case, `DynType` might reduce to a bool (`expr is const-foldable`) or vanish entirely — replaced by checking `expr.as_literal()` at the call site.

`get_dyn_type` does a recursive expression walk. If it can be eliminated, that's a measurable simplification of the optimizer's hot path.

---

## 3. Dual scope representations: `ScopeTracker` vs `TypeEnv`

The optimizer uses `ScopeTracker` (stores `TrackedValue` + `types` side-table). Shared inference uses `TypeEnv` (stores only `AbstractType`). `build_type_env()` bridges them by walking the parent chain and cloning out types.

This is fine *because* the optimizer needs to store `Value` for `Const` bindings (for constant folding) — `TypeEnv` alone can't carry that. But the bridging is O(scope_depth) and gets called for every expression the optimizer type-infers. Not a correctness issue, but worth checking:

- Could `ScopeTracker` embed a `TypeEnv` directly and mutate it in lockstep, eliminating the snapshot? Answer depends on how `TypeEnv` handles scope push/pop.
- Is there a shared trait worth defining (`ScopeQuery` — `get_type(name)`, `get_value(name)`) so `infer_expr` is agnostic to which representation it's querying?

This is the most architecturally significant question on the list but also the most speculative. Not obviously a win.

---

## 4. Helper extraction: `infer_binding_type`

The pattern `(type_hint_from_ast).or_else(|| { let mut env = scope.build_type_env(); infer_expr(ctx, &mut env, expr).as_single_arg_type() })` appears at least 4 times in `optimizer.rs` (Assignment, Export, and similar sites) and structurally in the `ast.rs` capture paths. Worth extracting to a helper once #1 is resolved — doing it now risks locking in redundant branches.

---

## 5. `analyze_const_captures` vs `inline_const_captures` near-duplication

Both methods loop over statements and branch on `Statement::Assignment` / `Statement::DestructureAssignment` with structurally identical bodies (four blocks, each ~15–20 lines). The only real difference is *what else* they do per statement — `analyze` tracks a bool, `inline` actually mutates expressions.

With the shared type-inference plumbing now landed, the divergence between them is mostly bookkeeping. One shared walker parameterized by a callback or visitor trait is plausible. Medium-risk refactor; would remove ~80–100 lines and collapse two similar code paths that need to stay in sync.

---

## 6. `optimizer.rs` Assignment vs Export duplication

The `Statement::Assignment` arm and the `TopLevelStatement::Export` arm are near-identical (I edited both to mirror each other through Levels 2 and 3). If Export is always "Assignment + mark exported", a shared helper would eliminate ~40 lines. Low-risk.

---

## 7. Naming: `pre_resolved_ty`

The local `pre_resolved_ty` still appears in optimizer.rs. The "pre_resolve" pipeline (`pre_resolve_expr_type`) is gone; nothing about this value is a *pre*-resolution anymore. Rename to `inferred_ty` or `known_ty`. Trivial.

---

## Suggested order if we come back to this

1. **#1 first.** It's the question that drives everything else. If `Arg`/`Dyn` merge, it changes #2, #4, and partially #5.
2. **#7** is a 5-minute rename, do it whenever.
3. **#6** is a small, standalone win — good warm-up.
4. **#5** (deduplicating the capture walkers) is most valuable but wants #1 resolved first so the per-branch logic is already minimal.
5. **#4** falls out of #1+#5 naturally.
6. **#3** is speculative. Only pursue if profiling shows `build_type_env` is hot, or if someone's actively designing something that benefits from unified scope representation.

---

## What NOT to touch

- **AST-level `ClosureArg.type_hint`.** It represents the syntactically declared parameter type and is load-bearing for signature hashing at `optimizer.rs:479-480`. Level 3 deliberately left it alone.
- **`TrackedValueRef`.** It's already unit-variant-clean post-Level-1. No further compression available.
- **The shared `infer_expr` / `TypeEnv` surface.** It's the product of the refactor and should stay stable while downstream consumers settle in.
