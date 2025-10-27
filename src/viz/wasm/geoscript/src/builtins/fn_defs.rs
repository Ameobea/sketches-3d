use std::{f32::consts::PI, rc::Rc};

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;
use nanoserde::SerJson;

use crate::{
  builtins::FUNCTION_ALIASES,
  lights::{AmbientLight, DirectionalLight},
  ArgType, Value, Vec2,
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

pub(crate) static FN_SIGNATURE_DEFS: phf::Map<&'static str, FnDef> = phf::phf_map! {
  "box" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh, ArgType::Light],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Translates a mesh or light",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
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
  },
  "rot" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "rotation",
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Required,
            description: "Rotation defined by Euler angles in radians"
          },
          ArgDef {
            name: "light",
            valid_types: &[ArgType::Light],
            default_value: DefaultValue::Required,
            description: "Light to rotate"
          },
        ],
        description: "Rotates a light using a Vec3 of Euler angles (radians)",
        return_type: &[ArgType::Light],
      },
      FnSignature {
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
            name: "light",
            valid_types: &[ArgType::Light],
            default_value: DefaultValue::Required,
            description: "Light to rotate"
          },
        ],
        description: "Rotates a light using individual Euler angle components in radians",
        return_type: &[ArgType::Light],
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
      FnSignature {
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
        description: "Orients a mesh to look at a target point.  This replaces any currently applied rotation.\n\nThere's a good chance this implementation is broken too...",
        return_type: &[ArgType::Vec3],
      }
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
      FnSignature {
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
  },
  "origin_to_geometry" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "apply_transforms" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "flip_normals" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Flips the normals of a mesh, returning a new mesh with inverted normals.  This actually flips the winding order of the mesh's triangles under the hood and re-computes normals based off that.",
        return_type: &[ArgType::Mesh],
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
      FnSignature {
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
  },
  "vec3" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "xy",
            valid_types: &[ArgType::Vec2],
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
        description: "Creates a Vec3 from a Vec2 and a z component",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "x",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "yz",
            valid_types: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Creates a Vec3 with all components set to `value`",
        return_type: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Sequence],
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
            valid_types: &[ArgType::String],
            // this needs to stay required in order to allow the signatures to be disambiguated
            default_value: DefaultValue::Required,
            description: "String to insert between each mesh"
          },
          ArgDef {
            name: "strings",
            valid_types: &[ArgType::Sequence],
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
      FnSignature {
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
  },
  "difference" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 26 }],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "intersect" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 25 }],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "fold" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "fold_while" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
            description: "Callable with signature `|acc, x|: acc | nil`.  If this callback returns `nil`, the final state of the accumulator passed into that iteration will be returned."
          },
          ArgDef {
            name: "sequence",
            valid_types: &[ArgType::Sequence],
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
  },
  "any" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "all" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "for_each" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
            valid_types: &[ArgType::Sequence],
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
            valid_types: &[ArgType::Int],
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
            valid_types: &[ArgType::Float],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Bool],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Negates each component of a Vec2",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Passes through the input unchanged (implementation detail of the unary `+` operator)",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Int],
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
            valid_types: &[ArgType::Float],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise absolute value of a Vec2",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Float],
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
            valid_types: &[ArgType::Int],
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
            valid_types: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Component-wise square root of a Vec2",
        return_type: &[ArgType::Vec2],
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
        description: "Combines two meshes into one mesh containing all geometry from both inputs.\n\nThis does NOT perform a boolean union; for that, use the `union` function or the `|` operator.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
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
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "vec",
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "num",
            valid_types: &[ArgType::Numeric],
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
        description: "Adds two Vec2s component-wise",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
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
        description: "Adds a numeric value to each component of a Vec2",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
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
        description: "Concatenates two strings",
        return_type: &[ArgType::String],
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
        description: "Returns the boolean difference of two meshes (`a - b`).  This is equivalent to the `difference` function.",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
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
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "vec",
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "num",
            valid_types: &[ArgType::Numeric],
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
        description: "Subtracts two Vec2s component-wise",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
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
        description: "Subtracts a numeric value from each component of a Vec2",
        return_type: &[ArgType::Vec2],
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
  },
  "div" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
        description: "Returns the component-wise division of two `Vec2`s",
        return_type: &[ArgType::Vec2],
      },
      FnSignature {
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
        description: "Divides each component of a Vec2 by a scalar",
        return_type: &[ArgType::Vec2],
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
      FnSignature {
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
        description: "Floating point modulus (`a % b`)",
        return_type: &[ArgType::Float],
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
      FnSignature {
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
        description: "Returns maximum of the provided arguments",
        return_type: &[ArgType::Float],
      },
      FnSignature {
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
      FnSignature {
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
        description: "Component-wise maximum of two Vec2s",
        return_type: &[ArgType::Vec2],
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
      FnSignature {
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
        description: "Returns minimum of the provided arguments",
        return_type: &[ArgType::Float],
      },
      FnSignature {
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
      FnSignature {
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
        description: "Component-wise minimum of two Vec2s",
        return_type: &[ArgType::Vec2],
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
      FnSignature {
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
      FnSignature {
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Clamps each component of a Vec2 between min and max",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric, ArgType::String],
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
            valid_types: &[ArgType::Numeric, ArgType::String],
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
            valid_types: &[ArgType::Any],
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
      FnSignature {
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
  },
  "or" => FnDef {
    module: "logic",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "xor" => FnDef {
    module: "logic",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "not" => FnDef {
    module: "logic",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "bit_and" => FnDef {
    module: "bits",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "bit_or" => FnDef {
    module: "bits",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "map" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            valid_types: &[ArgType::Callable],
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x: T, ix: int|: y`"
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
      FnSignature {
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
  },
  "filter" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "fn",
            valid_types: &[ArgType::Callable],
            default_value: DefaultValue::Required,
            description: "Callable with signature `|x: T, ix: int|: bool`"
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
  },
  "scan" => FnDef {
    module: "seq",
    examples: &[FnExample { composition_id: 40 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "initial",
            valid_types: &[ArgType::Any],
            default_value: DefaultValue::Required,
            description: "Initial value to seed the state with.  This value will NOT be included in the output sequence."
          },
          ArgDef {
            name: "fn",
            valid_types: &[ArgType::Callable],
            default_value: DefaultValue::Required,
            description: "Callable with signature `|acc, x, ix|: acc`"
          },
          ArgDef {
            name: "sequence",
            valid_types: &[ArgType::Sequence],
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
  },
  "skip" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "take_while" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "skip_while" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "chain"=> FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "first" => FnDef {
    module: "seq",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "sequence",
            valid_types: &[ArgType::Sequence],
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
            valid_types: &[ArgType::Sequence],
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
            valid_types: &[ArgType::Any],
            default_value: DefaultValue::Required,
            description: "The value to add to the end of the sequence"
          },
          ArgDef {
            name: "seq",
            valid_types: &[ArgType::Sequence],
            default_value: DefaultValue::Required,
            description: "The sequence to which the value should be added"
          }
        ],
        description: "Appends a value to the end of a sequence, returning a new sequence.  The old sequence is left unchanged.\n\nIf the sequence is not eager (as produced by the `collect` function, an array literal, or similar), it will be collected into memory before this happens.\n\nNote that this isn't very efficient and requires collecting and/or cloning the underlying sequence.  It's better to keep things lazy and use sequences and sequence combinators where possible.",
        return_type: &[ArgType::Sequence]
      }
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
            valid_types: &[ArgType::Sequence],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a new sequence with the elements in reverse order.  \n\nThis is NOT lazy and will evaluate the entire sequence immediately and collect all of its elements into memory.",
        return_type: &[ArgType::Sequence],
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
            valid_types: &[ArgType::Sequence],
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
            valid_types: &[ArgType::Any],
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
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Renders a mesh to the scene",
        return_type: &[ArgType::Nil],
      },
      FnSignature {
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
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "meshes",
            valid_types: &[ArgType::Sequence],
            default_value: DefaultValue::Required,
            description: "Either:\n - `Seq<Mesh | Light | Seq<Vec3>>` of objects to render to the scene, or\n - `Seq<Vec3>` of points representing a path to render",
          },
        ],
        description: "Renders a sequence of entities to the scene.  The sequence can contain a heterogeneous mix of meshes, lights, and paths (`Seq<Vec3>`).  Each entity will be rendered separately.",
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the sine of each component of a Vec3",
        return_type: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the cosine of each component",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the tangent of each component",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the arccosine of each component",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the arcsine of each component",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the arctangent of each component",
        return_type: &[ArgType::Vec2],
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
        description: "Returns the arctangent of `y / x`, using the signs of both arguments to determine the correct quadrant.",
        return_type: &[ArgType::Float],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "xy",
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the arctangent of `y / x` for a Vec2, using the signs of both components to determine the correct quadrant.",
        return_type: &[ArgType::Float],
      }
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
      FnSignature {
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
      FnSignature {
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
  },
  "exp" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "value",
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the exponential of each component",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the base-2 logarithm of each component",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Truncates each component of a Vec2 to its integer part",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a Vec2 with the fractional part of each component",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec2 to the nearest integer",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec2 up to the nearest integer",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Rounds each component of a Vec2 down to the nearest integer",
        return_type: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Vec3],
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
            valid_types: &[ArgType::Vec2],
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
            valid_types: &[ArgType::Numeric],
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
            valid_types: &[ArgType::Numeric],
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
      FnSignature {
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
  },
  "lte" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "gt" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "lt" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "eq" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
  },
  "neq" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
      FnSignature {
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
  },
  "point_distribute" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 28 }],
    signatures: &[
      FnSignature {
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
  },
  "lerp" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: ""
          },
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
        description: "Linearly interpolates between two Vec3 values `a` and `b` by a factor `t`",
        return_type: &[ArgType::Vec3],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "t",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: ""
          },
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
        description: "Linearly interpolates between two numeric values `a` and `b` by a factor `t`",
        return_type: &[ArgType::Float],
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
  },
  "linearstep" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "compose" => FnDef {
    module: "fn",
    examples: &[FnExample { composition_id: 34 }],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "convex_hull" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 23 }],
    signatures: &[
      FnSignature {
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
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
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
  },
  "tessellate" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "subdivide_by_plane" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "plane_normal",
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Required,
            description: "Normal vector of the plane to cut the mesh with"
          },
          ArgDef {
            name: "plane_offset",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: "Offset of the plane from the origin along the plane normal"
          },
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subdivides a mesh by a plane, splitting all edges and faces that intersect the plane.",
        return_type: &[ArgType::Sequence],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "plane_normals",
            valid_types: &[ArgType::Sequence],
            default_value: DefaultValue::Required,
            description: "Sequence of normal vectors of the planes to cut the mesh with"
          },
          ArgDef {
            name: "plane_offsets",
            valid_types: &[ArgType::Sequence],
            default_value: DefaultValue::Required,
            description: "Sequence of offsets of the planes from the origin along the plane normals"
          },
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Subdivides a mesh by a sequence of planes, splitting all edges and faces that intersect any of the planes.  This is more efficient than repeatedly subdividing by a single plane multiple times.",
        return_type: &[ArgType::Sequence],
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
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Required,
            description: "Normal vector of the plane to cut the mesh with"
          },
          ArgDef {
            name: "plane_offset",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: "Offset of the plane from the origin along the plane normal"
          },
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
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
            valid_types: &[ArgType::Mesh],
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
  },
  "intersects_ray" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 36 }],
    signatures: &[
      FnSignature {
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
  },
  "len" => FnDef {
    module: "core",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            valid_types: &[ArgType::String],
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
            valid_types: &[ArgType::Sequence],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns the number of elements in a sequence.  This will fully evaluate the sequence.  Calling this with an infinite sequence will result in the program hanging or crashing.",
        return_type: &[ArgType::Int],
      }
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
            valid_types: &[ArgType::String],
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
            valid_types: &[ArgType::Bool],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "message",
            valid_types: &[ArgType::String],
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
      FnSignature {
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
  },
  "normalize" => FnDef {
    module: "math",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "v",
            valid_types: &[ArgType::Vec2],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Returns a normalized Vec2 (length 1) in the same direction as the input vector",
        return_type: &[ArgType::Vec2],
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
  },
  "superellipse_path" => FnDef {
    module: "path",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "extrude_pipe" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 8 }, FnExample { composition_id: 41 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "radius",
            valid_types: &[ArgType::Numeric, ArgType::Callable],
            default_value: DefaultValue::Required,
            description: "Radius of the pipe or a callable with signature `|point_ix: int, path_point: vec3|: float | seq` that returns the radius at each point along the path.  If a sequence is returned by the callback, its length must match the resolution and it should contain the distance of each point along the ring from the pipe's center."
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
  },
  "torus_knot_path" => FnDef {
    module: "path",
    examples: &[FnExample { composition_id: 13 }],
    signatures: &[
      FnSignature {
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
  },
  "lissajous_knot_path" => FnDef {
    module: "path",
    examples: &[FnExample { composition_id: 35 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "amp",
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(1., 1., 1.))),
            description: "Amplitude of the Lissajous curve in each axis"
          },
          ArgDef {
            name: "freq",
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(3., 5., 7.))),
            description: "Frequency of the Lissajous curve in each axis.  These should be \"pairwise-coprime\" integers, meaning that the ratio of any two frequencies should not be reducible to a simpler fraction."
          },
          ArgDef {
            name: "phase",
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Optional(|| Value::Vec3(Vec3::new(0., PI / 2., PI / 5.))),
            description: "Phase offset of the Lissajous curve in each axis"
          },
          ArgDef {
            name: "count",
            valid_types: &[ArgType::Int],
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
  },
  "stitch_contours" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 22 }],
    signatures: &[
      FnSignature {
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
  },
  "trace_geodesic_path" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 49 }],
    signatures: &[
      FnSignature {
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
        description: "Traces a geodesic path across the surface of a mesh, following a sequence of 2D points.  The mesh must be manifold.\n\nReturns a `Seq<Vec3>` of points on the surface of the mesh that were visited during the walk.",
        return_type: &[ArgType::Sequence],
      },
    ],
  },
  "alpha_wrap" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 57 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "alpha",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(1. / 30.)),
            description: "Controls the feature size of the computed wrapping.  Smaller values will capture more detail and produce more faces.\n\nThis value is relative to the bounding box of the input mesh.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "offset",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(0.03)),
            description: "Controls the offset distance between the the input and the wrapped output mesh surfaces.  Larger values will result in simpler outputs with better triangle quality, potentially at the cost of sharp edges and fine details.\n\nThis value is relative to the bounding box of the input mesh.  Values should be in the range (0, 1)."
          },
        ],
        description: "Computes an alpha-wrap of a mesh.  This is kind of like a concave version of a convex hull.  This function is guaranteed to produce watertight/2-manifold outputs.\n\nFor more details, see here: https://doc.cgal.org/latest/Alpha_wrap_3/index.html",
        return_type: &[ArgType::Mesh],
      },
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "points",
            valid_types: &[ArgType::Sequence],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "alpha",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(1. / 30.)),
            description: "Controls the feature size of the computed wrapping.  Smaller values will capture more detail and produce more faces.\n\nThis value is relative to the bounding box of the input points.  Values should be in the range (0, 1)."
          },
          ArgDef {
            name: "offset",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(0.03)),
            description: "Controls the offset distance between the the input and the wrapped output mesh surfaces.  Larger values will result in simpler outputs with better triangle quality, potentially at the cost of sharp edges and fine details.\n\nThis value is relative to the bounding box of the input points.  Values should be in the range (0, 1)."
          },
        ],
        description: "Computes an alpha-wrap of a sequence of points.  This is kind of like a concave version of a convex hull.  This function is guaranteed to produce watertight/2-manifold outputs.\n\nFor more details, see here: https://doc.cgal.org/latest/Alpha_wrap_3/index.html",
        return_type: &[ArgType::Mesh],
      }
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
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "type",
            valid_types: &[ArgType::String],
            default_value: DefaultValue::Optional(|| Value::String("catmullclark".to_string())),
            description: "Type of smoothing to perform.  Supported values are \"catmullclark\", \"loop\", \"doosabin\", and \"sqrt\".",
          },
          ArgDef {
            name: "iterations",
            valid_types: &[ArgType::Int],
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
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "max_angle_deg",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(5.)),
            description: "Maximum angle in degrees between face normals for faces to be considered coplanar"
          },
          ArgDef {
            name: "max_offset",
            valid_types: &[ArgType::Numeric, ArgType::Nil],
            default_value: DefaultValue::Optional(|| Value::Nil),
            description: "Maximum distance from the plane for faces to be considered coplanar.  This is an absolute distance in the mesh's local space.  If not provided or set to `nil`, it will default to 1% of the diagonal length of the mesh's bounding box."
          },
        ],
        description: "Remeshes a mesh by identifying planar regions and simplifying them into a simpler set of triangles.  This is useful for optimizing meshes produced by a variety of other built-in methods that tend to produce a lot of small triangles in flat areas.\n\nSee the docs for the underlying CGAL function for more details: https://doc.cgal.org/5.6.3/Polygon_mesh_processing/index.html - Section 2.1.2 Remeshing",
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
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: "Target edge length for the remeshed output.  Edges will be split or collapsed to try to achieve this length."
          },
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "iterations",
            valid_types: &[ArgType::Int],
            default_value: DefaultValue::Optional(|| Value::Int(1)),
            description: "Number of remeshing iterations to perform.  Typical values are between 1 and 5."
          },
          ArgDef {
            name: "protect_borders",
            valid_types: &[ArgType::Bool],
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, edges on the border of the mesh will not be modified."
          },
          ArgDef {
            name: "protect_sharp_edges",
            valid_types: &[ArgType::Bool],
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, edges with angles >= the `sharp_angle_threshold_degrees` will not be modified."
          },
          ArgDef {
            name: "sharp_angle_threshold_degrees",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(30.)),
            description: "Angle threshold in degrees for edges to be considered sharp.  Only used if `protect_sharp_edges` is true."
          },
        ],
        description: "Remeshes a mesh to have more uniform edge lengths, targeting the specified edge length.  \n\nSee the docs for the underlying CGAL function for more details: https://doc.cgal.org/5.6.3/Polygon_mesh_processing/index.html - Section 2.1.2 Remeshing",
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
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "facet_distance",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(0.14)),
            description: "This controls the precision of the output mesh.  Smaller values will produce meshes that more closely approximate the input, but will also produce more faces.  This value represents units in the local space of the mesh, so it must be chosen relative to the size of the input mesh."
          },
          ArgDef {
            name: "target_edge_length",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(0.2)),
            description: "This serves as an upper bound for the lengths of curve edges in the underlying Delaunay triangulation.  Smaller values will produce more faces.  This value represents units in the local space of the mesh, so it must be chosen relative to the size of the input mesh."
          },
          ArgDef {
            name: "protect_sharp_edges",
            valid_types: &[ArgType::Bool],
            default_value: DefaultValue::Optional(|| Value::Bool(true)),
            description: "If true, edges with angles >= the `sharp_angle_threshold_degrees` will not be modified."
          },
          ArgDef {
            name: "sharp_angle_threshold_degrees",
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Optional(|| Value::Float(30.)),
            description: "Angle threshold in degrees for edges to be considered sharp.  Only used if `protect_sharp_edges` is true."
          },
        ],
        description: "Remeshes a mesh using a Delaunay refinement of a restricted Delaunay triangulation.  TODO improve docs once we understand the behavior of this function better.\n\nSee the docs for the underlying CGAL function for more details: https://doc.cgal.org/latest/Polygon_mesh_processing/index.html - Section 2.1.2 Remeshing",
        return_type: &[ArgType::Mesh],
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
  },
  "simplify" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "verts" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
            default_value: DefaultValue::Required,
            description: ""
          },
          ArgDef {
            name: "world_space",
            valid_types: &[ArgType::Bool],
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
  "randi" => FnDef {
    module: "rand",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
        arg_defs: &[],
        description: "Returns a random integer.  Any 64-bit integer is equally possible, positive or negative.",
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
      FnSignature {
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
      FnSignature {
        arg_defs: &[],
        description: "Returns a random Vec3 where each component is between 0. and 1.",
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
            valid_types: &[ArgType::Vec3],
            default_value: DefaultValue::Required,
            description: ""
          },
        ],
        description: "Generates a fractal Brownian motion (FBM) value at a given position using default parameters",
        return_type: &[ArgType::Float],
      },
      FnSignature {
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
  },
  "icosphere" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "cylinder" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "grid" => FnDef {
    module: "mesh",
    examples: &[],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "size",
            valid_types: &[ArgType::Vec2, ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: "Size of the grid along the X and Z axes (providing a number will set the same size for both axes).  The grid is centered at the origin, so a size of `vec2(1, 1)` will produce a grid that extends from -0.5 to +0.5 along both axes."
          },
          ArgDef {
            name: "divisions",
            valid_types: &[ArgType::Vec2, ArgType::Int],
            default_value: DefaultValue::Optional(|| Value::Vec2(Vec2::new(1., 1.))),
            description: "Number of subdivisions along each axis.  If an integer is provided, it will be used for both axes."
          },
          ArgDef {
            name: "flipped",
            valid_types: &[ArgType::Bool],
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
  // "suzanne" => FnDef {
  //   module: "mesh",
  //   examples: &[],
  //   signatures: &[
  //     FnSignature {
  //       arg_defs: &[],
  //       description: "Generates a Suzanne mesh, a 3D model from Blender often used as a test or placeholder object.\n\nThis mesh is NOT manifold, so it cannot be used with functions that require a manifold mesh such as mesh boolean ops, `trace_geodesic_path`, and others.",
  //       return_type: &[ArgType::Mesh],
  //     },
  //   ],
  // },
  "call" => FnDef {
    module: "fn",
    examples: &[],
    signatures: &[
      FnSignature {
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
      FnSignature {
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
  },
  "mesh" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 32 }],
    signatures: &[
      FnSignature {
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
              Value::Map(Rc::new(FxHashMap::from_iter([
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
              Value::Map(Rc::new(FxHashMap::from_iter([
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
  },
  "ambient_light" => FnDef {
    module: "light",
    examples: &[],
    signatures: &[
      FnSignature {
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
  },
  "set_material" => FnDef {
    module: "mesh",
    examples: &[FnExample { composition_id: 30 }],
    signatures: &[
      FnSignature {
        arg_defs: &[
          ArgDef {
            name: "material",
            valid_types: &[ArgType::Material],
            default_value: DefaultValue::Required,
            description: "Can be either a `Material` value or a string specifying the name of an externally defined material"
          },
          ArgDef {
            name: "mesh",
            valid_types: &[ArgType::Mesh],
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
            valid_types: &[ArgType::Material],
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
            valid_types: &[ArgType::Int],
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
            valid_types: &[ArgType::Numeric],
            default_value: DefaultValue::Required,
            description: "The angle at which edges are considered sharp, in degrees"
          }
        ],
        description: "Sets the sharp angle threshold for computing auto-smooth-shaded normals (specified in degrees).  If the angle between two adjacent faces is greater than this angle, the edge will be considered sharp.",
        return_type: &[ArgType::Nil],
      }
    ]
  }
};

pub fn serialize_fn_defs() -> String {
  let serializable_defs: FxHashMap<&'static str, SerializableFnDef> = FN_SIGNATURE_DEFS
    .entries()
    .map(|(name, def)| (*name, SerializableFnDef::new(name, def)))
    .collect();
  SerJson::serialize_json(&serializable_defs)
}
