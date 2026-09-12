use crate::ValueMap;
use std::{f32::consts::PI, ptr::addr_of, rc::Rc};

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;
use nanoserde::SerJson;

use crate::{
  builtins::FUNCTION_ALIASES,
  lights::{AmbientLight, DirectionalLight, HemisphereLight, RectAreaLight},
  ArgType, Sym, Value, Vec2,
};

pub enum DefaultValue {
  Required,
  Optional(fn() -> Value),
}

#[derive(SerJson)]
pub enum SerializableDefaultValue {
  Required,
  Optional(String),
}

pub struct ArgDef {
  pub name: &'static str,
  /// This will be populated lazily
  pub interned_name: Sym,
  /// Bitflags
  pub valid_types: u32,
  pub default_value: DefaultValue,
  pub description: &'static str,
}

#[derive(SerJson)]
pub struct SerializableArgDef {
  pub name: &'static str,
  pub valid_types: Vec<ArgType>,
  pub default_value: SerializableDefaultValue,
  pub description: &'static str,
}

impl From<&ArgDef> for SerializableArgDef {
  fn from(arg_def: &ArgDef) -> Self {
    Self {
      name: arg_def.name,
      valid_types: ArgType::list_from_bitflags(arg_def.valid_types),
      default_value: match arg_def.default_value {
        DefaultValue::Required => SerializableDefaultValue::Required,
        DefaultValue::Optional(get_default) => {
          SerializableDefaultValue::Optional(format!("{:?}", get_default()))
        }
      },
      description: arg_def.description,
    }
  }
}

#[derive(Clone, SerJson)]
pub struct FnExample {
  pub composition_id: usize,
}

pub struct FnDef {
  pub module: &'static str,
  pub examples: &'static [FnExample],
  pub signatures: &'static [FnSignature],
}

#[derive(SerJson)]
pub struct SerializableFnDef {
  pub module: &'static str,
  pub examples: &'static [FnExample],
  pub signatures: Vec<SerializableFnSignature>,
  pub aliases: Vec<&'static str>,
}

impl SerializableFnDef {
  pub fn new(name: &str, def: &FnDef) -> Self {
    Self {
      module: def.module,
      examples: def.examples,
      signatures: SerializableFnSignature::new(def.signatures),
      aliases: FUNCTION_ALIASES
        .entries()
        .filter_map(
          |(&alias, &fn_name)| {
            if fn_name == name {
              Some(alias)
            } else {
              None
            }
          },
        )
        .collect(),
    }
  }
}

pub struct FnSignature {
  pub arg_defs: &'static [ArgDef],
  pub description: &'static str,
  pub return_type: &'static [ArgType],
}

#[derive(SerJson)]
pub struct SerializableFnSignature {
  pub arg_defs: Vec<SerializableArgDef>,
  pub description: &'static str,
  pub return_type: &'static [ArgType],
}

impl SerializableFnSignature {
  fn new(defs: &[FnSignature]) -> Vec<Self> {
    defs
      .iter()
      .map(|def| Self {
        arg_defs: def.arg_defs.iter().map(SerializableArgDef::from).collect(),
        description: def.description,
        return_type: def.return_type,
      })
      .collect()
  }
}

macro_rules! argtype_flags {
  ( $( $x:expr ),* ) => {
    {
      let mut flags = 0;
      $(
        flags |= $x.as_bitflags();
      )*
      flags
    }
  };
}

const CONCAT_CHANNELS_DESC: &str =
  "Joins the channels of several same-sized textures into one texture, in argument order (numpy \
   `concatenate(axis=-1)`).  A numeric argument contributes one channel filled with that \
   constant.  At least one argument must be a texture; it fixes the output dims, wrap mode, and \
   placement transform.  Total channels must be <= 4.\n\nThe idiom for compositing with a mask \
   sourced from an unrelated texture: `concat_channels(rgb, mask)` builds an RGBA stamp for \
   `blit`.";

const CONCAT_CHANNELS_ARG_DESC: &str =
  "Texture whose channels are appended in order, or a constant filling a single channel";

macro_rules! concat_channels_arg {
  ($name:literal) => {
    ArgDef {
      name: $name,
      interned_name: Sym(0),
      valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
      default_value: DefaultValue::Required,
      description: CONCAT_CHANNELS_ARG_DESC,
    }
  };
}

const MORPH_RADIUS_DESC: &str = "Box structuring-element radius in pixels; the window is \
                                 (2r+1)x(2r+1).  <= 0 is a no-op for `morph_open`/`morph_close` \
                                 and yields an all-zero result for the difference forms.";

macro_rules! morph_fn_def {
  ($desc:literal) => {
    morph_fn_def!($desc, MORPH_RADIUS_DESC)
  };
  ($desc:literal, $radius_desc:expr) => {
    FnDef {
      module: "texture",
      examples: &[],
      signatures: &[FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: $radius_desc,
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "",
          },
        ],
        description: $desc,
        return_type: &[ArgType::Texture],
      }],
    }
  };
}

