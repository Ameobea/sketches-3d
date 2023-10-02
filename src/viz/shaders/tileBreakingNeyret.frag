/**
 * Adapted from https://www.shadertoy.com/view/MdyfDV
 */

#define CON 1      // contrast preserving interpolation. cf https://www.shadertoy.com/view/4dcSDr
#define Z 8.     // patch scale inside example texture
#define LOOKUP_SKIP_THRESHOLD 0.01 // The mix magnitude under which texture lookups will be skipped if `SKIP_LOW_MAGNITUDE_LOOKUPS` is enabled
#define LOOKUP_SKIP_POW 8. // The exponent to raise the mix magnitude to before comparing to `LOOKUP_SKIP_THRESHOLD`
#define LOW_MAG_SKIP_FADE 0.02

#define rnd22(p)    fract(sin((p) * mat2(127.1,311.7,269.5,183.3) )*43758.5453)
#define srgb2rgb(V) pow( max(V,0.), vec4( 2.2 )  )          // RGB <-> sRGB conversions
#define rgb2srgb(V) pow( max(V,0.), vec4(1./2.2) )

// (textureGrad handles MIPmap through patch borders)
#define C(I)  ( srgb2rgb( textureGrad(samp, U/Z-rnd22(I) ,Gx,Gy)) - m*float(CON) )

vec4 textureNoTileNeyret(sampler2D samp, vec2 uv) {
    mat2 M0 = mat2( 1,0, .5,sqrt(3.)/2. ),
          M = inverse( M0 );                           // transform matrix <-> tilted space
    vec2 z = vec2(0.2),
         U = uv  *Z/8.* exp2(z.y==0.?2.:4.*z.y+1.),
         V = M * U,                                    // pre-hexa tilted coordinates
         I = floor(V);
    float p = .7*dFdy(U.y);                            // pixel size (for antialiasing)
    vec2 Gx = dFdx(U/Z), Gy = dFdy(U/Z);               // (for cross-borders MIPmap)
    #if CON
    vec4 m = srgb2rgb( texture(samp,U,99.) );     // mean texture color
    #else
    vec4 m = vec4(0.);
    #endif

    vec3 F = vec3(fract(V),0), A, W; F.z = 1.-F.x-F.y; // local hexa coordinates
    vec4 fragColor = vec4(0.);

    if (F.z > 0.) {
        W = vec3(F.z, F.y, F.x);
        W = pow(W, vec3(LOOKUP_SKIP_POW));
        W = W / dot(W, vec3(1.));

        if (W.x > LOOKUP_SKIP_THRESHOLD) {
            fragColor += C(I) * W.x;
        }
        if (W.y > LOOKUP_SKIP_THRESHOLD) {
            fragColor += C(I + vec2(0, 1)) * W.y;
        }
        if (W.z > LOOKUP_SKIP_THRESHOLD) {
            fragColor += C(I + vec2(1, 0)) * W.z;
        }
    } else {            
        W = vec3(-F.z, 1. - F.y, 1. - F.x);
        W = pow(W, vec3(LOOKUP_SKIP_POW));
        W = W / dot(W, vec3(1.));

        if (W.x > 0.01) {
            fragColor += C(I + 1.) * W.x;
        }
        if (W.y > 0.01) {
            fragColor += C(I + vec2(1, 0)) * W.y;
        }
        if (W.z > 0.01) {
            fragColor += C(I + vec2(0, 1)) * W.z;
        }
    }
#if CON
    fragColor = m + fragColor/length(W);  // contrast preserving interp. cf https://www.shadertoy.com/view/4dcSDr
#endif
    fragColor = clamp( rgb2srgb(fragColor), 0., 1.);
    if (m.g==0.) fragColor = fragColor.rrrr;                           // handles B&W (i.e. "red") textures

    return fragColor;
}
