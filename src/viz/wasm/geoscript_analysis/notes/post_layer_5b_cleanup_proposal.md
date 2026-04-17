# Post-Layer-5b Cleanup Proposal

Date: 2026-04-16

This is a fresh review of the current `geoscript` / `geoscript_analysis` architecture after the Layer 5b unification work. It incorporates:

- `abstract_interpretation_plan.md`
- `post_layer_5b_cleanup_candidates.md`
- the current `geoscript`, `geoscript_analysis`, and `geoscript_analysis_wasm` sources
- the editor integration in `src/geoscript/*`

## Executive Summary

The current architecture is in a much better place than the cleanup-candidates note makes it sound.

The important structural shift has already happened:

- `AbstractType` and the pure inference engine now live in the `geoscript` crate (`geoscript/src/ty.rs`, `geoscript/src/type_infer.rs`).
- `geoscript_analysis` is now mostly an IDE-facing consumer layered on top of that shared type system.
- the optimizer no longer owns its own separate expression-type world; it now asks the shared inference engine targeted questions and writes the answers back into AST/runtime caches.

Because of that, I would **not** recommend another large unification pass right now. The highest-value remaining work is smaller and more local:

- remove duplicated non-const binding tracking logic in `optimizer.rs` and `ast.rs`
- stop discarding richer inferred types in a few optimizer/capture paths
- fix the `TypeEnv` / prelude-global mismatch so optimizer-side inference sees the same intrinsic names that analysis sees

The real remaining debt is mostly in the **optimizer-side const-capture / binding bookkeeping**, not in the overall crate boundary.

## Current Architecture

### High-level roles

- `geoscript/src/ty.rs` defines the shared abstract type vocabulary: `AbstractType`, `CallableType`, `PartialApplication`, and `merge_types`.
- `geoscript/src/type_infer.rs` is the pure forward inference engine. It owns `TypeEnv`, `infer_expr`, `infer_statement`, and shared call-resolution helpers such as `resolve_builtin_call` and `resolve_paf_call`.
- `geoscript/src/optimizer.rs` is still the AST-mutating constant-folder / pre-resolution pass. It uses `ScopeTracker` for const-vs-nonconst tracking, and snapshots that state into `TypeEnv` when it needs type information.
- `geoscript/src/ast.rs` still contains the optimizer-facing scope/capture machinery: `TrackedValue`, `ScopeTracker`, `DynType`, `get_dyn_type`, and closure const-capture helpers.
- `geoscript_analysis/src/analysis.rs` is a combined symbol walker for IDE features. It tracks defs/refs/function calls/diagnostics while reproducing enough type propagation to attach `AbstractType` information to definitions and call sites.
- `geoscript_analysis/src/{hover,completions,goto,diagnostics}` are thin consumers of `Analysis`.
- `geoscript_analysis_wasm` is a serialization/export shim for the editor worker.
- `src/geoscript/{analysisWorker.worker.ts,analysisClient.ts,analysisExtensions.ts}` wires the Codemirror UI to the Wasm analysis entrypoints.

### Interaction diagram

```text
Codemirror extensions
  -> analysisClient.ts
  -> analysisWorker.worker.ts
  -> geoscript_analysis_wasm
  -> AnalysisCtx
  -> parse_program_maybe_with_prelude(...)
  -> Program AST
  -> Analysis::build / AnalysisWalker
     -> shared AbstractType vocabulary (geoscript::ty)
     -> shared call resolution helpers (geoscript::type_infer::resolve_*)
     -> builtin metadata / signature tables
  -> Analysis result
     -> diagnostics
     -> hover
     -> completions
     -> goto-definition

Program AST
  -> optimize_ast / optimizer.rs
     -> ScopeTracker + TrackedValue + DynType
     -> build_type_env()
     -> geoscript::type_infer::infer_expr(...)
     -> writes pre_resolved_signature / pre_resolved_def_ix
  -> EvalCtx runtime evaluation
     -> uses those cached resolution results to skip repeated signature matching
```

### The important boundary

The cleanest way to think about the current system is:

- `geoscript` owns the language semantics.
- `geoscript_analysis` owns editor-facing indexing and presentation.
- the optimizer is a language-internal consumer of the shared semantics, not a second type-system owner.

That is a good boundary. It is worth preserving.

## Design Assessment

### What is working well

