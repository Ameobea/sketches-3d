ambient_light(color=0xffffff, intensity=1.8) | render

dir_light(
  color=0xffffff,
  intensity=5,
  cast_shadow=true,
  shadow_map_size={width: 2048*2, height: 2048*2},
  shadow_map_radius=4,
  shadow_map_blur_samples=16,
  shadow_map_type="vsm",
  shadow_map_bias=-0.0001,
  shadow_camera={near: 8, far: 300, left: -300, right: 380, top: 94, bottom: -140},
  target=vec3(0)
)
  | trans(-20, 50, 0)
  | render;

// set_default_material("default");

// set_rng_seed(1822215726251595)
