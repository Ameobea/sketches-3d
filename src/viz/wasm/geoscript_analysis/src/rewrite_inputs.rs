//! Surgical `default=` rewrites for `input_*` call sites from stored control values. Works on
//! the raw Pest tree (the AST keeps only start locations) and reports edits as UTF-16 offsets
//! into the original source so the host can splice a CodeMirror doc directly.

use fxhash::FxHashSet;
use geoscript::{
  parse_string_literal,
  preprocess::{preprocess, Preprocessed},
  GSParser, RampSpecWire, Rule,
};
use nanoserde::{DeJson, SerJson};
use pest::{iterators::Pair, Parser};

/// Same payload shape as the run-time injection wire (`GizmoValueWire`).
#[derive(DeJson, Debug)]
pub struct InputDefaultRequest {
  pub handle_id: String,
  pub kind: String,
  pub value: Vec<f32>,
  pub str_value: Option<String>,
}

/// UTF-16 offsets into the original source.
#[derive(SerJson, Debug, PartialEq, Eq)]
pub struct SourceEdit {
  pub from: usize,
  pub to: usize,
  pub insert: String,
}

#[derive(SerJson, Debug, PartialEq, Eq)]
pub struct RewriteError {
  pub handle_id: String,
  pub message: String,
}

#[derive(SerJson, Debug, Default)]
pub struct RewriteResult {
  pub edits: Vec<SourceEdit>,
  pub errors: Vec<RewriteError>,
}

/// (callee, control kind, positional index of `default`) — must track `fn_defs.rs`.
const INPUT_CALLEES: &[(&str, &str, usize)] = &[
  ("input_float", "float", 4),
  ("input_int", "int", 4),
  ("input_bool", "bool", 1),
  ("input_color", "color", 1),
  ("input_select", "select", 2),
  ("input_spline", "spline", 1),
  ("input_ramp", "ramp", 1),
  ("input_color_ramp", "ramp", 1),
  ("input_image_levels", "image_levels", 2),
];

struct Site<'a> {
  callee: &'a str,
  kind: &'a str,
  default_ix: usize,
  /// Literal `name`; `None` when computed at runtime.
  name: Option<String>,
  positional: Vec<(usize, usize)>,
  kwargs: Vec<(&'a str, (usize, usize))>,
  /// End of the last arg (or of `name(` when there are none): where an inserted kwarg goes.
  args_end: usize,
  call_start: usize,
}

fn span(p: &Pair<Rule>) -> (usize, usize) {
  let s = p.as_span();
  (s.start(), s.end())
}

/// `expr` that is exactly one string literal, unescaped.
fn as_string_literal(expr: &Pair<Rule>) -> Option<String> {
  fn only_child<'a>(p: Pair<'a, Rule>) -> Option<Pair<'a, Rule>> {
    let mut it = p.into_inner();
    let c = it.next()?;
    it.next().is_none().then_some(c)
  }
  let ct = only_child(expr.clone())?;
  if ct.as_rule() != Rule::chained_term {
    return None;
  }
  let term = only_child(ct)?;
  if term.as_rule() != Rule::term {
    return None;
  }
  let lit = only_child(term)?;
  matches!(
    lit.as_rule(),
    Rule::double_quote_string_literal | Rule::single_quote_string_literal
  )
  .then(|| parse_string_literal(lit))
}

fn collect_sites<'a>(pair: Pair<'a, Rule>, out: &mut Vec<Site<'a>>) {
  if pair.as_rule() == Rule::func_call {
    let mut inner = pair.clone().into_inner();
    let head = inner.next().unwrap();
    let callee = head.as_str().trim_start_matches('@').trim_end_matches('(');
    if let Some(&(callee, kind, default_ix)) = INPUT_CALLEES.iter().find(|(c, ..)| *c == callee) {
      let mut site = Site {
        callee,
        kind,
        default_ix,
        name: None,
        positional: Vec::new(),
        kwargs: Vec::new(),
        args_end: head.as_span().end(),
        call_start: head.as_span().start(),
      };
      let (mut first_positional, mut name_kwarg) = (None, None);
      for arg in inner {
        let a = arg.into_inner().next().unwrap();
        site.args_end = a.as_span().end();
        if a.as_rule() == Rule::keyword_arg {
          let mut kv = a.into_inner();
          let k = kv.next().unwrap().as_str();
          let v = kv.next().unwrap();
          site.kwargs.push((k, span(&v)));
          if k == "name" {
            name_kwarg = Some(v);
          }
        } else {
          if site.positional.is_empty() {
            first_positional = Some(a.clone());
          }
          site.positional.push(span(&a));
        }
      }
      site.name = first_positional
        .or(name_kwarg)
        .and_then(|e| as_string_literal(&e));
      out.push(site);
    }
  }
  for child in pair.into_inner() {
    collect_sites(child, out);
  }
}