- The center of gravity has moved into the right crate. `geoscript::type_infer` is the semantic core now, which is the correct direction.
- Shared field-access typing is consolidated in `geoscript/src/ast.rs:2564-2595`.
- Shared builtin/PAF resolution is consolidated in `geoscript/src/type_infer.rs:88-219`.
- The runtime evaluator benefits directly from optimizer-side pre-resolution through `pre_resolved_signature` and `pre_resolved_def_ix` (`geoscript/src/lib.rs:1915-1956`, `geoscript/src/lib.rs:2748-2786`).
- The analysis crate is no longer doing fundamentally separate type reasoning for builtins and operators; it reuses the shared semantic tables and resolution helpers.

### What still feels loose

- `AnalysisWalker::walk_expr` still mirrors a large portion of `type_infer::infer_expr` almost structurally line-for-line (`geoscript_analysis/src/analysis.rs:343-816` vs `geoscript/src/type_infer.rs:314-687`).
- optimizer-side non-const binding bookkeeping is duplicated in too many places:
  - `ClosureBody::analyze_const_captures` (`ast.rs:1182-1272`)
  - `ClosureBody::inline_const_captures` (`ast.rs:1274-1336`)
  - `optimize_statement` assignment handling (`optimizer.rs:1776-1818`)
  - `optimize_top_level_statement` export handling (`optimizer.rs:1879-1921`)
- some optimizer paths still flatten rich `AbstractType` information down to `Option<ArgType>` and then immediately wrap it back into `AbstractType::Concrete`, which throws away capability that the shared system already computed.
- `TypeEnv::with_prelude` exists (`type_infer.rs:46-58`) but none of the optimizer bridges use it; `ScopeTracker::build_type_env` currently seeds from local scope only (`ast.rs:2521-2549`).

## Assessment of the Existing Cleanup Candidates

| Candidate | Assessment | Recommendation |
|---|---|---|
| Collapse `TrackedValue::Arg` and `TrackedValue::Dyn` | I do **not** think this is safe yet. The type side of the distinction has mostly collapsed, but the const-capture side has not. `DynType`, `get_dyn_type`, identifier capture checks, and nested-closure handling still rely on "`depends only on local args`" being distinct from "`truly dynamic`" (`ast.rs:2641-2761`, especially `2712-2725`). | Keep the variants for now. Eliminate the type-storage asymmetry instead. |
| Remove `DynType` | Same answer. It is still semantically load-bearing for constification and nested closure capture propagation. | Defer. Only revisit if the whole const-capture model is redesigned. |
| Unify `ScopeTracker` and `TypeEnv` | Plausible in theory, but not yet a clear win. `ScopeTracker` fundamentally stores const values; `TypeEnv` does not. The current bridge is ugly but conceptually justified. | Defer unless profiling or future feature work makes the bridge a proven problem. |
| Extract `infer_binding_type` helper | Yes, but it should return `AbstractType`, not `Option<ArgType>`. The current flatten-then-rewrap pattern is part of the problem. | Do this. |
| Deduplicate `analyze_const_captures` and `inline_const_captures` | Yes. This is one of the clearest maintainability wins left. | High priority. |
| Deduplicate Assignment vs Export in `optimizer.rs` | Yes. The code is nearly identical. | Low-risk quick win. |
| Rename `pre_resolved_ty` | Yes. It is no longer a pre-resolution pipeline. | Do it opportunistically. |

## Additional Cleanup Opportunities I Recommend

### 1. Preserve full `AbstractType` in optimizer-side non-const bindings

Right now optimizer assignment/export handling still does:

- infer `AbstractType`
- flatten via `.as_single_arg_type()`
- store only `AbstractType::Concrete(ty)` if that succeeded

That happens in `optimizer.rs:1801-1815` and `optimizer.rs:1903-1917`.

This is unnecessary information loss. `ScopeTracker::types` already stores `AbstractType`, and `build_type_env()` already knows how to rehydrate from that side-table. The optimizer does not have to use every rich type immediately in order for it to be worth storing.

Recommended change:

- switch the binding helper to compute/stash `AbstractType` directly
- only fall back to `Unknown` when inference truly cannot say anything

This same fix should also be applied to the `DynType::Arg` branches in `ClosureBody::{analyze_const_captures, inline_const_captures}` so arg-derived locals can keep their inferred types instead of silently dropping them.

### 2. Actually wire intrinsic globals into optimizer-side `TypeEnv`

`TypeEnv::with_prelude` exists, but the optimizer bridge never uses it:

- `type_infer.rs:46-58`
- `ast.rs:2521-2549`

That means optimizer-side `infer_expr` snapshots do not automatically know that `pi` and `tau` are floats, even though analysis does seed them (`analysis.rs:108-123`) and runtime evaluation does know them via globals.

This is a real loose end, not just cosmetic duplication.

Recommended change:

- either change the bridge to seed default globals when building a `TypeEnv`
- or delete `with_prelude` if you intentionally do not want that behavior