pub(crate) static mut FN_SIGNATURE_DEFS: phf::Map<&'static str, FnDef> = phf::phf_map! {
  "box" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Width along the X axis"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Height along the Y axis"
          },
          ArgDef {
            name: "depth",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Depth along the Z axis"
          },
        ],
        description: "Creates a rectangular prism mesh with the specified width, height, and depth",
        return_type: &[ArgType::Mesh],
      },
      // TODO: this should be split into two variants
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "size",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(1., 1., 1.))),
            description: "Size of the box as a Vec3, or a single numeric value for a cube"
          },
        ],
        description: "Creates a box using a uniform or vector size",
        return_type: &[ArgType::Mesh],
      },
    ]
  },
  "translate" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 33 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "translation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a mesh in local space (right-multiply: M = M * T). The translation is relative to the object's current orientation. Alias: `trans`.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a mesh in local space (right-multiply: M = M * T). Alias: `trans`.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "translation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a light in local space (right-multiply: M = M * T). The translation is relative to the light's current orientation. Alias: `trans`.",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a light in local space (right-multiply: M = M * T). Alias: `trans`.",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "translation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a translation onto a transform matrix in local space (right-multiply: M = M * T). Alias: `trans`.",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a translation onto a transform matrix in local space (right-multiply: M = M * T). Alias: `trans`.",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a texture's placement transform in base-UV units (right-multiply: M = M * T). Alias: `trans`.",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a texture's placement transform in base-UV units (right-multiply: M = M * T). Alias: `trans`.",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Translation."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Translates a path.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X translation."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y translation."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Translates a path.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "translate_global" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "translation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a mesh in world space (left-multiply: M = T * M). The translation is always along world axes regardless of the object's current orientation. Alias: `trans_global`.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a mesh in world space (left-multiply: M = T * M). Alias: `trans_global`.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "translation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a light in world space (left-multiply: M = T * M). The translation is always along world axes regardless of the light's current orientation. Alias: `trans_global`.",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a light in world space (left-multiply: M = T * M). Alias: `trans_global`.",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "translation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a translation onto a transform matrix in world space (left-multiply: M = T * M). Alias: `trans_global`.",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a translation onto a transform matrix in world space (left-multiply: M = T * M). Alias: `trans_global`.",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a texture's placement in base-UV units, unaffected by its current rotation/scale (left-multiply: M = T * M). The idiomatic final placement op: `stamp | scale(s) | rot(a) | trans_global(pos)`. Alias: `trans_global`.",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a texture's placement in base-UV units, unaffected by its current rotation/scale (left-multiply: M = T * M). Alias: `trans_global`.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "rot" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Rotation defined by Euler angles in radians"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to rotate"
          },
        ],
        description: "Rotates a mesh in local space using a Vec3 of Euler angles in radians (right-multiply: M = M * R). The rotation is relative to the object's current orientation.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about X axis (radians)"
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Y axis (radians)"
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Z axis (radians)"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to rotate"
          },
        ],
        description: "Rotates a mesh in local space using individual Euler angle components in radians (right-multiply: M = M * R).",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Rotation defined by Euler angles in radians"
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: "Light to rotate"
          },
        ],
        description: "Rotates a light in local space using a Vec3 of Euler angles in radians (right-multiply: M = M * R).",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about X axis (radians)"
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Y axis (radians)"
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Z axis (radians)"
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: "Light to rotate"
          },
        ],
        description: "Rotates a light in local space using individual Euler angle components in radians (right-multiply: M = M * R).",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a rotation onto a transform matrix in local space using a Vec3 of Euler angles in radians (right-multiply: M = M * R).",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a rotation onto a transform matrix in local space using individual Euler angle components in radians (right-multiply: M = M * R).",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "rotation", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "Euler angles in radians." },
          ArgDef { name: "point", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a 3D point around the origin by a Vec3 of Euler angles in radians.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "x", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "y", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "z", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "point", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a 3D point around the origin by individual Euler angle components in radians.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "angle", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "Rotation angle in radians." },
          ArgDef { name: "point", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec2), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a 2D point counter-clockwise around the origin.  There is only one rotation axis in 2D, so this takes a single angle in radians.",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "angle",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation angle in radians."
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rotates a texture's placement transform in the UV plane (right-multiply: M = M * R; local ops act about the texture's centered origin).  Counter-clockwise, matching the 2D point `rot` convention.",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "angle",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Angle in radians."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Rotates a path counter-clockwise about the origin.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "rot_global" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Rotation defined by Euler angles in radians"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to rotate"
          },
        ],
        description: "Rotates a mesh in world space around the world origin using a Vec3 of Euler angles in radians (left-multiply: M = R * M). This rotates both the object's orientation and its position around the origin.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about X axis (radians)"
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Y axis (radians)"
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Z axis (radians)"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to rotate"
          },
        ],
        description: "Rotates a mesh in world space around the world origin using individual Euler angle components in radians (left-multiply: M = R * M).",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Rotation defined by Euler angles in radians"
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: "Light to rotate"
          },
        ],
        description: "Rotates a light in world space around the world origin using a Vec3 of Euler angles in radians (left-multiply: M = R * M).",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about X axis (radians)"
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Y axis (radians)"
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Z axis (radians)"
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: "Light to rotate"
          },
        ],
        description: "Rotates a light in world space around the world origin using individual Euler angle components in radians (left-multiply: M = R * M).",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a rotation onto a transform matrix in world space around the world origin using a Vec3 of Euler angles in radians (left-multiply: M = R * M).",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a rotation onto a transform matrix in world space around the world origin using individual Euler angle components in radians (left-multiply: M = R * M).",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "rotation", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "Euler angles in radians." },
          ArgDef { name: "point", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a 3D point around the origin by a Vec3 of Euler angles in radians.  A bare point has no frame of its own, so this matches `rot`.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "x", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "y", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "z", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "point", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a 3D point around the origin by individual Euler angle components in radians.  A bare point has no frame of its own, so this matches `rot`.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "angle", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "Rotation angle in radians." },
          ArgDef { name: "point", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec2), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a 2D point counter-clockwise around the origin.  There is only one rotation axis in 2D, so this takes a single angle in radians.  A bare point has no frame of its own, so this matches `rot`.",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "angle",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation angle in radians."
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rotates a texture's placement in the UV plane about the base-UV origin (left-multiply: M = R * M).  Counter-clockwise, matching the 2D point `rot` convention.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "rot_around_center" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Rotation defined by Euler angles in radians"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to rotate"
          },
        ],
        description: "Rotates a mesh around its current position (the translation component of its transform matrix). The object spins in place without changing its world-space position. Equivalent to: translate to origin, rotate, translate back.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about X axis (radians)"
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Y axis (radians)"
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Z axis (radians)"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to rotate"
          },
        ],
        description: "Rotates a mesh around its current position using individual Euler angle components in radians.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Rotation defined by Euler angles in radians"
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: "Light to rotate"
          },
        ],
        description: "Rotates a light around its current position using a Vec3 of Euler angles in radians.",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about X axis (radians)"
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Y axis (radians)"
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation about Z axis (radians)"
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: "Light to rotate"
          },
        ],
        description: "Rotates a light around its current position using individual Euler angle components in radians.",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rotates a transform matrix around the position it encodes (the translation component), using a Vec3 of Euler angles in radians.",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rotates a transform matrix around the position it encodes (the translation component), using individual Euler angle components in radians.",
        return_type: &[ArgType::Mat4],
      },
    ],
  },
  "rot_axis" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "axis", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "Axis to rotate around (auto-normalized; must be non-zero)." },
          ArgDef { name: "angle", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "Rotation angle in radians." },
          ArgDef { name: "mesh", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mesh), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a mesh around an arbitrary local axis by `angle` radians (right-multiply: M = M * R).",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "axis", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "Axis to rotate around (auto-normalized; must be non-zero)." },
          ArgDef { name: "angle", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "Rotation angle in radians." },
          ArgDef { name: "light", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Light), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a light around an arbitrary local axis by `angle` radians (right-multiply: M = M * R).",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "axis", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "Axis to rotate around (auto-normalized; must be non-zero)." },
          ArgDef { name: "angle", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "Rotation angle in radians." },
          ArgDef { name: "transform", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mat4), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a transform matrix around an arbitrary local axis by `angle` radians (right-multiply: M = M * R).",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "axis", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "Axis to rotate around (auto-normalized; must be non-zero)." },
          ArgDef { name: "angle", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "Rotation angle in radians." },
          ArgDef { name: "point", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Rotates a 3D point around an arbitrary axis through the origin by `angle` radians.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "look_at" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Source position (the eye)"
          },
          ArgDef {
            name: "target",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Target position to look at"
          },
          ArgDef {
            name: "up",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 1., 0.))),
            description: "Up hint used to control roll"
          },
        ],
        description: "Euler angles (radians, XYZ) for an orientation whose local -Z points from `pos` toward `target`, with local +Y aligned to `up`. Feed the result to `rot`.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "target",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Target position to look at"
          },
          ArgDef {
            name: "up",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 1., 0.))),
            description: "Up hint used to control roll"
          },
        ],
        description: "Orients a mesh so its local -Z points at `target`, with `up` controlling roll. Replaces the current rotation while preserving position and scale.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "target",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Target position to look at"
          },
          ArgDef {
            name: "up",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 1., 0.))),
            description: "Up hint used to control roll"
          },
        ],
        description: "Orients a light so its local -Z (emission direction) points at `target`, with `up` controlling roll. Replaces the current rotation while preserving position.",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "target",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Target position to look at"
          },
          ArgDef {
            name: "up",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 1., 0.))),
            description: "Up hint used to control roll"
          },
        ],
        description: "Orients a transform matrix so its local -Z points at `target`, with `up` controlling roll. Replaces the current rotation while preserving position and scale.",
        return_type: &[ArgType::Mat4],
      }
    ],
  },
  "align" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "World-space direction to point the `from` axis along (auto-normalized)"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "from",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 0., -1.))),
            description: "Local-space axis to align (auto-normalized); defaults to -Z"
          },
          ArgDef {
            name: "up_from",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional secondary local axis for roll control, rolled toward `up_to` (best-effort, projected perpendicular to `from`). Must be paired with `up_to`."
          },
          ArgDef {
            name: "up_to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "World-space target for `up_from`. Must be paired with `up_from`."
          },
        ],
        description: "Rotates a mesh so its local `from` axis points along world-space direction `to`. Without `up_from`/`up_to` this is the minimal rotation (no roll control); supplying them additionally rolls about `to` so `up_from` points toward `up_to`. Replaces the current rotation while preserving position and scale.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "World-space direction to point the `from` axis along (auto-normalized)"
          },
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "from",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 0., -1.))),
            description: "Local-space axis to align (auto-normalized); defaults to -Z"
          },
          ArgDef {
            name: "up_from",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional secondary local axis for roll control, rolled toward `up_to` (best-effort, projected perpendicular to `from`). Must be paired with `up_to`."
          },
          ArgDef {
            name: "up_to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "World-space target for `up_from`. Must be paired with `up_from`."
          },
        ],
        description: "Rotates a light so its local `from` axis points along world-space direction `to`. Without `up_from`/`up_to` this is the minimal rotation (no roll control); supplying them additionally rolls about `to` so `up_from` points toward `up_to`. Replaces the current rotation while preserving position.",
        return_type: &[ArgType::Light],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "World-space direction to point the `from` axis along (auto-normalized)"
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "from",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 0., -1.))),
            description: "Local-space axis to align (auto-normalized); defaults to -Z"
          },
          ArgDef {
            name: "up_from",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional secondary local axis for roll control, rolled toward `up_to` (best-effort, projected perpendicular to `from`). Must be paired with `up_to`."
          },
          ArgDef {
            name: "up_to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "World-space target for `up_from`. Must be paired with `up_from`."
          },
        ],
        description: "Rotates a transform matrix so its local `from` axis points along world-space direction `to`. Without `up_from`/`up_to` this is the minimal rotation (no roll control); supplying them additionally rolls about `to` so `up_from` points toward `up_to`. Replaces the current rotation while preserving position and scale.",
        return_type: &[ArgType::Mat4],
      },
    ],
  },
  "scale" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Scales a mesh in local space by separate factors along each axis (right-multiply: M = M * S).",
        return_type: &[ArgType::Mesh],
      },
      // TODO: this should be split into two variants
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Scales a mesh in local space (right-multiply: M = M * S). Accepts a Vec3 for non-uniform scaling or a number for uniform scaling.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a non-uniform scale onto a transform matrix in local space (right-multiply: M = M * S).",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transform",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes a scale onto a transform matrix in local space (right-multiply: M = M * S). Accepts a Vec3 for non-uniform scaling or a number for uniform scaling.",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Scales a texture's placement transform (right-multiply: M = M * S; local ops act about the texture's centered origin). Accepts a Vec2 for non-uniform scaling or a number for uniform scaling.",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Uniform factor, or per-axis factors as a `vec2`."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Scales a path about the origin. A non-uniform scale converts arcs to cubic Béziers.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X factor."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y factor."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Scales a path about the origin. A non-uniform scale converts arcs to cubic Béziers.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "origin_to_geometry" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Moves the mesh so that its origin is at the center of its geometry (averge of all its vertices), returning a new mesh.\n\nThis will actually modify the vertex positions and preserve any existing transforms.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "Path to center."
          },
        ],
        description: "Moves a 2D path so that its origin is at its centroid (signed-area centroid of the filled region when any subpath is closed, arc-length centroid otherwise), returning a new path. Preserves any existing transforms.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "apply_transforms" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies rotation, translation, and scale transforms to the vertices of a mesh, resetting the transforms to identity",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "Path whose transform will be baked into its geometry."
          },
        ],
        description: "Bakes the 2D affine transform into the path's segment control points, resetting the transform to identity. Only works with trace_path/trace_svg_path paths.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "meshes",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of meshes whose transforms will be baked into their vertices."
          },
        ],
        description: "Applies transforms to each mesh in a sequence, resetting each transform to identity.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "apply_mat4" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "m00", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m01", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m02", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m03", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m10", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m11", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m12", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m13", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m20", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m21", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m22", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m23", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m30", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m31", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m32", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m33", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "mesh", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mesh), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Sets the transform of a mesh to an explicit 4x4 matrix (row-major order)",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef { name: "matrix", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mat4), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "mesh", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mesh), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Sets the transform of a mesh to the given `mat4`",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "mat4" => FnDef {
    module: "math",
    examples: &[],
    // The zero-arg identity signature must stay LAST: `get_args` ignores extra positional args, so
    // a leading empty signature would match every call and shadow the explicit form.
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "m00", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m01", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m02", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m03", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m10", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m11", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m12", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m13", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m20", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m21", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m22", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m23", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m30", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m31", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m32", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "m33", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Builds a transform matrix from 16 explicit elements in row-major order.  Transforms are treated as affine throughout: applying one to a point does not divide by `w`, so projective/perspective matrices are not supported.",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[],
        description: "Returns the identity transform matrix.  Use it as the seed for a transform chain: `mat4() | trans(v3(2, 0, 0)) | rot(v3(0, pi/2, 0))`.",
        return_type: &[ArgType::Mat4],
      },
    ],
  },
  "transform_point" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "transform", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mat4), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "point", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Transforms a 3D point by a transform matrix, treating it as a position (`w = 1`) so translation applies.  Equivalent to `transform * point`.  Assumes an affine matrix; there is no perspective divide.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "transform_dir" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "transform", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mat4), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "dir", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Transforms a 3D direction by a transform matrix, treating it as a vector (`w = 0`) so translation is ignored.  Rotation and scale still apply, so the result is not normalized.  For normals under non-uniform scale use `transform_normal`.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "transform_normal" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "transform", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mat4), default_value: DefaultValue::Required, description: "" },
          ArgDef { name: "normal", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Transforms a surface normal by a transform matrix using the inverse transpose, so normals stay perpendicular to the surface under non-uniform scale.  The result is normalized.  Errors if the matrix is singular or the normal is degenerate.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "compose_transforms" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "transforms", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Sequence), default_value: DefaultValue::Required, description: "Sequence of `mat4`s to multiply together, left to right." },
        ],
        description: "Multiplies a sequence of transform matrices together: `compose_transforms([a, b, c])` is `a * b * c`.  Matrix multiplication is not commutative — the LAST transform in the sequence is applied to a point FIRST.  An empty sequence returns the identity.",
        return_type: &[ArgType::Mat4],
      },
    ],
  },
  "inverse" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "transform", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mat4), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Returns the inverse of a transform matrix, which undoes it.  Errors if the matrix is singular (e.g. it has a zero scale on some axis).",
        return_type: &[ArgType::Mat4],
      },
    ],
  },
  "transpose" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "transform", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mat4), default_value: DefaultValue::Required, description: "" },
        ],
        description: "Returns the transpose of a transform matrix (rows and columns swapped).",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Swaps a texture's axes (`out[x, y] = in[y, x]`, so a WxH input yields HxW).  O(1): returns a view of the same pixel data.  Same as `texture_transpose`.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "flip_normals" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Flips the normals of a mesh, returning a new mesh with inverted normals.  This actually flips the winding order of the mesh's triangles under the hood and re-computes normals based off that.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "reflect" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "normal",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Normal of the mirror plane (need not be normalized)."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a mesh across the plane through the origin with the given `normal`, returning a new mesh.  Vertex positions and normals are reflected and triangle winding is reversed so the result stays consistently oriented (rather than turning inside-out).  Operates in the mesh's local space.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "normal",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Normal of the mirror plane (need not be normalized)."
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Signed distance of the mirror plane from the origin, measured along the unit `normal`."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a mesh across the plane with the given `normal`, shifted from the origin by `offset` along that normal, returning a new mesh.  Vertex positions and normals are reflected and triangle winding is reversed so the result stays consistently oriented.  Operates in the mesh's local space.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "axis",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Direction of the mirror line, which passes through the origin."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Reflects a path across the line through the origin running along `axis`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "axis",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Direction of the mirror line."
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Distance of the mirror line from the origin along the line's normal."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Reflects a path across the line running along `axis`, shifted by `offset` along the line's normal.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "reflect_x" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a mesh across the `x = 0` plane (negating x), returning a new mesh with winding reversed to stay consistently oriented.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X position of the mirror plane."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a mesh across the plane `x = offset`, returning a new mesh with winding reversed to stay consistently oriented.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Reflects a path across the x axis (negates y).",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Mirror over the line `y = offset` instead."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Reflects a path across the line `y = offset`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "reflect_y" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a mesh across the `y = 0` plane (negating y), returning a new mesh with winding reversed to stay consistently oriented.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y position of the mirror plane."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a mesh across the plane `y = offset`, returning a new mesh with winding reversed to stay consistently oriented.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Reflects a path across the y axis (negates x).",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Mirror over the line `x = offset` instead."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Reflects a path across the line `x = offset`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "reflect_z" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a mesh across the `z = 0` plane (negating z), returning a new mesh with winding reversed to stay consistently oriented.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Z position of the mirror plane."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a mesh across the plane `z = offset`, returning a new mesh with winding reversed to stay consistently oriented.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "partition_faces" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "predicate",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Called once per triangle with its three vertex positions `|v0: vec3, v1: vec3, v2: vec3|` in CCW winding order and the mesh's local space.  Return a bool to sort the face into output mesh 0 (false) or 1 (true), or an int to route it into the output mesh at that index (clamped to `0..=100000`)."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "transformed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true, the mesh's transform is applied to the vertex positions passed to `predicate` (world space).  When false (default), positions are in the mesh's local space.  Either way, the mesh's transform is copied to every output mesh."
          },
        ],
        description: "Partitions a mesh's faces into separate sub-meshes using `predicate`, returning a sequence of meshes.  Always returns at least two meshes (trailing partitions may be empty).  Shared-vertex connectivity and per-vertex attributes (normals, uv, tangent) are preserved within each partition.  This is NOT lazy; the partition is computed at the time this function is called.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "is_manifold" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "The mesh to check"
          },
          ArgDef {
            name: "two_manifold",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true (default), checks that every edge is shared by exactly two faces (watertight/2-manifold).  If false, checks only that no edge is shared by more than two faces (1-manifold)."
          },
        ],
        description: "Returns true if the mesh satisfies the manifold condition.  Assumes the mesh is a single connected component; disconnected islands or parts may produce incorrect results.  By default checks for 2-manifold (watertight): every edge shared by exactly two faces.  Set `two_manifold=false` to check only for 1-manifold (no edge shared by more than two faces).  This check is purely topological; vertex positions, normals, and winding order are not considered.",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "vec2" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec2 given x, y",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec2 with both components set to `value`",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Builds a 2-channel texture from 1-channel textures and/or constants",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Splats a 1-channel texture into a 2-channel texture (zero-copy)",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "vec3" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec3 given x, y, z",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "xy",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec3 from a Vec2 and a z component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "yz",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec3 from an x component and a Vec2",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec3 with all components set to `value`",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Builds a 3-channel texture from 1-channel textures and/or constants",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Splats a 1-channel texture into a 3-channel texture (zero-copy)",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "vec4" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "w",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec4 given x, y, z, w",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "xyz",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "w",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec4 from a Vec3 and a w component",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "xy",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "zw",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec4 from two Vec2s",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec4 with all components set to `value`",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "z",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "w",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Builds a 4-channel texture from 1-channel textures and/or constants",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Splats a 1-channel texture into a 4-channel texture (zero-copy)",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "join" => FnDef {
    // TODO: would be nice to add this to multiple modules depending on signature
    module: "mesh",
    examples: &[FnExample { composition_id: 31 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "meshes",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Combines a sequence of meshes into one mesh containing all geometry from the inputs.\n\nThis does NOT perform a boolean union; for that, use the `union` function or the `|` operator to create a union over a sequence of meshes.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "separator",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            // this needs to stay required in order to allow the signatures to be disambiguated
            default_value: DefaultValue::Required,
            description: "String to insert between each mesh"
          },
          ArgDef {
            name: "strings",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of strings to join"
          },
        ],
        description: "Joins a sequence of strings into a single string, inserting the `separator` between each element",
        return_type: &[ArgType::String],
      }
    ],
  },
  "union" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When the operands carry per-vertex attributes, the cut curve is an attribute seam.  By default its vertices are welded, blending both sides' values, so the output stays 2-manifold.  `true` keeps each side's values on duplicate vertices instead (like `rail_sweep`'s `split_seams`), leaving the mesh open along the cut.  Authored seams such as UV cuts survive either way."
          },
        ],
        description: "Returns the boolean union of two meshes (`a | b`)",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "meshes",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of meshes to union"
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When the operands carry per-vertex attributes, the cut curve is an attribute seam.  By default its vertices are welded, blending both sides' values, so the output stays 2-manifold.  `true` keeps each side's values on duplicate vertices instead (like `rail_sweep`'s `split_seams`), leaving the mesh open along the cut.  Authored seams such as UV cuts survive either way."
          },
        ],
        description: "Returns the boolean union of a sequence of meshes (`meshes[0] | meshes[1] | ...`)",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "difference" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 26 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Base mesh"
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to subtract"
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When the operands carry per-vertex attributes, the cut curve is an attribute seam.  By default its vertices are welded, blending both sides' values, so the output stays 2-manifold.  `true` keeps each side's values on duplicate vertices instead (like `rail_sweep`'s `split_seams`), leaving the mesh open along the cut.  Authored seams such as UV cuts survive either way."
          },
        ],
        description: "Returns the boolean difference of two meshes (`a - b`)",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "meshes",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of meshes to subtract in order"
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When the operands carry per-vertex attributes, the cut curve is an attribute seam.  By default its vertices are welded, blending both sides' values, so the output stays 2-manifold.  `true` keeps each side's values on duplicate vertices instead (like `rail_sweep`'s `split_seams`), leaving the mesh open along the cut.  Authored seams such as UV cuts survive either way."
          },
        ],
        description: "Returns the boolean difference of a sequence of meshes (`meshes[0] - meshes[1] - ...`)",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "intersect" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 25 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When the operands carry per-vertex attributes, the cut curve is an attribute seam.  By default its vertices are welded, blending both sides' values, so the output stays 2-manifold.  `true` keeps each side's values on duplicate vertices instead (like `rail_sweep`'s `split_seams`), leaving the mesh open along the cut.  Authored seams such as UV cuts survive either way."
          },
        ],
        description: "Returns the boolean intersection of two meshes (`a & b`)",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "meshes",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of meshes to intersect"
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When the operands carry per-vertex attributes, the cut curve is an attribute seam.  By default its vertices are welded, blending both sides' values, so the output stays 2-manifold.  `true` keeps each side's values on duplicate vertices instead (like `rail_sweep`'s `split_seams`), leaving the mesh open along the cut.  Authored seams such as UV cuts survive either way."
          },
        ],
        description: "Returns the boolean intersection of a sequence of meshes (`meshes[0] & meshes[1] & ...`)",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "fold" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "initial_val",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|acc, x, i|: acc`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Same as `reduce` but with an explicit initial value",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "fold_while" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "initial_val",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|acc, x, i|: acc | nil`.  If this callback returns `nil`, the final state of the accumulator passed into that iteration will be returned."
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Same as `fold` but with the option early-exiting.  If the provided callback returns `nil`, the final state of the accumulator passed into that iteration will be returned.",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "reduce" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|acc, x, i|: acc`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence to reduce"
          },
        ],
        description: "Same as `fold` but with the first element of the sequence as the initial value",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "any" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "cb",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x|: bool`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns true if any element of the sequence makes the callback return true",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "all" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "cb",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x|: bool`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns true if all elements of the sequence make the callback return true",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "for_each" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "cb",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x|`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies the callback to each element of the sequence, returning `nil`.  This is useful for running side effects.",
        return_type: &[ArgType::Nil],
      },
    ],
  },
  "flatten" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Flattens a sequence of sequences into a single sequence.  Any non-sequence elements are passed through unchanged.\n\nNote that only a single level of flattening is performed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "neg" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Negates an integer",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Negates a float",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Negates each component of a Vec3",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Inverts a boolean (logical NOT)",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Negates each component of a Vec2",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Negates each component of a Vec4",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Negates every channel of a texture",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "pos" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "h"
          },
        ],
        description: "Passes through the input unchanged (implementation detail of the unary `+` operator)",
        return_type: &[ArgType::Numeric],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Passes through the input unchanged (implementation detail of the unary `+` operator)",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Passes through the input unchanged (implementation detail of the unary `+` operator)",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Passes through the input unchanged (implementation detail of the unary `+` operator)",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "abs" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Absolute Value",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Absolute Value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise absolute value of a Vec3",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise absolute value of a Vec2",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Absolute value of every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise absolute value of a Vec4",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "signum" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns -1, 0, or 1 depending on the sign of the input.  For infinity, NaN, etc, the behavior is undefined.",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns -1, 0, or 1 depending on the sign of the input",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns -1, 0, or 1 depending on the sign of the input element-wise",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns -1, 0, or 1 depending on the sign of the input element-wise",
        return_type: &[ArgType::Vec3],
      }
    ],
  },
  "sqrt" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Square Root",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise square root of a Vec3",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise square root of a Vec2",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies the square root to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise square root of a Vec4",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "sigmoid" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Sigmoid function",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise sigmoid of a Vec3",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise sigmoid of a Vec2",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies sigmoid to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise sigmoid of a Vec4",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "add" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds two Vec3s component-wise",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a + b",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a + b",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a + b",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Combines two meshes into one mesh containing all geometry from both inputs.\n\nThis does NOT perform a boolean union; for that, use the `union` function or the `|` operator.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates the given mesh by a Vec3 offset",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "vec",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "num",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds a numeric value to each component of a Vec3",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds two Vec2s component-wise",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds a numeric value to each component of a Vec2",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Concatenates two strings",
        return_type: &[ArgType::String],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds two Vec4s component-wise",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds a numeric value to each component of a Vec4",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds two textures elementwise; dims and channel counts must match",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds a scalar to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Adds a scalar to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture + vec2` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec2 + texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture + vec3` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec3 + texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture + vec4` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec4 + texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "sub" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise subtraction of two Vec3s",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a -b",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a - b",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a - b",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Base mesh"
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to subtract"
          },
        ],
        description: "Returns the boolean difference of two meshes (`a - b`).  This is equivalent to the `difference` function.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates the mesh by (`-offset`)",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "vec",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "num",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subtracts a numeric value from each component of a Vec3",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subtracts two Vec2s component-wise",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subtracts a numeric value from each component of a Vec2",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise subtraction of two Vec4s",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subtracts a numeric value from each component of a Vec4",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subtracts two same-shape textures element-wise",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subtracts a scalar from every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Scalar minus every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture - vec2` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec2 - texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture - vec3` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec3 - texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture - vec4` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec4 - texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "mul" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the component-wise product of two Vec3 values",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplied each element of a Vec3 by a scalar",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a * b",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a * b",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a * b",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh to scale"
          },
          ArgDef {
            name: "factor",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Uniform scale to apply to the mesh"
          },
        ],
        description: "Uniformly scales a mesh by a scalar factor",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "factor",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Scales `mesh` by `factor` along each axis",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the component-wise product of two Vec2 values",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplied each element of a Vec2 by a scalar",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplied each element of a Vec2 by a scalar",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplied each element of a Vec3 by a scalar",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Composes two transform matrices.  Matrix multiplication is not commutative: in `a * b`, `b` is applied to a point first and `a` second.  See `compose_transforms` for the named equivalent.",
        return_type: &[ArgType::Mat4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mat4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Transforms a 3D point by a transform matrix, treating it as a position (`w = 1`), so translation applies.  Same as `transform_point`.  Use `transform_dir` for directions, which ignores translation.  Assumes an affine matrix; there is no perspective divide.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the component-wise product of two Vec4 values",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplies each element of a Vec4 by a scalar",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplies each element of a Vec4 by a scalar",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplies every texel of a texture by a scalar",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplies every texel of a texture by a scalar",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Multiplies two textures elementwise (masking); dims and channel counts must match",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture * vec2` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec2 * texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture * vec3` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec3 * texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture * vec4` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec4 * texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "div" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the component-wise division of two Vec3 values",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Divides each element of a Vec3 by a scalar",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a / b",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a / b",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Divides two integers a / b and returns a float.  The integers are converted to floats before division.  If you need true integer division, use the `mod` function or `%` operator instead.",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the component-wise division of two `Vec2`s",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Divides each component of a Vec2 by a scalar",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the component-wise division of two Vec4 values",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Divides each element of a Vec4 by a scalar",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Divides two same-shape textures element-wise",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Divides every channel of a texture by a scalar",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Scalar divided by every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture / vec2` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec2 / texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture / vec3` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec3 / texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `texture / vec4` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `vec4 / texture` (vec length must match the channel count)",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "mod" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "a % b",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Floating point modulus (`a % b`)",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `a % b` of two textures (1-channel broadcasts against either side)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `a % b` of a texture against a constant",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `a % b` of a constant against a texture",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "max" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the minimum of the provided arguments",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns maximum of the provided arguments",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise maximum of two Vec3s",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise maximum of two Vec2s",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Element-wise maximum of two same-shape textures",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel maximum of a texture and a scalar",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel maximum of a texture and a scalar",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise maximum of two Vec4s",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "by",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Key function with signature `|x: T, ix: int|: K`, where `K` is an int, float, string, bool, or a list of those (compared element-wise, so `[primary, secondary]` sorts on multiple keys).  Elements are used as their own keys if omitted."
          },
        ],
        description: "Returns the element of the sequence with the largest key, or `nil` if the sequence is empty.  The first of several equal-key elements wins.",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "min" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the minimum of the provided arguments",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns minimum of the provided arguments",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise minimum of two Vec3s",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise minimum of two Vec2s",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Element-wise minimum of two same-shape textures",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel minimum of a texture and a scalar",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel minimum of a texture and a scalar",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise minimum of two Vec4s",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "by",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Key function with signature `|x: T, ix: int|: K`, where `K` is an int, float, string, bool, or a list of those (compared element-wise, so `[primary, secondary]` sorts on multiple keys).  Elements are used as their own keys if omitted."
          },
        ],
        description: "Returns the element of the sequence with the smallest key, or `nil` if the sequence is empty.  The first of several equal-key elements wins.",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "clamp" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Clamps a value between min and max",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Clamps a value between min and max",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Clamps each component of a Vec3 between min and max",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Clamps each component of a Vec2 between min and max",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Clamps every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Clamps each component of a Vec4 between min and max",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "float" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Converts a value to a float",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "int" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Converts a value to an int",
        return_type: &[ArgType::Int],
      },
    ],
  },
  "str" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Converts a value to a string.\n\nFor simple types like numbers and booleans, this performs the basic conversion as you'd expect.  For more complicated or internal types like meshes, lights, sequences, etc., the format is not defined and may change at a later time.",
        return_type: &[ArgType::String],
      },
    ],
  },
  "and" => FnDef {
    module: "logic",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Logical AND operation of two booleans",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "or" => FnDef {
    module: "logic",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Logical OR operation of two booleans",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "xor" => FnDef {
    module: "logic",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Logical XOR operation of two booleans",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "not" => FnDef {
    module: "logic",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Inverts the boolean value (logical NOT)",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "bit_and" => FnDef {
    module: "bits",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Bitwise AND operation of two integers (`a & b`)",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the boolean intersection of two meshes (`a & b`)",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "bit_or" => FnDef {
    module: "bits",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Bitwise OR operation of two integers (`a | b`)",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the boolean union of two meshes (`a | b`)",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "map" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x: T, ix: int|: y`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence to map over"
          },
        ],
        description: "Applies a function to each element of a sequence and returns a new sequence.  \n\nThis is lazy and will not evaluate the function until the output sequence is consumed.",
        return_type: &[ArgType::Sequence],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|vtx: Vec3, normal: Vec3|: Vec3` that will be invoked for each vertex in the new mesh, returning a new position for that vertex  An optional trailing parameter must be a destructured attribute bag naming the per-vertex data to read, e.g. `|pos, normal, {uv, color, ix}|`: any attribute on the mesh (see `set_attr`), plus `ix` (vertex index in `verts` order), `pos`, and `normal`.  Only the named keys are read, so omitting the bag costs nothing."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies a function to each vertex in a mesh and returns a new mesh with the transformed vertices.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|val: float|vec2|vec3|vec4, uv: vec2, x_ix: int, y_ix: int|: float | vec2 | vec3 | vec4`, invoked once per pixel.  `val`'s type matches the input's channel count; the return type sets the output's channel count."
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies a function to each pixel of a texture and returns a new texture with the same dimensions and wrap mode.",
        return_type: &[ArgType::Texture],
      }
    ],
  },
  "filter" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x: T, ix: int|: bool`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence to filter"
          },
        ],
        description: "Filters a sequence using a predicate function.  \n\nThis is lazy and will not evaluate the function until the output sequence is consumed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "scan" => FnDef {
    module: "seq",
    examples: &[FnExample { composition_id: 40 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "initial",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: "Initial value to seed the state with.  This value will NOT be included in the output sequence."
          },
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|acc, x, ix|: acc`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence to scan"
          },
        ],
        description: "Applies a function cumulatively to the elements of a sequence, returning a new sequence of intermediate results.  Similar to the `Iterator::scan` function from Rust, but with a little less freedom in the way it can be used.\n\nThis is lazy and will not evaluate the function until the output sequence is consumed.\n\nNOTE: This function may be subject to change in the future.  It would be much better for it to return a tuple to allow de-coupling output sequence values from the retained state.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "take" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of elements to take from the start of the sequence"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence to take elements from"
          },
        ],
        description: "Returns a new sequence containing the first `n` elements of the input sequence.  If `n` is greater than the length of the sequence, the entire sequence is returned.  \n\nThis is lazy and will not evaluate the sequence until the output sequence is consumed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "skip" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of elements to skip from the start of the sequence"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence to skip elements from"
          },
        ],
        description: "Returns a new sequence with the first `n` elements skipped.  If `n` is greater than the length of the sequence, an empty sequence is returned.  \n\nThis is lazy and will not evaluate the sequence until the output sequence is consumed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "take_while" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x|: bool`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence to take elements from"
          },
        ],
        description: "Returns a new sequence containing elements from the start of the input sequence until the predicate function returns false.  \n\nThis is lazy and will not evaluate the function until the output sequence is consumed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "skip_while" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x|: bool`"
          },
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a new sequence with elements skipped from the start of the input sequence until the predicate function returns false.  \n\nThis is lazy and will not evaluate the function until the output sequence is consumed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "chain"=> FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequences",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of sequences to chain together"
          },
        ],
        description: "Returns a new sequence that concatenates all input sequences.  \n\nThis is lazy and will not evaluate the sequences until the output sequence is consumed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "first" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the first element of a sequence, or `Nil` if the sequence is empty.",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "last" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the last element of a sequence, or `Nil` if the sequence is empty.",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "append" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "val",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: "The value to add to the end of the sequence"
          },
          ArgDef {
            name: "seq",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "The sequence to which the value should be added"
          }
        ],
        description: "Appends a value to the end of a sequence, returning a new sequence.  The old sequence is left unchanged.\n\nIf the sequence is not eager (as produced by the `collect` function, an array literal, or similar), it will be collected into memory before this happens.\n\nNote that this isn't very efficient and requires collecting and/or cloning the underlying sequence.  It's better to keep things lazy and use sequences and sequence combinators where possible.",
        return_type: &[ArgType::Sequence]
      }
    ],
  },
  "sort" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "by",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Key function with signature `|x: T, ix: int|: K`, where `K` is an int, float, string, bool, or a list of those (compared element-wise, so `[primary, secondary]` sorts on multiple keys).  Elements are used as their own keys if omitted."
          },
          ArgDef {
            name: "stable",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, elements with equal keys keep their original relative order"
          },
          ArgDef {
            name: "desc",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "Sort in descending order"
          },
        ],
        description: "Sorts a sequence by key in ascending order.  Ints and floats compare with each other numerically; mixing any other key kinds is an error.  The key function is called once per element.  \n\nThis is NOT lazy and will evaluate the entire sequence immediately and collect all of its elements into memory.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "reverse" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a new sequence with the elements in reverse order.  \n\nThis is NOT lazy and will evaluate the entire sequence immediately and collect all of its elements into memory.",
        return_type: &[ArgType::Sequence],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Reverses a path's direction: `t` runs the other way, subpath order and every segment flip.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "collect" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Makes the sequence eager, collecting all elements into memory.  This will allow the sequence to indexed with `[]`.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "print" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Prints all provided args and kwargs to the console",
        return_type: &[ArgType::Nil],
      },
    ],
  },
  "render" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "attrs",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Names of custom per-vertex attributes (see `set_attr`) to export as GPU vertex attributes, in addition to the well-known `uv`, `tangent`, and `color` which always export.  Each named attribute must exist on the mesh.  Exported attributes are available to custom shaders as `attribute`s of the same name."
          },
        ],
        description: "Renders a mesh to the scene",
        return_type: &[ArgType::Nil],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "light",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Light),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Renders a light to the scene",
        return_type: &[ArgType::Nil],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "meshes",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Either:\n - `Seq<Mesh | Light | Seq<Vec3 | Vec2>>` of objects to render to the scene, or\n - `Seq<Vec3>` of points representing a path to render, or\n - `Seq<Vec2>` of 2D points representing a path to render in the XZ plane",
          },
          ArgDef {
            name: "attrs",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Names of custom per-vertex attributes (see `set_attr`) to export as GPU vertex attributes, in addition to the well-known `uv`, `tangent`, and `color` which always export.  Each named attribute must exist on the mesh.  Exported attributes are available to custom shaders as `attribute`s of the same name."
          },
        ],
        description: "Renders a sequence of entities to the scene.  The sequence can contain a heterogeneous mix of meshes, lights, and paths (`Seq<Vec3>` or `Seq<Vec2>`).  `Vec2` paths are rendered in the XZ plane.  Each entity will be rendered separately.",
        return_type: &[ArgType::Nil],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path to render as a closed path in the XZ plane",
          },
        ],
        description: "Renders a path in the XZ plane, one polyline per subpath, flattened at the ambient curve tolerance. The resulting 2D positions are projected onto the XZ plane (y=0).",
        return_type: &[ArgType::Nil],
      },
    ],
  },
  "gizmo" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Stable handle id, scoped to the node. Omit for a positional `@N` id." },
          ArgDef { name: "origin", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::zeros())), description: "Node-local anchor where the handle is drawn in the viewport." },
          ArgDef { name: "absolute", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Bool), default_value: DefaultValue::Optional(|| Value::Bool(false)), description: "If true the returned value is the handle position itself; otherwise it's the drag offset from `origin` (zero until dragged)." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Optional(|| Value::Nil), description: "Value to return when no value has been stored for this handle." },
          ArgDef { name: "ghost", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Bool), default_value: DefaultValue::Optional(|| Value::Nil), description: "Override ghost rendering for this handle: `nil` follows the global Geotoy setting; `true`/`false` force it on/off." },
        ],
        description: "A viewport-draggable `vec3` handle. Interactive only inside the Geotoy editor; everywhere else (level loader, tests) it returns the stored baked value, or the `default`/zero when unset.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "gizmo2d" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Stable handle id, scoped to the node. Omit for a positional `@N` id." },
          ArgDef { name: "origin", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::zeros())), description: "Node-local anchor where the handle is drawn in the viewport." },
          ArgDef { name: "absolute", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Bool), default_value: DefaultValue::Optional(|| Value::Bool(false)), description: "If true the returned value is the handle position itself; otherwise it's the drag offset from `origin` (zero until dragged)." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec2), default_value: DefaultValue::Optional(|| Value::Nil), description: "Value to return when no value has been stored for this handle." },
          ArgDef { name: "axes", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Which two axes are draggable, e.g. \"xz\" (default), \"xy\", \"yz\". The returned `vec2` maps to these axes in order." },
          ArgDef { name: "ghost", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Bool), default_value: DefaultValue::Optional(|| Value::Nil), description: "Override ghost rendering for this handle: `nil` follows the global Geotoy setting; `true`/`false` force it on/off." },
        ],
        description: "A viewport-draggable `vec2` handle restricted to two axes (default XZ, the ground plane). Like `gizmo` but the live gizmo only exposes the two chosen axes.",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "gizmo1d" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Stable handle id, scoped to the node. Omit for a positional `@N` id." },
          ArgDef { name: "origin", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::zeros())), description: "Node-local anchor where the handle is drawn in the viewport." },
          ArgDef { name: "absolute", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Bool), default_value: DefaultValue::Optional(|| Value::Bool(false)), description: "If true the returned value is the handle position itself; otherwise it's the drag offset from `origin` (zero until dragged)." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Value to return when no value has been stored for this handle." },
          ArgDef { name: "axis", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Which axis is draggable: \"x\", \"y\" (default), or \"z\"." },
          ArgDef { name: "ghost", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Bool), default_value: DefaultValue::Optional(|| Value::Nil), description: "Override ghost rendering for this handle: `nil` follows the global Geotoy setting; `true`/`false` force it on/off." },
        ],
        description: "A viewport-draggable `num` handle restricted to a single axis (default Y). Like `gizmo` but the live gizmo only exposes one axis.",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "gizmo_transform" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Stable handle id, scoped to the node. Omit for a positional `@N` id." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Mat4), default_value: DefaultValue::Optional(|| Value::Nil), description: "Transform to return when no value has been stored for this handle." },
          ArgDef { name: "ghost", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Bool), default_value: DefaultValue::Optional(|| Value::Nil), description: "Override ghost rendering for this handle: `nil` follows the global Geotoy setting; `true`/`false` force it on/off." },
        ],
        description: "A viewport-draggable `mat4` transform handle (absolute). Consume it with `apply_mat4`. Returns the stored baked transform, or the `default`/identity when unset.",
        return_type: &[ArgType::Mat4],
      },
    ],
  },
  "input_float" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Required, description: "Stable control id, scoped to the node. Also the default panel label." },
          ArgDef { name: "min", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Slider minimum." },
          ArgDef { name: "max", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Slider maximum." },
          ArgDef { name: "step", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Slider step; omit for continuous." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Value returned when the control is unset." },
          ArgDef { name: "label", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Display label override; defaults to `name`." },
          ArgDef { name: "style", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Widget style: \"slider\" (default), \"entry\", or \"knob\"." },
        ],
        description: "A panel-driven `num` control (slider by default). Interactive in the Geotoy editor and injectable from level defs; elsewhere returns the stored value, or `default`/zero when unset.",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "input_int" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Required, description: "Stable control id, scoped to the node. Also the default panel label." },
          ArgDef { name: "min", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Slider minimum." },
          ArgDef { name: "max", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Slider maximum." },
          ArgDef { name: "step", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Slider step; defaults to 1 in the panel." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Numeric), default_value: DefaultValue::Optional(|| Value::Nil), description: "Value returned when the control is unset." },
          ArgDef { name: "label", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Display label override; defaults to `name`." },
          ArgDef { name: "style", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Widget style: \"slider\" (default), \"entry\", or \"knob\"." },
        ],
        description: "A panel-driven integer control (rounded slider). Injectable from level defs; returns the stored value, or `default`/zero when unset.",
        return_type: &[ArgType::Int],
      },
    ],
  },
  "input_bool" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Required, description: "Stable control id, scoped to the node. Also the default panel label." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Bool), default_value: DefaultValue::Optional(|| Value::Nil), description: "Value returned when the control is unset." },
          ArgDef { name: "label", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Display label override; defaults to `name`." },
        ],
        description: "A panel-driven checkbox `bool` control. Returns the stored value, or `default`/false when unset.",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "input_color" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Required, description: "Stable control id, scoped to the node. Also the default panel label." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Vec3), default_value: DefaultValue::Optional(|| Value::Nil), description: "RGB color returned when the control is unset." },
          ArgDef { name: "label", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Display label override; defaults to `name`." },
        ],
        description: "A panel-driven color picker; returns the chosen color as an RGB `vec3`. Returns the stored value, or `default`/black when unset.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "input_select" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Required, description: "Stable control id, scoped to the node. Also the default panel label." },
          ArgDef { name: "options", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Sequence), default_value: DefaultValue::Required, description: "The selectable string options." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Option returned when unset; defaults to the first option." },
          ArgDef { name: "label", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Display label override; defaults to `name`." },
        ],
        description: "A panel-driven dropdown; returns the chosen option as a `string`. An injected/stored value not in `options` falls back to `default` or the first option.",
        return_type: &[ArgType::String],
      },
    ],
  },
  "input_spline" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Required, description: "Stable control id, scoped to the node. Also the default panel label." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Sequence), default_value: DefaultValue::Optional(|| Value::Nil), description: "Sequence of vec3 control points returned when the control is unset." },
          ArgDef { name: "label", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Display label override; defaults to `name`." },
        ],
        description: "An interactively-editable sequence of vec3 control points (a polyline/spline). Edited in the viewport in Geotoy and the level editor; injectable from level defs. Returns the stored points, or `default`/empty when unset. Feed into `extrude_pipe`, `fillet_path_3d`, `catmull_rom_3d`, etc.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "input_ramp" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Required, description: "Stable control id, scoped to the node. Also the default panel label." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Sequence, ArgType::Callable), default_value: DefaultValue::Required, description: "A `ramp` stop list (any of its forms) or a built `ramp(...)` value. Named easings only — closures can't be edited in the panel." },
          ArgDef { name: "label", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Display label override; defaults to `name`." },
        ],
        description: "An interactively-editable scalar transfer function; returns the same callable `|x: float|: float` that `ramp` builds. Edited as a curve/stop list in the controls panel.",
        return_type: &[ArgType::Callable],
      },
    ],
  },
  "input_color_ramp" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef { name: "name", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Required, description: "Stable control id, scoped to the node. Also the default panel label." },
          ArgDef { name: "default", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::Sequence, ArgType::Callable), default_value: DefaultValue::Required, description: "A `color_ramp` stop list (vec3 linear-RGB values; any of its forms) or a built `color_ramp(...)` value. Stop-list defaults get `color_ramp`'s defaults (oklab, clamp). Named easings only." },
          ArgDef { name: "label", interned_name: Sym(0), valid_types: argtype_flags!(ArgType::String), default_value: DefaultValue::Optional(|| Value::Nil), description: "Display label override; defaults to `name`." },
        ],
        description: "An interactively-editable color gradient; returns the same callable `|x: float|: vec3` that `color_ramp` builds. Edited via a gradient bar with draggable stops in the controls panel.",
        return_type: &[ArgType::Callable],
      },
    ],
  },
  "render_path" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to render.",
          },
          ArgDef {
            name: "resolution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(1000)),
            description: "Initial uniform probe count for lazy paths; known boundaries and adaptive refinement add samples as needed.",
          },
        ],
        description: "Renders a path in the XZ plane, one polyline per subpath. Both concrete and lazy paths use the ambient curve-angle tolerance; `resolution` controls initial probe density for lazy paths.",
        return_type: &[ArgType::Nil],
      },
    ],
  },
  "sin" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the sine of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the sine of each component of a Vec3",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the sine of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies sine to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the sine of each component of a Vec4",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "cos" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Cosine",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the cosine of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the cosine of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies cosine to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec4 with the cosine of each component",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "tan" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Tangent",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the tangent of each component of a Vec3",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the tangent of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies tangent to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the tangent of each component of a Vec4",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "sinh" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the hyperbolic sine of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the hyperbolic sine of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the hyperbolic sine of each component",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "cosh" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the hyperbolic cosine of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the hyperbolic cosine of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the hyperbolic cosine of each component",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "tanh" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the hyperbolic tangent of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the hyperbolic tangent of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the hyperbolic tangent of each component",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "acos" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the arccosine of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the arccosine of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the arccosine of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies arccosine to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec4 with the arccosine of each component",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "asin" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the arcsine of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the arcsine of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the arcsine of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies arcsine to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec4 with the arcsine of each component",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "atan" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the arctangent of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the arctangent of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the arctangent of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies arctangent to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec4 with the arctangent of each component",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "atan2" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the arctangent of `y / x`, using the signs of both arguments to determine the correct quadrant.",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "xy",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the arctangent of `y / x` for a Vec2, using the signs of both components to determine the correct quadrant.",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel atan2 of two textures (1-channel broadcasts against either side)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel atan2 of a texture `y` against a constant `x`",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel atan2 of a constant `y` against a texture `x`",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "pow" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      // TODO: should split into int and float versions
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "base",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "exponent",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`Returns `base` raised to the power of `exponent``",
        return_type: &[ArgType::Numeric],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "base",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "exponent",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with each component raised to the power of `exponent`",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "base",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "exponent",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with each component raised to the power of `exponent`",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "base",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "exponent",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Raises every channel of a texture to a power",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "base",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "exponent",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec4 with each component raised to the power of `exponent`",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "exp" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the exponential of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the exponential of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the exponential of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies e^x to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec4 with the exponential of each component",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "log10" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the base-10 logarithm of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the base-10 logarithm of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the base-10 logarithm of each component",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "log2" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the base-2 logarithm of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the base-2 logarithm of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the base-2 logarithm of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies log base 2 to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec4 with the base-2 logarithm of each component",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "ln" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the natural logarithm of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the natural logarithm of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the natural logarithm of each component",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "trunc" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Truncates a numeric value to its integer part",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Truncates each component of a Vec3 to its integer part",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Truncates each component of a Vec2 to its integer part",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies trunc to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Truncates each component of a Vec4 to its integer part",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "fract" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the fractional part of a numeric value",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec3 with the fractional part of each component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the fractional part of each component",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies fract to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec4 with the fractional part of each component",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "round" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds a numeric value to the nearest integer",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec3 to the nearest integer",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec2 to the nearest integer",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies round to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec4 to the nearest integer",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "ceil" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds a numeric value up to the nearest integer",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec3 up to the nearest integer",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec2 up to the nearest integer",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies ceil to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec4 up to the nearest integer",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "floor" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds a numeric value down to the nearest integer",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec3 down to the nearest integer",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec2 down to the nearest integer",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies floor to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec4 down to the nearest integer",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "fix_float" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "If the provided float is NaN, non-infinite, or subnormal, returns 0.0.  Otherwise, returns the float unchanged.",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Vec3 value to fix"
          },
        ],
        description: "For each component of the Vec3, if it is NaN, non-infinite, or subnormal, returns 0.0.  Otherwise, returns the component unchanged.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Vec2 value to fix"
          },
        ],
        description: "For each component of the Vec2, if it is NaN, non-infinite, or subnormal, returns 0.0.  Otherwise, returns the component unchanged.",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "rad2deg" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Converts radians to degrees",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "deg2rad" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Converts degrees to radians",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "gte" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a >= b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a >= b`",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "lte" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a <= b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a <= b`",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "gt" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a > b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a > b`",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "lt" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a < b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a < b`",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "eq" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a == b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a == b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Nil),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a == b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Nil),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a == b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a == b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a == b`",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "neq" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a != b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a != b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Nil),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a != b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Nil),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a != b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a != b`",
        return_type: &[ArgType::Bool],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`a != b`",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "point_distribute" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 28 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Nil),
            default_value: DefaultValue::Required,
            description: "The number of points to distribute across the mesh.  If `nil`, returns an infinite sequence."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "cb",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional callable with signature `|point: vec3, normal: vec3|: any`.  If provided, this function will be called for each generated point and normal and whatever it returns will be included in the output sequence instead of the point."
          },
          ArgDef {
            name: "world_space",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, points and normals will be returned in world space.  If false, they will be returned in the local space of the mesh."
          }
        ],
        description: "Distributes a specified number of points uniformly across the surface of a mesh returned as a sequence.  If `cb` is Nil or not provided, a sequence of vec3 positions will be returned.  If `cb` is provided, the sequence will consist of the return values from calling `cb(pos, normal)` for each sampled point.\n\nThis is lazy; the points will not be generated until the sequence is consumed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "lerp" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Linearly interpolates between two Vec3 values `a` and `b` by a factor `t`",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Linearly interpolates between two numeric values `a` and `b` by a factor `t`",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Linearly interpolates between two Vec2 values `a` and `b` by a factor `t`",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Linearly interpolates between two Vec4 values `a` and `b` by a factor `t`",
        return_type: &[ArgType::Vec4],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Linearly interpolates between two textures by a constant factor `t` (1-channel broadcasts against either side)",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Linearly interpolates between two textures with a per-texel factor from texture `t` (1-channel broadcasts on any operand)",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "ramp" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "stops",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Stop list. Positioned form: `[[pos, value], …]` / `[[pos, value, ease], …]` / `[{pos, value, ease?}, …]` with positions in raw input units. Bare form: `[value, …]` spaced evenly over `domain`. Values must be all numbers or all vec3s (interpolated componentwise). Duplicate positions make a hard edge."
          },
          ArgDef {
            name: "domain",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Vec2),
            default_value: DefaultValue::Optional(|| Value::Vec2(Vec2::new(0., 1.))),
            description: "`[lo, hi]` placement range used only by the bare-values stop form."
          },
          ArgDef {
            name: "extend",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("clamp".to_owned())),
            description: "Out-of-range behavior: \"clamp\" | \"repeat\" | \"mirror\" (period = stop extent)."
          },
          ArgDef {
            name: "ease",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Callable),
            default_value: DefaultValue::Optional(|| Value::String("linear".to_owned())),
            description: "Default segment easing: \"linear\" | \"smooth\" | \"smoother\" | \"step\", or a `|t|: float` callable evaluated at construct time. A stop's own `ease` overrides it for the segment leaving that stop."
          },
        ],
        description: "Builds a multi-stop transfer function and returns it as a callable `|x: float|: float|vec3`. Native easings evaluate segments exactly; closure easings bake into a 256-entry LUT at construct time (documented quantization). Common use: mapping noise into other ranges, e.g. `fbm(pos=p) | ramp([[-1., 0.], [1., 1.]])`.",
        return_type: &[ArgType::Callable],
      },
    ],
  },
  "color_ramp" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "stops",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Stop list as in `ramp`; values must be vec3 colors in linear RGB (use `srgb(0xRRGGBB)` to bring in design-tool hex values)."
          },
          ArgDef {
            name: "domain",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Vec2),
            default_value: DefaultValue::Optional(|| Value::Vec2(Vec2::new(0., 1.))),
            description: "`[lo, hi]` placement range used only by the bare-values stop form."
          },
          ArgDef {
            name: "extend",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("clamp".to_owned())),
            description: "Out-of-range behavior: \"clamp\" | \"repeat\" | \"mirror\" (period = stop extent)."
          },
          ArgDef {
            name: "ease",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Callable),
            default_value: DefaultValue::Optional(|| Value::String("linear".to_owned())),
            description: "Default segment easing: \"linear\" | \"smooth\" | \"smoother\" | \"step\", or a `|t|: float` callable evaluated at construct time."
          },
          ArgDef {
            name: "space",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("oklab".to_owned())),
            description: "Interpolation space: \"oklab\" (perceptually even; default) | \"oklch\" (hue as shorter-arc angle, stays saturated) | \"linear\" (raw RGB / light mixing) | \"srgb\" (legacy gamma-space, for matching design-tool gradients)."
          },
        ],
        description: "Color-specialized `ramp`: builds a gradient over vec3 stops (linear RGB in and out) and returns a callable `|x: float|: vec3`. Non-linear-space mixing bakes into a 256-entry LUT at construct time.",
        return_type: &[ArgType::Callable],
      },
    ],
  },
  "remap" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "in_lo",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Input range start."
          },
          ArgDef {
            name: "in_hi",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Input range end."
          },
          ArgDef {
            name: "out_lo",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Output range start."
          },
          ArgDef {
            name: "out_hi",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Output range end."
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Vec3, ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Value to remap (componentwise for vec3; every channel for a texture). Last so pipelines partially apply: `v | remap(-1., 1., 0., 1.)`."
          },
          ArgDef {
            name: "clamp",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "Clamp the normalized position to [0, 1] before mapping (no extrapolation)."
          },
        ],
        description: "Linearly maps `x` from `[in_lo, in_hi]` to `[out_lo, out_hi]`, extrapolating unless `clamp=true`.",
        return_type: &[ArgType::Float, ArgType::Vec3, ArgType::Texture],
      },
    ],
  },
  "srgb" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "hex",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "sRGB-encoded hex color like `0xC97B4A`."
          },
        ],
        description: "Decodes an sRGB-encoded hex color to a linear-RGB vec3 — the correct way to bring design-tool colors into geoscript's linear color world.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "r",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "sRGB-encoded red in [0, 1]."
          },
          ArgDef {
            name: "g",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "sRGB-encoded green in [0, 1]."
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "sRGB-encoded blue in [0, 1]."
          },
        ],
        description: "Decodes sRGB-encoded channel values (0–1) to a linear-RGB vec3.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "smoothstep" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "edge0",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "edge1",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Works the same as `smoothstep` in GLSL.\n\nIt returns 0 if `x < edge0`, 1 if `x > edge1`, and a smooth Hermite interpolation between 0 and 1 for values of `x` between `edge0` and `edge1`.",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "edge0",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "edge1",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies smoothstep to every channel of a texture",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "linearstep" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "edge0",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "edge1",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Same as `smoothstep` but with simple linear interpolation instead of Hermite interpolation.\n\nIt returns 0 if `x < edge0`, 1 if `x > edge1`, and a linear interpolation between 0 and 1 for values of `x` between `edge0` and `edge1`.",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "deriv" => FnDef {
    module: "fn",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "f",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Closure to differentiate with respect to its FIRST parameter, which must carry an explicit type annotation (numeric, vec2, or vec3). Further parameters pass through unchanged (constant w.r.t. the derivative); the returned closure takes the same parameter list."
          },
          ArgDef {
            name: "dir",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2, ArgType::Vec3, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Tangent direction seeding the forward-mode derivative; must match the closure's parameter type."
          },
        ],
        description: "Forward-mode automatic differentiation.  Returns a new closure computing the directional derivative (JVP) of `f` seeded by `dir` — e.g. for an embedding `phi = |p: vec2|: vec3`, `deriv(phi, vec2(1, 0))` computes the partial `d phi / d u`.",
        return_type: &[ArgType::Callable],
      },
    ],
  },
  "grad" => FnDef {
    module: "fn",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "f",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Scalar-output closure to differentiate with respect to its FIRST parameter, which must carry an explicit type annotation (numeric, vec2, or vec3). Any further parameters pass through unchanged and are treated as constants, so values that change between calls can be arguments instead of captures; the returned closure takes the same parameter list. Prefer that over calling `grad` inside a loop: every `grad` call re-derives and re-optimizes the closure body, while a hoisted gradient is derived once."
          },
        ],
        description: "Returns a closure computing the gradient of a scalar-output function `f`.  For a vec2/vec3 input the result returns the vector of partial derivatives; for a scalar input it is just `f'`.",
        return_type: &[ArgType::Callable],
      },
    ],
  },
  "compose" => FnDef {
    module: "fn",
    examples: &[FnExample { composition_id: 34 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          }
        ],
        description: "Composes all arguments, returning a callable like `|x| arg1(arg2(arg3(x)))`",
        return_type: &[ArgType::Callable],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "callables",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of callables to compose"
          },
        ],
        description: "Composes a sequence of callables, returning a callable like `|x| callables[0](callables[1](...callables[n](x)))`",
        return_type: &[ArgType::Callable],
      },
    ],
  },
  "convex_hull" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 23 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Computes the convex hull of a sequence of points, returning a mesh representing the convex hull",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Computes the convex hull of a mesh, returning a mesh representing the convex hull.\n\nThis will apply all transforms on the mesh, and the returned mesh will have an identity transform.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "warp" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 24 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|pos: vec3, normal: vec3|: vec3`.  Given the position and normal of each vertex in the mesh, returns a new position for that vertex in the output mesh.  An optional trailing parameter must be a destructured attribute bag naming the per-vertex data to read, e.g. `|pos, normal, {uv, color, ix}|`: any attribute on the mesh (see `set_attr`), plus `ix` (vertex index in `verts` order), `pos`, and `normal`.  Only the named keys are read, so omitting the bag costs nothing."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Applies a warp function to each vertex of the mesh, returning a new mesh with each vertex transformed by `fn`.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "tessellate" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "target_edge_length",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Tessellates a mesh, splitting edges to achieve a target edge length.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "subdivide_by_plane" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "plane_normal",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Normal vector of the plane to cut the mesh with"
          },
          ArgDef {
            name: "plane_offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Offset of the plane from the origin along the plane normal"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subdivides a mesh by a plane, splitting all edges and faces that intersect the plane.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "plane_normals",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of normal vectors of the planes to cut the mesh with"
          },
          ArgDef {
            name: "plane_offsets",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of offsets of the planes from the origin along the plane normals"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subdivides a mesh by a sequence of planes, splitting all edges and faces that intersect any of the planes.  This is more efficient than repeatedly subdividing by a single plane multiple times.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "subdivide_by_line" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "line_origin",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "A point on the line that skewers the mesh"
          },
          ArgDef {
            name: "line_dir",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Direction of the line.  Need not be unit-length; it will be normalized internally."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "eps",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1e-4)),
            description: "World-space tolerance for snapping the hit point to a nearby vertex or edge.  Larger values make the skewer more forgiving of near-coincident geometry; smaller values snap less aggressively."
          },
        ],
        description: "Skewers a mesh with an infinite line.  Triangles whose interiors are intersected by the line are split into three fan triangles around the hit point.  Hits coincident with an edge split the edge (and both adjacent triangles).  Hits coincident with a vertex are no-ops.  Aliased as `skewer`.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "split_by_plane" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 29 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "plane_normal",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Normal vector of the plane to cut the mesh with"
          },
          ArgDef {
            name: "plane_offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Offset of the plane from the origin along the plane normal"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Splits a mesh by a plane, returning a sequence containing two meshes: the part in front of the plane and the part behind the plane.\n\nThis will apply all transforms on the mesh, and the returned meshes will have an identity transform.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "connected_components" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 51 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Splits a mesh into its connected components, returning a sequence of meshes where each mesh is a connected component of the input mesh.  The sequence of connected components is sorted by vertex count from highest to lowest.\n\nThis is NOT lazy; the connected components are computed at the time this function is called.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "intersects" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns true if any triangle of `a` intersects one of `b`.  Surfaces only: a mesh fully inside another doesn't count, and neither does contact at a single point.",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "aabb" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "The mesh to compute the axis-aligned bounding box of."
          },
        ],
        description: "Returns the axis-aligned bounding box of `mesh` as a 2-element sequence `[mins, maxs]` of Vec3 corners, in world space (after the mesh's transform is applied). Errors if the mesh is empty.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "fit_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A 2D path."
          },
          ArgDef {
            name: "pad",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.)),
            description: "Padding on every side as a fraction of the unit square, in [0, 0.5)."
          },
        ],
        description: "Uniformly scales and translates a 2D path so its bounding box fits inside `[pad, 1-pad]²`, filling the longer axis and centering the shorter one.  Aspect ratio is preserved.  The usual first step before `rasterize_path` / `path_sdf` / `path_uv` for glyphs, traced SVGs, or anything authored in arbitrary units.\n\nUses the exact analytic bounding box when available (see `path_aabb`), falling back to the discretized outline for black-box samplers or arcs under non-uniform transforms.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "path_aabb" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path with analytic segment topology (e.g. from `circle`, `rect`, `path()` pen ops, `trace_svg_path`, `text_to_path`)."
          },
        ],
        description: "Returns the exact axis-aligned bounding box of a 2D path as a 2-element sequence `[mins, maxs]` of Vec2 corners.\n\nThe bound is computed analytically from the path's line segments, beziers, and arcs (exact modulo floating-point rounding), not from a polyline discretization. Any transforms applied to the path are baked in. Errors if the path is empty, or if it contains arc segments under a non-uniform transform (skew or non-uniform scale), since transformed arcs are conics with no closed-form axis-aligned bound; bake the transform with `apply_transforms` first in that case.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "is_self_intersecting" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "The mesh to check for self-intersections"
          },
        ],
        description: "Checks if a mesh has any self-intersecting triangles.  Returns `nil` if no self-intersections are found.  If a self-intersection is found, returns a map with keys: `tri0` and `tri1` (sequences of 3 Vec3 vertices for each intersecting triangle), `point` (a Vec3 on the intersection), and `type` (\"segment\" or \"coplanar\").",
        return_type: &[ArgType::Nil, ArgType::Map],
      },
    ],
  },
  "intersects_ray" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 36 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "ray_origin",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "ray_direction",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max_distance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max distance to check for intersection (`nil` considers intersections at any distance), measured in multiples of `ray_direction` (world distance for a unit vector).  If the intersection occurs at a distance greater than this, `false` will be returned."
          },
        ],
        description: "Casts a ray from `ray_origin` in `ray_direction` and checks if it intersects `mesh` within `max_distance` (or any distance if `max_distance` is `nil`)",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "len" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the length/magnitude of a Vec3",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the length/magnitude of a Vec2",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the number of unicode characters in a string",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the number of elements in a sequence.  This will fully evaluate the sequence.  Calling this with an infinite sequence will result in the program hanging or crashing.",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "m",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the number of vertices in a mesh",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-texel length/magnitude of a texture's channel vector, producing a 1-channel texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the length/magnitude of a Vec4",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "m",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the number of entries in a map",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Total arc length of a path over all its subpaths. Lazy paths are measured by sampling.",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "chars" => FnDef {
    module: "str",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "s",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a sequence of the unicode characters in a string",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "assert" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "condition",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "message",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("Assertion failed".to_string())),
            description: "Optional message to include in the error if the assertion fails"
          },
        ],
        description: "Raises an error if `condition` is false, optionally including `message` in the error",
        return_type: &[ArgType::Nil],
      },
    ],
  },
  "distance" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`sqrt((a.x - b.x)^2 + (a.y - b.y)^2 + (a.z - b.z)^2)`",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`sqrt((a.x - b.x)^2 + (a.y - b.y)^2`",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-texel euclidean distance between two textures' channel vectors, producing a 1-channel texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "`sqrt((a.x - b.x)^2 + (a.y - b.y)^2 + (a.z - b.z)^2 + (a.w - b.w)^2)`",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "normalize" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a normalized Vec3 (length 1) in the same direction as the input vector",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a normalized Vec2 (length 1) in the same direction as the input vector",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-texel normalization of a texture's channel vector to length 1",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a normalized Vec4 (length 1) in the same direction as the input vector",
        return_type: &[ArgType::Vec4],
      },
    ],
  },
  "dot" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Dot product of two Vec3s: `a.x*b.x + a.y*b.y + a.z*b.z`",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Dot product of two Vec2s: `a.x*b.x + a.y*b.y`",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-texel dot product of two textures' channel vectors, producing a 1-channel texture",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec4),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Dot product of two Vec4s: `a.x*b.x + a.y*b.y + a.z*b.z + a.w*b.w`",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "cross" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Cross product of two Vec3s, returning the Vec3 perpendicular to both (right-handed)",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "bezier3d" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "p0",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "p1",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "p2",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "p3",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Generates a sequence of `count` evenly-spaced points along a cubic Bezier curve defined by four control points",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "superellipse" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Parameter in [0, 1] specifying the position along the superellipse perimeter"
          },
          ArgDef {
            name: "n",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Exponent that controls the shape of the superellipse.  A value of 2 produces an ellipse, higher values produce more rectangular shapes, and lower values produce diamond and star-like shapes."
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
        ],
        description: "Returns the `Vec2` position at parameter `t` along the perimeter of a superellipse",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "superellipse_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "n",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Exponent that controls the shape of the superellipse.  A value of 2 produces an ellipse, higher values produce more rectangular shapes, and lower values produce diamond and star-like shapes."
          },
          ArgDef {
            name: "point_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of points to generate along the path"
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
        ],
        description: "Generates a sequence of points defining a superellipse, or rounded rectangle.  Returns a sequence of `point_count` `Vec2` points",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "extrude_pipe" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 8 }, FnExample { composition_id: 41 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Radius of the pipe or a callable with signature `|point_ix: int, path_point: vec3|: float | seq` that returns the radius at each point along the path.  If a sequence is returned by the callback, its length must match the resolution and it should contain the distance of each point along the ring from the pipe's center."
          },
          ArgDef {
            name: "resolution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(8)),
            description: "Number of segments to use for the pipe's circular cross-section"
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of Vec3 points defining the path of the pipe.  Often the output of a function like `bezier3d`."
          },
          ArgDef {
            name: "close_ends",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "Whether to close the ends of the pipe with triangle fans"
          },
          ArgDef {
            name: "connect_ends",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "Whether the pipe should be a closed loop, connecting the last point back to the first.  If true, the first and last points of the path will be connected with triangles."
          },
          ArgDef {
            name: "twist",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Callable),
            default_value: DefaultValue::Optional(|| Value::Float(0.)),
            description: "Twist angle in radians to apply along the path, or a callable with signature `|point_ix: int, path_point: vec3|: float` that returns the twist angle at each point along the path.  A value of 0 means no twist."
          },
          ArgDef {
            name: "adaptive_path_sampling",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true (default), uses curvature-aware adaptive sampling to distribute path points. Concentrates vertices in high-curvature regions of the path, improving mesh quality at bends while using fewer vertices on straight sections. Set to false to use the input points directly without resampling. Note: automatically disabled when a dynamic twist callable is provided, as twist complicates the geometry in ways the adaptive sampler cannot account for."
          }
        ],
        description: "Extrudes a pipe along a sequence of points.  The radius can be constant or vary along the path using a callable.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "rail_sweep" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "spine_resolution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of samples to take along the spine"
          },
          ArgDef {
            name: "ring_resolution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Number of samples to take around each profile ring. For a multi-subpath profile (holed/disjoint), each subpath gets this full count (they are not split). Pass a sequence of ints to set a per-subpath count instead (in sampler subpath order); its length must match the number of subpaths."
          },
          ArgDef {
            name: "spine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Spine definition as a callable with signature `|u: float|: vec3` or a sequence of Vec3 points.  When a sequence is provided, the default behavior is to use the points as-is (requiring `spine_resolution` to match the sequence length); pass an explicit `spine_sampling_scheme` such as \"uniform\" or \"chebyshev\" to resample the polyline."
          },
          ArgDef {
            name: "profile",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Path, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Callable with signature `|u: float, v: float, u_ix: int, v_ix: int, spine_center: vec3|: vec2` that returns the local (x, y) offset for each point in the ring. Optional when `dynamic_profile` is provided."
          },
          ArgDef {
            name: "frame_mode",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::String("rmf".to_owned())),
            description: "Frame mode for the sweep.  Use \"rmf\" (default) for rotation-minimizing frames, or provide a Vec3 up direction for fixed-up framing."
          },
          ArgDef {
            name: "twist",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Callable),
            default_value: DefaultValue::Optional(|| Value::Float(0.)),
            description: "Twist angle in radians to apply per ring, or a callable with signature `|point_ix: int, spine_center: vec3|: float`."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "Whether to connect the last ring back to the first."
          },
          ArgDef {
            name: "capped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "Whether to cap the start and end rings with triangulated faces.  Ignored when `closed` is true.  For an open (non-closed) profile there is no closed boundary ring to cap: an explicit `capped=true` errors, while the default is silently treated as uncapped.  For holed/disjoint (multi-subpath) profiles, caps are triangulated per nesting group (outer + its holes) and are only available in wasm builds."
          },
          ArgDef {
            name: "profile_samplers",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Path, ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional path or sequence of paths whose critical points align the profile's `v` sampling."
          },
          ArgDef {
            name: "dynamic_profile",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Alternative to `profile` (mutually exclusive). Callable with signature `|u: float, u_ix: int|: path | { sampler: |v|: vec2, path_samplers: path | Seq<path>, sharp: bool, adaptive: bool, closed: bool }`. Returns either a path directly (critical points and subpath topology extracted automatically if it's a PathTracerCallable) or a map with a sampler plus optional keys: `path_samplers` (trace_paths for critical points), `sharp` (mark ring edges as sharp), `adaptive` (override global adaptive_profile_sampling for this ring), `closed` (for a black-box sampler, whether the profile ring wraps closed — default true; an open profile sweeps into a sheet with boundary; not valid for multi-subpaths, whose per-subpath openness comes from their own topology). Multi-subpath profiles (disjoint loops, or nested opposite-winding loops for a hollow tube) require a real path and a topology (count/closedness/winding/nesting) that stays constant along the spine. More efficient than `profile` for dynamic profiles as it's called once per ring instead of per vertex. Cannot be used together with `profile_samplers`."
          },
          ArgDef {
            name: "fku_stitching",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "When true (default), uses the Fuchs/Kedem/Uselton (FKU) dynamic programming algorithm to find optimal triangulation between adjacent rings. FKU minimizes total edge length, producing better triangle quality when ring vertex counts differ or when vertices drift between rings. When false, uses simple quad-based stitching for predictable topology and uniform wireframe appearance."
          },
          ArgDef {
            name: "spine_sampling_scheme",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Sequence, ArgType::Callable, ArgType::Map, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Controls how sample points are distributed along the spine. When unset (the default), sequence spines are used as-is (requires `spine_resolution` to match the sequence length) and callable spines fall back to \"uniform\". String options: \"passthrough\"/\"as_is\"/\"raw\" - use sequence points as-is (sequence input only); \"uniform\" - evenly spaced samples; \"chebyshev\"/\"cos\"/\"cosine\" - Chebyshev node spacing (denser near endpoints); \"superellipse\"/\"bevel\" - superellipse-adapted spacing with default exponent 5; \"adaptive\" - curvature-aware sampling that concentrates vertices in high-curvature regions of the spine. Map options: { type: \"uniform\" }, { type: \"chebyshev\" }, { type: \"superellipse\", exponent: N } - superellipse-adapted spacing where higher exponent concentrates samples more at endpoints (exponent=2 is similar to chebyshev, exponent>2 is progressively sharper), { type: \"adaptive\" } - adaptive curvature-based sampling. Also accepts: a sequence of floats - explicit t values in [0, 1] (length must match spine_resolution); a callable `|i: int|: float` - returns t value for each sample index. The t values are used both for sampling the spine/rail and as the `u` parameter passed to the profile function."
          },
          ArgDef {
            name: "adaptive_profile_sampling",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "When true (default), uses curvature-aware adaptive sampling for profile rings. Concentrates vertices in high-curvature regions of the profile curve, often achieving equivalent visual quality with significantly fewer vertices. Only applies when using `dynamic_profile`. Can be overridden per-ring by returning `{ sampler: ..., adaptive: true/false }` from the dynamic_profile callable. Set to false to use uniform sampling."
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true, splits vertices at the profile UV seam (so textures wrap seamlessly, V: 0→1) and duplicates cap rings (so caps get planar UVs + an in-plane tangent + a sharp cap edge). Because this duplicates coincident vertices, the result is NO LONGER watertight 2-manifold; intended for final render meshes, not as input to CSG/boolean ops. Default false: shared vertices and watertight topology, at the cost of one smeared seam column and caps that inherit the body's swept UV/tangent."
          },
          ArgDef {
            name: "cap_uv_scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Optional(|| Value::Vec2(Vec2::new(1., 1.))),
            description: "Extra per-axis multiplier on the planar cap UVs when `split_seams` is set. Caps are baked to match the swept walls' texel density automatically: U in world units (like the body's arc-length U) and V normalized by the cap's mean loop perimeter (like the body's per-loop [0,1] V), so the same material `uvScale` reads consistently across the seam and holed caps land between the outer/inner wall density. This multiplier layers on top for manual tweaks; default (1, 1) keeps the matched density. No effect without `split_seams` (unsplit caps inherit the body's swept UV)."
          },
          ArgDef {
            name: "crease_angle_threshold_deg",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Experimental strength-aware profile sampling and FKU stitching. nil (default) preserves legacy critical-point handling. A number from 0 to 180 enables automatic turning-angle measurements at the existing profile guides after interpolation/transforms. Guides below this angle in degrees no longer reserve samples; endpoints and unmeasurable guides are retained. FKU's critical-pair attraction fades smoothly from zero at a flat guide to its existing full strength at 15 degrees, limited by the weaker endpoint. Use 0 to test weighting without dropping guides, or try 1 to ignore almost-flat guides. This measures profile turns, not 3D dihedrals, and does not guarantee crease connections."
          },
        ],
        description: "Sweeps a profile along a spine to produce a mesh.  Profile points are connected in increasing `v` order; for outward-facing normals, the profile winding should be counter-clockwise when viewed along the local tangent.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "parametric_surface" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 82 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "u_res",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of segments along the U axis"
          },
          ArgDef {
            name: "v_res",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of segments along the V axis"
          },
          ArgDef {
            name: "u_closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, connects the U-end back to U-start (creates cylindrical topology along U)"
          },
          ArgDef {
            name: "v_closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, connects the V-end back to V-start (creates cylindrical topology along V)"
          },
          ArgDef {
            name: "flip_normals",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, reverses triangle winding to flip normal direction. By default, normals follow the cross product of U and V tangents."
          },
          ArgDef {
            name: "generator",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "A function `|u: num, v: num| -> vec3` that returns the 3D position for each (u, v) coordinate in [0, 1]"
          },
          ArgDef {
            name: "fku_stitching",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "When true (default), uses the Fuchs/Kedem/Uselton (FKU) dynamic programming algorithm to find optimal triangulation between adjacent rows. FKU minimizes total edge length, avoiding sharp angles and long edges when vertex positions drift between rows. When false, uses simple quad-based stitching for predictable topology and uniform wireframe appearance."
          },
          ArgDef {
            name: "adaptive_u_sampling",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true, uses curvature-aware adaptive sampling along the U axis. Concentrates rows in high-curvature U regions of the surface. Disabled by default since parametric_surface is very general-purpose and adaptive sampling may not always be appropriate."
          },
          ArgDef {
            name: "adaptive_v_sampling",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true, uses curvature-aware adaptive sampling along the V axis within each row. Concentrates vertices in high-curvature V regions. Works well with FKU stitching which can handle rows with non-uniform vertex distributions. Disabled by default since parametric_surface is very general-purpose and adaptive sampling may not always be appropriate."
          },
          ArgDef {
            name: "capped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true, triangulates the open ends of a tube-like surface (exactly one of `u_closed`/`v_closed` is true).  Each end-ring is projected to its best-fit plane (via Newell's normal oriented along the axis) and triangulated with CGAL's constrained Delaunay triangulation (the same machinery rail_sweep uses), and it handles concave/non-convex ring shapes correctly.  Rings that aren't co-planar are projected as a best effort.  Cap winding follows `flip_normals` so the caps match the surface's facing direction.  No-op when both axes are closed (torus) or neither is (sheet)."
          },
        ],
        description: "Generates a mesh from a parametric function over a 2D domain. Handles topological wrapping via the closed flags and automatically welds coincident vertices at boundary poles to maintain manifold topology. By default, normals point outward when V increases counter-clockwise (looking down +Y); use flip_normals=true to reverse.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "torus_knot_path" => FnDef {
    module: "path",
    examples: &[FnExample { composition_id: 13 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "tube_radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "p",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of times the knot wraps around the torus in the longitudinal direction"
          },
          ArgDef {
            name: "q",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of times the knot wraps around the torus in the meridional direction"
          },
          ArgDef {
            name: "point_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of points to generate along the path"
          },
        ],
        description: "Generates a sequence of points defining a torus knot path",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "lissajous_knot_path" => FnDef {
    module: "path",
    examples: &[FnExample { composition_id: 35 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "amp",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(1., 1., 1.))),
            description: "Amplitude of the Lissajous curve in each axis"
          },
          ArgDef {
            name: "freq",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(3., 5., 7.))),
            description: "Frequency of the Lissajous curve in each axis.  These should be \"pairwise-coprime\" integers, meaning that the ratio of any two frequencies should not be reducible to a simpler fraction."
          },
          ArgDef {
            name: "phase",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., PI / 2., PI / 5.))),
            description: "Phase offset of the Lissajous curve in each axis"
          },
          ArgDef {
            name: "count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of points to sample along the path"
          },
        ],
        description: "Generates a sequence of points defining a Lissajous knot path",
        return_type: &[ArgType::Sequence],
      }
    ]
  },
  "extrude" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "up",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Direction to extrude the mesh.  Each vertex will be displaced by this amount.  If a callable is provided, it should have signature `|vertex_pos: vec3|: vec3` and return the displacement for each vertex.  An optional second parameter must be a destructured attribute bag, e.g. `|pos, {uv, ix}|`; see `warp`."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          }
        ],
        description: "Extrudes a mesh in the direction of `up` by displacing each vertex along that direction.  This is designed to be used with 2D meshes; using it on meshes with volume or thickness will probably not work.",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "extrude_along_normals" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "distance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Distance to extrude each vertex along its computed normal.  If a callable is provided, it should have signature `|vertex_pos: vec3|: num` and return the per-vertex distance.  An optional second parameter must be a destructured attribute bag, e.g. `|pos, {uv, ix}|`; see `warp`."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          }
        ],
        description: "Extrudes a 2D mesh into a shell along its per-vertex normals.  Normals are computed per connected component as the area-weighted average of adjacent face normals.\n\nUseful for adding thickness to a surface that follows a curve through space, e.g. a mesh draped over a curved surface via `warp`, where a single global `up` direction won't follow the surface curvature.\n\nLike `extrude`, designed for 2D (single-layer) meshes: the input is duplicated, displaced, the original is flipped to face the other way, and boundary edges are stitched into side walls to produce a closed 2-manifold shell.\n\nFails to produce a clean shell when the offset distance exceeds the local radius of curvature on concave regions; adjacent normals converge past their starting positions and the duplicated layer self-intersects.  For mild undulation this is a non-issue.",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "extrude_path" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path, ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Either:\n - A path, sampled adaptively per `curve_angle_degrees` and optionally capped by `sample_count` (including lazy paths).  Returned 2D points are embedded in the XZ plane (`vec2(x, y)` → `vec3(x, 0, y)`).\n - A `Seq<Vec2 | Vec3>` of pre-discretized points used as-is (no resampling).  `Vec2` points are embedded in the XZ plane; `Vec3` points are used directly, which lets you extrude a polyline that already lives in 3D space.  Errors if the sequence has fewer than 2 points or contains any other element type."
          },
          ArgDef {
            name: "up",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "Direction to sweep the path along.  Each point in the path is duplicated at `+up` to form the top of the strip."
          },
          ArgDef {
            name: "flipped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, reverses the winding order of the generated triangles, flipping the strip's facing direction."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves in a path input, including lazy paths."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional cap on total points across all subpaths after adaptive materialization, including lazy paths. A cap may sacrifice the curve-angle tolerance."
          },
        ],
        description: "Sweeps a path along an `up` vector to produce a triangle-strip surface mesh.  Each point along the path is duplicated at `+up` to form the top edge of the strip.\n\nMultiple subpaths (from path inputs) are extruded independently; closed paths are not supported and trigger an error.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "stitch_contours" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 22 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "contours",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "A `Seq<Seq<Vec3>>`, where each inner sequence contains points representing a contour that will be stitched together into a mesh"
          },
          ArgDef {
            name: "flipped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the winding order of the triangles generated will be flipped - inverting the inside/outside of the generated mesh."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, the contours will be stitched together as closed loops - connecting the last point to the first for each one."
          },
          ArgDef {
            name: "cap_start",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, a triangle fan will be created to cap the first contour"
          },
          ArgDef {
            name: "cap_end",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, a triangle fan will be created to cap the last contour"
          },
          ArgDef {
            name: "cap_ends",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "shorthand for `cap_start=true, cap_end=true`"
          }
        ],
        description: "Stitches together a sequence of contours into a single mesh.  The contours should be closed loops.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "trace_geodesic_path" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 49 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "A sequence of `Vec2` points representing movements to take across the surface of the mesh relative to the current position.  For example, a sequence of `[vec2(0, 1), vec2(1, 0)]` would move 1 unit up and then 1 unit right."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "world_space",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, points will be returned in world space.  If false, they will be returned in the local space of the mesh."
          },
          ArgDef {
            name: "full_path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "This controls behavior when the path crosses between faces in the mesh.  If true, intermediate points will be included in the output for whenever the path hits an edge.  This can result in the output sequence having more elements than the input sequence, and it will ensure that all generated edges in the output path lie on the surface of the mesh."
          },
          ArgDef {
            name: "start_pos_local_space",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "If provided, the starting position for the path will be snapped to the surface of the mesh at this position.  If `nil`, the walk will start at an arbitrary point on the mesh surface."
          },
          ArgDef {
            name: "up_dir_world_space",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 1., 0.))),
            description: "When the walk starts, it will be oriented such that the positive Y axis of the local tangent space is aligned as closely as possible with this up direction at the starting position.  Another way of saying this is that it lets you set what direction is north when first starting the walk on the mesh's surface."
          },
        ],
        description: "Traces a geodesic path across the surface of a mesh, following a sequence of 2D points.  The mesh must be manifold.\n\nReturns a `Seq<Vec3>` of points on the surface of the mesh that were visited during the walk.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "text_to_mesh" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 69 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "text",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "font_family",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("IBM Plex Sans".to_string())),
            description: "Font family to use for the text.  Must exist on Google Fonts."
          },
          ArgDef {
            name: "font_size",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(24.)),
            description: ""
          },
          ArgDef {
            name: "font_weight",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil, ArgType::String),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: ""
          },
          ArgDef {
            name: "font_style",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Must be one of \"normal\", \"italic\", or \"oblique\".  If nil, defaults to \"normal\".",
          },
          ArgDef {
            name: "letter_spacing",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: ""
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Width of the generated mesh in world units along the X axis.  If only one of `width` or `height` is provided, the other dimension will be scaled to maintain the aspect ratio of the text."
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Height of the generated mesh in world units along the Z axis.  If only one of `width` or `height` is provided, the other dimension will be scaled to maintain the aspect ratio of the text.",
          },
          ArgDef {
            name: "depth",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Float(0.2)),
            description: "Depth to extrude the text into a 3D mesh.  If 0 or nil, will produce a flat 2D mesh in the XZ plane."
          },
        ],
        description: "Generates a 2D path representing the given text string, triangulates it into a mesh, and optionally extrudes it into 3D.  The generated mesh lies in the XZ plane, with Y being the up direction.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "text_to_svg" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "text",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: "",
          },
          ArgDef {
            name: "font_family",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("IBM Plex Sans".to_string())),
            description: "Font family to use for the text. Must exist on Google Fonts.",
          },
          ArgDef {
            name: "font_size",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(24.)),
            description: "",
          },
          ArgDef {
            name: "font_weight",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil, ArgType::String),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "",
          },
          ArgDef {
            name: "font_style",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Must be one of \"normal\", \"italic\", or \"oblique\". If nil, defaults to \"normal\".",
          },
          ArgDef {
            name: "letter_spacing",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "",
          },
        ],
        description: "Fetches the SVG path data string for the given text in the specified font. Requires an internet connection to fetch from the font server.",
        return_type: &[ArgType::String],
      },
    ],
  },
  "text_to_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "text",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: "",
          },
          ArgDef {
            name: "font_family",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("IBM Plex Sans".to_string())),
            description: "Font family to use for the text. Must exist on Google Fonts.",
          },
          ArgDef {
            name: "font_size",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(24.)),
            description: "",
          },
          ArgDef {
            name: "font_weight",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil, ArgType::String),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "",
          },
          ArgDef {
            name: "font_style",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Must be one of \"normal\", \"italic\", or \"oblique\". If nil, defaults to \"normal\".",
          },
          ArgDef {
            name: "letter_spacing",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "",
          },
        ],
        description: "Fetches SVG path data for the given text and returns a path. Suitable for use with `tessellate_path`, `render_path`, `subpaths`, etc.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "alpha_wrap_3d" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 57 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "alpha",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1. / 30.)),
            description: "Controls the feature size of the computed wrapping.  Smaller values will capture more detail and produce more faces.\n\nThis value is relative to the bounding box of the input mesh.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.03)),
            description: "Controls the offset distance between the the input and the wrapped output mesh surfaces.  Larger values will result in simpler outputs with better triangle quality, potentially at the cost of sharp edges and fine details.\n\nThis value is relative to the bounding box of the input mesh.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "manifold",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "Post-processes the wrap so it is geometrically 2-manifold as well as combinatorially (no separate sheets touching at a vertex or edge).  Disable for a slightly faster wrap that may contain such pinches."
          },
          ArgDef {
            name: "seeds",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional sequence of Vec3 points from which the wrap starts instead of from the outside.  Each seed must sit inside a cavity of the input with room for a sphere of radius `alpha` around it; the result is then the wrap of that enclosed space (e.g. the inside of a room) rather than of the outside."
          },
        ],
        description: "Computes an alpha-wrap of a mesh.  This is kind of like a concave version of a convex hull.  This function is guaranteed to produce watertight/2-manifold outputs.  `alpha_wrap` is an alias.\n\nFor more details, see here: https://doc.cgal.org/latest/Alpha_wrap_3/index.html",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "alpha",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1. / 30.)),
            description: "Controls the feature size of the computed wrapping.  Smaller values will capture more detail and produce more faces.\n\nThis value is relative to the bounding box of the input points.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.03)),
            description: "Controls the offset distance between the the input and the wrapped output mesh surfaces.  Larger values will result in simpler outputs with better triangle quality, potentially at the cost of sharp edges and fine details.\n\nThis value is relative to the bounding box of the input points.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "manifold",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "Post-processes the wrap so it is geometrically 2-manifold as well as combinatorially (no separate sheets touching at a vertex or edge).  Disable for a slightly faster wrap that may contain such pinches."
          },
          ArgDef {
            name: "seeds",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional sequence of Vec3 points from which the wrap starts instead of from the outside.  Each seed must sit inside a cavity of the input with room for a sphere of radius `alpha` around it; the result is then the wrap of that enclosed space (e.g. the inside of a room) rather than of the outside."
          },
        ],
        description: "Computes an alpha-wrap of a sequence of points.  This is kind of like a concave version of a convex hull.  This function is guaranteed to produce watertight/2-manifold outputs.\n\nFor more details, see here: https://doc.cgal.org/latest/Alpha_wrap_3/index.html",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "alpha_wrap_2d" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path.  Every subpath is discretized into line segments; closed subpaths are sealed, open ones are treated as strokes."
          },
          ArgDef {
            name: "alpha",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1. / 30.)),
            description: "Controls the feature size of the computed wrapping: the wrap can only enter gaps and concavities wider than about `alpha`.  Smaller values follow the input more closely and produce more vertices.\n\nThis value is relative to the bounding box of the input.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.03)),
            description: "Distance between the input and the output boundary.  Larger values give simpler, rounder outlines at the cost of sharp corners and fine detail.\n\nThis value is relative to the bounding box of the input.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "manifold",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "Post-processes the wrap so its boundary is geometrically 1-manifold as well as combinatorially (no separate loops touching at a vertex).  Disable for a slightly faster wrap that may contain such pinches."
          },
          ArgDef {
            name: "seeds",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional sequence of Vec2 points from which the wrap starts instead of from the outside.  Each seed must sit inside an enclosed region of the input with room for a disk of radius `alpha` around it; the result is then the wrap of that enclosed space (an inset of the region's boundary) rather than of the outside."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(128)),
            description: "Initial uniform probe count for lazy paths. Critical points and structural boundaries are also sampled, then refined adaptively per curve_angle_degrees; this is not an output cap."
          },
        ],
        description: "Computes a 2D alpha-wrap of a path: a simple, hole-aware outline that strictly encloses every segment of the input, roughly `offset` away from it, with concavities narrower than `alpha` filled in.  Think of it as a concave hull of the strokes and filled regions of the path.  Overlapping or self-intersecting subpaths and open strokes are all fine as input.  The output is a polyline path (no continuous curve detail) with holes represented as nested subpaths under even-odd filling.\n\nFor more details, see here: https://doc.cgal.org/latest/Alpha_wrap_2/index.html",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "A sequence of Vec2 points"
          },
          ArgDef {
            name: "alpha",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1. / 30.)),
            description: "Controls the feature size of the computed wrapping: the wrap can only enter gaps between points wider than about `alpha`, so larger values merge nearby points into one blob.  Smaller values produce tighter outlines around each cluster.\n\nThis value is relative to the bounding box of the input points.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.03)),
            description: "Distance between the input points and the output boundary.  Larger values give simpler, rounder outlines.\n\nThis value is relative to the bounding box of the input points.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "manifold",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "Post-processes the wrap so its boundary is geometrically 1-manifold as well as combinatorially (no separate loops touching at a vertex).  Disable for a slightly faster wrap that may contain such pinches."
          },
          ArgDef {
            name: "seeds",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional sequence of Vec2 points from which the wrap starts instead of from the outside.  Each seed must sit inside an enclosed region of the input with room for a disk of radius `alpha` around it; the result is then the wrap of that enclosed space rather than of the outside."
          },
        ],
        description: "Computes a 2D alpha-wrap of a set of points: a simple, hole-aware outline enclosing all of them, roughly `offset` away from the outermost points, with gaps narrower than `alpha` closed over.  Useful for blob-like silhouettes around scattered points.  The output is a polyline path with holes represented as nested subpaths under even-odd filling.\n\nFor more details, see here: https://doc.cgal.org/latest/Alpha_wrap_2/index.html",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "compute_uvs" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "type",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("auto".to_owned())),
            description: "UV generation method.  \"auto\", \"unwrap\", \"disk\", and \"sphere\" produce a minimal-distortion conformal unwrap via the BFF module.  \"planar\" projects onto the mesh's dominant plane natively.  \"cylindrical\" wraps U around the auto-detected axis exactly once (seamless 0/1, meridian seam split); V is scaled by the tube circumference so texels stay square (isotropic), and cap faces get a planar disk projection at the tube's density.  It works best on tube-dominant meshes (length > diameter) where the axis is unambiguous and mostly straight.  \"tube\" generalizes it to bent/deformed closed tube-like meshes (elbows, arches, trim, pipes) via harmonic fields: U wraps the cross-section once, V runs along the tube arc-length-uniformly, and crease-bounded end caps become planar islands (see `options`).  Requires closed genus-0.  \"strip\" is a solver-free topological trim-strip unwrap: the mesh is split into smooth patches at sharp edges, and each patch that is a quad/tri strip (dual graph a path or ring) is mapped STRAIGHT in UV space — U = arc length along the strip's rails, V constant per rail — so a horizontally-patterned texture flows smoothly along a curved strip.  Ring strips are cut and rounded to an integer repeat count; non-strip patches fall back to planar islands (see `options`).  Best for hard-edged extrusions/profiles (square/polygonal pipes, frames, trim).  \"toroidal\" (closed genus-1) is reserved and not yet implemented.",
          },
          ArgDef {
            name: "scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Multiplier applied to the generated UV coordinates.  Larger values tile a texture more densely.  For \"cylindrical\" and \"tube\", integer values preserve the seamless wrap.  Closed \"strip\" rings stay seamless at any scale (their repeat count is rounded to an integer after scaling).",
          },
          ArgDef {
            name: "n_cones",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: "Number of cone singularities for the conformal unwrap.  0 lets BFF pick automatically for closed surfaces.  Ignored by non-BFF types.",
          },
          ArgDef {
            name: "island_rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "Ignored; kept so older programs still parse.  Island orientation for BFF types is controlled via `options` (`up`, `fallback`, `axis`).",
          },
          ArgDef {
            name: "options",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Map of options specific to the chosen `type`.  For \"auto\"/\"unwrap\"/\"disk\"/\"sphere\": `{ up: '+y', fallback: '-z', axis: '+v', pre_split: true }` — when `up` is given, each UV island is rotated so texture axis `axis` ('+v' = texture up, or '+u'/'-v'/'-u') follows that local-space direction on every face, with faces whose normal is within 15° of `up` using `fallback` instead; omit `up` to let islands rotate freely for tighter packing.  `pre_split` (default true) splits sharp edges topologically (per the sharp-angle threshold) before the conformal solve, so creases become boundaries the unwrap can cut islands along — usually what you want for hard-surface meshes; set false to unwrap the welded smooth surface instead (note \"sphere\" mapping needs closed topology, so on a creased mesh it only engages with `pre_split: false`).  For \"cylindrical\": `{ normalize_v: bool }` — when true, V is stretched to span 0..1 across the mesh's axial extent instead of being circumference-scaled for square texels.  For \"tube\": `{ caps: 'auto'|'none', cap_angle: degrees, cap_max_span: float, cap_alignment: float, normalize_v: bool, seam_straightness: float, detwist: bool }` — `caps` toggles end-cap island detection; `cap_angle` is the crease angle bounding a cap patch (defaults to the mesh sharp-edge threshold); `cap_max_span` is the max fraction of tube length a cap may span (default 0.15); `cap_alignment` is the min alignment between cap normal and local tube direction, 0..1 (default 0.6); `normalize_v` makes V span 0..1 over the tube length instead of isotropic scaling; `seam_straightness` (default 8) penalizes the internal seam cut for traveling around the tube instead of along it, preventing the texture from twisting along the spine (0 = plain shortest-path seam); `detwist` (default true) cancels any residual rotational drift of U along the spine by referencing a rotation-minimizing frame.  For \"strip\": `{ strip_angle: degrees, layout: 'stack'|'overlap'|'fill', u_mode: 'uniform'|'rail', fallback: 'planar'|'error' }` — `strip_angle` is the crease angle that splits the mesh into patches (defaults to the mesh sharp-edge threshold); `layout` places islands in V: 'stack' (default) stacks them at integer V offsets, 'overlap' gives every island the same V band starting at 0, 'fill' stretches each island's V to span 0..1 with U scaled to keep texels square; `u_mode` 'uniform' (default) gives both rails a shared U so quads map to true rectangles (a ring band's inner rail stretches instead of shearing), 'rail' keeps each rail's own arc length (exact per-rail texel density, shears when rail lengths differ); `fallback` controls non-strip patches: 'planar' (default) maps them as planar islands, 'error' fails so unexpected topology is surfaced.",
          },
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "When given, registers an editor control for this call's UV params (like the `input_*` builtins): the UI's stored value overrides `type`/`scale`/`n_cones`/`options`, which act as defaults.  Unique per node; always pass as a kwarg (`name='...'`).",
          },
          ArgDef {
            name: "label",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Display label for the control; falls back to `name`.",
          },
        ],
        description: "Procedurally generates UV coordinates and tangents for a mesh, returning a new mesh with `uv` and `tangent` vertex attributes populated.  Conformal types (auto/unwrap/disk/sphere) are backed by the BFF unwrap module; \"planar\" is a native planar projection.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "type",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: "UV generation method; see the first signature for the full list.",
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Multiplier applied to the generated UV coordinates.",
          },
          ArgDef {
            name: "n_cones",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: "Number of cone singularities for the conformal unwrap; ignored by non-BFF types.",
          },
          ArgDef {
            name: "options",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Map of options specific to the chosen `type`; see the first signature.",
          },
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "When given, registers an editor control for this call's UV params; see the first signature.",
          },
          ArgDef {
            name: "label",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Display label for the control; falls back to `name`.",
          },
        ],
        description: "Pipe-friendly form with `type` first: `mesh | compute_uvs('planar', scale=2)` partially applies and the piped mesh fills the `mesh` slot.  Args past `type` should be passed as kwargs when piping.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "compute_normals" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "smooth_angle",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Dihedral crease threshold in degrees: an edge whose two faces differ in orientation by more than this becomes a sharp (hard) shading edge, while smoother edges are shaded smooth.  Larger values smooth more.  Defaults to the runtime sharp-angle threshold (see `set_sharp_angle_threshold`).",
          },
        ],
        description: "Recomputes a mesh's shading normals using dihedral-angle auto-smoothing (like Blender's shade-auto-smooth), replacing any existing normals so hard edges get crisp creasing while smooth regions stay smooth.  Existing `uv`/`tangent` attributes are preserved, and authored attribute seams (e.g. a `rail_sweep` `split_seams` UV seam, or a `compute_uvs` cut) that run through a smooth region are re-smoothed so they don't read as a lighting crease.  Useful after `rail_sweep` with `split_seams` (which bakes fully-smooth normals) to restore crisp creasing without losing the procedural UVs.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "smooth" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "type",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("catmullclark".to_string())),
            description: "Type of smoothing to perform.  Supported values are \"catmullclark\", \"loop\", \"doosabin\", and \"sqrt\".",
          },
          ArgDef {
            name: "iterations",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(1)),
            description: "Number of smoothing iterations to perform"
          },
        ],
        description: "Smooths a mesh using the specified algorithm (defaults to catmullclark).  More iterations result in a smoother mesh.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "remesh_planar_patches" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max_angle_deg",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(5.)),
            description: "Maximum angle in degrees between face normals for faces to be considered coplanar"
          },
          ArgDef {
            name: "max_offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Maximum distance from the plane for faces to be considered coplanar.  This is an absolute distance in the mesh's local space.  If not provided or set to `nil`, it will default to 1% of the diagonal length of the mesh's bounding box."
          },
          ArgDef {
            name: "least_squares",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When false (default), each planar region is measured against the plane of its seed face (largest faces first), so every face in a region is within `max_angle_deg`/`max_offset` of one real input face.  When true, the region's plane is refit by least squares as it grows, which merges gently curved areas into fewer, larger facets that cut through the surface rather than lying on it."
          },
        ],
        description: "Remeshes a mesh by identifying planar regions and simplifying them into a simpler set of triangles.  This is useful for optimizing meshes produced by a variety of other built-in methods that tend to produce a lot of small triangles in flat areas.\n\nSee the docs for the underlying CGAL function for more details: https://doc.cgal.org/latest/Polygon_mesh_processing/index.html - Section 2.1.2 Remeshing",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "isotropic_remesh" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "target_edge_length",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Target edge length for the remeshed output.  Edges will be split or collapsed to try to achieve this length."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "iterations",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(1)),
            description: "Number of remeshing iterations to perform.  Typical values are between 1 and 5."
          },
          ArgDef {
            name: "protect_borders",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, edges on the border of the mesh will not be modified."
          },
          ArgDef {
            name: "protect_sharp_edges",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, edges with angles >= the `sharp_angle_threshold_degrees` will not be modified."
          },
          ArgDef {
            name: "sharp_angle_threshold_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(30.)),
            description: "Angle threshold in degrees for edges to be considered sharp.  Only used if `protect_sharp_edges` is true."
          },
        ],
        description: "Remeshes a mesh to have more uniform edge lengths, targeting the specified edge length.  \n\nSee the docs for the underlying CGAL function for more details: https://doc.cgal.org/latest/Polygon_mesh_processing/index.html - Section 2.1.2 Remeshing",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "delaunay_remesh" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "facet_distance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.14)),
            description: "This controls the precision of the output mesh.  Smaller values will produce meshes that more closely approximate the input, but will also produce more faces.  This value represents units in the local space of the mesh, so it must be chosen relative to the size of the input mesh."
          },
          ArgDef {
            name: "target_edge_length",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.2)),
            description: "This serves as an upper bound for the lengths of curve edges in the underlying Delaunay triangulation.  Smaller values will produce more faces.  This value represents units in the local space of the mesh, so it must be chosen relative to the size of the input mesh."
          },
          ArgDef {
            name: "protect_sharp_edges",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, edges with angles >= the `sharp_angle_threshold_degrees` will not be modified."
          },
          ArgDef {
            name: "sharp_angle_threshold_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(30.)),
            description: "Angle threshold in degrees for edges to be considered sharp.  Only used if `protect_sharp_edges` is true."
          },
        ],
        description: "Remeshes a mesh using a Delaunay refinement of a restricted Delaunay triangulation.  TODO improve docs once we understand the behavior of this function better.\n\nSee the docs for the underlying CGAL function for more details: https://doc.cgal.org/latest/Polygon_mesh_processing/index.html - Section 2.1.2 Remeshing",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "sample_voxels" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 72 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "dims",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: "The bounds of the voxel grid to sample"
          },
          ArgDef {
            name: "cb",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "A callable with signature `|x_ix: int, y_ix: int, z_ix: int|: bool | int | nil` that will be called for each voxel in the grid.  Returning 0, false, or nil will leave the voxel empty.  Returning true or any non-zero integer will fill the voxel.  Values > 1 will use material `n - 1` from the `materials` argument.  The maximum material index is 254."
          },
          ArgDef {
            name: "materials",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "An optional sequence of materials to assign to filled voxels.  The first material in the sequence will be used for voxels where the callback returns true or 1, the second material for voxels where the callback returns 2, and so on.  If not provided or set to `nil`, all filled voxels will use the default material.  The maximum number of materials is 254."
          },
          ArgDef {
            name: "cgal_remesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Determines whether the generated mesh(es) will be remeshed using CGAL's `remesh_planar_patches` function.  This produces better outputs with fewer faces and vertices, but takes more time to compute.  If not provided or set to `nil`, this will be dynamically enabled/disabled for each output mesh depending on its vertex count.",
          },
          ArgDef {
            name: "fill_internal_voids",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, internal voids in the generated meshes will be filled.  This can help reduce face/vertex counts, but requires extra computation."
          }
        ],
        description: "Generates a mesh or sequence of meshes by sampling a 3D grid defined by `dims`.  For each voxel in the grid, the provided callback will be called with the voxel's coordinate.  If the callback returns 0, false, or nil, the voxel will be left empty.  If it returns true or any non-zero integer, the voxel will be filled.  Values > 1 will use material `n - 1` from the `materials` argument.  A separate mesh will be generated for each unique material used.  The generated meshes are guaranteed to be 2-manifold/watertight, meaning that they can be further processed by other builtins like `smooth`, `simplify`, CSG (`union`/`intersect`/etc.).",
        return_type: &[ArgType::Mesh, ArgType::Sequence],
      }
    ],
  },
  "fan_fill" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 27 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "A sequence of Vec3 points representing the path to fill"
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, the path will be treated as closed - connecting the last point to the first."
          },
          ArgDef {
            name: "flipped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the winding order of the triangles generated will be flipped - inverting the inside/outside of the generated mesh."
          },
          ArgDef {
            name: "center",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "If provided, the center point for the fan will be placed at this position.  Otherwise, the center will be computed as the average of the points in the path."
          }
        ],
        description: "Builds a fan of triangles from a sequence of points, filling the area inside them.  One triangle will be built for each pair of adjacent points in the path, connecting them to the center point.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path.  Sampled in the XZ plane (`vec2(x, y)` -> `vec3(x, 0, y)`).  Multi-subpath inputs are fanned independently and combined into one mesh."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional override for treating each subpath as closed.  When nil (default), closedness is inherited from the path's topology (or inferred from `p(0) ~= p(1)` for black-box callables)."
          },
          ArgDef {
            name: "flipped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the winding order of the triangles generated will be flipped - inverting the inside/outside of the generated mesh."
          },
          ArgDef {
            name: "center",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Vec2, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "If provided, the center point for each fan will be placed at this position.  A `Vec2` is embedded in the XZ plane.  Otherwise, the center is computed as the average of each subpath's discretized points."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when adaptively discretizing curves, including lazy paths (`lerp_paths`, `catmull_rom`, `path(fn)`)."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional cap on total points across all subpaths after adaptive materialization, including lazy paths. When nil, use the curve-angle tolerance without a point cap."
          }
        ],
        description: "Builds a fan of triangles by discretizing a path and filling the area inside each subpath.  One triangle will be built per pair of adjacent points in each subpath, connecting to that subpath's center.  Output lives in the XZ plane.",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "tessellate_path" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A sequence of Vec2 points, a sequence of Vec2 sequences, or a path."
          },
          ArgDef {
            name: "flipped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the winding order of the generated triangles will be flipped."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing trace_path curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Maximum total number of output points across all subpaths. When provided, adaptively resamples the path to this count using curvature+arc-length weighting while preserving critical corners. When nil (default), uses the path's natural tessellation resolution."
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule used to identify the interior of the shape: \"nonzero\", \"evenodd\", \"positive\", or \"negative\".  If nil, inherited from the path (e.g. set via `trace_path`) and otherwise defaulting to \"nonzero\".\n\nThe CGAL engine always uses nesting-based fill (equivalent to \"evenodd\"); requesting any winding-dependent rule (\"nonzero\"/\"positive\"/\"negative\") with multiple subpaths under CGAL is a runtime error.  The lyon engine honors all four rules per its tessellator semantics."
          },
          ArgDef {
            name: "engine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Tessellation engine: \"cgal\" or \"lyon\".  If nil (default), the engine is chosen automatically.\n\n- CGAL is preferred: cleaner topology (no T-junctions), supports holes via nesting (any number of subpaths), supports mesh refinement via `max_edge_len`.\n- lyon is chosen when a winding-dependent fill rule (\"nonzero\"/\"positive\"/\"negative\") is requested with multiple subpaths, since CGAL only implements nesting-based fill.\n\nForcing engine=\"cgal\" in a case lyon would be auto-selected is a runtime error."
          },
          ArgDef {
            name: "max_edge_len",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Upper bound on the length of any triangle edge after refinement.  Triangles with a longer edge are split.  When set together with `min_angle_degrees` or used with the default angle bound, drives the density of the refined mesh.  CGAL-only; setting this with engine=\"lyon\" is a runtime error."
          },
          ArgDef {
            name: "min_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Lower bound (in degrees) on the smallest angle in any triangle after refinement.  Triangles with a smaller angle are split.  Range: [0, ~20.7]; 0 disables the shape criterion entirely (refinement then driven purely by `max_edge_len`).\n\nThe upper limit comes from CGAL: Delaunay refinement only provably terminates for aspect bounds up to 0.125 (squared sine), i.e. min angle ≲ 20.7°.  When this kwarg is omitted but `max_edge_len` is set, CGAL's default ~20.6° applies.  Narrow regions (e.g. slivers between holes) can then split well below `max_edge_len`.  Pass 0 if you want only the edge-length constraint."
          },
          ArgDef {
            name: "plane",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Coordinate plane the mesh is triangulated into, as a two-axis swizzle mapping the 2D (u, v) point to two of x/y/z (the remaining axis is 0).  Order matters: \"xz\" (default) maps u→x, v→z; \"zx\" maps u→z, v→x (a mirror embedding).  Any two distinct axes are accepted (\"xy\", \"yx\", \"xz\", \"zx\", \"yz\", \"zy\").  The default front face points along the +remaining axis (+Y for \"xz\"); use `flipped` to reverse it."
          },
        ],
        description: "Tessellates a 2D path into a triangle mesh, by default in the XZ plane (override with `plane`).\n\nPath topology (subpaths and the holes they imply via nesting) is preserved by both backends.  Build paths-with-holes by either using a multi-subpath path (e.g. `path([rect(...), rect(...) | reverse])`) or applying a Clipper2 boolean op upstream.\n\nCGAL refinement runs when either `max_edge_len` or `min_angle_degrees` is supplied; otherwise the raw constrained Delaunay triangulation is returned.  Each constraint independently triggers splitting of triangles that violate it; a triangle is split if it has either an edge longer than `max_edge_len` OR an angle smaller than `min_angle_degrees`.",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "embed_path" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A filled 2D region to embed and thicken.  A sequence of Vec2 points, a sequence of Vec2 sequences (outer + holes via subpath nesting), or a path.  Holes drilled through the plate come straight from subpath nesting, exactly like `tessellate_path`."
          },
          ArgDef {
            name: "embed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "The map φ used to embed the region into 3D: `|uv: vec2|: vec3`.  Receives the path's native 2D coordinates (not normalized), so `|p| v3(p.x, 0, p.y)` reproduces a flat plate in the XZ plane with thickness growing +Y, matching `tessellate_path`.  In general the default cap faces −(∂φ/∂u × ∂φ/∂v); `flipped` picks the other side.  Make φ follow a curve, spline, helix, or analytic surface to bend the plate."
          },
          ArgDef {
            name: "thickness",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "How far to thicken the embedded surface into a closed solid, offset along the per-vertex embedded normal.  A number for uniform thickness, or a callable for per-vertex thickness (like `extrude_along_normals`): `|pos: vec3|: num` or `|pos: vec3, uv: vec2|: num` — the 2-arg form also receives the pre-embed 2D domain coordinate, so thickness can be authored in path space without inverting `embed`."
          },
          ArgDef {
            name: "flipped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, flips the cap winding, reversing which side the thickness grows toward."
          },
          ArgDef {
            name: "tolerance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Distortion-aware cap refinement tolerance, in world units: the maximum allowed deviation between the faceted cap and the true embedded surface.  When nil (default), the cap is a single coarse triangulation of the input path (fast, but a straight domain edge that bends under `embed` will facet into long slivers).  When set to a positive number, each boundary loop is densified under `embed` so straight edges resolve into their true 3D curves, and the interior is Delaunay-refined until every triangle is within tolerance.  Smaller values give a finer, more faithful surface at the cost of more triangles.\n\nWhen `thickness` is a callable (and `normal_mode` isn't \"mesh\"), refinement also tracks the offset cap `embed(uv) ∓ thickness·normal`, so a spatially-varying thickness that curves the bottom cap or walls is resolved even where `embed` itself is flat.\n\nDistinct from `curve_angle_degrees`: that controls how finely the input path's own 2D curves are sampled *before* embedding; `tolerance` controls how finely the embedded 3D surface is resolved *after*.  They compose."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing the input path's 2D curves (beziers/arcs/circles) into the boundary polyline that gets embedded.  Falls back to the runtime default (settable via `set_curve_angle_threshold`, seeded by the prelude) when nil.  Only affects callable/path inputs with actual curve features; a raw `Seq<Vec2>` is used as-is."
          },
          ArgDef {
            name: "normal_mode",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "The surface-normal *source*: always drives the thickening offset direction, and additionally the cap's authored (smooth) shading normals when `split_seams` is true.  Nil (default) picks the best available: exact symbolic autodiff of `embed` when it is differentiable, otherwise finite differences.  \"autodiff\" forces exact derivatives (errors if `embed` isn't differentiable), \"finite_diff\" forces central differences, and \"mesh\" reverts to topological face-weighted normals.  The explicit modes are handy for validation and debugging.  Orthogonal to `split_seams`, which controls output topology."
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true, splits the cap/wall seam and each wall's UV wrap seam, then authors exact analytic cap shading normals, procedural UVs (caps: domain coords; walls: arc-length × thickness), and glTF tangents.  Because this duplicates coincident vertices, the result is NO LONGER watertight 2-manifold (NO_WELD); intended for final render meshes, not as input to CSG/boolean ops.  Default false: a welded watertight 2-manifold with no authored attributes (shading is left to the render path's auto-smooth) — the thickening direction still comes from `normal_mode`'s analytic normal."
          },
        ],
        description: "Embeds a filled 2D path into 3D through an arbitrary map φ: ℝ²→ℝ³ and thickens it into a closed 2-manifold solid.  Generalizes `tessellate_path` from an affine coordinate-plane embedding to any nonlinear map, and fuses in `extrude_along_normals`-style thickening in a single pass.\n\nThe region is constrained-Delaunay-triangulated (holes via subpath nesting), each vertex is mapped through `embed`, and the resulting surface is offset along its per-vertex normals and stitched into a watertight solid.  A hole in the path becomes a hole drilled through the thickness of the plate.\n\nBy default the cap is a single coarse triangulation, so strongly-curved embeddings facet; set `tolerance` for distortion-aware refinement that resolves the true warped shape (densifies boundary curves under `embed` and refines the interior to tolerance).\n\nThe thickening direction comes from `normal_mode` (default: exact autodiff of `embed` when possible, else finite differences; \"mesh\" for topological normals).  By default the output is a welded watertight 2-manifold, CSG-ready; pass `split_seams=true` for a render-oriented seam-split mesh with authored analytic shading normals, procedural UVs, and tangents.",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "simplify" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "tolerance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.01)),
            description: "Positive finite simplification tolerance in mesh-local distance units. 0.01 is a good starting point. The engines use different error estimates, so equal tolerances need not produce equal triangle counts; this is not a certified maximum surface-distance bound."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "engine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("meshopt".to_owned())),
            description: "Simplification engine: \"meshopt\" (default) or \"manifold\". Meshopt keeps original vertex positions and attributes, preserves authored and smooth source normals, and favors regular triangles. It validates manifold output and may reduce less to preserve topology. Manifold uses the existing solid-library simplifier. Use `compute_normals` afterward to deliberately recompute shading from the reduced mesh."
          },
        ],
        description: "Reduces mesh complexity within an error budget. Meshopt is the default; engine=\"manifold\" selects the original implementation. Both preserve materials and vertex attributes. Output is checked for manifold topology; self-intersection freedom is not guaranteed by this check.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "tolerance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.01)),
            description: "Positive finite simplification tolerance in mesh-local distance units. 0.01 is a good starting point. The engines use different error estimates, so equal tolerances need not produce equal triangle counts; this is not a certified maximum surface-distance bound."
          },
          ArgDef {
            name: "engine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("meshopt".to_owned())),
            description: "Simplification engine: \"meshopt\" (default) or \"manifold\". Meshopt keeps original vertex positions and attributes, preserves authored and smooth source normals, and favors regular triangles. It validates manifold output and may reduce less to preserve topology. Manifold uses the existing solid-library simplifier. Use `compute_normals` afterward to deliberately recompute shading from the reduced mesh."
          },
        ],
        description: "Reduces mesh complexity within an error budget. Meshopt is the default; engine=\"manifold\" selects the original implementation. Both preserve materials and vertex attributes. Output is checked for manifold topology; self-intersection freedom is not guaranteed by this check.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "verts" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "world_space",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the vertices will be returned in world space coordinates.  If false, they will be returned in the local space of the mesh."
          }
        ],
        description: "Returns a sequence of all vertices in a mesh in an arbitrary order",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "randf" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a random float between `min` and `max`",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[],
        description: "Returns a random float between 0. and 1.",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "srandf" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a deterministic random float between `min` and `max` for `seed`.",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a deterministic random float between 0. and 1. for `seed`.",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "randi" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a random integer in `[min, max)`.  If `min == max`, returns `min`.",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[],
        description: "Returns a random integer.  Any 64-bit integer is equally possible, positive or negative.",
        return_type: &[ArgType::Int],
      },
    ],
  },
  "srandi" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a deterministic random integer in `[min, max)` for `seed`. If `min == max`, returns `min`.",
        return_type: &[ArgType::Int],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a deterministic random integer for `seed`. Any 64-bit integer is equally possible, positive or negative.",
        return_type: &[ArgType::Int],
      },
    ],
  },
  "randv" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a random Vec3 where each component is between the corresponding components of `min` and `max`",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a random Vec3 where each component is between `min` and `max`",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[],
        description: "Returns a random Vec3 where each component is between 0. and 1.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "srandv" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a deterministic Vec3 for `seed` where each component is between the corresponding components of `min` and `max`.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "min",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a deterministic Vec3 for `seed` where each component is between `min` and `max`.",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a deterministic Vec3 for `seed` where each component is between 0. and 1.",
        return_type: &[ArgType::Vec3],
      },
    ],
  },
  "fbm" => FnDef {
    module: "rand",
    examples: &[FnExample { composition_id: 5 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 3D fractal Brownian motion (FBM) at a given position using default parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "octaves",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(4)),
            description: ""
          },
          ArgDef {
            name: "frequency",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "lacunarity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "persistence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: ""
          },
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 3D fractal Brownian motion (FBM) at a given position using the specified parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "tileable",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "`true` tiles seamlessly with period 1 in `pos` units; a number tiles with that period. Each octave's frequency is snapped so a whole number of noise cells fits the period."
          },
        ],
        description: "Samples 2D fractal Brownian motion (FBM) at a given position using default parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "octaves",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(4)),
            description: ""
          },
          ArgDef {
            name: "frequency",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "lacunarity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "persistence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: ""
          },
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "tileable",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "`true` tiles seamlessly with period 1 in `pos` units; a number tiles with that period. Each octave's frequency is snapped so a whole number of noise cells fits the period."
          },
        ],
        description: "Samples 2D fractal Brownian motion (FBM) at a given position using the specified parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 1D fractal Brownian motion (FBM) at a given position using default parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "octaves",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(4)),
            description: ""
          },
          ArgDef {
            name: "frequency",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "lacunarity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "persistence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: ""
          },
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 1D fractal Brownian motion (FBM) at a given position using the specified parameters",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "curl_noise" => FnDef {
    module: "rand",
    examples: &[FnExample { composition_id: 66 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 3D curl noise at a given position using default parameters",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "octaves",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(4)),
            description: ""
          },
          ArgDef {
            name: "frequency",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "lacunarity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "persistence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: ""
          },
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 3D curl noise at a given position using the specified parameters",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 2D curl noise at a given position using default parameters",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "octaves",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(4)),
            description: ""
          },
          ArgDef {
            name: "frequency",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "lacunarity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "persistence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: ""
          },
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 2D curl noise at a given position using the specified parameters",
        return_type: &[ArgType::Vec2],
      },
    ],
  },
  "ridged_multifractal" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 3D ridged multifractal noise at a given position using default parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "octaves",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(4)),
            description: ""
          },
          ArgDef {
            name: "frequency",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "lacunarity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "persistence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: ""
          },
          ArgDef {
            name: "gain",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 3D ridged multifractal noise at a given position using the specified parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 2D ridged multifractal noise at a given position using default parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "octaves",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(4)),
            description: ""
          },
          ArgDef {
            name: "frequency",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: ""
          },
          ArgDef {
            name: "lacunarity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "persistence",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: ""
          },
          ArgDef {
            name: "gain",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: ""
          },
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Samples 2D ridged multifractal noise at a given position using the specified parameters",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "worley_noise" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "range_fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("euclidean".to_owned())),
            description: r#"Distance function used to sample the noise.  Must be one of the following: "euclidean" (default), "euclidean_squared", "manhattan", "chebyshev", or "quadratic"."#,
          },
          ArgDef {
            name: "return_type",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("distance".to_owned())),
            description: r#"Flag to control whether distances or values are returned.  Must be either "distance" (default) or "value"."#,
          },
        ],
        description: "Samples 3D Worley noise at a given position using default parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "pos",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
          ArgDef {
            name: "range_fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("euclidean".to_owned())),
            description: r#"Distance function used to sample the noise.  Must be one of the following: "euclidean" (default), "euclidean_squared", "manhattan", "chebyshev", or "quadratic"."#,
          },
          ArgDef {
            name: "return_type",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("distance".to_owned())),
            description: r#"Flag to control whether distances or values are returned.  Must be either "distance" (default) or "value"."#,
          },
        ],
        description: "Samples 2D Worley noise at a given position using the specified parameters",
        return_type: &[ArgType::Float],
      },
    ],
  },
  "icosphere" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "resolution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of subdivisions to apply when generating the icosphere.  0 -> 20 faces, 1 -> 80 faces, 2 -> 320 faces, ..."
          },
        ],
        description: "Generates an icosphere mesh",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "octahedron" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Circumradius (center-to-vertex distance)"
          },
        ],
        description: "Regular octahedron with vertices on the ±X/±Y/±Z axes (a vertex points straight up). Alias: `diamond`.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "tetrahedron" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Circumradius (center-to-vertex distance)"
          },
        ],
        description: "Regular tetrahedron with circumradius `radius`, inscribed in the cube (sits edge-up).",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "dodecahedron" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Circumradius (center-to-vertex distance)"
          },
        ],
        description: "Regular dodecahedron with circumradius `radius` (sits edge-up).",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "icosahedron" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Circumradius (center-to-vertex distance)"
          },
        ],
        description: "Regular icosahedron with circumradius `radius` (sits edge-up; same as `icosphere(radius, 0)`).",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "bipyramid" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "n",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of sides of the equatorial polygon (>= 3)"
          },
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Circumradius of the equatorial polygon"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Distance from the center to each apex along Y"
          },
        ],
        description: "N-gonal bipyramid: a regular `n`-gon equator (radius) in the XZ plane with apexes at ±`height` on Y. `bipyramid(4)` is the axis-aligned octahedron; larger `n` gives gem-like diamonds.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "cylinder" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "radial_segments",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "height_segments",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(1)),
            description: ""
          },
        ],
        description: "Generates a cylinder mesh",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "capsule" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Radius of the capsule"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Height of the cylindrical middle section.  The total height of the capsule is `height + 2 * radius`."
          },
          ArgDef {
            name: "cap_segments",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(4)),
            description: "Number of curve segments used to build each hemispherical cap"
          },
          ArgDef {
            name: "radial_segments",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(8)),
            description: "Number of segmented faces around the circumference"
          },
          ArgDef {
            name: "height_segments",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(1)),
            description: "Number of rows of faces along the height of the middle section"
          },
        ],
        description: "Generates a capsule mesh (cylinder with hemispherical caps)",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "cone" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "radial_segments",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "height_segments",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(1)),
            description: ""
          },
        ],
        description: "Generates a cone mesh.  The base of the cone is centered at the origin, and the cone extends upwards along the positive Y axis.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "grid" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "size",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Size of the grid along the X and Z axes (providing a number will set the same size for both axes).  The grid is centered at the origin, so a size of `vec2(1, 1)` will produce a grid that extends from -0.5 to +0.5 along both axes."
          },
          ArgDef {
            name: "divisions",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2, ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Vec2(Vec2::new(1., 1.))),
            description: "Number of subdivisions along each axis.  If an integer is provided, it will be used for both axes."
          },
          ArgDef {
            name: "flipped",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the winding order of the triangles will be flipped - making the front side face downwards (negative Y)."
          }
        ],
        description: "Generates a flat grid mesh in the XZ plane centered at the origin with the specified size and number of subdivisions.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "utah_teapot" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 37 }],
    signatures: &[
      FnSignature {
        arg_defs: &[],
        description: "Generates a Utah teapot mesh, a well-known 3D model often used in computer graphics as a test object.\n\nNote that the teapot is NOT manifold, so it cannot be used with functions that require a manifold mesh such as mesh boolean ops, `trace_geodesic_path`, and others.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "stanford_bunny" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 38 }],
    signatures: &[
      FnSignature {
        arg_defs: &[],
        description: "Generates a Stanford bunny mesh, a well-known 3D model often used in computer graphics as a test object.\n\nThis mesh IS manifold, so it can be used with functions that require a manifold mesh such as mesh boolean ops, `trace_geodesic_path`, and others.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "call" => FnDef {
    module: "fn",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Calls `fn` with no arguments, returning its return value",
        return_type: &[ArgType::Any],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "args",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Calls `fn` with the provided arguments, returning its return value",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "mesh" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 32 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "verts",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of Vec3 vertices, pointed into by `faces`"
          },
          ArgDef {
            name: "indices",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "A flag sequence of integer indices corresponding to triangles.  Must have `length % 3 == 0`"
          },
        ],
        description: "Creates a mesh from a sequence of vertices and indices",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[],
        description: "Creates an empty mesh with no vertices or indices.  This can be useful as the initial value for `fold` or and other situations like that.",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "dir_light" => FnDef {
    module: "light",
    examples: &[FnExample { composition_id: 50 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "target",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Vec3(DirectionalLight::default().target)),
            description: "Target point that the light is pointing at.  If the light as at the same position as the target, it will point down towards negative Y"
          },
          ArgDef {
            name: "color",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Int(DirectionalLight::default().color as i64)),
            description: "Color of the light in hex format (like 0xffffff) or webgl format (like `vec3(1., 1., 1.)`)"
          },
          ArgDef {
            name: "intensity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(DirectionalLight::default().intensity)),
            description: ""
          },
          ArgDef {
            name: "cast_shadow",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(DirectionalLight::default().cast_shadow)),
            description: ""
          },
          ArgDef {
            name: "shadow_map_size",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map, ArgType::Int),
            default_value: DefaultValue::Optional(|| {
              let shadow_map_size = DirectionalLight::default().shadow_map_size;
              Value::Map(Rc::new(ValueMap::from_iter([
                ("width".to_string(), Value::Int(shadow_map_size.width as i64)),
                ("height".to_string(), Value::Int(shadow_map_size.height as i64)),
              ].into_iter())))
            }),
            description: "Size of the shadow map.  Allowed keys: `width`, `height`. OR, a single integer value that will be used for both width and height."
          },
          ArgDef {
            name: "shadow_map_radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(DirectionalLight::default().shadow_map_radius)),
            description: "Radius for shadow map filtering"
          },
          ArgDef {
            name: "shadow_map_blur_samples",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(DirectionalLight::default().shadow_map_blur_samples as i64)),
            description: "Number of samples for shadow map blur"
          },
          ArgDef {
            name: "shadow_map_type",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String(DirectionalLight::default().shadow_map_type.to_str().to_owned())),
            description: "Allowed values: `vsm`"
          },
          ArgDef {
            name: "shadow_map_bias",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(DirectionalLight::default().shadow_map_bias)),
            description: ""
          },
          ArgDef {
            name: "shadow_camera",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map, ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Orthographic shadow frustum.  `\"auto\"` or `nil` (the default) fits the frustum and light distance to the scene's bounding box automatically.  Pass a map to set it explicitly.  Allowed keys: `near`, `far`, `left`, `right`, `top`, `bottom`"
          },
        ],
        description: "Creates a directional light.\n\nNote: This will not do anything until it is added to the scene via `render`",
        return_type: &[ArgType::Light],
      }
    ],
  },
  "ambient_light" => FnDef {
    module: "light",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "color",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Int(AmbientLight::default().color as i64)),
            description: "Color of the light in hex format (like 0xffffff) or webgl format (like `vec3(1., 1., 1.)`)"
          },
          ArgDef {
            name: "intensity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(AmbientLight::default().intensity)),
            description: ""
          },
        ],
        description: "Creates an ambient light.\n\nNote: This will not do anything until it is added to the scene via `render`",
        return_type: &[ArgType::Light],
      }
    ],
  },
  "hemisphere_light" => FnDef {
    module: "light",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sky_color",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Int(HemisphereLight::default().sky_color as i64)),
            description: "Color of the light coming from above (the sky), in hex format (like 0xffffff) or webgl format (like `vec3(1., 1., 1.)`)"
          },
          ArgDef {
            name: "ground_color",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Int(HemisphereLight::default().ground_color as i64)),
            description: "Color of the light coming from below (the ground), in hex format (like 0x444444) or webgl format (like `vec3(0.2, 0.2, 0.2)`)"
          },
          ArgDef {
            name: "intensity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(HemisphereLight::default().intensity)),
            description: ""
          },
        ],
        description: "Creates a hemisphere light: a cheap directional ambient that blends `sky_color` (from above) and `ground_color` (from below) by surface normal.\n\nNote: This will not do anything until it is added to the scene via `render`",
        return_type: &[ArgType::Light],
      }
    ],
  },
  "rect_area_light" => FnDef {
    module: "light",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "color",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Vec3),
            default_value: DefaultValue::Optional(|| Value::Int(RectAreaLight::default().color as i64)),
            description: "Color of the light in hex format (like 0xffffff) or webgl format (like `vec3(1., 1., 1.)`)"
          },
          ArgDef {
            name: "intensity",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(RectAreaLight::default().intensity)),
            description: ""
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(RectAreaLight::default().width)),
            description: "Width of the emitting rectangle"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(RectAreaLight::default().height)),
            description: "Height of the emitting rectangle"
          },
        ],
        description: "Creates a rectangular area light (e.g. a softbox or window). The rectangle emits from its local -Z face; position and orientation come from transforms applied via `translate`/`rotate`/etc. Does not cast shadows.\n\nNote: This will not do anything until it is added to the scene via `render`",
        return_type: &[ArgType::Light],
      }
    ],
  },
  "set_attr" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: "Attribute name (letters, digits, underscores).  Well-known names carry fixed types and export rules: `uv` (vec2), `tangent` (vec4), `color` (vec3 or vec4, linear RGB).  Any other name defines a custom attribute.  `pos`, `normal`, and `ix` are reserved."
          },
          ArgDef {
            name: "value",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable, ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Either a callable `|pos: vec3, normal: vec3, {...attrs}|: num | vec2 | vec3 | vec4` invoked once per vertex (see `warp` for the attribute bag), or a sequence with one value per vertex in `verts` order.  The value type sets the attribute's arity and must be the same for every vertex."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "spatial",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "How the attribute responds to transforms baked into the mesh: `\"none\"` (default; colors, weights, uvs) or `\"direction\"` (a unit direction lying in the surface: rotated with the mesh, negated when it is mirrored, and renormalized wherever vertices are blended)."
          },
        ],
        description: "Returns a copy of `mesh` with per-vertex attribute `name` set, replacing any existing attribute of that name.  Attributes ride along through warps, transforms, seam splits, `extrude`, `join`/`+`, CSG booleans (interpolated at the cut, operand attribute sets unioned), `simplify`, and `split_by_plane`; are resampled by nearest point through re-tessellating ops (see `transfer_attrs`); are interpolated when vertices are split; can be read inside per-vertex callbacks via the attribute bag; and are exported to the GPU as vertex attributes (well-known names always, custom names via `render(attrs=)`).",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "attr" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the values of per-vertex attribute `name` as a sequence in `verts` order (`nil` for vertices lacking a value).  Errors if the mesh has no such attribute.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "attrs" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the names of the mesh's per-vertex attributes, sorted.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "drop_attr" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a copy of `mesh` without per-vertex attribute `name`.  Errors if the mesh has no such attribute.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "transfer_attrs" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "src",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: "Mesh whose per-vertex attributes are sampled."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a copy of `mesh` carrying every per-vertex attribute of `src`, sampled by nearest point: each vertex takes the barycentric blend of the closest `src` triangle (in world space).  Exact wherever `mesh` lies on `src`'s surface, e.g. after remeshing; also the way to paint attributes on a low-poly proxy and carry them onto detailed geometry.  Attribute seams (UV cuts) can't survive re-tessellation, so recompute UVs rather than transferring them.  `smooth`, `isotropic_remesh`, `delaunay_remesh`, `remesh_planar_patches`, `alpha_wrap`, and `convex_hull` apply this automatically.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "faces" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the mesh's triangles as `[i, j, k]` sequences of vertex indices into `verts` order, CCW winding.  Together with `verts` this is the raw indexed mesh; `mesh(verts, indices)` rebuilds one.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "smooth_attr" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: "Attribute to smooth."
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "iterations",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(1)),
            description: "Relaxation passes."
          },
          ArgDef {
            name: "lambda",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: "How far each vertex moves toward its neighbours' mean per pass, 0..1."
          },
          ArgDef {
            name: "weights",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("uniform".to_owned())),
            description: "`\"uniform\"` weights every neighbour equally; `\"cotan\"` uses cotangent (geometry-aware) weights that don't bias toward densely tessellated regions."
          },
          ArgDef {
            name: "pin",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Name of a scalar attribute; vertices where it is nonzero keep their value (feathering toward painted regions)."
          },
        ],
        description: "Returns a copy of `mesh` with attribute `name` blurred over the mesh's own connectivity: each vertex relaxes toward the weighted mean of its one-ring, repeated `iterations` times.  Direction attributes stay unit length.  Use it to denoise `bake_ao` output, feather weights, or spread any painted attribute across the surface.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "bake_ao" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "samples",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Rays per vertex; `nil` = 32, or 256 with `refine`.  Cost is vertices x samples; 16 is fine while iterating, 64+ for a final bake.  Estimator noise falls as ~0.34/samples^0.75 (RMS 0.025 at 32, 0.005 at 256), which is also what bounds how finely `refine` can resolve."
          },
          ArgDef {
            name: "max_dist",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Ignore hits farther than this (world units) for local occlusion; `nil` = unlimited."
          },
          ArgDef {
            name: "occluders",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Mesh, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Other meshes that block rays in addition to `mesh` itself (world space)."
          },
          ArgDef {
            name: "bias",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Ray-origin offset along the normal to avoid self-hits; defaults to 1e-4 of the mesh's bounding diagonal."
          },
          ArgDef {
            name: "into",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("ao".to_owned())),
            description: "Name of the scalar attribute written."
          },
          ArgDef {
            name: "refine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "AO tolerance for adaptive tessellation.  Each face's edge midpoints and centroid are sampled (quarter points too on edges whose ends differ by more than this); where a sample differs from the linear interpolation of the corners by more than this, the face is bisected (longest edge first, Rivara-style, so triangles stay well-shaped; seam twins split together) and the new faces tested in turn, so vertices land only where a linear ramp would be wrong.  Costs roughly one extra sample per edge and per face of the output.  Clamped to the sampling noise floor, ~1.3/samples^0.75 (0.095 at 32 samples, 0.02 at 256, 0.012 at 512), with a printed notice; below it noise would refine every penumbra down to `min_edge`.  So `samples` sets the finest resolvable feature and `refine` the accepted interpolation error.  `nil` leaves the mesh as is."
          },
          ArgDef {
            name: "min_edge",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "With `refine`, edges shorter than this (world units) are never split; defaults to 1% of the mesh's bounding diagonal."
          },
          ArgDef {
            name: "split_seams",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "Split the mesh along sharp edges and recompute shading normals first, exactly like `compute_normals` with the runtime sharp-angle threshold, so each side of a crease bakes with its own normal and holds its own AO (and `smooth_attr` can't bleed across it).  Off, a crease vertex gets one value blended over both sides.  Leaves the mesh open along creases like the exported mesh already is, so bake last; already-split meshes are unaffected."
          },
        ],
        description: "Bakes ambient occlusion per vertex into a scalar attribute: the fraction of cosine-weighted hemisphere rays from each vertex (around its smooth normal) that escape `mesh` and `occluders`; 1 = open, 0 = buried.  Sampling is deterministic, so re-evaluation is stable.  Resolution is the mesh's own: a large triangle gets a linear ramp between its corners, so either `tessellate` first where occlusion detail matters or pass `refine` to split edges only where the ramp is wrong, and `smooth_attr` the result to soften noise (typically `smooth_attr(\"ao\", iterations=2, weights=\"cotan\")`).  Read it in shaders via the material's `vertexAttrs`, or write it into `color`, which every material multiplies in by default.",
        return_type: &[ArgType::Mesh],
      },
    ],
  },
  "set_material" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 30 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "material",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Material, ArgType::String),
            default_value: DefaultValue::Required,
            description: "Can be either a `Material` value or a string specifying the name of an externally defined material"
          },
          ArgDef {
            name: "mesh",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Mesh),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Sets the material for a mesh",
        return_type: &[ArgType::Mesh],
      }
    ],
  },
  "set_default_material" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "material",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Material, ArgType::String),
            default_value: DefaultValue::Required,
            description: "Can be either a `Material` value or a string specifying the name of an externally defined material"
          },
        ],
        description: "Sets the default material for all meshes that do not have a specific material set",
        return_type: &[ArgType::Nil],
      }
    ],
  },
  "set_rng_seed" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          }
        ],
        description: "Sets the seed for the shared PRNG used by functions like `randi`, `randf`, etc.\n\nThis will reset the state of the RNG, so it will always return the same value the next time it's used after this is called.",
        return_type: &[ArgType::Nil],
      }
    ]
  },
  "set_sharp_angle_threshold" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "The angle at which edges are considered sharp, in degrees"
          }
        ],
        description: "Sets the sharp angle threshold for computing auto-smooth-shaded normals (specified in degrees).  If the angle between two adjacent faces is greater than this angle, the edge will be considered sharp.\n\nMust be called as a top-level statement (not inside closures or conditionals); use per-call kwargs like `compute_normals(mesh, angle)` for finer-grained control.",
        return_type: &[ArgType::Nil],
      }
    ]
  },
  "set_curve_angle_threshold" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Maximum angular deviation per segment, in degrees; smaller is finer."
          }
        ],
        description: "Sets the default `curve_angle_degrees` used when discretizing continuous path features (Clipper2/CGAL boundaries, `tessellate_path`, `extrude_path`, etc.) and the kwarg is omitted.  Builtins that take an explicit `curve_angle_degrees` override this per-call.\n\nMust be called as a top-level statement (not inside closures or conditionals).",
        return_type: &[ArgType::Nil],
      }
    ]
  },
  "trace_svg_path" => FnDef {
    module: "path",
    examples: &[], // TODO
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "svg_path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: "An SVG path data string using move, line, cubic/quadratic, smooth, and arc commands."
          },
        ],
        description: "Parses SVG path data (`M`, `L`, `H`, `V`, `C`, `S`, `Q`, `T`, `A`, `Z`, absolute or relative) into a path.",
        return_type: &[ArgType::Path]
      }
    ]
  },
  "subpaths" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path to split"
          }
        ],
        description: "Returns a lazy sequence of paths, one for each disconnected subpath in the input path.\n\nFor example, if a path is created with multiple `move` commands, each segment between moves becomes a separate subpath. Each returned sampler works like the original, with `t` in [0,1] sampling along that particular subpath.",
        return_type: &[ArgType::Sequence]
      }
    ]
  },
  "offset_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path."
          },
          ArgDef {
            name: "delta",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Offset distance (positive inflates, negative deflates)."
          },
          ArgDef {
            name: "join_type",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::String("round".to_string())),
            description: "Join type at corners: square, bevel, round, miter, superellipse, knob, step, spike (or numeric enum)."
          },
          ArgDef {
            name: "end_type",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::String("round".to_string())),
            description: "End cap type for open paths: polygon, joined, butt, square, round, superellipse, triangle, arrow, teardrop (or numeric enum)."
          },
          ArgDef {
            name: "miter_limit",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.0)),
            description: "Miter limit for sharp corners."
          },
          ArgDef {
            name: "arc_tolerance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.0)),
            description: "Arc tolerance for round joins."
          },
          ArgDef {
            name: "preserve_collinear",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, preserve collinear vertices during offset."
          },
          ArgDef {
            name: "reverse_solution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, reverse output path orientation."
          },
          ArgDef {
            name: "step_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: "Number of steps for stepped joins/caps."
          },
          ArgDef {
            name: "superellipse_exponent",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.5)),
            description: "Superellipse exponent for superellipse joins/caps."
          },
          ArgDef {
            name: "end_extension_scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.0)),
            description: "Scale for open end caps."
          },
          ArgDef {
            name: "arrow_back_sweep",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.0)),
            description: "Arrow back sweep for arrow end caps."
          },
          ArgDef {
            name: "teardrop_pinch",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: "Teardrop pinch amount for teardrop end caps."
          },
          ArgDef {
            name: "join_angle_threshold",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.0)),
            // TODO: better description
            description: "Angle threshold used by some join types."
          },
          ArgDef {
            name: "chebyshev_spacing",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, use Chebyshev spacing for round joins."
          },
          ArgDef {
            name: "simplify_epsilon",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.0001)),
            description: "Epsilon for pre-offset simplification (0 disables)."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(128)),
            description: "Initial uniform probe count for lazy paths. Critical points and structural boundaries are also sampled, then refined adaptively per curve_angle_degrees; this is not an output cap."
          },
        ],
        description: "Offsets a 2D path using Clipper2 and returns a new path.  Note: continuous curve detail is lost; the output is a polyline representation.",
        return_type: &[ArgType::Path],
      }
    ]
  },
  "lerp_paths" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path_a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "First path."
          },
          ArgDef {
            name: "path_b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "Second path."
          },
          ArgDef {
            name: "mix",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Interpolation factor [0, 1]. 0 returns path_a, 1 returns path_b."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(64)),
            description: "Fallback critical point resolution when inputs lack topology data."
          },
        ],
        description: "Interpolates between two paths, returning a new path. At each `t`, the output point is `lerp(path_a(t), path_b(t), mix)`. Critical points from both input paths are merged to preserve sharp features during interpolation.",
        return_type: &[ArgType::Path],
      }
    ]
  },
  "catmull_rom" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `vec2` control points the spline passes through."
          },
          ArgDef {
            name: "tension",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: "Tangent scale factor. `0.5` (default) gives standard Catmull-Rom. Higher values produce more pronounced curves; `0.0` produces a linear interpolation between points."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the spline wraps from the last control point back to the first, forming a closed loop."
          },
        ],
        description: "Returns a path `|t: float|: vec2` that evaluates a Catmull-Rom spline through the given 2D control points. The spline passes through every control point. All interior joints are C1 smooth; only the endpoints of an open spline are non-smooth. `tension` generalises to the full cardinal spline family (`0.5` = standard Catmull-Rom).",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "catmull_rom_3d" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `vec3` control points the spline passes through."
          },
          ArgDef {
            name: "tension",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: "Tangent scale factor. `0.5` (default) gives standard Catmull-Rom. Higher values produce more pronounced curves; `0.0` produces a linear interpolation between points."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the spline wraps from the last control point back to the first, forming a closed loop."
          },
        ],
        description: "Returns a callable `|t: float|: vec3` that evaluates a Catmull-Rom spline through the given 3D control points. The spline passes through every control point. All interior joints are C1 smooth; only the endpoints of an open spline are non-smooth. `tension` generalises to the full cardinal spline family (`0.5` = standard Catmull-Rom).",
        return_type: &[ArgType::Callable],
      },
    ],
  },
  "fillet_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `vec2` points defining a polyline whose interior corners will be smoothed."
          },
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Target radius of the inscribed circle at each corner. Either a number (constant for every corner) or a callable `|corner_ix: int, vertex: vec2|: float` that returns a per-corner radius."
          },
          ArgDef {
            name: "resolution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(8)),
            description: "Number of arc segments generated per filleted corner.  Higher values produce smoother bends.  Each corner replaces 1 input vertex with `resolution + 1` output vertices."
          },
          ArgDef {
            name: "clamp_radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "When true, automatically reduces the per-corner radius so that the fillet's tangent points never cross the midpoint of an adjacent segment.  When false, an error is raised if the requested radius is too large for a given corner."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the path is treated as a closed loop and the wrap-around corner between the last and first points is also filleted."
          },
        ],
        description: "Smooths the interior corners of a 2D polyline by replacing each corner with a circular-arc fillet of the requested radius.  Collinear vertices, U-turns, and zero-length segments pass through unmodified.  Returns a new sequence of `vec2` points (the original endpoints are preserved when `closed=false`).",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "fillet_path_3d" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `vec3` points defining a polyline whose interior corners will be smoothed.  Often piped directly into `extrude_pipe`."
          },
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Target radius of the inscribed circle at each corner. Either a number (constant for every corner) or a callable `|corner_ix: int, vertex: vec3|: float` that returns a per-corner radius."
          },
          ArgDef {
            name: "resolution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(8)),
            description: "Number of arc segments generated per filleted corner.  Higher values produce smoother bends.  Each corner replaces 1 input vertex with `resolution + 1` output vertices."
          },
          ArgDef {
            name: "clamp_radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "When true, automatically reduces the per-corner radius so that the fillet's tangent points never cross the midpoint of an adjacent segment.  When false, an error is raised if the requested radius is too large for a given corner."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "If true, the path is treated as a closed loop and the wrap-around corner between the last and first points is also filleted."
          },
        ],
        description: "Smooths the interior corners of a 3D polyline by replacing each corner with a true circular-arc fillet of the requested radius (lying in the plane of the bend).  Collinear vertices, U-turns, and zero-length segments pass through unmodified.  Returns a new sequence of `vec3` points (the original endpoints are preserved when `closed=false`).  Designed to be piped directly into `extrude_pipe` or other path-consuming builtins.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "critical_points" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path (e.g. from trace_path, offset_path, lerp_paths)."
          },
        ],
        description: "Returns the critical t values of a path as a sequence of floats. Critical points are parameter values where sharp features (corners, segment boundaries) occur. Only works with paths that have topology information.",
        return_type: &[ArgType::Sequence],
      }
    ]
  },
  "path_union" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "subject",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The first path."
          },
          ArgDef {
            name: "clip",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The second path."
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule for determining path interiors: evenodd, nonzero, positive, negative (or numeric enum 0-3).  Defaults to `nonzero` for the `clipper` engine and `evenodd` for `cgal` (the only rule `cgal` accepts)."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(64)),
            description: "Initial uniform probe count for lazy paths. Critical points and structural boundaries are also sampled, then refined adaptively per curve_angle_degrees; this is not an output cap."
          },
          ArgDef {
            name: "engine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Backend: `clipper` (default; fast, fixed-point) or `cgal` (slower, exact-arithmetic via `Polygon_set_2` over EPECK)."
          },
        ],
        description: "Computes the union of two 2D paths. The union contains all areas inside either path.\n\nWhich regions count as \"inside\" depends on `fill_rule` (default `nonzero`):\n- `nonzero`: a point is inside if the signed-crossing count of a ray from it to infinity is non-zero.  Inner subpaths must wind opposite to outer subpaths to register as holes.\n- `evenodd`: a point is inside if the unsigned crossing count is odd.  Winding direction is ignored; holes arise purely from nesting.\n- `positive` / `negative`: like `nonzero` but keep only regions with positive or negative winding number respectively.\n\n**Engine selection** (`engine` kwarg, default `clipper`):\n- `clipper`: Clipper2, fast but operates in fixed-point internally so float coordinates are quantized and exact-coincident points may shift slightly.  This can cause T-junctions and tiny gaps in the output topology that break downstream operations like 2-manifold extrusion.\n- `cgal`: CGAL `Polygon_set_2` over `Exact_predicates_exact_constructions_kernel`; exact arithmetic preserves coincident edges precisely.  Slower (often 10–100×) but produces clean topology suitable for `extrude` / `tessellate_path`.  Only `evenodd` fill rule is supported; within each input, subpaths combine under XOR (matching the nesting-based fill model).\n\nFor paths built procedurally as non-overlapping subpaths (e.g. an outer shape plus enclosed holes), skip the boolean op entirely and pass the multi-subpath path directly to downstream consumers; they will treat nested subpaths as holes under their own fill rule.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "paths",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of paths to union together."
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule for determining path interiors: evenodd, nonzero, positive, negative (or numeric enum 0-3).  Defaults to `nonzero` for the `clipper` engine and `evenodd` for `cgal` (the only rule `cgal` accepts)."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(64)),
            description: "Initial uniform probe count for lazy paths. Critical points and structural boundaries are also sampled, then refined adaptively per curve_angle_degrees; this is not an output cap."
          },
          ArgDef {
            name: "engine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Backend: `clipper` (default; fast, fixed-point) or `cgal` (slower, exact-arithmetic via `Polygon_set_2` over EPECK)."
          },
        ],
        description: "Computes the union of every path in a sequence in a single boolean pass.  Much faster than chaining pairwise unions (`reduce(path_union)` and `fold(init, path_union)` route here automatically), since each pairwise step would re-sample and re-analyze the growing result.  Options and engines behave as for the two-path form; with `engine=\"cgal\"` the inputs are combined pairwise internally.",
        return_type: &[ArgType::Path],
      }
    ]
  },
  "path_intersect" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "subject",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The first path."
          },
          ArgDef {
            name: "clip",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The second path."
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule for determining path interiors: evenodd, nonzero, positive, negative (or numeric enum 0-3).  Defaults to `nonzero` for the `clipper` engine and `evenodd` for `cgal` (the only rule `cgal` accepts)."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(64)),
            description: "Initial uniform probe count for lazy paths. Critical points and structural boundaries are also sampled, then refined adaptively per curve_angle_degrees; this is not an output cap."
          },
          ArgDef {
            name: "engine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Backend: `clipper` (default; fast, fixed-point) or `cgal` (slower, exact-arithmetic)."
          },
        ],
        description: "Computes the intersection of two 2D paths. The intersection contains only areas inside both paths.\n\nSee `path_union` for the full fill-rule and engine reference; the same conventions apply here.",
        return_type: &[ArgType::Path],
      }
    ]
  },
  "path_intersects" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "a",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The first path."
          },
          ArgDef {
            name: "b",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The second path. Same restriction as `a`."
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::String("nonzero".to_owned())),
            description: "Fill rule used to determine the interior of each path when checking for overlap: evenodd, nonzero, positive, negative (or numeric enum 0-3)."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(64)),
            description: "Initial uniform probe count for lazy paths, augmented by critical points and adaptive refinement; not an output cap."
          },
        ],
        description: "Returns `true` if the two 2D path regions overlap under the given fill rule, `false` otherwise.\n\nDetects both cases where path segments cross and cases where one path is fully contained inside the other. Uses Clipper2's region intersection internally, so winding order and the chosen fill rule determine what counts as interior.\n\nOnly supported for paths with known topology (e.g. from `trace_svg_path`, `text_to_path`, `lerp_path`, `catmull_rom`); generic black-box `|t|: vec2` callables raise an error.",
        return_type: &[ArgType::Bool],
      }
    ]
  },
  "path_difference" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "subject",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The first path."
          },
          ArgDef {
            name: "clip",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The second path."
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule for determining path interiors: evenodd, nonzero, positive, negative (or numeric enum 0-3).  Defaults to `nonzero` for the `clipper` engine and `evenodd` for `cgal` (the only rule `cgal` accepts)."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(64)),
            description: "Initial uniform probe count for lazy paths. Critical points and structural boundaries are also sampled, then refined adaptively per curve_angle_degrees; this is not an output cap."
          },
          ArgDef {
            name: "engine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Backend: `clipper` (default; fast, fixed-point) or `cgal` (slower, exact-arithmetic)."
          },
        ],
        description: "Computes the difference of two 2D paths (subject minus clip). The result contains areas inside subject but not inside clip.\n\nSee `path_union` for the full fill-rule and engine reference; the same conventions apply here.",
        return_type: &[ArgType::Path],
      }
    ]
  },
  "path_xor" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "subject",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The first path."
          },
          ArgDef {
            name: "clip",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The second path."
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule for determining path interiors: evenodd, nonzero, positive, negative (or numeric enum 0-3).  Defaults to `nonzero` for the `clipper` engine and `evenodd` for `cgal` (the only rule `cgal` accepts)."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(64)),
            description: "Initial uniform probe count for lazy paths. Critical points and structural boundaries are also sampled, then refined adaptively per curve_angle_degrees; this is not an output cap."
          },
          ArgDef {
            name: "engine",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Backend: `clipper` (default; fast, fixed-point) or `cgal` (slower, exact-arithmetic)."
          },
        ],
        description: "Computes the exclusive-or (XOR) of two 2D paths. The result contains areas inside either path but not both.\n\nSee `path_union` for the full fill-rule and engine reference; the same conventions apply here.",
        return_type: &[ArgType::Path],
      }
    ]
  },
  "simplify_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path. Lazy paths (e.g. `lerp_paths`) are discretized first at the ambient curve angle."
          },
          ArgDef {
            name: "tolerance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.01)),
            description: "Max distance (in path units) any removed vertex may lie from the simplified outline. 0 only removes collinear vertices."
          },
        ],
        description: "Removes vertices from the straight runs of a path (Ramer–Douglas–Peucker) while keeping every remaining point of the original within `tolerance` of the result. Curve segments are kept verbatim, and critical points marked by producers (`path_union`, `offset_path`, `alpha_wrap_2d`, `discretize_path`, …) are never removed, so `critical_points` survive; authored joints from pen ops / `polygon` are all candidates. Use it to thin dense polylines (boolean/wrap output, traced text, SVG) before sweeping, lerping, or further booleans.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "tolerance",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Max distance (in path units) any removed vertex may lie from the simplified outline."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path."
          },
        ],
        description: "Tolerance-first form for pipelines: `path | simplify_path(0.02)`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "discretize_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path. Concrete and lazy paths both use adaptive sampling and preserve existing critical points."
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves."
          },
          ArgDef {
            name: "sample_count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(128)),
            description: "Initial uniform probe count for lazy paths, augmented by critical points and adaptive refinement; not an output cap."
          },
        ],

        description: "Replaces every continuous curve in the input path with a polyline of straight line segments, returning a new path.\n\nThis is the same discretization step that `path_union` / `offset_path` apply internally before handing geometry to Clipper2; running it explicitly is useful for inspecting the polyline that those operations would see, or for paths where polyline-only consumers need a guaranteed-segment-only input.\n\nUses adaptive sampling driven by the global or explicit `curve_angle_degrees`, including lazy paths. Lazy sampling starts from `sample_count` uniform probes plus known critical points and segment boundaries, then refines between them. Existing anchors are retained; additional refinement vertices are not marked as creases. Finite probes can miss arbitrary black-box oscillations; increase `sample_count` if needed.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "path_segments" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "path from any path constructor or op / boolean ops."
          },
        ],
        description: "Returns a sequence of tagged dicts, one per segment of the path, in subpath traversal order with the path's transform applied.\n\nEvery segment dict has these common fields:\n- `type`: `\"line\"` | `\"quad\"` | `\"cubic\"` | `\"arc\"`\n- `start`, `end`: vec2 endpoints\n- `length`: arc length of the segment\n- `subpath`: int - index of the parent subpath\n- `closed`: bool - whether the parent subpath is closed\n- `t_start`, `t_end`: floats in [0, 1] - arc-length parameters within the parent subpath\n- `t_start_global`, `t_end_global`: floats in [0, 1] - arc-length parameters across the full path\n\nPer-type extras:\n- `quad`: `ctrl: vec2`\n- `cubic`: `ctrl1: vec2`, `ctrl2: vec2`\n- `arc`: `center: vec2`, `rx: num`, `ry: num`, `x_axis_rotation: num` (radians), `large_arc: bool`, `sweep: bool`, `theta_start: num`, `theta_delta: num`\n\nThe path's `reverse` flag is a sampling-order concern and is intentionally not honoured here; segments are always emitted in their as-built order.\n\nOnly works with paths that expose segment topology (i.e. those backed by a path tracer); paths from `catmull_rom` / `lerp_paths` are not supported.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "items",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Path, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "A path, a sequence of paths, or nested sequences of them; `nil` entries are skipped."
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule for the grouped path (`\"nonzero\"`, `\"evenodd\"`, `\"positive\"`, `\"negative\"`). When omitted it is inherited if every item agrees; conflicting rules are an error."
          },
        ],
        description: "Groups paths into one path whose subpaths run in order. Concrete inputs are merged into a single flat list of subpaths; lazy inputs (`lerp_paths`, `catmull_rom`, `path(fn)`) are kept as they are. `path()` is the empty path, the usual start of a pen-op chain: `path() | move(0, 0) | line(1, 1) | close`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "f",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "A `|t: num|: vec2` callable sampled over `t` in [0, 1]."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Whether the sampled curve is a closed loop. When nil it is inferred from whether `f(0)` and `f(1)` coincide."
          },
        ],
        description: "Wraps a `|t: num|: vec2` callable as a lazy path. It has no draw commands, so pen ops and `path_segments` need `discretize_path` first; everything else samples it.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "circle" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "center",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Center of the circle."
          },
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Radius of the circle."
          },
        ],
        description: "Closed circular path built from two arcs, starting at the rightmost point and running counter-clockwise.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "cx",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Center x."
          },
          ArgDef {
            name: "cy",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Center y."
          },
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Radius of the circle."
          },
        ],
        description: "Closed circular path built from two arcs, starting at the rightmost point and running counter-clockwise.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "rect" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "center",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Center of the rectangle."
          },
          ArgDef {
            name: "size",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Width and height as a `vec2`, or one number for a square."
          },
        ],
        description: "Closed rectangular path traced counter-clockwise from the top-right corner.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "cx",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Center x."
          },
          ArgDef {
            name: "cy",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Center y."
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Width."
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Height."
          },
        ],
        description: "Closed rectangular path traced counter-clockwise from the top-right corner.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "polygon" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `vec2` vertices (at least 3 distinct)."
          },
        ],
        description: "Closed path through `points`, joined by straight segments and closed back to the first point.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "polyline" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `vec2` vertices (at least 2)."
          },
        ],
        description: "Open path through `points`, joined by straight segments.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "move" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X of the new subpath start."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y of the new subpath start."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: starts a new subpath at the given point. Pen ops append to the open last subpath; on a closed or empty path they start a new subpath from the current point (the last subpath's end, its start if it is closed, or the origin when the path is empty). Chain them with `|`: `path() | move(0, 0) | line(1, 0) | close`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Start of the new subpath."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: starts a new subpath at the given point. Pen ops append to the open last subpath; on a closed or empty path they start a new subpath from the current point (the last subpath's end, its start if it is closed, or the origin when the path is empty). Chain them with `|`: `path() | move(0, 0) | line(1, 0) | close`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "line" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X of the segment end."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y of the segment end."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: straight segment from the current point. Pen ops append to the open last subpath; on a closed or empty path they start a new subpath from the current point (the last subpath's end, its start if it is closed, or the origin when the path is empty). Chain them with `|`: `path() | move(0, 0) | line(1, 0) | close`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "End of the segment."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: straight segment from the current point. Pen ops append to the open last subpath; on a closed or empty path they start a new subpath from the current point (the last subpath's end, its start if it is closed, or the origin when the path is empty). Chain them with `|`: `path() | move(0, 0) | line(1, 0) | close`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "quadratic_bezier" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "ctrl",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Control point."
          },
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "End point."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: quadratic Bézier from the current point through `ctrl` to `to`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "cx",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Control point x."
          },
          ArgDef {
            name: "cy",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Control point y."
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End x."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End y."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: quadratic Bézier from the current point through `(cx, cy)` to `(x, y)`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "smooth_quadratic_bezier" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "End point."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: quadratic Bézier whose control point mirrors the previous quadratic's across the current point (the current point itself when there is none), like SVG `T`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End x."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End y."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: quadratic Bézier whose control point mirrors the previous quadratic's across the current point (the current point itself when there is none), like SVG `T`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "cubic_bezier" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "ctrl1",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "First control point."
          },
          ArgDef {
            name: "ctrl2",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Second control point."
          },
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "End point."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: cubic Bézier from the current point with control points `ctrl1` and `ctrl2` to `to`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "c1x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "First control point x."
          },
          ArgDef {
            name: "c1y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "First control point y."
          },
          ArgDef {
            name: "c2x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Second control point x."
          },
          ArgDef {
            name: "c2y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Second control point y."
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End x."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End y."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: cubic Bézier from the current point with control points `(c1x, c1y)` and `(c2x, c2y)` to `(x, y)`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "smooth_cubic_bezier" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "ctrl2",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Second control point."
          },
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "End point."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: cubic Bézier whose first control point mirrors the previous cubic's second across the current point (the current point itself when there is none), like SVG `S`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "c2x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Second control point x."
          },
          ArgDef {
            name: "c2y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Second control point y."
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End x."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End y."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: cubic Bézier whose first control point mirrors the previous cubic's second across the current point (the current point itself when there is none), like SVG `S`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "arc" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rx",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X radius."
          },
          ArgDef {
            name: "ry",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y radius."
          },
          ArgDef {
            name: "x_axis_rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation of the ellipse's x axis in degrees."
          },
          ArgDef {
            name: "large_arc",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: "Take the larger of the two arcs."
          },
          ArgDef {
            name: "sweep",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: "Sweep counter-clockwise."
          },
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "End point."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: elliptical arc from the current point to the end point with radii `rx`, `ry` and x-axis rotation `x_axis_rotation` (degrees), following SVG's arc parameterization. `large_arc` picks the longer of the two candidate arcs; `sweep` picks the counter-clockwise one. The short forms use `large_arc=false, sweep=true`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rx",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X radius."
          },
          ArgDef {
            name: "ry",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y radius."
          },
          ArgDef {
            name: "x_axis_rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation of the ellipse's x axis in degrees."
          },
          ArgDef {
            name: "large_arc",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: "Take the larger of the two arcs."
          },
          ArgDef {
            name: "sweep",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Required,
            description: "Sweep counter-clockwise."
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End x."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End y."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: elliptical arc from the current point to the end point with radii `rx`, `ry` and x-axis rotation `x_axis_rotation` (degrees), following SVG's arc parameterization. `large_arc` picks the longer of the two candidate arcs; `sweep` picks the counter-clockwise one. The short forms use `large_arc=false, sweep=true`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rx",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X radius."
          },
          ArgDef {
            name: "ry",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y radius."
          },
          ArgDef {
            name: "x_axis_rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation of the ellipse's x axis in degrees."
          },
          ArgDef {
            name: "to",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "End point."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: elliptical arc from the current point to the end point with radii `rx`, `ry` and x-axis rotation `x_axis_rotation` (degrees), following SVG's arc parameterization. `large_arc` picks the longer of the two candidate arcs; `sweep` picks the counter-clockwise one. The short forms use `large_arc=false, sweep=true`.",
        return_type: &[ArgType::Path],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rx",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "X radius."
          },
          ArgDef {
            name: "ry",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Y radius."
          },
          ArgDef {
            name: "x_axis_rotation",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Rotation of the ellipse's x axis in degrees."
          },
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End x."
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "End y."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: elliptical arc from the current point to the end point with radii `rx`, `ry` and x-axis rotation `x_axis_rotation` (degrees), following SVG's arc parameterization. `large_arc` picks the longer of the two candidate arcs; `sweep` picks the counter-clockwise one. The short forms use `large_arc=false, sweep=true`.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "close" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Pen op: closes the open last subpath with a straight segment back to its start. No-op when the last subpath is already closed or empty.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "close_all" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Closes every open subpath of the path.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "fill_rule" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "One of `\"nonzero\"`, `\"evenodd\"`, `\"positive\"`, `\"negative\"` (or the Clipper2 numeric code)."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "The path to operate on; pen ops take it last so they can be chained with `|`."
          },
        ],
        description: "Returns the path with the given fill rule, which tessellation, rasterization and boolean ops read from the path when they are not given one explicitly.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "trim_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path. Concrete paths are sliced geometrically (curves and sharp corners preserved exactly); lazy paths are wrapped and resampled over the trimmed range."
          },
          ArgDef {
            name: "start",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Start of the kept range. Negative values count back from the end. `nil` keeps from the path start."
          },
          ArgDef {
            name: "end",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "End of the kept range. Negative values count back from the end. `nil` keeps to the path end."
          },
          ArgDef {
            name: "unit",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("t".to_owned())),
            description: "`\"t\"` (default) treats `start`/`end` as normalized arc-length parameters in [0, 1]; `\"distance\"` treats them as arc-length distances."
          },
        ],
        description: "Returns a new path covering the portion of `path` between `start` and `end`.\n\nTrimming is done in the global arc-length parameterization that spans all subpaths, so `start`/`end` cut across the concatenated subpaths; iterate `path_subpaths` first to trim an individual subpath. Negative bounds count back from the end (e.g. `trim_path(p, start=4, end=-4, unit='distance')` drops 4 units from each end).\n\nFor tracer-backed paths the result is a real sliced path: lines, beziers and arcs keep their exact geometry and every sharp corner inside the range is preserved. Black-box callables are wrapped and resampled, with `distance` bounds resolved against a sampling-based length estimate.",
        return_type: &[ArgType::Path],
      },
    ],
  },
  "path_frame" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Arc-length parameter in [0, 1]; clamped if out of range."
          },
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "path."
          },
          ArgDef {
            name: "inward_normal",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "When true and `t` lies inside a closed subpath whose orientation can be determined, the normal is flipped to point inward (toward the interior of the closed shape) regardless of CW/CCW winding. For open subpaths or paths without topology, has no effect; the normal is the left-perpendicular of the tangent."
          },
        ],
        description: "Samples a path at parameter `t` and returns a frame dict `{pos: vec2, tangent: vec2, normal: vec2}`.\n\n- `pos`: the path point `p(t)`.\n- `tangent`: a unit vector along the path direction, computed via central finite difference (with one-sided fallback at t=0 and t=1).\n- `normal`: a unit vector perpendicular to the tangent. By default it's the left-perpendicular (counter-clockwise rotation by 90°). For closed subpaths whose orientation can be determined, it is flipped to consistently point inward when `inward_normal` is true.\n\nUseful for sweeps, ribbons, offset constructions, or any procedural geometry that needs to follow a path with a consistent local frame.",
        return_type: &[ArgType::Map],
      },
    ],
  },
  "polyline_frames" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `vec2` or `vec3` points defining the polyline.  All points must be the same type.  At least 2 distinct points are required (3 when `closed=true`)."
          },
          ArgDef {
            name: "n",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of evenly-spaced (in arc length) samples.  Open polylines sample inclusively, `t = i / (n - 1)`, so the first and last samples land exactly on the endpoints.  Closed polylines sample half-open, `t = i / n`, so the wrap-around sample doesn't duplicate the first."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true the polyline is treated as a loop: a closing segment from the last point back to the first is added, and frames stay coherent across the seam."
          },
          ArgDef {
            name: "smooth",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.)),
            description: "Turn-smoothing band half-width, in world units of arc length.  `0` (the default) gives the exact per-segment frame, so orientation snaps at each vertex.  Otherwise the frame blends from the incoming to the outgoing segment's frame over `smooth` units either side of the corner.  Automatically clamped per-corner to half the shorter adjacent segment, so bands never overlap and no value is ever \"too large\"."
          },
          ArgDef {
            name: "up",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "vec3 polylines only.  When set, `normal` is `tangent × up`, giving a fixed-reference frame that never rolls — usually what you want when placing upright objects like pillars along a ground path.  When nil (the default), normals are parallel-transported segment to segment (rotation-minimizing), which stays coherent on spines that turn out of any single plane.  Errors if passed for a vec2 polyline."
          },
          ArgDef {
            name: "inward_normal",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "vec2 polylines only.  When true and the polyline is closed, the normal is flipped to point into the interior regardless of CW/CCW winding, matching `path_frame`.  No effect on open or vec3 polylines."
          },
        ],
        description: "Samples a polyline (a `Seq<Vec2>` or `Seq<Vec3>` of points) at a set of positions and returns a `Seq` of frame dicts.\n\nvec2 polylines yield `{t, pos, tangent, normal}`; vec3 polylines additionally yield `binormal`.  `t` is normalized arc length in [0, 1], so evenly-spaced `t` gives evenly-spaced points no matter how the input vertices are distributed.  Values outside [0, 1] are clamped.\n\nFrames are piecewise constant per segment: a sample lands on a segment and takes that segment's direction, with `pos` interpolated along it.  Pass `smooth` to blend orientation across corners instead of snapping.\n\nThis is the polyline counterpart to `path_frame`, which works on continuous 2D paths.  Every call walks the whole point sequence to build its arc-length table, so it's built for short static point lists (tens of points) sampled in one shot — not for repeated random access into long paths.  Consecutive duplicate points are dropped.",
        return_type: &[ArgType::Sequence],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `vec2` or `vec3` points defining the polyline.  All points must be the same type.  At least 2 distinct points are required (3 when `closed=true`)."
          },
          ArgDef {
            name: "t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Explicit sample positions as a sequence of numbers, each a normalized arc-length parameter in [0, 1] (clamped if out of range).  Order is arbitrary; output frames are returned in the order given."
          },
          ArgDef {
            name: "closed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true the polyline is treated as a loop: a closing segment from the last point back to the first is added, and frames stay coherent across the seam."
          },
          ArgDef {
            name: "smooth",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.)),
            description: "Turn-smoothing band half-width, in world units of arc length.  `0` (the default) gives the exact per-segment frame, so orientation snaps at each vertex.  Otherwise the frame blends from the incoming to the outgoing segment's frame over `smooth` units either side of the corner.  Automatically clamped per-corner to half the shorter adjacent segment, so bands never overlap and no value is ever \"too large\"."
          },
          ArgDef {
            name: "up",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec3, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "vec3 polylines only.  When set, `normal` is `tangent × up`, giving a fixed-reference frame that never rolls — usually what you want when placing upright objects like pillars along a ground path.  When nil (the default), normals are parallel-transported segment to segment (rotation-minimizing), which stays coherent on spines that turn out of any single plane.  Errors if passed for a vec2 polyline."
          },
          ArgDef {
            name: "inward_normal",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "vec2 polylines only.  When true and the polyline is closed, the normal is flipped to point into the interior regardless of CW/CCW winding, matching `path_frame`.  No effect on open or vec3 polylines."
          },
        ],
        description: "Samples a polyline (a `Seq<Vec2>` or `Seq<Vec3>` of points) at a set of positions and returns a `Seq` of frame dicts.\n\nvec2 polylines yield `{t, pos, tangent, normal}`; vec3 polylines additionally yield `binormal`.  `t` is normalized arc length in [0, 1], so evenly-spaced `t` gives evenly-spaced points no matter how the input vertices are distributed.  Values outside [0, 1] are clamped.\n\nFrames are piecewise constant per segment: a sample lands on a segment and takes that segment's direction, with `pos` interpolated along it.  Pass `smooth` to blend orientation across corners instead of snapping.\n\nThis is the polyline counterpart to `path_frame`, which works on continuous 2D paths.  Every call walks the whole point sequence to build its arc-length table, so it's built for short static point lists (tens of points) sampled in one shot — not for repeated random access into long paths.  Consecutive duplicate points are dropped.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "texture_levels" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "in_lo",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Input black point"
          },
          ArgDef {
            name: "in_hi",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Input white point"
          },
          ArgDef {
            name: "out_lo",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Output level for the black point (swap with `out_hi` to invert)"
          },
          ArgDef {
            name: "out_hi",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Output level for the white point"
          },
          ArgDef {
            name: "gamma",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Midtone gamma; > 1 brightens midtones, < 1 darkens"
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Photoshop-style levels: `out_lo + (out_hi - out_lo) * clamp((x - in_lo) / (in_hi - in_lo), 0, 1)^(1/gamma)` on color channels; alpha is preserved on 4-channel textures.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "input_image_levels" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: "Stable control id, scoped to the node. Also the default panel label."
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Input texture; also sources the histogram shown behind the control.  Put this downstream of expensive synthesis so scrubbing stays cheap."
          },
          ArgDef {
            name: "default",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional map with any of the keys in_lo/in_hi/out_lo/out_hi/gamma; missing keys use identity values."
          },
          ArgDef {
            name: "label",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Display label override; defaults to `name`."
          },
        ],
        description: "An interactively-editable levels adjustment (`texture_levels` with UI-configured params over a histogram); returns the adjusted texture.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "resize" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Output width in pixels"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Output height in pixels"
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "filter",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("mitchell".to_owned())),
            description: "\"nearest\", \"box\", \"triangle\", \"mitchell\" (the default), or \"lanczos3\""
          },
        ],
        description: "Resamples a texture to new dimensions via a separable filter.  Downsampling is area-correct (the kernel widens with the minification ratio); boundaries respect the texture's wrap mode.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "dilate" => morph_fn_def!("Morphological dilation (per-channel running max over a box window), wrap-aware at boundaries.  O(1) per pixel at any radius.", "Box structuring-element radius in pixels; the window is (2r+1)x(2r+1).  <= 0 returns the input unchanged."),
  "erode" => morph_fn_def!("Morphological erosion (per-channel running min over a box window), wrap-aware at boundaries.  O(1) per pixel at any radius.", "Box structuring-element radius in pixels; the window is (2r+1)x(2r+1).  <= 0 returns the input unchanged."),
  "texture_std" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel population standard deviation over all pixels; returns a float for 1-channel textures, vec2/vec3/vec4 otherwise",
        return_type: &[ArgType::Float, ArgType::Vec2, ArgType::Vec3, ArgType::Vec4],
      },
    ],
  },
  "texture_quantile" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "q",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Quantile in [0, 1]: 0 = min, 0.5 = median, 1 = max"
          },
        ],
        description: "Per-channel value at quantile `q` (e.g. `texture_quantile(t, 0.7)` is the threshold above which 30% of pixels lie); returns a float for 1-channel textures, vec2/vec3/vec4 otherwise.  Exact for textures up to 64k pixels, estimated from a 64k stride-sample above that.",
        return_type: &[ArgType::Float, ArgType::Vec2, ArgType::Vec3, ArgType::Vec4],
      },
    ],
  },
  "texture_standardize" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel `(x - mean) / std`, so every channel has mean 0 and std 1 (the convention `spectral_noise` emits; makes any roughly-Gaussian field interchangeable with it and usable with z-score ramp stops).  A constant channel maps to 0.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "texture_equalize" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel histogram equalization: maps each value to its empirical CDF, so the output is uniformly distributed on [0, 1] whatever the input's distribution (min -> 0, median -> 0.5, max -> 1).  Thresholding the result at `p` covers exactly `1 - p` of the pixels.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "texture_min" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel minimum over all pixels; returns a float for 1-channel textures, vec2/vec3/vec4 otherwise",
        return_type: &[ArgType::Float, ArgType::Vec2, ArgType::Vec3, ArgType::Vec4],
      },
    ],
  },
  "texture_max" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel maximum over all pixels; returns a float for 1-channel textures, vec2/vec3/vec4 otherwise",
        return_type: &[ArgType::Float, ArgType::Vec2, ArgType::Vec3, ArgType::Vec4],
      },
    ],
  },
  "texture_mean" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Per-channel mean over all pixels; returns a float for 1-channel textures, vec2/vec3/vec4 otherwise",
        return_type: &[ArgType::Float, ArgType::Vec2, ArgType::Vec3, ArgType::Vec4],
      },
    ],
  },
  "texture_zip" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable with signature `|in0, in1, ..., uv: vec2, x_ix: int, y_ix: int|: float | vec2 | vec3 | vec4`, invoked once per pixel.  One leading param per input texture, each typed by that texture\'s channel count (1ch -> float, 2ch -> vec2, ...); channel counts may differ freely between inputs.  Trailing params may be omitted.  The return type sets the output\'s channel count."
          },
          ArgDef {
            name: "textures",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Seq of one or more textures with matching dims (channel counts may differ), zipped per-texel and bound to the callable\'s leading params in order"
          },
        ],
        description: "Combines two or more textures into a new one by invoking a callable once per pixel with the corresponding texel of each input.  \n\nAll inputs must have identical dimensions; channel counts are independent.  The result inherits its dimensions, wrap mode, transform, filters, and format from the FIRST input; the others contribute pixels only.  \n\nA single-element seq is allowed and behaves like `map` over that texture.  \n\nLike `map` over a texture and `texture` generators, the body is auto-vectorized into whole-texture kernel passes when it stays inside the supported set; conditionals lower to an exact per-texel select, so masked/conditional blends are as fast as dedicated builtins would be.  Watch the usual fallback triggers: conditional arms must have matching arity, an int arm (`if c { v } else { 0 }`) falls back (write `0.`), and an `if` without `else` or an early `return` falls back.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "concat_channels" => FnDef {
    module: "texture",
    examples: &[],
    // Longest-first: `get_args` returns the first signature that validates without
    // checking that every positional was consumed, so a shorter arity listed first
    // would shadow the longer ones and silently drop the trailing args.
    signatures: &[
      FnSignature {
        arg_defs: &[
          concat_channels_arg!("a"),
          concat_channels_arg!("b"),
          concat_channels_arg!("c"),
          concat_channels_arg!("d"),
        ],
        description: CONCAT_CHANNELS_DESC,
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          concat_channels_arg!("a"),
          concat_channels_arg!("b"),
          concat_channels_arg!("c"),
        ],
        description: CONCAT_CHANNELS_DESC,
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          concat_channels_arg!("a"),
          concat_channels_arg!("b"),
        ],
        description: CONCAT_CHANNELS_DESC,
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "crop" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Left edge of the crop, in pixels"
          },
          ArgDef {
            name: "y",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Top edge of the crop, in pixels"
          },
          ArgDef {
            name: "w",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Crop width in pixels; must be >= 1 and fit within the texture"
          },
          ArgDef {
            name: "h",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Crop height in pixels; must be >= 1 and fit within the texture"
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "O(1) rectangular crop view; no pixels are copied.  Equivalent to `t[y..y+h, x..x+w]`.  `wrap` applies in view space, so a repeat-wrapped crop tiles the crop.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "sharpen" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "amt",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.5)),
            description: "Strength of the re-added high-frequency detail.  0 is a no-op; negative values soften."
          },
          ArgDef {
            name: "sigma",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(2.)),
            description: "Gaussian radius of the blur subtracted to isolate detail; larger picks up coarser features."
          },
        ],
        description: "Unsharp mask: `t + (t - blur(sigma, t)) * amt`.  The texture is the first arg so pipelines partially apply: `t | sharpen(amt=0.3)`.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "texture_invert" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Inverts every channel (`1 - x`), alpha included.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "texture_normalize" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "sigmas",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Sequence, ArgType::Vec2, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Window to stretch instead of the exact [min, max]: a number `k` maps `mean ± k*std` to [0, 1] (2.5 covers ~99% of a Gaussian field; the tails clip instead of washing out the contrast), or a signed `[lo, hi]` pair of z-positions (`[0., 2.]` maps `mean..mean + 2*std`, everything below the mean to 0)."
          },
        ],
        description: "Per-channel linear stretch of a window onto [0, 1], clamped: by default the exact [min, max] (a constant channel maps to 0), or a mean/std-relative window via `sigmas`.  Useful ahead of `texture_levels` / `input_image_levels` when a synthesis step's output range is unknown; see `texture_standardize` / `texture_equalize` for the other normal forms.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "morph_open" => morph_fn_def!("Morphological opening (erode then dilate): removes bright specks smaller than the structuring element, leaving larger shapes at their original size."),
  "morph_close" => morph_fn_def!("Morphological closing (dilate then erode): fills dark pinholes and gaps smaller than the structuring element.  Dual of `morph_open`."),
  "morph_outline" => morph_fn_def!("Morphological gradient (`dilate - erode`): a band straddling every edge, 2r+1 px wide."),
  "morph_tophat" => morph_fn_def!("White top-hat (`t - morph_open(t)`): isolates bright features smaller than the structuring element."),
  "morph_blackhat" => morph_fn_def!("Black top-hat (`morph_close(t) - t`): isolates dark features smaller than the structuring element."),
  "materialize" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Copies a strided texture view (crop/swizzle/flip) into dense storage; no-op on already-dense textures.  A perf hint only — every op accepts views directly.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "flip_x" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a texture horizontally.  O(1): returns a view of the same pixel data.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "flip_y" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Mirrors a texture vertically.  O(1): returns a view of the same pixel data.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "texture_transpose" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Swaps a texture's axes (`out[x, y] = in[y, x]`, so a WxH input yields HxW).  O(1): returns a view of the same pixel data.  Column-wise versions of row-wise ops come from `transpose | op | transpose`.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "texture_roll" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "dx",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Horizontal shift in pixels; positive moves content toward +x (right).  Any int; taken modulo the width."
          },
          ArgDef {
            name: "dy",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Vertical shift in pixels; positive moves content toward +y (down).  Any int; taken modulo the height."
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Circularly shifts a texture by whole pixels: `out[x, y] = in[(x - dx) mod w, (y - dy) mod h]`.  Always toroidal regardless of the texture's wrap mode; exact (no resampling).  For fractional or per-pixel offsets use `sample`.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "sample" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "uv",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Vec2),
            default_value: DefaultValue::Required,
            description: "Continuous coordinate in the same space as texel-closure `uv` params: texel (x, y) is centered at `((x + 0.5) / w, (y + 0.5) / h)`.  Any value; out-of-range coordinates resolve through the wrap mode."
          },
          ArgDef {
            name: "filter",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("bilinear".to_owned())),
            description: "\"bilinear\" (the default) or \"nearest\".  Nearest is an exact texel pick (`floor(uv * dims)`), so integer-stepped coordinates gather pixels without any blending."
          },
          ArgDef {
            name: "wrap",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "\"repeat\", \"clamp\", or \"mirror\"; defaults to the texture's own wrap mode"
          },
        ],
        description: "Reads one texel-space value from a texture at a continuous coordinate, like a GPU texture fetch: returns a float for 1-channel textures, vec2/vec3/vec4 otherwise.  All channels are filtered independently (no alpha premultiplication).  \n\nThe main use is inside texel closures, where it is the gather primitive that expresses warps, stretches, offsets, displacement maps, polar remaps, and so on: `texture(w, h, |uv| sample(src, uv + v2(.05 * sin(uv.y * tau), 0.)))`.  Such bodies auto-vectorize into a single gather pass over the coordinate field.  The output stays tileable whenever the coordinate function is periodic up to whole-texture translations.  Coordinates are floats, so exact pixel-index gathers are written as `sample(src, (v2(x, y) + .5) / dims, filter=\"nearest\")`.",
        return_type: &[ArgType::Float, ArgType::Vec2, ArgType::Vec3, ArgType::Vec4],
      },
    ],
  },
  "texture" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Width in pixels"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Height in pixels"
          },
          ArgDef {
            name: "generator",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Callable of signature `|uv: vec2, x_ix: int, y_ix: int|: float | vec2 | vec3 | vec4`, invoked once per pixel at the pixel's center UV; `x_ix`/`y_ix` are absolute pixel indices and may be omitted from the closure's params.  The return type sets the channel count (float -> 1, vec2 -> 2, vec3 -> 3, vec4 -> 4/RGBA)."
          },
          ArgDef {
            name: "wrap",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("repeat".to_owned())),
            description: "Boundary behavior consulted by texture ops: \"repeat\" (seamless/toroidal, the default), \"clamp\", or \"mirror\""
          },
        ],
        description: "Synthesizes a new texture by evaluating `generator` at every pixel",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "rasterize_path" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path. Path space maps onto the texture's [0,1]² UV space; place it with `translate`/`scale`/`rot` on the path."
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(256)),
            description: "Output width in texels"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(256)),
            description: "Output height in texels"
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule for determining path interiors: evenodd, nonzero, positive, negative (or numeric enum 0-3).  Defaults to the path's own fill rule, else `nonzero`."
          },
          ArgDef {
            name: "tileable",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "`true` tiles seamlessly with period 1 in path units (the texture's UV extent); a number tiles with that period. The path is replicated at every period offset before rasterizing, so shapes crossing the texture edge wrap around and distances/`t` are measured to the nearest copy."
          },
          ArgDef {
            name: "wrap",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("repeat".to_owned())),
            description: "Boundary behavior consulted by texture ops: \"repeat\" (seamless/toroidal, the default), \"clamp\", or \"mirror\""
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves.  When nil, curves are instead flattened to within 0.05 texels of the true curve (and at most 12° per segment), which is usually far fewer segments than the ambient 1° setting."
          },
        ],
        description: "Rasterizes a 2D path into a 1-channel anti-aliased coverage texture: each texel holds the fraction of its area inside the fill under `fill_rule`, so edge texels get partial values.  Follows SVG fill semantics: open subpaths are implicitly closed.\n\nFor strokes, threshold `abs(path_sdf(p))` (round joins/caps) or fill `offset_path(w/2, p)` (exact joins/caps).  The result is an ordinary texture: blit it, scatter it, use it as a mask.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "path_sdf" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path. Path space maps onto the texture's [0,1]² UV space; place it with `translate`/`scale`/`rot` on the path."
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(256)),
            description: "Output width in texels"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(256)),
            description: "Output height in texels"
          },
          ArgDef {
            name: "fill_rule",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Fill rule for determining path interiors: evenodd, nonzero, positive, negative (or numeric enum 0-3).  Defaults to the path's own fill rule, else `nonzero`."
          },
          ArgDef {
            name: "tileable",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "`true` tiles seamlessly with period 1 in path units (the texture's UV extent); a number tiles with that period. The path is replicated at every period offset before rasterizing, so shapes crossing the texture edge wrap around and distances/`t` are measured to the nearest copy."
          },
          ArgDef {
            name: "wrap",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("repeat".to_owned())),
            description: "Boundary behavior consulted by texture ops: \"repeat\" (seamless/toroidal, the default), \"clamp\", or \"mirror\""
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves.  When nil, curves are instead flattened to within 0.05 texels of the true curve (and at most 12° per segment), which is usually far fewer segments than the ambient 1° setting."
          },
        ],
        description: "Signed distance field of a 2D path as a 1-channel texture sampled at texel centers.  Distance is to the drawn curve in path units (the same units as `offset_path` deltas, so `path_sdf(p) < r` agrees with `rasterize_path(offset_path(r, p))`).  Negative inside closed subpaths under `fill_rule`; open subpaths contribute unsigned distance only.\n\nSelf-overlapping shapes keep interior seams in the field; run `path_union(p, p)` first for distance to the filled boundary.  Distance fields compose exactly with `min`/`max` and `blit(..., blend=\"min\")`; threshold once at the end (`smoothstep` with a texel-sized ramp for anti-aliasing).",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "path_uv" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Path),
            default_value: DefaultValue::Required,
            description: "A path. Path space maps onto the texture's [0,1]² UV space; place it with `translate`/`scale`/`rot` on the path."
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(256)),
            description: "Output width in texels"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(256)),
            description: "Output height in texels"
          },
          ArgDef {
            name: "local_t",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "When true, `t` runs over `[0, 1]` separately along every subpath (each ring/stroke gets a full lap) instead of each subpath owning a slice of the global `[0, 1]` proportional to its share of the total length.  Use it for effects that must wrap seamlessly around every loop, e.g. `cos(t * 2 * pi)` on concentric rings."
          },
          ArgDef {
            name: "tileable",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool, ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Bool(false)),
            description: "`true` tiles seamlessly with period 1 in path units (the texture's UV extent); a number tiles with that period. The path is replicated at every period offset before rasterizing, so shapes crossing the texture edge wrap around and distances/`t` are measured to the nearest copy."
          },
          ArgDef {
            name: "wrap",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("repeat".to_owned())),
            description: "Boundary behavior consulted by texture ops: \"repeat\" (seamless/toroidal, the default), \"clamp\", or \"mirror\""
          },
          ArgDef {
            name: "curve_angle_degrees",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Max turning angle (degrees) per segment when discretizing curves.  When nil, curves are instead flattened to within 0.05 texels of the true curve (and at most 12° per segment), which is usually far fewer segments than the ambient 1° setting."
          },
        ],
        description: "Along/across parameterization of a 2D path as a 2-channel `(t, n)` texture.  `t` is the arc-length parameter of the nearest point on the path, global across all subpaths (as used by `path_frame` / `trim_path`) or per-subpath with `local_t=true`; `n` is the signed distance along the `path_frame` normal there: left of travel for open subpaths, inward for closed ones (so `n > 0` inside a closed shape while `path_sdf` is negative).  The 2D analog of `rail_sweep` UVs: stitches via `fract(t * count)` masked by `abs(n) < w`, gradients along a curve via a ramp on `t`.\n\n`t` jumps across the medial axis (texels equidistant from two parts of the path); that discontinuity is inherent to nearest-point parameterization.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "spectral_noise" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "bands",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "8 rows of 4 floats: log-polar spectrum gains, radial bands (low->high frequency, log-spaced over cycles/pixel [1/256, 0.5]) x angular sectors. Each value is a log-power gain relative to the loudest cell, in [-14, 0] nats (0 = loudest, -14 = silent)."
          },
          ArgDef {
            name: "kernels",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Optional spectral peaks; each is [f0y, f0x, sig1, sig2, angle, energy], all floats: f0 in cycles/pixel [-0.5, 0.5]; sig1/sig2 = log10 of the peak's spectral widths, in [-3, -0.5]; angle in radians [0, pi]; energy = log10 of the peak's energy relative to the band spectrum, in [-4, 2]."
          },
          ArgDef {
            name: "width",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(256)),
            description: "Output width in pixels; power of two (FFT synthesis)"
          },
          ArgDef {
            name: "height",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(256)),
            description: "Output height in pixels; power of two (FFT synthesis)"
          },
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: "Noise instance seed; same params + seed + size -> identical texture. Different seeds are independent instances of the same texture, suitable for equal-power crossfade morphing."
          },
          ArgDef {
            name: "freq_scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Scales all model frequencies (>1 = finer detail) without changing the fingerprint; [0.125, 8]"
          },
          ArgDef {
            name: "distribution",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("gaussian".to_owned())),
            description: "\"gaussian\" (default): standardized field, mean 0 / std 1 — the form that composes linearly (equal-power seed crossfades stay exact); pair with z-score ramp stop positions. \"uniform\": values remapped to uniform [0, 1] via the Gaussian CDF, for direct use as a mask/height."
          },
        ],
        description: "Synthesizes a seamless 1-channel Gaussian noise texture from a compact spectral fingerprint (as produced by the texture-utils noise-signature extractor). Params are size-independent: any power-of-two output size yields the same texture statistics.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "load_image" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "uri",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Required,
            description: "`data:image/...;base64,...` data URI. Decoded host-side in the browser; PNG is the canonical format."
          },
          ArgDef {
            name: "srgb",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Bool),
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "Decode sRGB-encoded color channels to linear (the convention for color images). Pass false for data images (heightmaps, texton kernels, masks) whose bytes are raw values. Alpha is never sRGB-decoded."
          },
          ArgDef {
            name: "scale",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(1.)),
            description: "Multiplier applied to every non-alpha channel after decode (decode yields [0, 1])"
          },
          ArgDef {
            name: "offset",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Optional(|| Value::Float(0.)),
            description: "Added to every non-alpha channel after `scale`"
          },
          ArgDef {
            name: "channels",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Force the output channel count (1, 3, or 4). Default auto-detects: 4 if any alpha < 1, 1 if fully gray, else 3."
          },
        ],
        description: "Decodes an embedded base64 image into a float texture (wrap=repeat). The image-decode dependency loads lazily on first use.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "blur" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Gaussian standard deviation in pixels.  <= 0 returns the input unchanged."
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Gaussian-blurs a texture (all channels), respecting its wrap mode at the boundaries",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "height_to_normal" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Heightmap; channel 0 is read as the height"
          },
        ],
        description: "Generates a 3-channel tangent-space normal map (OpenGL convention, encoded 0-1) from a heightmap via wrap-aware central differences",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "strength",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Numeric),
            default_value: DefaultValue::Required,
            description: "Scale applied to the height gradient (height units per texel)"
          },
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Heightmap; channel 0 is read as the height"
          },
        ],
        description: "Generates a 3-channel tangent-space normal map (OpenGL convention, encoded 0-1) from a heightmap via wrap-aware central differences",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "render_texture" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "texture",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("default".to_owned())),
            description: "Output channel name this texture is published under"
          },
          ArgDef {
            name: "usage",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Semantic role of this output: one of \"albedo\", \"normal\", \"roughness\", \"height\", \"metalness\", \"ao\", \"mask\". Drives colorspace handling and preview auto-binding in consumers."
          },
        ],
        description: "Registers a texture as a named output of the composition, symmetric to `render` for meshes",
        return_type: &[ArgType::Nil],
      },
    ],
  },
  "render_texture_stack" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "slices",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Seq of 2-256 textures with matching dims/channels/wrap, interpolated by a normalized index t in [0,1] at render time (slice 0 at t=0, last at t=1)"
          },
          ArgDef {
            name: "name",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("default".to_owned())),
            description: "Output channel name this stack is published under"
          },
          ArgDef {
            name: "usage",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Nil),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Semantic role of this output: one of \"albedo\", \"normal\", \"roughness\", \"height\", \"metalness\", \"ao\", \"mask\". Drives colorspace handling and preview auto-binding in consumers."
          },
        ],
        description: "Registers an ordered set of texture slices as a named stack output of the composition. Consumers sample it as a texture array, interpolating adjacent slices by a per-fragment index t in [0,1].",
        return_type: &[ArgType::Nil],
      },
    ],
  },
  "blit" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "stamp",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Texture to draw; its transform places its centered [-0.5, 0.5]² local frame in the base's UV space. Sampled with decal semantics — its own wrap mode is ignored and outside its bounds contributes nothing."
          },
          ArgDef {
            name: "base",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Texture to draw into. Writes follow ITS wrap mode: \"repeat\" wraps the stamp's footprint around edges (preserving seamless tiling); \"clamp\"/\"mirror\" clip."
          },
          ArgDef {
            name: "blend",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("over".to_owned())),
            description: "One of \"over\", \"add\", \"sub\", \"mul\", \"max\", \"min\". A stamp alpha channel (4-channel always; 2-channel onto a 1-channel base) modulates any mode — \"over\" then alpha-composites like a sprite. Without an alpha channel \"over\" replaces the full stamp quad, so give heightfield stamps a `v2(height, alpha)` shape rather than bare floats."
          },
          ArgDef {
            name: "filter",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("bilinear".to_owned())),
            description: "\"bilinear\" (default; trilinear mip prefiltering kicks in when minified) or \"nearest\" (raw texels, no prefiltering)"
          },
        ],
        description: "Draws `stamp` into `base` at the placement carried by the stamp's transform (see `trans`/`rot`/`scale` on textures), returning a new texture. The base's own transform and wrap mode are preserved on the result.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "composite" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "top",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Texture to composite on top. Must match `bottom`'s dimensions; its transform and wrap mode are ignored."
          },
          ArgDef {
            name: "bottom",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Texture to composite onto. Its transform, wrap mode, and GPU params are preserved on the result."
          },
          ArgDef {
            name: "blend",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("over".to_owned())),
            description: "One of \"over\", \"add\", \"sub\", \"mul\", \"max\", \"min\". A `top` alpha channel (4-channel always; 2-channel onto a 1-channel bottom) modulates any mode."
          },
        ],
        description: "Per-pixel composite of two same-size textures: texel (x, y) of `top` over texel (x, y) of `bottom`, returning a new texture. This is the whole-image counterpart to `blit`, which places a stamp by its transform and resamples — `composite` has no placement, does no filtering, and needs no `trans_global(0.5, 0.5)` to cover.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "scatter" => FnDef {
    module: "texture",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Number of instances to generate and blit"
          },
          ArgDef {
            name: "stamps",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Generator callable `|ix: int|: texture` returning a PLACED texture for each instance (position/rotation/scale via the texture transform ops). Invoked once per instance in order."
          },
          ArgDef {
            name: "base",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Texture the instances are blitted into"
          },
          ArgDef {
            name: "blend",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("over".to_owned())),
            description: "One of \"over\", \"add\", \"sub\", \"mul\", \"max\", \"min\". A stamp alpha channel (4-channel always; 2-channel onto a 1-channel base) modulates any mode — \"over\" then alpha-composites like a sprite. Without an alpha channel \"over\" replaces the full stamp quad, so give heightfield stamps a `v2(height, alpha)` shape rather than bare floats."
          },
          ArgDef {
            name: "filter",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("bilinear".to_owned())),
            description: "\"bilinear\" or \"nearest\""
          },
        ],
        description: "Blits `count` generated stamp instances into `base` (instance order = blend order), returning a new texture. Equivalent to folding `blit` over the generated stamps, but with a single pixel-buffer copy.",
        return_type: &[ArgType::Texture],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "stamps",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of placed textures to blit in order"
          },
          ArgDef {
            name: "base",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Texture),
            default_value: DefaultValue::Required,
            description: "Texture the instances are blitted into"
          },
          ArgDef {
            name: "blend",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("over".to_owned())),
            description: "One of \"over\", \"add\", \"sub\", \"mul\", \"max\", \"min\". A stamp alpha channel (4-channel always; 2-channel onto a 1-channel base) modulates any mode — \"over\" then alpha-composites like a sprite. Without an alpha channel \"over\" replaces the full stamp quad, so give heightfield stamps a `v2(height, alpha)` shape rather than bare floats."
          },
          ArgDef {
            name: "filter",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String),
            default_value: DefaultValue::Optional(|| Value::String("bilinear".to_owned())),
            description: "\"bilinear\" or \"nearest\""
          },
        ],
        description: "Blits each placed texture in `stamps` into `base` in sequence order, returning a new texture.",
        return_type: &[ArgType::Texture],
      },
    ],
  },
  "poisson_points_2d" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Float),
            default_value: DefaultValue::Required,
            description: "Minimum distance between points, in UV units"
          },
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
        ],
        description: "Generates a blue-noise (Poisson-disk) point set over [0,1)² via Bridson's algorithm, packed as densely as the radius allows. Distances are TOROIDAL, so the set tiles seamlessly with repeat-wrapped textures. Deterministic for a given seed.",
        return_type: &[ArgType::Sequence],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "count",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Required,
            description: "Exact number of points to return"
          },
          ArgDef {
            name: "seed",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Int),
            default_value: DefaultValue::Optional(|| Value::Int(0)),
            description: ""
          },
        ],
        description: "Generates exactly `count` blue-noise (Poisson-disk) points over [0,1)² with toroidal distances (tiles seamlessly). The point order is shuffled, so any prefix is itself a well-spaced subset. Deterministic for a given seed.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "keys" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "map",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a lazy sequence of the keys of the given map.  Iteration order is arbitrary but stable for a given map.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "values" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "map",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a lazy sequence of the values of the given map.  Iteration order is arbitrary but stable for a given map, and matches the order of `keys` and `entries`.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "entries" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "map",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a lazy sequence of `[key, value]` pairs for the given map.  Iteration order is arbitrary but stable for a given map, and matches the order of `keys` and `values`.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "from_entries" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "entries",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of `[key, value]` pairs.  Keys must be strings or ints; ints are converted to string keys."
          },
        ],
        description: "Builds a map from a sequence of `[key, value]` pairs; the inverse of `entries`.  Later entries overwrite earlier ones with the same key.",
        return_type: &[ArgType::Map],
      },
    ],
  },
  "has" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "key",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::String, ArgType::Int),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "map",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns `true` if the map contains the given key.  Unlike indexing (which yields `nil` for missing keys), this distinguishes a stored `nil` from an absent key.",
        return_type: &[ArgType::Bool],
      },
    ],
  },
  "group_by" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "cb",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Called with `(elem, index)` for each element; must return a string or int key for the element's group."
          },
          ArgDef {
            name: "seq",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Groups the elements of a sequence into a map of `key -> [elements]`, keyed by the value returned by `cb` for each element.",
        return_type: &[ArgType::Map],
      },
    ],
  },
  "get_in" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of string or int keys to follow through nested maps."
          },
          ArgDef {
            name: "map",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "default",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Returned when the path is missing, hits a non-map value, or resolves to `nil`."
          },
        ],
        description: "Reads the value at a path of keys through nested maps, returning `default` if the path can't be fully resolved.",
        return_type: &[ArgType::Any],
      },
    ],
  },
  "set_in" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of string or int keys to follow through nested maps."
          },
          ArgDef {
            name: "val",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "map",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map),
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a copy of the map with `val` stored at the given path of keys.  Missing or `nil` intermediate entries are created as empty maps; a non-map intermediate value is an error.",
        return_type: &[ArgType::Map],
      },
    ],
  },
  "update_in" => FnDef {
    module: "map",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "path",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Sequence),
            default_value: DefaultValue::Required,
            description: "Sequence of string or int keys to follow through nested maps."
          },
          ArgDef {
            name: "cb",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Callable),
            default_value: DefaultValue::Required,
            description: "Called with the current value at the path (or `default` if missing/`nil`); its return value is stored."
          },
          ArgDef {
            name: "map",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Map),
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "default",
            interned_name: Sym(0),
            valid_types: argtype_flags!(ArgType::Any),
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Passed to `cb` in place of a missing or `nil` value at the path."
          },
        ],
        description: "Returns a copy of the map with the value at the given path of keys replaced by `cb(current)`.  Missing or `nil` intermediate entries are created as empty maps; a non-map intermediate value is an error.",
        return_type: &[ArgType::Map],
      },
    ],
  },
};

#[inline(always)]
pub fn fn_sigs() -> &'static phf::Map<&'static str, FnDef> {
  unsafe { &*addr_of!(FN_SIGNATURE_DEFS) }
}

pub fn get_builtin_fn_sig_entry_ix(name: &str) -> Option<usize> {
  let hashes = phf_shared::hash(name, &fn_sigs().key);
  let index = phf_shared::get_index(&hashes, fn_sigs().disps, fn_sigs().entries.len());
  let entry = &fn_sigs().entries[index as usize];
  let b = entry.0;
  if b == name {
    Some(index as usize)
  } else {
    None
  }
}

pub fn serialize_fn_defs() -> String {
  let serializable_defs: FxHashMap<&'static str, SerializableFnDef> = fn_sigs()
    .entries()
    .map(|(name, def)| (*name, SerializableFnDef::new(name, def)))
    .collect();
  SerJson::serialize_json(&serializable_defs)
}