fn utf16_offset(src: &str, byte: usize) -> usize {
  src[..byte].encode_utf16().count()
}

fn line_indent(src: &str, byte: usize) -> &str {
  let line_start = src[..byte].rfind('\n').map_or(0, |i| i + 1);
  let line = &src[line_start..];
  &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// Six decimal places, shortest round-trip after rounding, always with a `.` so it reads as a
/// float literal. The grammar has no exponent form; Rust's `Display` never emits one.
fn fmt_float(f: f32) -> Result<String, String> {
  if !f.is_finite() {
    return Err("value is not a finite number".to_owned());
  }
  let r = (f as f64 * 1e6).round() / 1e6;
  let mut s = format!("{r}");
  if !s.contains('.') {
    s.push_str(".0");
  }
  Ok(s)
}

fn fmt_vec3(v: &[f32]) -> Result<String, String> {
  Ok(format!(
    "vec3({}, {}, {})",
    fmt_float(v[0])?,
    fmt_float(v[1])?,
    fmt_float(v[2])?
  ))
}

fn fmt_string(s: &str) -> String {
  format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn fmt_list(items: &[String], indent: &str) -> String {
  if items.len() <= 3 {
    return format!("[{}]", items.join(", "));
  }
  let body: Vec<String> = items.iter().map(|i| format!("{indent}  {i},")).collect();
  format!("[\n{}\n{indent}]", body.join("\n"))
}

fn print_ramp(json: &str, indent: &str) -> Result<String, String> {
  let w = RampSpecWire::deserialize_json(json).map_err(|e| format!("malformed ramp value: {e}"))?;
  if w.stops.is_empty() {
    return Err("ramp has no stops".to_owned());
  }
  let width = if w.scalar { 1 } else { 3 };
  let mut stops = Vec::with_capacity(w.stops.len());
  for s in &w.stops {
    if s.value.len() < width {
      return Err("malformed ramp stop".to_owned());
    }
    let val = if w.scalar {
      fmt_float(s.value[0])?
    } else {
      fmt_vec3(&s.value)?
    };
    let ease = if s.ease == "linear" {
      String::new()
    } else {
      format!(", {}", fmt_string(&s.ease))
    };
    stops.push(format!("[{}, {val}{ease}]", fmt_float(s.pos)?));
  }
  let list = fmt_list(&stops, indent);
  // A bare stop list gets clamp + the kind's default space, so only spell out deviations.
  let mut kwargs = Vec::new();
  if w.extend != "clamp" {
    kwargs.push(format!("extend={}", fmt_string(&w.extend)));
  }
  if !w.scalar && w.space != "oklab" {
    kwargs.push(format!("space={}", fmt_string(&w.space)));
  }
  if kwargs.is_empty() {
    return Ok(list);
  }
  let ctor = if w.scalar { "ramp" } else { "color_ramp" };
  Ok(format!("{ctor}(stops={list}, {})", kwargs.join(", ")))
}

fn print_value(req: &InputDefaultRequest, indent: &str) -> Result<String, String> {
  let v = &req.value;
  let need = |n: usize| {
    (v.len() >= n)
      .then_some(())
      .ok_or_else(|| format!("malformed {} value", req.kind))
  };
  let str_value = || {
    req
      .str_value
      .as_deref()
      .ok_or_else(|| format!("malformed {} value", req.kind))
  };
  match req.kind.as_str() {
    "float" => {
      need(1)?;
      fmt_float(v[0])
    }
    "int" => {
      need(1)?;
      if !v[0].is_finite() {
        return Err("value is not a finite number".to_owned());
      }
      Ok((v[0].round() as i64).to_string())
    }
    "bool" => {
      need(1)?;
      Ok((v[0] != 0.).to_string())
    }
    "color" => {
      need(3)?;
      fmt_vec3(v)
    }
    "select" => Ok(fmt_string(str_value()?)),
    "spline" => {
      let pts = v
        .chunks_exact(3)
        .map(fmt_vec3)
        .collect::<Result<Vec<_>, _>>()?;
      Ok(fmt_list(&pts, indent))
    }
    "ramp" => print_ramp(str_value()?, indent),
    "image_levels" => {
      need(5)?;
      Ok(format!(
        "{{ in_lo: {}, in_hi: {}, out_lo: {}, out_hi: {}, gamma: {} }}",
        fmt_float(v[0])?,
        fmt_float(v[1])?,
        fmt_float(v[2])?,
        fmt_float(v[3])?,
        fmt_float(v[4])?
      ))
    }
    other => Err(format!("unknown control kind `{other}`")),
  }
}

fn plan_edit(
  src: &str,
  pre: &Preprocessed,
  sites: &[Site],
  req: &InputDefaultRequest,
) -> Result<SourceEdit, String> {
  let named: Vec<&Site> = sites
    .iter()
    .filter(|s| s.name.as_deref() == Some(req.handle_id.as_str()))
    .collect();
  let site = match named.as_slice() {
    [s] => *s,
    [] if sites.iter().any(|s| s.name.is_none()) => {
      return Err(
        "no `input_*` call with this literal name in the node's source (computed names can't be \
         rewritten)"
          .to_owned(),
      )
    }
    [] => return Err("no `input_*` call with this name in the node's source".to_owned()),
    _ => {
      return Err(format!(
        "declared {} times in the node's source; edit the defaults by hand",
        named.len()
      ))
    }
  };
  if site.kind != req.kind {
    return Err(format!(
      "`{}` is a {} control but the stored value is {}; re-run the program and retry",
      site.callee, site.kind, req.kind
    ));
  }
  let indent = line_indent(src, pre.rewritten_to_original(site.call_start));
  let literal = print_value(req, indent)?;
  let existing = site
    .kwargs
    .iter()
    .find(|(k, _)| *k == "default")
    .map(|(_, s)| *s)
    .or_else(|| site.positional.get(site.default_ix).copied());
  let (from, to, insert) = match existing {
    Some((a, b)) => (a, b, literal),
    None => {
      let sep = if site.positional.is_empty() && site.kwargs.is_empty() {
        ""
      } else {
        ", "
      };
      (
        site.args_end,
        site.args_end,
        format!("{sep}default={literal}"),
      )
    }
  };
  let (from, to) = (
    pre.rewritten_to_original(from),
    pre.rewritten_to_original(to),
  );
  Ok(SourceEdit {
    from: utf16_offset(src, from),
    to: utf16_offset(src, to),
    insert,
  })
}

/// Plans one non-overlapping edit per distinct `handle_id` (first occurrence wins). Edits are
/// relative to the unmodified `src`, sorted ascending.
pub fn rewrite_input_defaults(src: &str, requests: &[InputDefaultRequest]) -> RewriteResult {
  let mut res = RewriteResult::default();
  let fail_all = |res: &mut RewriteResult, message: String| {
    for r in requests {
      res.errors.push(RewriteError {
        handle_id: r.handle_id.clone(),
        message: message.clone(),
      });
    }
  };
  let pre = match preprocess(src) {
    Ok(p) => p,
    Err(e) => {
      fail_all(&mut res, format!("syntax error: {}", e.message));
      return res;
    }
  };
  let program = match GSParser::parse(Rule::program, &pre.rewritten) {
    Ok(mut pairs) => pairs.next().unwrap(),
    Err(e) => {
      fail_all(&mut res, format!("syntax error: {e}"));
      return res;
    }
  };
  let mut sites = Vec::new();
  collect_sites(program, &mut sites);

  let mut seen = FxHashSet::default();
  for req in requests {
    if !seen.insert(req.handle_id.as_str()) {
      continue;
    }
    match plan_edit(src, &pre, &sites, req) {
      Ok(edit) => res.edits.push(edit),
      Err(message) => res.errors.push(RewriteError {
        handle_id: req.handle_id.clone(),
        message,
      }),
    }
  }
  res.edits.sort_by_key(|e| e.from);
  res
}

#[cfg(test)]
mod tests {
  use super::*;
  use geoscript::{parse_and_eval_program, Value};

  fn req(
    handle_id: &str,
    kind: &str,
    value: &[f32],
    str_value: Option<&str>,
  ) -> InputDefaultRequest {
    InputDefaultRequest {
      handle_id: handle_id.to_owned(),
      kind: kind.to_owned(),
      value: value.to_vec(),
      str_value: str_value.map(str::to_owned),
    }
  }

  /// Applies edits in UTF-16 space, exactly as the host would.
  fn apply(src: &str, res: &RewriteResult) -> String {
    assert!(res.errors.is_empty(), "{:?}", res.errors);
    let mut units: Vec<u16> = src.encode_utf16().collect();
    for e in res.edits.iter().rev() {
      units.splice(e.from..e.to, e.insert.encode_utf16());
    }
    String::from_utf16(&units).unwrap()
  }

  fn rewrite(src: &str, r: InputDefaultRequest) -> String {
    apply(src, &rewrite_input_defaults(src, &[r]))
  }

  fn only_error(src: &str, r: InputDefaultRequest) -> String {
    let res = rewrite_input_defaults(src, &[r]);
    assert!(res.edits.is_empty());
    assert_eq!(res.errors.len(), 1);
    res.errors[0].message.clone()
  }

  #[test]
  fn replaces_existing_default_kwarg_only() {
    let src = r#"r = input_float("radius", min=0, max=2, default=1.0, label="R") // tail
render(box(r))"#;
    assert_eq!(
      rewrite(src, req("radius", "float", &[0.35], None)),
      r#"r = input_float("radius", min=0, max=2, default=0.35, label="R") // tail
render(box(r))"#
    );
  }

  #[test]
  fn inserts_default_after_last_arg_or_replaces_positional() {
    assert_eq!(
      rewrite(
        r#"r = input_float("r", 0, 2)"#,
        req("r", "float", &[0.5], None)
      ),
      r#"r = input_float("r", 0, 2, default=0.5)"#
    );
    assert_eq!(
      rewrite(
        r#"r = input_float("r", 0, 2, 0.1, 0.7)"#,
        req("r", "float", &[0.5], None)
      ),
      r#"r = input_float("r", 0, 2, 0.1, 0.5)"#
    );
    assert_eq!(
      rewrite(
        r#"r = input_float(name="r")"#,
        req("r", "float", &[2.0], None)
      ),
      r#"r = input_float(name="r", default=2.0)"#
    );
  }

  #[test]
  fn multiline_call_with_trailing_comma_keeps_layout() {
    let src = "c = input_color(\n  \"tint\",\n  label=\"Tint\",\n)\n";
    assert_eq!(
      rewrite(src, req("tint", "color", &[1., 0.5, 0.25], None)),
      "c = input_color(\n  \"tint\",\n  label=\"Tint\", default=vec3(1.0, 0.5, 0.25),\n)\n"
    );
  }

  #[test]
  fn offsets_survive_preprocessor_rewrites() {
    // Shorthand closure bodies get `{ }` inserted before Pest runs; the site inside one must
    // still land on the original text.
    let src = "f = |x| x * input_float(\"k\", default=1.0)\ng = |y| y + 1\nr = f(2) + \
               g(input_int(\"n\", default=3))";
    assert_eq!(
      rewrite(src, req("k", "float", &[2.5], None)),
      "f = |x| x * input_float(\"k\", default=2.5)\ng = |y| y + 1\nr = f(2) + g(input_int(\"n\", \
       default=3))"
    );
    assert_eq!(
      rewrite(src, req("n", "int", &[7.], None)),
      "f = |x| x * input_float(\"k\", default=1.0)\ng = |y| y + 1\nr = f(2) + g(input_int(\"n\", \
       default=7))"
    );
    let src = "a = 1\n[0, 1] -> |i| i\nr = input_bool(\"on\")";
    assert_eq!(
      rewrite(src, req("on", "bool", &[1.], None)),
      "a = 1\n[0, 1] -> |i| i\nr = input_bool(\"on\", default=true)"
    );
  }

  #[test]
  fn edits_are_utf16_offsets() {
    let src = "// café ☕ 𝄞\nr = input_float(\"r\", default=1.0)";
    let res = rewrite_input_defaults(src, &[req("r", "float", &[3.], None)]);
    assert_eq!(
      apply(src, &res),
      "// café ☕ 𝄞\nr = input_float(\"r\", default=3.0)"
    );
    let prefix_units = "// café ☕ 𝄞\nr = input_float(\"r\", default="
      .encode_utf16()
      .count();
    assert_eq!(
      (res.edits[0].from, res.edits[0].to),
      (prefix_units, prefix_units + 3)
    );
  }

  #[test]
  fn refuses_ambiguous_or_mismatched_sites() {
    let dynamic = r#"xs = 0..3 -> |i| input_float("amp" + str(i))"#;
    assert!(only_error(dynamic, req("amp0", "float", &[1.], None)).contains("computed names"));
    let dup = "a = input_float(\"k\")\nb = input_float(\"k\")";
    assert!(only_error(dup, req("k", "float", &[1.], None)).contains("declared 2 times"));
    let missing = r#"a = input_float("k")"#;
    assert!(only_error(missing, req("z", "float", &[1.], None)).contains("no `input_*` call"));
    let changed = r#"a = input_int("k")"#;
    assert!(only_error(changed, req("k", "float", &[1.], None)).contains("is a int control"));
    let broken = r#"a = input_float("k"#;
    assert!(only_error(broken, req("k", "float", &[1.], None)).starts_with("syntax error"));
    assert!(only_error(missing, req("k", "float", &[f32::NAN], None)).contains("finite"));
  }

  #[test]
  fn prints_every_kind() {
    let float = |v: f32| rewrite(r#"x = input_float("x")"#, req("x", "float", &[v], None));
    assert_eq!(float(1.9999999), r#"x = input_float("x", default=2.0)"#);
    assert_eq!(float(0.1), r#"x = input_float("x", default=0.1)"#);
    assert_eq!(float(-0.5), r#"x = input_float("x", default=-0.5)"#);
    assert_eq!(float(100.), r#"x = input_float("x", default=100.0)"#);
    assert_eq!(
      float(0.5019608),
      r#"x = input_float("x", default=0.501961)"#
    );
    assert_eq!(
      rewrite(r#"n = input_int("n", 0, 9)"#, req("n", "int", &[4.6], None)),
      r#"n = input_int("n", 0, 9, default=5)"#
    );
    assert_eq!(
      rewrite(
        r#"m = input_select("m", ["a", "b\"c"])"#,
        req("m", "select", &[], Some("b\"c"))
      ),
      r#"m = input_select("m", ["a", "b\"c"], default="b\"c")"#
    );
    assert_eq!(
      rewrite(
        r#"p = input_spline("p")"#,
        req("p", "spline", &[0., 0., 0., 1., 2., 3.], None)
      ),
      r#"p = input_spline("p", default=[vec3(0.0, 0.0, 0.0), vec3(1.0, 2.0, 3.0)])"#
    );
    assert_eq!(
      rewrite(
        "  p = input_spline(\"p\")",
        req("p", "spline", &[0., 0., 0., 1., 0., 0., 2., 0., 0., 3., 0., 0.], None)
      ),
      "  p = input_spline(\"p\", default=[\n    vec3(0.0, 0.0, 0.0),\n    vec3(1.0, 0.0, 0.0),\n    vec3(2.0, 0.0, 0.0),\n    vec3(3.0, 0.0, 0.0),\n  ])"
    );
    assert_eq!(
      rewrite(
        r#"t = input_image_levels("lv", tex)"#,
        req("lv", "image_levels", &[0.1, 0.9, 0., 1., 2.2], None)
      ),
      r#"t = input_image_levels("lv", tex, default={ in_lo: 0.1, in_hi: 0.9, out_lo: 0.0, out_hi: 1.0, gamma: 2.2 })"#
    );
  }

  #[test]
  fn ramps_elide_defaults_and_spell_out_deviations() {
    let scalar = r#"{"scalar":true,"stops":[{"pos":0,"value":[0],"ease":"linear"},{"pos":1,"value":[1],"ease":"smooth"}],"extend":"clamp","space":"linear"}"#;
    assert_eq!(
      rewrite(
        r#"r = input_ramp("r", [0, 1])"#,
        req("r", "ramp", &[], Some(scalar))
      ),
      r#"r = input_ramp("r", [[0.0, 0.0], [1.0, 1.0, "smooth"]])"#
    );
    let repeating = scalar.replace("\"clamp\"", "\"repeat\"");
    assert_eq!(
      rewrite(
        r#"r = input_ramp("r", default=[0, 1])"#,
        req("r", "ramp", &[], Some(&repeating))
      ),
      r#"r = input_ramp("r", default=ramp(stops=[[0.0, 0.0], [1.0, 1.0, "smooth"]], extend="repeat"))"#
    );
    let color = r#"{"scalar":false,"stops":[{"pos":0,"value":[1,0,0],"ease":"linear"},{"pos":0.5,"value":[0,1,0],"ease":"linear"},{"pos":1,"value":[0,0,1],"ease":"step"}],"extend":"mirror","space":"oklch"}"#;
    assert_eq!(
      rewrite(
        r#"c = input_color_ramp("c", default=[vec3(1, 0, 0), vec3(0, 0, 1)])"#,
        req("c", "ramp", &[], Some(color))
      ),
      r#"c = input_color_ramp("c", default=color_ramp(stops=[[0.0, vec3(1.0, 0.0, 0.0)], [0.5, vec3(0.0, 1.0, 0.0)], [1.0, vec3(0.0, 0.0, 1.0), "step"]], extend="mirror", space="oklch"))"#
    );
  }

  #[test]
  fn baked_defaults_evaluate_to_the_stored_values() {
    let src = "r = input_float(\"r\", min=0, max=10)\nm = input_select(\"m\", [\"a\", \"b\"])\nc \
               = input_color(\"c\")\ng = input_color_ramp(\"g\", [vec3(0, 0, 0), vec3(1, 1, \
               1)])\nrender(box(r))";
    let color = r#"{"scalar":false,"stops":[{"pos":0,"value":[0.2,0.3,0.4],"ease":"linear"},{"pos":1,"value":[1,1,1],"ease":"smooth"}],"extend":"clamp","space":"oklab"}"#;
    let res = rewrite_input_defaults(
      src,
      &[
        req("r", "float", &[4.25], None),
        req("m", "select", &[], Some("b")),
        req("c", "color", &[0.25, 0.5, 0.75], None),
        req("g", "ramp", &[], Some(color)),
      ],
    );
    let baked = apply(src, &res);
    let ctx = parse_and_eval_program(baked.clone()).unwrap_or_else(|e| panic!("{e}\n{baked}"));
    let controls = ctx.rendered_controls.inner.borrow();
    let find = |id: &str| controls.iter().find(|c| c.handle_id == id).unwrap();
    assert_eq!(find("r").current_value.as_float(), Some(4.25));
    assert!(matches!(&find("m").current_value, Value::String(s) if s == "b"));
    let c = match &find("c").current_value {
      Value::Vec3(v) => [v.x, v.y, v.z],
      other => panic!("{other:?}"),
    };
    assert_eq!(c, [0.25, 0.5, 0.75]);
    let g = geoscript::ramp_control_value_json(&find("g").current_value).unwrap();
    let g = RampSpecWire::deserialize_json(&g).unwrap();
    assert_eq!(
      (g.stops.len(), g.stops[1].ease.as_str(), g.space.as_str()),
      (2, "smooth", "oklab")
    );
    assert_eq!(g.stops[0].value, vec![0.2, 0.3, 0.4]);
  }
}
