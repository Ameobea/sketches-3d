use geoscript::preprocess::preprocess;
use geoscript_analysis::{rewrite_input_defaults, AnalysisCtx, InputDefaultRequest};
use nanoserde::{DeJson, SerJson};
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOC: geoscript::aligned_alloc::CacheAligned = geoscript::aligned_alloc::CacheAligned;

static mut DID_INIT: bool = false;

fn maybe_init() {
  unsafe {
    if DID_INIT {
      return;
    }
    DID_INIT = true;
  }
  console_error_panic_hook::set_once();
  wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
}

#[wasm_bindgen]
pub fn analysis_init() -> *mut AnalysisCtx {
  maybe_init();
  Box::into_raw(Box::new(AnalysisCtx::new()))
}

#[wasm_bindgen]
pub fn analysis_free(ctx: *mut AnalysisCtx) {
  if !ctx.is_null() {
    unsafe {
      drop(Box::from_raw(ctx));
    }
  }
}

#[wasm_bindgen]
pub fn analysis_analyze(
  ctx: *const AnalysisCtx,
  src: &str,
  include_prelude: bool,
  ambient_src: &str,
) -> String {
  let ctx = unsafe { &*ctx };
  let result = ctx.analyze(src, include_prelude, ambient_src);
  nanoserde::SerJson::serialize_json(&result)
}

/// Get hover info at (line, col).  Returns JSON-serialized `HoverInfo` or empty string if nothing.
#[wasm_bindgen]
pub fn analysis_hover(
  ctx: *const AnalysisCtx,
  src: &str,
  line: u32,
  col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> String {
  let ctx = unsafe { &*ctx };
  match ctx.hover(src, line, col, include_prelude, ambient_src) {
    Some(info) => nanoserde::SerJson::serialize_json(&info),
    None => String::new(),
  }
}

/// Get completions at (line, col).  Returns JSON-serialized `Vec<CompletionItem>`.
#[wasm_bindgen]
pub fn analysis_completions(
  ctx: *const AnalysisCtx,
  src: &str,
  line: u32,
  col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> String {
  let ctx = unsafe { &*ctx };
  let items = ctx.completions(src, line, col, include_prelude, ambient_src);
  nanoserde::SerJson::serialize_json(&items)
}

/// Returns JSON `{ rewritten, edits }` on success, or `{ error: { message, line, col } }`
/// on preprocessor error. Callers distinguish by checking for an `error` key.
#[wasm_bindgen]
pub fn preprocess_source(src: &str) -> String {
  match preprocess(src) {
    Ok(out) => nanoserde::SerJson::serialize_json(&out),
    Err(err) => format!(
      "{{\"error\":{{\"message\":{},\"line\":{},\"col\":{}}}}}",
      nanoserde::SerJson::serialize_json(&err.message),
      err.line,
      err.col,
    ),
  }
}

/// Plans `default=` rewrites for `input_*` sites in `src` from stored control values
/// (`[{handle_id, kind, value, str_value}]`, the injection wire shape). Returns JSON
/// `{ edits: [{from, to, insert}], errors: [{handle_id, message}] }` with UTF-16 offsets.
#[wasm_bindgen]
pub fn analysis_rewrite_input_defaults(src: &str, requests_json: &str) -> String {
  let requests: Vec<InputDefaultRequest> = match DeJson::deserialize_json(requests_json) {
    Ok(r) => r,
    Err(err) => {
      return format!(
        "{{\"edits\":[],\"errors\":[{{\"handle_id\":\"\",\"message\":{}}}]}}",
        format!("invalid request JSON: {err}").serialize_json()
      )
    }
  };
  rewrite_input_defaults(src, &requests).serialize_json()
}

/// Signature help for the call enclosing (line, col).  Returns JSON-serialized `SignatureHelp`
/// or empty string if the cursor isn't inside a known call.
#[wasm_bindgen]
pub fn analysis_signature_help(
  ctx: *const AnalysisCtx,
  src: &str,
  line: u32,
  col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> String {
  let ctx = unsafe { &*ctx };
  match ctx.signature_help(src, line, col, include_prelude, ambient_src) {
    Some(help) => nanoserde::SerJson::serialize_json(&help),
    None => String::new(),
  }
}

/// Get go-to-definition location at (line, col).  Returns JSON-serialized `DefinitionLocation`
/// or empty string if nothing.
#[wasm_bindgen]
pub fn analysis_goto_definition(
  ctx: *const AnalysisCtx,
  src: &str,
  line: u32,
  col: u32,
  include_prelude: bool,
  ambient_src: &str,
) -> String {
  let ctx = unsafe { &*ctx };
  match ctx.goto_definition(src, line, col, include_prelude, ambient_src) {
    Some(loc) => nanoserde::SerJson::serialize_json(&loc),
    None => String::new(),
  }
}