I strongly prefer the first option because it keeps optimizer-side type reasoning aligned with the actual language environment.

As an added note here, the fact that we're manually constructing this prelude environment is a bad code smell to me.  This means that the type-level `TypeEnv::with_prelude` and the actual execution context code that constructs the globals need to be kept in sync manually.  This isn't a huge issue at the current time since there are only two variables, but I'd like whatever solution we land on here to use a unified method for managing these globals.  It should be impossible for the type and runtime representation of these globals to go out of sync when adding/removing/changing globals.

As a final note, the "prelude" naming is misleading here.  We already have a different concept of a prelude which is used in the "geotoy" app - the main consumer of this geoscript crate and the geoscipt language in general currently.  The prelude contains code which sets up basic lighting and shadows and stuff for the scene and is included by default for all scenes unless explicitly disabled.

### 3. Extract one helper for closure-param scope binding

The same "bind closure params into a `ScopeTracker` as `TrackedValue::Arg`, with optional type info" logic appears in at least three places:

- `ast.rs:699-718`
- `ast.rs:883-902`
- `optimizer.rs:1376-1393`

This is a clean, low-risk extraction point. A small helper such as `bind_closure_params_into_scope(...)` would pay for itself immediately.

### 4. Treat `AnalysisWalker` vs `type_infer::infer_expr` as a managed duplication, not a refactor target

This is the one place where there is still substantial semantic overlap across crates, but I do **not** recommend trying to merge them right now.

Why:

- the shared pure layer already exists where it matters most: type vocabulary, call resolution, overload matching, field access
- `AnalysisWalker` is not just inferring types; it is simultaneously building refs/defs/function-call metadata and emitting diagnostics
- another broad unification pass would be invasive and likely lower ROI than the optimizer-side cleanup work above

The right stance here is:

- acknowledge the duplication
- keep it tested
- only extract smaller helpers if a new feature forces the issue

### 5. Keep `definitions_visible_at()` on the watchlist, but not in this cleanup batch

`Analysis::definitions_visible_at()` currently returns every definition unfiltered (`analysis.rs:56-60`), which means completion visibility is still approximate.

That is a real TODO, but it is not primarily a simplification/maintainability cleanup. It is feature/correctness work. I would not mix it into this batch.

## Recommended Cleanup Sequence

### Phase 1: high-confidence cleanup

1. Introduce a shared optimizer/capture helper for "record the result of a non-const binding".
2. Make that helper store `AbstractType` directly instead of flattening to `ArgType`.
3. Use the same helper in:
   - `ClosureBody::analyze_const_captures`
   - `ClosureBody::inline_const_captures`
   - `optimize_statement` assignment
   - `optimize_top_level_statement` export
4. Preserve `TrackedValue::Arg` vs `TrackedValue::Dyn`, but allow both to carry type side-table entries.

This is the single best cleanup because it removes duplication and tightens semantics without changing the higher-level architecture.

### Phase 2: consistency cleanup

1. Fix the `TypeEnv`/intrinsic-globals mismatch.
2. Extract `bind_closure_params_into_scope(...)`.
3. Rename `pre_resolved_ty` to something like `inferred_ty`.
4. Fix the `maybe_pre_resolve_bulitin_call_signature` typo while touching nearby code.

### Phase 3: optional follow-up

1. Deduplicate Assignment vs Export through a shared helper if Phase 1 did not already subsume it.
2. Re-evaluate `ScopeTracker` vs `TypeEnv` only if profiling shows the snapshot bridge is materially hot.

## What I Would Not Do

- I would not collapse `TrackedValue::Arg` into `TrackedValue::Dyn` yet.
- I would not remove `DynType` yet.
- I would not start another major optimizer/analysis unification pass right now.
- I would not try to merge `ScopeTracker` and `TypeEnv` without a concrete performance or correctness pressure.

## Bottom Line

The codebase now has a sensible center:

- shared semantics in `geoscript`
- editor indexing/presentation in `geoscript_analysis`
- optimizer as a consumer of shared type inference

That is the right shape.

The next cleanup pass should be deliberately conservative and aimed at the remaining duplicated binding/capture plumbing. That is where the highest-confidence maintainability wins are now.

## Validation Notes

Local test run during this review:

- `cargo test -p geoscript_analysis --quiet`: passed, 50/50 tests
- `cargo test -p geoscript --quiet`: 206 passed, 1 failed
- remaining failure: `mesh_ops::adaptive_sampler::tests::test_bad_adaptive_sampler_repro` at `geoscript/src/mesh_ops/adaptive_sampler.rs:776`

That matches the current "one pre-existing geoscript failure" picture rather than suggesting new instability in the analysis/type-unification work.
