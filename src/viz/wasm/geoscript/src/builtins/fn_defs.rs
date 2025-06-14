use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;
use nanoserde::SerJson;

use crate::{ArgType, Value};

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
  pub valid_types: &'static [ArgType],
  pub default_value: DefaultValue,
  pub description: &'static str,
}

#[derive(SerJson)]
pub struct SerializableArgDef {
  pub name: &'static str,
  pub valid_types: &'static [ArgType],
  pub default_value: SerializableDefaultValue,
  pub description: &'static str,
}

impl From<&ArgDef> for SerializableArgDef {
  fn from(arg_def: &ArgDef) -> Self {
    Self {
      name: arg_def.name,
      valid_types: arg_def.valid_types,
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

pub struct FnDef {
  pub arg_defs: &'static [ArgDef],
  pub description: &'static str,
  pub return_type: &'static [ArgType],
}

#[derive(SerJson)]
pub struct SerializableFnDef {
  pub arg_defs: Vec<SerializableArgDef>,
  pub description: &'static str,
  pub return_type: &'static [ArgType],
}

impl SerializableFnDef {
  fn new(defs: &[FnDef]) -> Vec<Self> {
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

pub(crate) static FN_SIGNATURE_DEFS: phf::Map<&'static str, &'static [FnDef]> = phf::phf_map! {
  "box" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "width",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Width along the X axis"
        },
        ArgDef {
          name: "height",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Height along the Y axis"
        },
        ArgDef {
          name: "depth",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Depth along the Z axis"
        },
      ],
      description: "Creates a rectangular prism mesh with the specified width, height, and depth",
      return_type: &[ArgType::Mesh],
    },
    // TODO: this should be split into two variants
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "size",
          valid_types: &[ArgType::Vec3, ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(1., 1., 1.))),
          description: "Size of the box as a Vec3, or a single numeric value for a cube"
        },
      ],
      description: "Creates a box using a uniform or vector size",
      return_type: &[ArgType::Mesh],
    },
  ],
  "translate" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "translation",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Translates a mesh",
      return_type: &[ArgType::Mesh],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "x",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "y",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "z",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Translates a mesh",
      return_type: &[ArgType::Mesh],
    },
  ],
  "rot" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "rotation",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: "Rotation defined by Euler angles in radians"
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: "Mesh to rotate"
        },
      ],
      description: "Rotates a mesh using a Vec3 of Euler angles (radians)",
      return_type: &[ArgType::Mesh],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "x",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Rotation about X axis (radians)"
        },
        ArgDef {
          name: "y",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Rotation about Y axis (radians)"
        },
        ArgDef {
          name: "z",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Rotation about Z axis (radians)"
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: "Mesh to rotate"
        },
      ],
      description: "Rotates a mesh using individual Euler angle components in radians",
      return_type: &[ArgType::Mesh],
    },
  ],
  "look_at" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "pos",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: "Source position"
        },
        ArgDef {
          name: "target",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: "Target position"
        },
      ],
      description: "TODO: this is currently broken",
      return_type: &[ArgType::Vec3],
    },
  ],
  "scale" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "x",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "y",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "z",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Scales a mesh by separate factors along each axis",
      return_type: &[ArgType::Mesh],
    },
    // TODO: this should be split into two variants
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "scale",
          valid_types: &[ArgType::Vec3, ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Scales a mesh",
      return_type: &[ArgType::Mesh],
    },
  ],
  "vec3" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "x",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "y",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "z",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Creates a Vec3 given x, y, z",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Creates a Vec3 with all components set to `value`",
      return_type: &[ArgType::Vec3],
    },
  ],
  "union" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the boolean union of two meshes (`a | b`)",
      return_type: &[ArgType::Mesh],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "meshes",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence of meshes to union"
        },
      ],
      description: "Returns the boolean union of a sequence of meshes (`meshes[0] | meshes[1] | ...`)",
      return_type: &[ArgType::Mesh],
    },
  ],
  "difference" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: "Base mesh"
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: "Mesh to subtract"
        },
      ],
      description: "Returns the boolean difference of two meshes (`a - b`)",
      return_type: &[ArgType::Mesh],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "meshes",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence of meshes to subtract in order"
        },
      ],
      description: "Returns the boolean difference of a sequence of meshes (`meshes[0] - meshes[1] - ...`)",
      return_type: &[ArgType::Mesh],
    },
  ],
  "intersect" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the boolean intersection of two meshes (`a & b`)",
      return_type: &[ArgType::Mesh],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "meshes",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence of meshes to intersect"
        },
      ],
      description: "Returns the boolean intersection of a sequence of meshes (`meshes[0] & meshes[1] & ...`)",
      return_type: &[ArgType::Mesh],
    },
  ],
  "fold" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "initial_val",
          valid_types: &[ArgType::Any],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|acc, x|: acc`"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Same as `reduce` but with an explicit initial value",
      return_type: &[ArgType::Any],
    },
  ],
  "reduce" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|acc, x|: acc`"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence to reduce"
        },
      ],
      description: "Same as `fold` but with the first element of the sequence as the initial value",
      return_type: &[ArgType::Any],
    },
  ],
  "neg" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Negates an integer",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Negates a float",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Negates each component of a Vec3",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Required,
          description: "Boolean to negate (logical NOT)"
        },
      ],
      description: "Inverts a boolean (logical NOT)",
      return_type: &[ArgType::Bool],
    },
  ],
  "pos" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "h"
        },
      ],
      description: "Passes through the input unchanged (implementation detail of the unary `+` operator)",
      return_type: &[ArgType::Numeric],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Passes through the input unchanged (implementation detail of the unary `+` operator)",
      return_type: &[ArgType::Vec3],
    },
  ],
  "abs" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Absolute Value",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Absolute Value",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Component-wise absolute value of a Vec3",
      return_type: &[ArgType::Vec3],
    },
  ],
  "sqrt" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Square Root",
      return_type: &[ArgType::Float],
    },
  ],
  "add" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Adds two Vec3s component-wise",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a + b",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a + b",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a + b",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a boolean union of two meshes",
      return_type: &[ArgType::Mesh],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "offset",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Translates the given mesh by a Vec3 offset",
      return_type: &[ArgType::Mesh],
    },
  ],
  "sub" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Component-wise subtraction of two Vec3s",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a -b",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a - b",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a - b",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: "Base mesh"
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: "Mesh to subtract"
        },
      ],
      description: "Returns the boolean difference of two meshes (`a - b`)",
      return_type: &[ArgType::Mesh],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "offset",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Translates the mesh by (`-offset`)",
      return_type: &[ArgType::Mesh],
    },
  ],
  "mul" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the component-wise product of two Vec3 values",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Multiplied each element of a Vec3 by a scalar",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a * b",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a * b",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a * b",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: "Mesh to scale"
        },
        ArgDef {
          name: "factor",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Uniform scale to apply to the mesh"
        },
      ],
      description: "Uniformly scales a mesh by a scalar factor",
      return_type: &[ArgType::Mesh],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "factor",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Scales `mesh` by `factor` along each axis",
      return_type: &[ArgType::Mesh],
    },
  ],
  "div" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the component-wise division of two Vec3 values",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Divides each element of a Vec3 by a scalar",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a / b",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a / b",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a / b",
      return_type: &[ArgType::Int],
    },
  ],
  "mod" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "a % b",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Floating point modulus (`a % b`)",
      return_type: &[ArgType::Float],
    },
  ],
  "max" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the minimum of the provided arguments",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns maximum of the provided arguments",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Component-wise maximum of two Vec3s",
      return_type: &[ArgType::Vec3],
    },
  ],
  "min" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the minimum of the provided arguments",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns minimum of the provided arguments",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Component-wise minimum of two Vec3s",
      return_type: &[ArgType::Vec3],
    },
  ],
  "float" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Numeric value to convert to float"
        },
      ],
      description: "Converts a numeric value to a float",
      return_type: &[ArgType::Float],
    },
  ],
  "int" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Numeric value to convert to int"
        },
      ],
      description: "Converts a numeric value to an int",
      return_type: &[ArgType::Int],
    },
  ],
  "and" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Logical AND operation of two booleans",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the boolean intersection of two meshes (`a & b`)",
      return_type: &[ArgType::Mesh],
    },
  ],
  "or" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Logical OR operation of two booleans",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the boolean union of two meshes (`a | b`)",
      return_type: &[ArgType::Mesh],
    },
  ],
  "xor" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Logical XOR operation of two booleans",
      return_type: &[ArgType::Bool],
    },
  ],
  "not" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Inverts the boolean value (logical NOT)",
      return_type: &[ArgType::Bool],
    },
  ],
  "bit_and" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Bitwise AND operation of two integers (`a & b`)",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the boolean intersection of two meshes (`a & b`)",
      return_type: &[ArgType::Mesh],
    },
  ],
  "bit_or" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Bitwise OR operation of two integers (`a | b`)",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the boolean union of two meshes (`a | b`)",
      return_type: &[ArgType::Mesh],
    },
  ],
  "map" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|x|: y`"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence to map over"
        },
      ],
      description: "Applies a function to each element of a sequence and returns a new sequence.  \n\nThis is lazy and will not evaluate the function until the output sequence is consumed.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "filter" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|x|: bool`"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence to filter"
        },
      ],
      description: "Filters a sequence using a predicate function.  \n\nThis is lazy and will not evaluate the function until the output sequence is consumed.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "take" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "count",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of elements to take from the start of the sequence"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence to take elements from"
        },
      ],
      description: "Returns a new sequence containing the first `n` elements of the input sequence.  If `n` is greater than the length of the sequence, the entire sequence is returned.  \n\nThis is lazy and will not evaluate the sequence until the output sequence is consumed.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "skip" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "count",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of elements to skip from the start of the sequence"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence to skip elements from"
        },
      ],
      description: "Returns a new sequence with the first `n` elements skipped.  If `n` is greater than the length of the sequence, an empty sequence is returned.  \n\nThis is lazy and will not evaluate the sequence until the output sequence is consumed.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "take_while" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|x|: bool`"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence to take elements from"
        },
      ],
      description: "Returns a new sequence containing elements from the start of the input sequence until the predicate function returns false.  \n\nThis is lazy and will not evaluate the function until the output sequence is consumed.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "skip_while" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|x|: bool`"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a new sequence with elements skipped from the start of the input sequence until the predicate function returns false.  \n\nThis is lazy and will not evaluate the function until the output sequence is consumed.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "chain"=> &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "sequences",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence of sequences to chain together"
        },
      ],
      description: "Returns a new sequence that concatenates all input sequences.  \n\nThis is lazy and will not evaluate the sequences until the output sequence is consumed.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "first" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence to get the first element from"
        },
      ],
      description: "Returns the first element of a sequence, or `Nil` if the sequence is empty.  \n\nThis is lazy and will not evaluate the sequence until the output is consumed.",
      return_type: &[ArgType::Any],
    },
  ],
  "print" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "",
          valid_types: &[ArgType::Any],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Prints all provided args and kwargs to the console",
      return_type: &[ArgType::Nil],
    },
  ],
  "render" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: "Mesh to render"
        },
      ],
      description: "Renders a mesh to the scene",
      return_type: &[ArgType::Nil],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "meshes",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence of meshes to render"
        },
      ],
      description: "Renders a sequence of meshes to the scene.  Each mesh will be rendered separately.",
      return_type: &[ArgType::Nil],
    },
  ],
  "sin" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the sine of a numeric value",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the sine of each component of a Vec3",
      return_type: &[ArgType::Vec3],
    },
  ],
  "cos" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Cosine",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the cosine of each component of a Vec3",
      return_type: &[ArgType::Vec3],
    },
  ],
  "tan" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Tangent",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the tangent of each component of a Vec3",
      return_type: &[ArgType::Vec3],
    },
  ],
  "pow" => &[
    // TODO: should split into int and float versions
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "base",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "exponent",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`Returns `base` raised to the power of `exponent``",
      return_type: &[ArgType::Numeric],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "base",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "exponent",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a Vec3 with each component raised to the power of `exponent`",
      return_type: &[ArgType::Vec3],
    },
  ],
  "trunc" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Truncates a numeric value to its integer part",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Truncates each component of a Vec3 to its integer part",
      return_type: &[ArgType::Vec3],
    },
  ],
  "fract" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the fractional part of a numeric value",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a Vec3 with the fractional part of each component",
      return_type: &[ArgType::Vec3],
    },
  ],
  "round" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Rounds a numeric value to the nearest integer",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Rounds each component of a Vec3 to the nearest integer",
      return_type: &[ArgType::Vec3],
    },
  ],
  "ceil" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Rounds a numeric value up to the nearest integer",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Rounds each component of a Vec3 up to the nearest integer",
      return_type: &[ArgType::Vec3],
    },
  ],
  "floor" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Rounds a numeric value down to the nearest integer",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Rounds each component of a Vec3 down to the nearest integer",
      return_type: &[ArgType::Vec3],
    },
  ],
  "fix_float" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "If the provided float is NaN, non-infinite, or subnormal, returns 0.0.  Otherwise, returns the float unchanged.",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: "Vec3 value to fix"
        },
      ],
      description: "For each component of the Vec3, if it is NaN, non-infinite, or subnormal, returns 0.0.  Otherwise, returns the component unchanged.",
      return_type: &[ArgType::Vec3],
    },
  ],
  "rad2deg" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Converts radians to degrees",
      return_type: &[ArgType::Float],
    },
  ],
  "deg2rad" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Converts degrees to radians",
      return_type: &[ArgType::Float],
    },
  ],
  "gte" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a >= b`",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a >= b`",
      return_type: &[ArgType::Bool],
    },
  ],
  "lte" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a <= b`",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a <= b`",
      return_type: &[ArgType::Bool],
    },
  ],
  "gt" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a > b`",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a > b`",
      return_type: &[ArgType::Bool],
    },
  ],
  "lt" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a < b`",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a < b`",
      return_type: &[ArgType::Bool],
    },
  ],
  "eq" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a == b`",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a == b`",
      return_type: &[ArgType::Bool],
    },
  ],
  "neq" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a != b`",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`a != b`",
      return_type: &[ArgType::Bool],
    },
  ],
  "point_distribute" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "count",
          valid_types: &[ArgType::Int, ArgType::Nil],
          default_value: DefaultValue::Required,
          description: "The number of points to distribute across the mesh.  If `nil`, returns an infinite sequence."
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Distributes a specified number of points uniformly across the surface of a mesh returned as a sequence.  \n\nThis is lazy; the points will not be generated until the sequence is consumed.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "lerp" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "t",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Linearly interpolates between two Vec3 values `a` and `b` by a factor `t`",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "t",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Linearly interpolates between two numeric values `a` and `b` by a factor `t`",
      return_type: &[ArgType::Float],
    },
  ],
  "compose" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "",
          valid_types: &[],
          default_value: DefaultValue::Required,
          description: ""
        }
      ],
      description: "Composes all arguments, returning a callable like `|x| arg1(arg2(arg3(x)))`",
      return_type: &[ArgType::Callable],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "callables",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence of callables to compose"
        },
      ],
      description: "Composes a sequence of callables, returning a callable like `|x| callables[0](callables[1](...callables[n](x)))`",
      return_type: &[ArgType::Callable],
    },
  ],
  "convex_hull" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "points",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Computes the convex hull of a sequence of points, returning a mesh representing the convex hull",
      return_type: &[ArgType::Mesh],
    },
  ],
  "warp" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|pos: vec3, normal: vec3|: vec3`.  Given the position and normal of each vertex in the mesh, returns a new position for that vertex in the output mesh."
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Applies a warp function to each vertex of the mesh, returning a new mesh with each vertex transformed by `fn`.",
      return_type: &[ArgType::Mesh],
    },
  ],
  "tessellate" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "target_edge_length",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Tessellates a mesh, splitting edges to achieve a target edge length.",
      return_type: &[ArgType::Mesh],
    },
  ],
  "connected_components" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Splits a mesh into its connected components, returning a sequence of meshes where each mesh is a connected component of the input mesh.  The sequence of connected components is sorted by vertex count from highest to lowest.\n\nThis is NOT lazy; the connected components are computed at the time this function is called.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "len" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "v",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the length/magnitude of a Vec3",
      return_type: &[ArgType::Float],
    },
  ],
  "distance" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`sqrt((a.x - b.x)^2 + (a.y - b.y)^2 + (a.z - b.z)^2)`",
      return_type: &[ArgType::Float],
    },
  ],
  "normalize" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "v",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a normalized Vec3 (length 1) in the same direction as the input vector",
      return_type: &[ArgType::Vec3],
    },
  ],
  "bezier3d" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "p0",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "p1",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "p2",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "p3",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "count",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Generates a sequence of `count` evenly-spaced points along a cubic Bezier curve defined by four control points",
      return_type: &[ArgType::Sequence],
    },
  ],
  "extrude_pipe" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "radius",
          valid_types: &[ArgType::Numeric, ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Radius of the pipe or a callable with signature `|point_ix: int, path_point: vec3|: float` that returns the radius at each point along the path"
        },
        ArgDef {
          name: "resolution",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Optional(|| Value::Int(8)),
          description: "Number of segments to use for the pipe's circular cross-section"
        },
        ArgDef {
          name: "path",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence of Vec3 points defining the path of the pipe.  Often the output of a function like `bezier3d`."
        },
        ArgDef {
          name: "close_ends",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(true)),
          description: "Whether to close the ends of the pipe with triangle fans"
        },
        ArgDef {
          name: "twist",
          valid_types: &[ArgType::Numeric, ArgType::Callable],
          default_value: DefaultValue::Optional(|| Value::Float(0.)),
          description: "Twist angle in radians to apply along the path, or a callable with signature `|point_ix: int, path_point: vec3|: float` that returns the twist angle at each point along the path.  A value of 0 means no twist."
        }
        // TODO: support closed path that connects back to the start
      ],
      description: "Extrudes a pipe along a sequence of points.  The radius can be constant or vary along the path using a callable.",
      return_type: &[ArgType::Mesh],
    },
  ],
  "torus_knot_path" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "radius",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "tube_radius",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "p",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of times the knot wraps around the torus in the longitudinal direction"
        },
        ArgDef {
          name: "q",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of times the knot wraps around the torus in the meridional direction"
        },
        ArgDef {
          name: "point_count",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of points to generate along the path"
        },
      ],
      description: "Generates a sequence of points defining a torus knot path",
      return_type: &[ArgType::Sequence],
    },
  ],
  "torus_knot" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "radius",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "tube_radius",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "p",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of times the knot wraps around the torus in the longitudinal direction"
        },
        ArgDef {
          name: "q",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of times the knot wraps around the torus in the meridional direction"
        },
        ArgDef {
          name: "point_count",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of points to generate along the path"
        },
        ArgDef {
          name: "tube_resolution",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of segments to use for the tube's circular cross-section"
        },
      ],
      description: "Generates a torus knot mesh with a specified radius, tube radius, and number of twists.",
      return_type: &[ArgType::Mesh],
    },
  ],
  "simplify" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "tolerance",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Float(0.01)),
          description: "The maximum distance between the original and simplified meshes.  0.01 is a good starting point."
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Simplifies a mesh, reducing the number of vertices.  Maintains manifold-ness.",
      return_type: &[ArgType::Mesh],
    },
  ],
  "verts" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a sequence of all vertices in a mesh in an arbitrary order",
      return_type: &[ArgType::Sequence],
    },
  ],
  "randf" => &[
    FnDef {
      arg_defs: &[],
      description: "Returns a random float between 0. and 1.",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "min",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "max",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a random float between `min` and `max`",
      return_type: &[ArgType::Float],
    },
  ],
  "randi" => &[
    FnDef {
      arg_defs: &[],
      description: "Returns a random integer between 0 and 2^31 - 1",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "min",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "max",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a random integer between `min` and `max` (inclusive)",
      return_type: &[ArgType::Int],
    },
  ],
  "fbm" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "pos",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Generates a fractal Brownian motion (FBM) value at a given position using default parameters",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "seed",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Optional(|| Value::Int(0)),
          description: ""
        },
        ArgDef {
          name: "octaves",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Optional(|| Value::Int(4)),
          description: ""
        },
        ArgDef {
          name: "frequency",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Float(1.)),
          description: ""
        },
        ArgDef {
          name: "lacunarity",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Float(2.)),
          description: ""
        },
        ArgDef {
          name: "persistence",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Float(0.5)),
          description: ""
        },
        ArgDef {
          name: "pos",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Samples fractional brownian noise at a given position using the specified parameters",
      return_type: &[ArgType::Float],
    },
  ],
  "icosphere" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "radius",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "resolution",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of subdivisions to apply when generating the icosphere.  0 -> 20 faces, 1 -> 80 faces, 2 -> 320 faces, ..."
        },
      ],
      description: "Generates an icosphere mesh",
      return_type: &[ArgType::Mesh],
    },
  ],
  "cylinder" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "radius",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "height",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "radial_segments",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "height_segments",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Generates a cylinder mesh",
      return_type: &[ArgType::Mesh],
    },
  ],
  "call" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Calls `fn` with no arguments, returning its return value",
      return_type: &[ArgType::Any],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "args",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Calls `fn` with the provided arguments, returning its return value",
      return_type: &[ArgType::Any],
    },
  ],
  "mesh" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "verts",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Sequence of Vec3 vertices, pointed into by `faces`"
        },
        ArgDef {
          name: "indices",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "A flag sequence of integer indices corresponding to triangles.  Must have `length % 3 == 0`"
        },
      ],
      description: "Creates a mesh from a sequence of vertices and indices",
      return_type: &[ArgType::Mesh],
    },
  ]
};

pub fn serialize_fn_defs() -> String {
  let serializable_defs: FxHashMap<&'static str, Vec<SerializableFnDef>> = FN_SIGNATURE_DEFS
    .entries()
    .map(|(name, defs)| (*name, SerializableFnDef::new(defs)))
    .collect();
  SerJson::serialize_json(&serializable_defs)
}
