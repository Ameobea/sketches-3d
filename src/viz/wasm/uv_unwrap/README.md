This was generated from the `boundary-first-flattening` repository.

It implements a custom wrapper function that takes a mesh specified by vertices and indices and returns unwrapped UVs along with new vertices and indices.

I've committed the Wasm because of the crazy difficult process of generating it.  It required manually modifying and building two of the dependencies and figuring out how to get them compiled to Wasm.

I've included a summary of the process below for reference.  I can't guarantee that it will work exactly, but it's as close as I can give to what I had to change to get it compiled.

----

## BUILDING OPENBLAS FOR WASM

I applied the patches from here: https://github.com/msqr1/kaldi-wasm2

Not 100% sure if necessary.

Checked out commit `5ef8b19`

Source emsdk, then build with this:

```
make CC=emcc FC=emcc HOSTCC=gcc \
    TARGET=RISCV64_GENERIC \
        ONLY_CBLAS=1 NOFORTRAN=1gs NO_LAPACK=0 NO_LAPACKE=0 \
        C_LAPACK=1 BUILD_WITHOUT_LAPACK=1 USE_THREAD=0 \
        BUILD_BFLOAT16=0 BUILD_COMPLEX16=0 BUILD_COMPLEX=0 \
         CFLAGS="-O3 -ffast-math -msimd128 -mavx"
```

Install with:

```
mkdir /tmp/blas
PREFIX=/tmp/blas NO_SHARED=1 make install
```

## BUILDING SUITESPARSE

Have to update the CMakeLists.txt and add these lines:

```
set(BLAS_LIBRARIES "/tmp/blas/lib/libopenblas.a")
set(LAPACK_LIBRARIES ${BLAS_LIBRARIES})
```

Then build with:

```
mkdir build && cd build

emcmake cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=emcc -DCMAKE_CXX_COMPILER=em++ -DEMSCRIPTEN=True -DBUILD_SHARED_LIBS=OFF -DBUILD_STATIC_LIBS=ON -DCHOLMOD_USE_CUDA=OFF -DSUITESPARSE_USE_CUDA=OFF "-DSUITESPARSE_ENABLE_PROJECTS=suitesparse_config;amd;colamd;cholmod" -DSUITESPARSE_USE_FORTRAN=OFF -DBLA_STATIC=ON -DBLAS_VENDOR=OpenBLAS -DBLA_VENDOR=OpenBLAS -DSUITESPARSE_USE_OPENMP=OFF -DCMAKE_FIND_DEBUG_MODE=OFF -DBLA_F95=OFF ..

emmake make -j12
```

There will be warnings about conflicting function signatures, but the build should succeed.

This will generate several .a files that need to be copied to the emscripten sysroot so that they can be located while building BFF.

I discovered that emscripten will look at this location in its search path, which is nice and isolated:

```
~/emsdk/upstream/emscripten/cache/sysroot/usr/local/lib/
```

I copied the following files into there:

 - `/tmp/blas/lib/libopenblas.a` to `libopenblas.a` and `libblas.a` and `liblapack.a`
 - `SuiteSparse/build/AMD/libamd.a` to `libAMD.a`
 - `SuiteSparse/build/CAMD/libcamd.a` to `libcamd.a`
 - `SuiteSparse/build/CCOLAMD/libccolamd.a` to `libccolamd.a`
 - `SuiteSparse/build/CHOLMOD/libcholmod.a` to `libcholmod.a`
 - `SuiteSparse/build/SuiteSparse_config/libsuitesparseconfig.a` to `libsuitesparseconfig.a`

## BUILDING BOUNDARY-FIRST-FLATTENING

I had to patch the CMake files for this.

I edited `cmake/FindSuiteSparse.cmake` and updated the list of suitesparse libraries to this:

```
## Default behavior if user doesn't use the COMPONENTS flag in find_package(SuiteSparse ...) command
if(NOT SuiteSparse_FIND_COMPONENTS)
        list(APPEND SuiteSparse_FIND_COMPONENTS AMD CAMD CCOLAMD COLAMD CHOLMOD suitesparseconfig)  ## suitesparse and metis are not searched by default (special case)
endif()
```

This removes some of the libraries which aren't needed (and aren't actually built with the given config) and adds in the `suitesparseconfig` library explicitly which seemed to be missing.

I also had to update several files to add imports for `#include <cstdint>` (this was required even to build the library without emscripten)

I edited the root `CMakeLists.txt` file to add these three lines before the `# suitesparse` section:

```
include_directories("/home/casey/SuiteSparse/CHOLMOD/Include")
include_directories("/home/casey/SuiteSparse/SuiteSparse_config")
include_directories("/tmp/blas/include")
```

This fixes file not found errors when running `make`.

Then, building can begin:

```
mkdir build && cd build

emcmake cmake -DBFF_BUILD_GUI=OFF -DCMAKE_BUILD_TYPE=Release ..

# This builds a `libbff.a` file which contains unresolved references to a lot of crap in the .a files in the Emscripten sysroot.
#
# Those would have to be manually provided when building whatever library will eventually contain all of that.
#
# Or, this one will trigger the actual CLI app to be built:

emcmake cmake -DBFF_BUILD_GUI=OFF -DBFF_BUILD_CLI=ON -DCMAKE_BUILD_TYPE=Release ..

# Then run the build
emmake make -j12
```

## THE SAGA CONTINUES

There was code in the bff library that had hard-coded sizes for 64-bit architecture (`size_t`).  These were used as pointers to write into buffers, and the size of what was written ended up wrong.  I updated them to `int64_t` and it fixed that part.

Then, I got a new crash when cones were present in the geometry.  It turns out that those symbol signature conflict errors weren't innocuous after all.

Calls to some functions (`dpotrf_`, `spotrf_`, etc.) got replaced with an unreachable by emscripten, which caused the program to crash when actually trying to compute stuff for the cones

These were getting defined in both openblas as well as suitesparse.  They were defined as returning `void` in suitesparse, but `int` in openblas.

After much effort, I tracked down the place in suitesparse where they were getting defined (very difficult because of all the crazy preprocessor templating and indirection that's going on over there).  I swapped them to `int`, re-compiled, and it fixed the problem.
