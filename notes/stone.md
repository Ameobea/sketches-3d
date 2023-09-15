## ground fog experiment

- https://www.shadertoy.com/view/ssV3zh
  - godrays
- https://www.youtube.com/watch?v=RdN06E6Xn9E
- faster depth to worldspace compute: https://i.ameo.link/bgw.png
  - I think this is only applicable if computing for meshes rather than as full-screen quad in postprocessing
- https://www.shadertoy.com/view/XlBSRz
  - amazing shadertoy with extremely high-quality light-responsive volumetric ground fog. it casts and receives shadows, it's amazing.
  - phase function? https://www.pbr-book.org/3ed-2018/Volume_Scattering/Phase_Functions
    - idk what this really does; need to understand this better
- https://shaderbits.com/blog/ue4-volumetric-fog-techniques
  - some pretty detailed info on volumetric fog rendering method.
  - unreal engine 4-focused, but still a ton of good resources
- "ray marched heightmaps" https://shaderbits.com/blog/ray-marched-heightmaps
  - a shortcut/fastpath thing to self-shadowing for volumetric rendering
  - Against UE4-focused, but looks promising
- another fancy raymarched clouds shadertoy: https://www.shadertoy.com/view/XslGRr
  - _extremely_ performant; best by far. Uses several tricks to achieve this:
    - Cheap approximated lighting/shadowing
      - "Lighting is done with only one extra sample per raymarch": https://iquilezles.org/articles/derivative/
      - alternative to raymarched self-shadowing where the light/shadow is estimated based on the gradient of the fog wrt. the light source
    - aggressive bounding box clipping of rays
    - blue noise dithering of ray start positions to reduce artifacts/banding

So I think I have a pretty good idea of how to implement the basic raymarching. It's pretty basic after all.

The main challenge here is the self-shadowing. But honestly that's getting ahead of ourselves I think.

The priorities should be:

- get basic raymarched ground fog implemented.
  - Does not follow terrain; constant height with noise-based offset, animated over time
  - No self-shadowing of any kind
- basic performance optimizations
  - clipping rays to bounding boxes
  - faster noise sampling?
- raw self-shadowing modelled off the shadertoy
- fancy optimizations/experiments from there

---

Simpler non-volumetric fog options: https://advances.realtimerendering.com/s2006/Wenzel-Real-time_Atmospheric_Effects_in_Games.pdf

glide down from the spawn point to the ground
LOD displacement on the monoliths
