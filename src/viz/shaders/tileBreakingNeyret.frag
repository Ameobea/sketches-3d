/**
 * Adapted from https://www.shadertoy.com/view/MdyfDV
 */

#define CON 1      // contrast preserving interpolation. cf https://www.shadertoy.com/view/4dcSDr
#define Z 8.     // patch scale inside example texture

#define rnd22(p)    fract(sin((p) * mat2(127.1,311.7,269.5,183.3) )*43758.5453)
#define srgb2rgb(V) pow( max(V,0.), vec4( 2.2 )  )          // RGB <-> sRGB conversions
#define rgb2srgb(V) pow( max(V,0.), vec4(1./2.2) )

// (textureGrad handles MIPmap through patch borders)
#define C(I)  ( srgb2rgb( textureGrad(samp, U/Z-rnd22(I) ,Gx,Gy)) - m*float(CON) )

vec3 textureNoTileNeyret(sampler2D samp, vec2 uv) {
    mat2 M0 = mat2( 1,0, .5,sqrt(3.)/2. ),
          M = inverse( M0 );                           // transform matrix <-> tilted space
    vec2 z = vec2(0.2),
         U = uv  *Z/8.* exp2(z.y==0.?2.:4.*z.y+1.),
         V = M * U,                                    // pre-hexa tilted coordinates
         I = floor(V);
    float p = .7*dFdy(U.y);                            // pixel size (for antialiasing)
    vec2 Gx = dFdx(U/Z), Gy = dFdy(U/Z);               // (for cross-borders MIPmap)
    vec4 m = srgb2rgb( texture(samp,U,99.) );     // mean texture color

    vec3 F = vec3(fract(V),0), A, W; F.z = 1.-F.x-F.y; // local hexa coordinates
    vec4 fragColor = vec4(0.);
    if ( F.z > 0. )
        fragColor = ( W.x=   F.z ) * C(I)                      // smart interpolation
          + ( W.y=   F.y ) * C(I+vec2(0,1))            // of hexagonal texture patch
          + ( W.z=   F.x ) * C(I+vec2(1,0));           // centered at vertex
    else                                               // ( = random offset in texture )
        fragColor = ( W.x=  -F.z ) * C(I+1.)
          + ( W.y=1.-F.y ) * C(I+vec2(1,0))
          + ( W.z=1.-F.x ) * C(I+vec2(0,1));
#if CON
    fragColor = m + fragColor/length(W);  // contrast preserving interp. cf https://www.shadertoy.com/view/4dcSDr
#endif
    fragColor = clamp( rgb2srgb(fragColor), 0., 1.);
    if (m.g==0.) fragColor = fragColor.rrrr;                           // handles B&W (i.e. "red") textures

    return fragColor.xyz;
}

// void mainImage(out vec4 fragColor, vec2 fragCoord) {
//   vec2 uv = ( 2.*fragCoord - iResolution.xy ) / iResolution.y;
//   vec3 sampled = textureNoTileNeyret( iChannel0, uv );
//   fragColor = vec4( sampled, 1. );
// }
