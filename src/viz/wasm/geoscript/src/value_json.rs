//! Serializes runtime `Value`s to a tagged JSON representation for `geotoy eval`.
//!
//! Every value becomes `{"t": <type-tag>, ...}`. Scalars/vectors/maps map to their
//! natural JSON; sequences are materialized under a hard cap (lazy/infinite-safe);
//! meshes serialize as a lightweight reference (full geometry is exported separately);
//! callables can be sampled over t∈[0,1].

use nanoserde::SerJson;

use crate::{materials::Material, EvalCtx, Value, EMPTY_KWARGS};

const MAX_SEQ_ITEMS: usize = 4096;
const MAX_DEPTH: usize = 8;

pub fn serialize_value_to_json(ctx: &EvalCtx, val: &Value, sample_count: usize) -> String {
  let mut out = String::new();
  write_value(&mut out, ctx, val, sample_count, 0);
  out
}

/// Serialize a name→value map (e.g. a module's exports) as a JSON object of tagged values.
pub fn serialize_bindings_to_json(ctx: &EvalCtx, bindings: &[(String, Value)], sample_count: usize) -> String {
  let mut out = String::from("{");
  for (i, (name, val)) in bindings.iter().enumerate() {
    if i > 0 {
      out.push(',');
    }
    write_json_string(&mut out, name);
    out.push(':');
    write_value(&mut out, ctx, val, sample_count, 0);
  }
  out.push('}');
  out
}

fn write_value(out: &mut String, ctx: &EvalCtx, val: &Value, sample_count: usize, depth: usize) {
  match val {
    Value::Nil => out.push_str("{\"t\":\"nil\"}"),
    Value::Int(i) => {
      out.push_str("{\"t\":\"int\",\"v\":");
      out.push_str(&i.to_string());
      out.push('}');
    }
    Value::Float(f) => {
      out.push_str("{\"t\":\"float\",\"v\":");
      write_f32(out, *f);
      out.push('}');
    }
    Value::Bool(b) => {
      out.push_str(if *b {
        "{\"t\":\"bool\",\"v\":true}"
      } else {
        "{\"t\":\"bool\",\"v\":false}"
      });
    }
    Value::String(s) => {
      out.push_str("{\"t\":\"string\",\"v\":");
      write_json_string(out, s);
      out.push('}');
    }
    Value::Vec2(v) => {
      out.push_str("{\"t\":\"vec2\",\"v\":[");
      write_f32(out, v.x);
      out.push(',');
      write_f32(out, v.y);
      out.push_str("]}");
    }
    Value::Vec3(v) => {
      out.push_str("{\"t\":\"vec3\",\"v\":[");
      write_f32(out, v.x);
      out.push(',');
      write_f32(out, v.y);
      out.push(',');
      write_f32(out, v.z);
      out.push_str("]}");
    }
    Value::Mat4(m) => {
      out.push_str("{\"t\":\"mat4\",\"v\":[");
      for (i, e) in m.as_slice().iter().enumerate() {
        if i > 0 {
          out.push(',');
        }
        write_f32(out, *e);
      }
      out.push_str("]}");
    }
    Value::Map(map) => {
      if depth >= MAX_DEPTH {
        out.push_str("{\"t\":\"map\",\"truncated\":true}");
        return;
      }
      out.push_str("{\"t\":\"map\",\"v\":{");
      for (i, (k, v)) in map.iter().enumerate() {
        if i > 0 {
          out.push(',');
        }
        write_json_string(out, k);
        out.push(':');
        write_value(out, ctx, v, sample_count, depth + 1);
      }
      out.push_str("}}");
    }
    Value::Sequence(seq) => {
      if depth >= MAX_DEPTH {
        out.push_str("{\"t\":\"seq\",\"truncated\":true}");
        return;
      }
      out.push_str("{\"t\":\"seq\",\"v\":[");
      let mut count = 0usize;
      let mut truncated = false;
      let mut err: Option<String> = None;
      for item in seq.consume(ctx) {
        if count >= MAX_SEQ_ITEMS {
          truncated = true;
          break;
        }
        match item {
          Ok(v) => {
            if count > 0 {
              out.push(',');
            }
            write_value(out, ctx, &v, sample_count, depth + 1);
            count += 1;
          }
          Err(e) => {
            err = Some(format!("{e}"));
            break;
          }
        }
      }
      out.push_str("],\"len\":");
      out.push_str(&count.to_string());
      if truncated {
        out.push_str(",\"truncated\":true");
      }
      if let Some(e) = err {
        out.push_str(",\"error\":");
        write_json_string(out, &e);
      }
      out.push('}');
    }
    Value::Mesh(m) => {
      out.push_str("{\"t\":\"mesh\",\"vertices\":");
      out.push_str(&m.mesh.vertices.len().to_string());
      out.push_str(",\"faces\":");
      out.push_str(&m.mesh.faces.len().to_string());
      out.push('}');
    }
    Value::Material(mat) => {
      let Material::External(name) = &**mat;
      out.push_str("{\"t\":\"material\",\"name\":");
      write_json_string(out, name);
      out.push('}');
    }
    Value::Light(light) => {
      out.push_str("{\"t\":\"light\",\"v\":");
      out.push_str(&SerJson::serialize_json(light.as_ref()));
      out.push('}');
    }
    Value::Callable(c) => {
      out.push_str("{\"t\":\"callable\"");
      if sample_count > 0 && depth < MAX_DEPTH {
        out.push_str(",\"samples\":[");
        for i in 0..sample_count {
          if i > 0 {
            out.push(',');
          }
          let t = if sample_count == 1 { 0. } else { i as f32 / (sample_count as f32 - 1.) };
          out.push_str("{\"t_in\":");
          write_f32(out, t);
          match ctx.invoke_callable(c, &[Value::Float(t)], EMPTY_KWARGS) {
            Ok(v) => {
              out.push_str(",\"out\":");
              write_value(out, ctx, &v, 0, depth + 1);
            }
            Err(e) => {
              out.push_str(",\"error\":");
              write_json_string(out, &format!("{e}"));
            }
          }
          out.push('}');
        }
        out.push(']');
      }
      out.push('}');
    }
  }
}

/// JSON has no NaN/Infinity — emit those as string tokens so bad values stay visible.
fn write_f32(out: &mut String, f: f32) {
  if f.is_finite() {
    out.push_str(&f.to_string());
  } else if f.is_nan() {
    out.push_str("\"NaN\"");
  } else if f > 0. {
    out.push_str("\"Infinity\"");
  } else {
    out.push_str("\"-Infinity\"");
  }
}

fn write_json_string(out: &mut String, s: &str) {
  out.push('"');
  for c in s.chars() {
    match c {
      '"' => out.push_str("\\\""),
      '\\' => out.push_str("\\\\"),
      '\n' => out.push_str("\\n"),
      '\r' => out.push_str("\\r"),
      '\t' => out.push_str("\\t"),
      c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
      c => out.push(c),
    }
  }
  out.push('"');
}
