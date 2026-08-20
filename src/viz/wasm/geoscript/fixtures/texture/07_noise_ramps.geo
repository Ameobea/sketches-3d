n = 64
texture(n, n, |uv| fbm(octaves=4, frequency=3.7, pos=uv, tileable=true)) | render_texture(name="fbm_tileable")
texture(n, n, |uv| fbm(octaves=3, frequency=3., pos=uv, tileable=0.2)) | render_texture(name="fbm_partial_tileable")
texture(n, n, |uv| fbm(seed=5, octaves=4, frequency=2., pos=uv)) | render_texture(name="fbm_seeded")
texture(n, n, |uv| fbm(pos=v3(uv.x, uv.y, 0.3))) | render_texture(name="fbm_3d")

// domain warp: fbm nested in an fbm kwarg (the heaviest real generator shape)
texture(n, n, |uv| fbm(octaves=4, frequency=3.7, pos=uv + v2(fbm(octaves=6, pos=uv * 0.2, tileable=0.2) * 3.5, 0.), tileable=true)) | render_texture(name="fbm_warp")

bands = [[-3,-3,-3,-3],[-3.5,-3.5,-3.5,-3.5],[-4.1,-4.1,-4.1,-4.1],[-4.7,-4.7,-4.7,-4.7],[-5.2,-5.2,-5.2,-5.2],[-5.8,-5.8,-5.8,-5.8],[-6.3,-6.3,-6.3,-6.3],[-6.9,-6.9,-6.9,-6.9]]
sn = spectral_noise(bands=bands, width=64, height=64, seed=3)
spectral_noise(bands=bands, width=64, height=64, seed=3, distribution="uniform") | render_texture(name="spectral_uniform")

ramp = color_ramp(stops=[v3(0., 0., 0.1), v3(0.9, 0.5, 0.2), v3(1., 1., 0.9)], domain=[-2., 2.])
ramp(sn) | render_texture(name="ramped_spectral")
