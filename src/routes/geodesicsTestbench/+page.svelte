<script lang="ts">
  import { browser } from '$app/environment';

  const initTestbench = (mod: any) => {
    (window as any).Geodesics = mod;
    console.log(mod);

    const HEAPF32 = mod.HEAPF32 as Float32Array;
    const HEAPU32 = mod.HEAPU32 as Uint32Array;

    const vec_generic = (
      vecCtor: new () => any,
      mem: Float32Array | Uint32Array,
      vals: number[] | Float32Array | Uint32Array
    ) => {
      const vec = new vecCtor();
      vec.resize(vals.length, 0);
      const ptr = vec.data();
      const buf = mem.subarray(ptr / 4, ptr / 4 + vals.length);
      buf.set(vals);
      return vec;
    };

    const vec_f32 = (vals: number[] | Float32Array) => vec_generic(mod.vector$float$, HEAPF32, vals);

    const vec_uint32 = (vals: number[] | Uint32Array) => vec_generic(mod.vector$uint32_t$, HEAPU32, vals);

    const from_vec_f32 = (vec: any): Float32Array => {
      const length = vec.size();
      const ptr = vec.data();
      return HEAPF32.subarray(ptr / 4, ptr / 4 + length);
    };

    // build a simple pyramid mesh as a manifold surface.
    //
    // All triangles must be wound counter-clockwise.
    const vertices = vec_f32([0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1]);
    const indices = vec_uint32([0, 1, 2, 1, 0, 3, 0, 2, 3, 1, 3, 2]);

    const computed = mod.computeGeodesics(
      indices,
      vertices,
      vec_f32([0, 0, 0.4, 0.4, -0.9, -0.9, 240, -39032]),
      0,
      0
    );
    console.log(computed, from_vec_f32(computed.projectedNormals), from_vec_f32(computed.projectedPositions));
  };

  if (browser) {
    import('../../geodesics/geodesics.js')
      .then(mod => mod.Geodesics)
      .then(mod => mod())
      .then(initTestbench);
  }
</script>

<style>
  :global(body, html) {
    background: black;
  }
</style>
