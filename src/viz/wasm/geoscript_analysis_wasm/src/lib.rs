use geoscript_analysis::AnalysisCtx;
use wasm_bindgen::prelude::*;

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

/// Create a new analysis context.  Returns a pointer to be passed to all other functions.
#[wasm_bindgen]
pub fn analysis_init() -> *mut AnalysisCtx {
  maybe_init();
  Box::into_raw(Box::new(AnalysisCtx::new()))
}

/// Free an analysis context.
#[wasm_bindgen]
pub fn analysis_free(ctx: *mut AnalysisCtx) {
  if !ctx.is_null() {
    unsafe {
      drop(Box::from_raw(ctx));
    }
  }
}

/// Run diagnostics analysis on source code.  Returns JSON-serialized `AnalysisResult`.
#[wasm_bindgen]
pub fn analysis_analyze(ctx: *const AnalysisCtx, src: &str, include_prelude: bool) -> String {
  let ctx = unsafe { &*ctx };
  let result = ctx.analyze(src, include_prelude);
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
) -> String {
  let ctx = unsafe { &*ctx };
  match ctx.hover(src, line, col, include_prelude) {
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
) -> String {
  let ctx = unsafe { &*ctx };
  let items = ctx.completions(src, line, col, include_prelude);
  nanoserde::SerJson::serialize_json(&items)
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
) -> String {
  let ctx = unsafe { &*ctx };
  match ctx.goto_definition(src, line, col, include_prelude) {
    Some(loc) => nanoserde::SerJson::serialize_json(&loc),
    None => String::new(),
  }
}
