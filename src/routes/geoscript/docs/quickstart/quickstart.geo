// The base syntax is quite similar to Rust with some pieces from other languages like
// Python, TypeScript, and Elixir.

////////////
// BASICS //
////////////

// variables are defined like Python
a = 1
b = a + 3

// variables are immutable, but you can create new ones with the same name like in Rust
c = 0

// statements can optionally end with a semicolon
d = c - 1;

// the built-in `print` function can be used to print any value to the console
print(a, b, 1+2)

// types can optionally be specified when defining variables.  They are checked at
// runtime and sometimes enable more efficient runtime performance.
//
// `int`s are represented internally as a 64-bit signed integer.
my_int: int = -3

// the `num` type indicates that the value can either be an `int` or `float`
//
// numbers are automatically cast as needed for basic arithmetic operations
x: num = 1 + 3.0

// functions are defined by creating closures using a similar syntax to Rust:
add_one = |x| x + 1

// arg types, return types, and curly brackets are all optional
add_two = |x: int| { x + 2 }
add_three = |x: int|: int x + 3;

// closures automatically capture clones of variables they reference from outer scopes.
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

// functions are auto-curried
add_three = add(3)
nine = add_three(6)

///////////////
// SEQUENCES //
///////////////

// All sequences are lazy - if they're eager internally.  They work similarly to iterators
// in Rust; they do nothing until they're consumed.

// There are range operators like in Rust
exclusive_range = 0..10
inclusive_range = 0..=5

// They can be used with built-in iterator combinators:
two_three_four = 0..10 | skip(2) | take(3)

nine = two_three_four | reduce(add)

////////////
// MESHES //
////////////

// TODO
