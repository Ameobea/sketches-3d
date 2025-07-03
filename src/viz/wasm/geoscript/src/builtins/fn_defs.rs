use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;
use nanoserde::SerJson;

use crate::{
  lights::{AmbientLight, DirectionalLight},
  ArgType, Value,
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
          name: "object",
          valid_types: &[ArgType::Mesh, ArgType::Light],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Translates a mesh or light",
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
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "target",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "up",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 1., 0.))),
          description: ""
        },
      ],
      description: "Orients a mesh to look at a target point.  This replaces any currently applied rotation.",
      return_type: &[ArgType::Vec3],
    }
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
  "origin_to_geometry" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Moves the mesh so that its origin is at the center of its geometry (averge of all its vertices), returning a new mesh.\n\nThis will actually modify the vertex positions and preserve any existing transforms.",
      return_type: &[ArgType::Mesh],
    },
  ],
  "apply_transforms" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Applies rotation, translation, and scale transforms to the vertices of a mesh, resetting the transforms to identity",
      return_type: &[ArgType::Mesh],
    },
  ],
  "vec2" => &[
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
      ],
      description: "Creates a Vec2 given x, y",
      return_type: &[ArgType::Vec2],
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
      description: "Creates a Vec2 with both components set to `value`",
      return_type: &[ArgType::Vec2],
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
  "join" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "meshes",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Combines a sequence of meshes into one mesh containing all geometry from the inputs.\n\nThis does NOT perform a boolean union; for that, use the `union` function or the `|` operator to create a union over a sequence of meshes.",
      return_type: &[ArgType::Mesh],
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
  "any" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "cb",
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
      description: "Returns true if any element of the sequence makes the callback return true",
      return_type: &[ArgType::Bool],
    },
  ],
  "all" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "cb",
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
      description: "Returns true if all elements of the sequence make the callback return true",
      return_type: &[ArgType::Bool],
    },
  ],
  "for_each" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "cb",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|x|`"
        },
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Applies the callback to each element of the sequence, returning `nil`",
      return_type: &[ArgType::Nil],
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
          description: ""
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
      description: "Combines two meshes into one mesh containing all geometry from both inputs.\n\nThis does NOT perform a boolean union; for that, use the `union` function, the `|` operator, or the `join` function to create a union over a sequence of meshes.",
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
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec2],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec2],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the component-wise product of two Vec2 values",
      return_type: &[ArgType::Vec2],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec2],
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
      description: "Multiplied each element of a Vec2 by a scalar",
      return_type: &[ArgType::Vec2],
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
          valid_types: &[ArgType::Numeric],
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
          valid_types: &[ArgType::Numeric],
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
  "clamp" => &[
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
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Clamps a value between min and max",
      return_type: &[ArgType::Int],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "min",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "max",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Float],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Clamps a value between min and max",
      return_type: &[ArgType::Float],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "min",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "max",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Clamps each component of a Vec3 between min and max",
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
          description: ""
        },
      ],
      description: "Converts a value to a float",
      return_type: &[ArgType::Float],
    },
  ],
  "int" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "value",
          valid_types: &[ArgType::Numeric, ArgType::String],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Converts a value to an int",
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
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "fn",
          valid_types: &[ArgType::Callable],
          default_value: DefaultValue::Required,
          description: "Callable with signature `|vtx: Vec3, normal: Vec3|: Vec3` that will be invoked for each vertex in the new mesh, returning a new position for that vertex"
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Applies a function to each vertex in a mesh and returns a new mesh with the transformed vertices.",
      return_type: &[ArgType::Mesh],
    }
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
  "reverse" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "sequence",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a new sequence with the elements in reverse order.  \n\nThis is NOT lazy and will evaluate the entire sequence immediately and collect all of its elements into memory.",
      return_type: &[ArgType::Sequence],
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
          description: ""
        },
      ],
      description: "Renders a mesh to the scene",
      return_type: &[ArgType::Nil],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "light",
          valid_types: &[ArgType::Light],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Renders a light to the scene",
      return_type: &[ArgType::Nil],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "meshes",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "Either:\n - `Seq<Mesh | Light | Seq<Vec3>>` of objects to render to the scene, or\n - `Seq<Vec3>` of points representing a path to render",
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
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Nil],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Any],
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
          valid_types: &[ArgType::Any],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Nil],
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
      description: "`a == b`",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::String],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::String],
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
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Nil],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Any],
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
          valid_types: &[ArgType::Any],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Nil],
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
      description: "`a != b`",
      return_type: &[ArgType::Bool],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::String],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::String],
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
        ArgDef {
          name: "seed",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Optional(|| Value::Int(0)),
          description: ""
        },
        ArgDef {
          name: "cb",
          valid_types: &[ArgType::Callable, ArgType::Nil],
          default_value: DefaultValue::Optional(|| Value::Nil),
          description: "Optional callable with signature `|point: vec3, normal: vec3|: any`.  If provided, this function will be called for each generated point and normal and whatever it returns will be included in the output sequence instead of the point."
        },
        ArgDef {
          name: "world_space",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(true)),
          description: "If true, points and normals will be returned in world space.  If false, they will be returned in the local space of the mesh."
        }
      ],
      description: "Distributes a specified number of points uniformly across the surface of a mesh returned as a sequence.  If `cb` is Nil or not provided, a sequence of vec3 positions will be returned.  If `cb` is provided, the sequence will consist of the return values from calling `cb(pos, normal)` for each sampled point.\n\nThis is lazy; the points will not be generated until the sequence is consumed.",
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
  "smoothstep" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "edge0",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "edge1",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "x",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Works the same as `smoothstep` in GLSL.\n\nIt returns 0 if `x < edge0`, 1 if `x > edge1`, and a smooth Hermite interpolation between 0 and 1 for values of `x` between `edge0` and `edge1`.",
      return_type: &[ArgType::Float],
    },
  ],
  "linearstep" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "edge0",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "edge1",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "x",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Same as `smoothstep` but with simple linear interpolation instead of Hermite interpolation.\n\nIt returns 0 if `x < edge0`, 1 if `x > edge1`, and a linear interpolation between 0 and 1 for values of `x` between `edge0` and `edge1`.",
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
  "intersects" => &[
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
      description: "Returns true if the two meshes intersect, false otherwise",
      return_type: &[ArgType::Bool],
    },
  ],
  "intersects_ray" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "ray_origin",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "ray_direction",
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
        ArgDef {
          name: "max_distance",
          valid_types: &[ArgType::Float, ArgType::Nil],
          default_value: DefaultValue::Optional(|| Value::Nil),
          description: "Max distance to check for intersection (`nil` considers intersections at any distance).  If the intersection occurs at a distance greater than this, `false` will be returned."
        },
      ],
      description: "Casts a ray from `ray_origin` in `ray_direction` and checks if it intersects `mesh` within `max_distance` (or any distance if `max_distance` is `nil`)",
      return_type: &[ArgType::Bool],
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
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "v",
          valid_types: &[ArgType::Vec2],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns the length/magnitude of a Vec2",
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
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "a",
          valid_types: &[ArgType::Vec2],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "b",
          valid_types: &[ArgType::Vec2],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "`sqrt((a.x - b.x)^2 + (a.y - b.y)^2`",
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
  "superellipse_path" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "width",
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
          name: "n",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: "Exponent that controls the shape of the superellipse.  A value of 2 produces an ellipse, higher values produce more rectangular shapes, and lower values produce diamond and star-like shapes."
        },
        ArgDef {
          name: "point_count",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Required,
          description: "Number of points to generate along the path"
        },
      ],
      description: "Generates a sequence of points defining a superellipse, or rounded rectangle.  Returns a sequence of `point_count` `Vec2` points",
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
          name: "connect_ends",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(false)),
          description: "Whether the pipe should be a closed loop, connecting the last point back to the first.  If true, the first and last points of the path will be connected with triangles."
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
  "extrude" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "up",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: "Direction to extrude the mesh.  Vertices will be displaced by this amount."
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        }
      ],
      description: "Extrudes a mesh in the direction of `up` by displacing each vertex along that direction.  This is designed to be used with 2D meshes; using it on meshes with volume or thickness will probably not work.",
      return_type: &[ArgType::Mesh],
    }
  ],
  "stitch_contours" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "contours",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "A `Seq<Seq<Vec3>>`, where each inner sequence contains points representing a contour that will be stitched together into a mesh"
        },
        ArgDef {
          name: "flipped",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(false)),
          description: "If true, the winding order of the triangles generated will be flipped - inverting the inside/outside of the generated mesh."
        },
        ArgDef {
          name: "closed",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(true)),
          description: "If true, the contours will be stitched together as closed loops - connecting the last point to the first for each one."
        },
        ArgDef {
          name: "cap_start",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(false)),
          description: "If true, a triangle fan will be created to cap the first contour"
        },
        ArgDef {
          name: "cap_end",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(false)),
          description: "If true, a triangle fan will be created to cap the last contour"
        },
        ArgDef {
          name: "cap_ends",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(false)),
          description: "shorthand for `cap_start=true, cap_end=true`"
        }
      ],
      description: "Stitches together a sequence of contours into a single mesh.  The contours should be closed loops.",
      return_type: &[ArgType::Mesh],
    },
  ],
  "trace_geodesic_path" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "path",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "A sequence of `Vec2` points representing movements to take across the surface of the mesh relative to the current position.  For example, a sequence of `[vec2(0, 1), vec2(1, 0)]` would move 1 unit up and then 1 unit right."
        },
        ArgDef {
          name: "mesh",
          valid_types: &[ArgType::Mesh],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "world_space",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(true)),
          description: "If true, points will be returned in world space.  If false, they will be returned in the local space of the mesh."
        },
        ArgDef {
          name: "full_path",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(true)),
          description: "This controls behavior when the path crosses between faces in the mesh.  If true, intermediate points will be included in the output for whenever the path hits an edge.  This can result in the output sequence having more elements than the input sequence, and it will ensure that all generated edges in the output path lie on the surface of the mesh."
        },
        ArgDef {
          name: "start_pos_local_space",
          valid_types: &[ArgType::Vec3, ArgType::Nil],
          default_value: DefaultValue::Optional(|| Value::Nil),
          description: "If provided, the starting position for the path will be snapped to the surface of the mesh at this position.  If `nil`, the walk will start at an arbitrary point on the mesh surface."
        },
        ArgDef {
          name: "up_dir_world_space",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., 1., 0.))),
          description: "When the walk starts, it will be oriented such that the positive Y axis of the local tangent space is aligned as closely as possible with this up direction at the starting position.  Another way of saying this is that it lets you set what direction is north when first starting the walk on the mesh's surface."
        },
      ],
      description: "Traces a geodesic path across the surface of a mesh, following a sequence of 2D points.  The mesh must be manifold.",
      return_type: &[ArgType::Sequence],
    },
  ],
  "fan_fill" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "path",
          valid_types: &[ArgType::Sequence],
          default_value: DefaultValue::Required,
          description: "A sequence of Vec3 points representing the path to fill"
        },
        ArgDef {
          name: "closed",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(true)),
          description: "If true, the path will be treated as closed - connecting the last point to the first."
        },
        ArgDef {
          name: "flipped",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(false)),
          description: "If true, the winding order of the triangles generated will be flipped - inverting the inside/outside of the generated mesh."
        },
        ArgDef {
          name: "center",
          valid_types: &[ArgType::Vec3, ArgType::Nil],
          default_value: DefaultValue::Optional(|| Value::Nil),
          description: "If provided, the center point for the fan will be placed at this position.  Otherwise, the center will be computed as the average of the points in the path."
        }
      ],
      description: "Builds a fan of triangles from a sequence of points, filling the area inside them.  One triangle will be built for each pair of adjacent points in the path, connecting them to the center point.",
      return_type: &[ArgType::Mesh],
    }
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
    FnDef {
      arg_defs: &[],
      description: "Returns a random float between 0. and 1.",
      return_type: &[ArgType::Float],
    },
  ],
  "randi" => &[
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
    FnDef {
      arg_defs: &[],
      description: "Returns a random integer.  Any 64-bit integer is equally possible, positive or negative.",
      return_type: &[ArgType::Int],
    },
  ],
  "randv" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "min",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "max",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a random Vec3 where each component is between the corresponding components of `min` and `max`",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "min",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
        ArgDef {
          name: "max",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Required,
          description: ""
        },
      ],
      description: "Returns a random Vec3 where each component is between `min` and `max`",
      return_type: &[ArgType::Vec3],
    },
    FnDef {
      arg_defs: &[],
      description: "Returns a random Vec3 where each component is between 0. and 1.",
      return_type: &[ArgType::Vec3],
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
          default_value: DefaultValue::Optional(|| Value::Int(1)),
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
  ],
  "dir_light" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "target",
          valid_types: &[ArgType::Vec3],
          default_value: DefaultValue::Optional(|| Value::Vec3(DirectionalLight::default().target)),
          description: "Target point that the light is pointing at.  If the light as at the same position as the target, it will point down towards negative Y"
        },
        ArgDef {
          name: "color",
          valid_types: &[ArgType::Int, ArgType::Vec3],
          default_value: DefaultValue::Optional(|| Value::Int(DirectionalLight::default().color as i64)),
          description: "Color of the light in hex format (like 0xffffff) or webgl format (like `vec3(1., 1., 1.)`)"
        },
        ArgDef {
          name: "intensity",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Float(DirectionalLight::default().intensity)),
          description: ""
        },
        ArgDef {
          name: "cast_shadow",
          valid_types: &[ArgType::Bool],
          default_value: DefaultValue::Optional(|| Value::Bool(DirectionalLight::default().cast_shadow)),
          description: ""
        },
        ArgDef {
          name: "shadow_map_size",
          valid_types: &[ArgType::Map, ArgType::Int],
          default_value: DefaultValue::Optional(|| {
            let shadow_map_size = DirectionalLight::default().shadow_map_size;
            Value::Map(Box::new(FxHashMap::from_iter([
              ("width".to_string(), Value::Int(shadow_map_size.width as i64)),
              ("height".to_string(), Value::Int(shadow_map_size.height as i64)),
            ].into_iter())))
          }),
          description: "Size of the shadow map.  Allowed keys: `width`, `height`. OR, a single integer value that will be used for both width and height."
        },
        ArgDef {
          name: "shadow_map_radius",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Float(DirectionalLight::default().shadow_map_radius)),
          description: "Radius for shadow map filtering"
        },
        ArgDef {
          name: "shadow_map_blur_samples",
          valid_types: &[ArgType::Int],
          default_value: DefaultValue::Optional(|| Value::Int(DirectionalLight::default().shadow_map_blur_samples as i64)),
          description: "Number of samples for shadow map blur"
        },
        ArgDef {
          name: "shadow_map_type",
          valid_types: &[ArgType::String],
          default_value: DefaultValue::Optional(|| Value::String(DirectionalLight::default().shadow_map_type.to_str().to_owned())),
          description: "Allowed values: `vsm`"
        },
        ArgDef {
          name: "shadow_map_bias",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Float(DirectionalLight::default().shadow_map_bias)),
          description: ""
        },
        ArgDef {
          name: "shadow_camera",
          valid_types: &[ArgType::Map],
          default_value: DefaultValue::Optional(|| {
            let shadow_camera = DirectionalLight::default().shadow_camera;
            Value::Map(Box::new(FxHashMap::from_iter([
              ("near".to_string(), Value::Float(shadow_camera.near)),
              ("far".to_string(), Value::Float(shadow_camera.far)),
              ("left".to_string(), Value::Float(shadow_camera.left)),
              ("right".to_string(), Value::Float(shadow_camera.right)),
              ("top".to_string(), Value::Float(shadow_camera.top)),
              ("bottom".to_string(), Value::Float(shadow_camera.bottom)),
            ].into_iter())))
          }),
          description: "Camera parameters for the shadow map.  Allowed keys: `near`, `far`, `left`, `right`, `top`, `bottom`"
        },
      ],
      description: "Creates a directional light.\n\nNote: This will not do anything until it is added to the scene via `render`",
      return_type: &[ArgType::Light],
    }
  ],
  "ambient_light" => &[
    FnDef {
      arg_defs: &[
        ArgDef {
          name: "color",
          valid_types: &[ArgType::Int, ArgType::Vec3],
          default_value: DefaultValue::Optional(|| Value::Int(AmbientLight::default().color as i64)),
          description: "Color of the light in hex format (like 0xffffff) or webgl format (like `vec3(1., 1., 1.)`)"
        },
        ArgDef {
          name: "intensity",
          valid_types: &[ArgType::Numeric],
          default_value: DefaultValue::Optional(|| Value::Float(AmbientLight::default().intensity)),
          description: ""
        },
      ],
      description: "Creates an ambient light.\n\nNote: This will not do anything until it is added to the scene via `render`",
      return_type: &[ArgType::Light],
    }
  ],
};

pub fn serialize_fn_defs() -> String {
  let serializable_defs: FxHashMap<&'static str, Vec<SerializableFnDef>> = FN_SIGNATURE_DEFS
    .entries()
    .map(|(name, defs)| (*name, SerializableFnDef::new(defs)))
    .collect();
  SerJson::serialize_json(&serializable_defs)
}
