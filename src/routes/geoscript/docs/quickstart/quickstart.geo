////////////
// BASICS //
////////////

// variables are assigned like this:
a = 1
b = a + 3

// Statements can optionally end with a semicolon
c = b - 1;

// The built-in `print` function can be used to print any value to the console
print(a, b, 1+2)

// Types can optionally be specified when defining variables.  They are checked at
// runtime and sometimes enable more efficient runtime performance.
//
// `int`s are represented internally as a 64-bit signed integer.  `float`s are
// represented as a 32-bit single precision float.
my_int: int = -3

// The `num` type indicates that the value can either be an `int` or `float`
//
// Numbers are automatically cast as needed for basic arithmetic operations
x: num = 1 + 3.0

// there are built-in `vec2` and `vec3` types which hold floats:
v0 = vec2(0, 1)
v1 = vec3(3) // same as `vec3(3, 3, 3)`
// they support GLSL-style swizzles
v2 = v1.zyx

// Functions are defined by creating closures using a similar syntax to Rust.
// They return the value of their last expression.
add_one = |x| x + 1

// Arg types, return types, and curly brackets are all optional
add_two = |x: int| { x + 2 }
add_three = |x: int|: int x + 3;

// Closures automatically capture clones of variables they reference from outer scopes.
//
// This is similar to creating a `move` closure in Rust and then cloning all the values
// before moving them into it.
x = 3
add_x = |i| i + x

// There's a dedicated pipeline operator (like those from Elixir, F#, or Bash) that can be used
// to chain function calls:
times_two = |x| x * 2
plus_one = |x| x + 1
six = 1 | times_two | plus_one | times_two

// Functions are auto-curried if at least one argument is provided and the full set of provided
// arguments match at least one defined function signature
add_three: fn = add(3)
nine = add_three(6)

// Control flow is very similar to rust.  `if`/`else` blocks are expressions.
three = if 1 == 2 { 0 } else if 1 > 0 { 3 } else { -100 }

///////////////
// SEQUENCES //
///////////////

// All sequences are lazy - if they're eager internally.  They work similarly to iterators
// in Rust; they do nothing until they're consumed.

// There are range operators like in Rust
exclusive_range = 0..10
inclusive_range = 0..=5

// They can be used with built-in iterator combinators:
two_three_four: seq = 0..10 | skip(2) | take(3)

// All operators have equivalent named built-in functions
//
// The full reference for all built-ins can be found here:
// https://3d.ameo.design/geoscript/docs
nine = two_three_four | reduce(add)

// There's a built-in `->` operator which is a shorthand for `x | map(fn)`.  The following two
// statements are equivalent:
two_four_six = 0..8 | map(mul(2))
two_four_six = 0..8 -> mul(2)

////////////
// MESHES //
////////////

// Meshes are first-class data types.  This creates a cube mesh with a width of 1 unit:
my_mesh: mesh = box(1)

// Meshes can be added to the scene and displayed by using the built-in `render` function
render(my_mesh)

// Meshes can be translated, rotated, and scaled:
my_mesh = my_mesh
  | rot(0, pi/3, 0) // euler angles in radians
  | scale(2)
  | trans(0, 10, -10)

// There are many built-in functions to generating and manipulating meshes.  Some of the most
// useful are mesh boolean operations.
//
// This creates a new mesh which contains only the area which is inside both the sphere and
// the cube:
middle = intersect(icosphere(radius=10, resolution=2), box(16))

// It's also possible to use shortand operators to apply boolean operations on meshes:
middle = middle - box(20,4,8) // `a AND (NOT b)`
middle = middle | box(1,20,4) // `a OR b`

// There are also shorthand operators for translating meshes
moved = box(1) + vec3(0, 10, 0) // translates the mesh along the y axis by 10 units

// Boolean operations are rather computationally expensive - especially for larger meshes.
//
// If you just want to combine all the vertices/faces from two meshes into one WITHOUT
// splitting faces and removing interior geometry, you can use the `+` operator or the
// `join` function:
a = box(3) + box(4, 1, 4)
a = 0..3 -> (|i| box(1) + vec3(i)) | join

// There are various other built-in functions for interacting with meshes and doing more
// specialized things like collision detection, raycasting, sampling points on a mesh's
// surface, iterating over vertices, computing a convex hull, and more.

// This picks 10 random points on the surface of a cube, generates a smaller cuber at each
// of them, joins them together into a single mesh using boolean operations, and renders
// the result
box(5)
  | point_distribute(count=10)
  -> |pos| { box(1) | trans(pos) }
  | union
  | render

// Another very useful mesh function is warp.  It lets you map over the vertices of a mesh,
// setting a new position for each of them given their current position and normal:
warped_sphere = icosphere(radius=10, resolution=4)
  | warp(|pos: vec3, norm: vec3| {
    if pos.y > -2.5 && pos.y < 2.5 {
      pos = vec3(pos.x, 0, pos.z)
    }
    if pos.y > 0 {
      return pos
    }
    pos + norm*3
  })

// There's also a shorthand for warp using the map operator:
// `mesh -> warp_fn`

// The `simplify` function optimizes meshes by removing edges to a specified tolerance.
// 0.1 is a good default tolernace that works for most meshes.  It's often useful to
// `simplify` meshes before using them with boolean operations or other expensive functions.
warped_sphere
  | simplify(tolerance=0.1)
  | render

// See the docs for the full list of available functions:
// https://3d.ameo.design/geoscript/docs
