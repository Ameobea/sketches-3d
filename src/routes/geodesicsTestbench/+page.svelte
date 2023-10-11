<script lang="ts">
  import { browser } from '$app/environment';

  const initTestbench = (mod: any) => {
    (window as any).Geodesics = mod;
    console.log(mod);

    const vec_f32 = (vals: number[] | Float32Array) => {
      const vec = new mod.vector$float$();
      for (let i = 0; i < vals.length; i++) {
        vec.push_back(vals[i]);
      }
      return vec;
    };

    const vec_uint32 = (vals: number[] | Uint32Array) => {
      const vec = new mod.vector$uint32_t$();
      for (let i = 0; i < vals.length; i++) {
        vec.push_back(vals[i]);
      }
      return vec;
    };

    const from_vec_f32 = (vec: any) => {
      const arr = [];
      const length = vec.size();
      for (let i = 0; i < length; i++) {
        arr.push(vec.get(i));
      }
      return arr;
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
