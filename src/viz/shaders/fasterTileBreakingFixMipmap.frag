/**
 * Adapted from: https://www.shadertoy.com/view/WdVGWG
 */

const float layersCount = 25.0;

struct InterpNodes2 {
    vec2 seeds;
    vec2 weights;
};

InterpNodes2 GetNoiseInterpNodes(float smoothNoise) {
    vec2 globalPhases = vec2(smoothNoise * 0.5) + vec2(0.5, 0.0);
    vec2 phases = fract(globalPhases);
    vec2 seeds = floor(globalPhases) * 2.0 + vec2(0.0, 1.0);
    vec2 weights = min(phases, vec2(1.0f) - phases) * 2.0;
    return InterpNodes2(seeds, weights);
}

vec3 hash33( vec3 p ) {
	p = vec3( dot(p,vec3(127.1,311.7, 74.7)),
			  dot(p,vec3(269.5,183.3,246.1)),
			  dot(p,vec3(113.5,271.9,124.6)));
	return fract(sin(p)*43758.5453123);
}

vec4 GetTextureSample(sampler2D samp, vec2 pos, float freq, float seed) {
    vec3 hash = hash33(vec3(seed, 0.0, 0.0));
    float ang = hash.x * 2.0 * PI;
    // ang = quantize(ang, 1.);
    // float ang = 0.;
    mat2 rotation = mat2(cos(ang), sin(ang), -sin(ang), cos(ang));
    vec2 uv = rotation * pos * freq + hash.yz;
    return texture(samp, uv);
}

//Qizhi Yu, Fabrice Neyret, Eric Bruneton, and Nicolas Holzschuch. 2011.
//Lagrangian Texture Advection: Preserving Both Spectrum and Velocity Field.
//IEEE Transactions on Visualization and Computer Graphics 17, 11 (2011), 1612–1623
vec4 PreserveVariance(vec4 linearColor, vec4 meanColor, float moment2) {
    return (linearColor - meanColor) / sqrt(moment2) + meanColor;
}

vec4 mainImage(sampler2D samp, sampler2D noiseSampler, in vec2 uv, float texFreq) {
    float smoothNoise = texture(noiseSampler, uv * 0.35 + 10.).r;
    // return vec4(smoothNoise, smoothNoise, smoothNoise, 1.0);
    vec4 fragColor = vec4(0.0);
    InterpNodes2 interpNodes = GetNoiseInterpNodes(smoothNoise * layersCount);
    float moment2 = 0.0;
    for(int i = 0; i < 2; i++) {
        float weight = interpNodes.weights[i];
        moment2 += weight * weight;
        fragColor += GetTextureSample(samp, uv, texFreq, interpNodes.seeds[i]) * weight;
    }
    // uncomment for variance preservation; costs one extra texture lookup
    fragColor = PreserveVariance(fragColor, textureLod(samp, vec2(0.0), 10.0), moment2);
    return fragColor;
}

/**
 * samp - The texture to sample from.
 * noiseSampler - The noise texture to sample from.
 * uv - The uv coordinates to sample from.
 * v - unused, included for compatibility with other shaders.
 * scale - The tiling factor of the texture; higher numbers make the texture smaller and tiled more.
 */
vec4 textureNoTile(sampler2D samp, sampler2D noiseSampler, vec2 uv, float v, float scale ) {
  return mainImage(samp, noiseSampler, uv, scale);
}
