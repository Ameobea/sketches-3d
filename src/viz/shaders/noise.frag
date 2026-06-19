/**
 * Taken from: https://www.shadertoy.com/view/4dS3Wd
 * By Morgan McGuire @morgan3d, http://graphicscodex.com
 * Reuse permitted under the BSD license.
 */

#define NOISE fbm
#define NUM_NOISE_OCTAVES 4

float hash(float p) {
	p = fract(p * 0.011);
	p *= p + 7.5;
	p *= p + p;
	return fract(p);
}
float hash(vec2 p) {
	vec3 p3 = fract(vec3(p.xyx) * 0.13);
	p3 += dot(p3, p3.yzx + 3.333);
	return fract((p3.x + p3.y) * p3.z);
}

float noise(float x) {
	float i = floor(x);
	float f = fract(x);
	float u = f * f * (3. - 2. * f);
	return mix(hash(i), hash(i + 1.), u);
}

float noise(vec2 x) {
	vec2 i = floor(x);
	vec2 f = fract(x);

	// Four corners in 2D of a tile
	float a = hash(i);
	float b = hash(i + vec2(1., 0.));
	float c = hash(i + vec2(0., 1.));
	float d = hash(i + vec2(1., 1.));

	// Simple 2D lerp using smoothstep envelope between the values.
	// return vec3(mix(mix(a, b, smoothstep(0., 1., f.x)),
	//			mix(c, d, smoothstep(0., 1., f.x)),
	//			smoothstep(0., 1., f.y)));

	// Same code, with the clamps in smoothstep and common subexpressions
	// optimized away.
	vec2 u = f * f * (3. - 2. * f);
	return mix(a, b, u.x) + (c - a) * u.y * (1. - u.x) + (d - b) * u.x * u.y;
}

float noise(vec3 x) {
	const vec3 step = vec3(110, 241, 171);

	vec3 i = floor(x);
	vec3 f = fract(x);

	// For performance, compute the base input to a 1D hash from the integer part of the argument and the
	// incremental change to the 1D based on the 3D -> 1D wrapping
	float n = dot(i, step);

	vec3 u = f * f * (3. - 2. * f);
	return mix(mix(mix(hash(n + dot(step, vec3(0, 0, 0))), hash(n + dot(step, vec3(1, 0, 0))), u.x), mix(hash(n + dot(step, vec3(0, 1, 0))), hash(n + dot(step, vec3(1, 1, 0))), u.x), u.y), mix(mix(hash(n + dot(step, vec3(0, 0, 1))), hash(n + dot(step, vec3(1, 0, 1))), u.x), mix(hash(n + dot(step, vec3(0, 1, 1))), hash(n + dot(step, vec3(1, 1, 1))), u.x), u.y), u.z);
}

// Value noise with analytic gradient: vec4(value, d/dx, d/dy, d/dz). Same 8-corner
// hash as noise(vec3); the derivative is the exact gradient of the smoothstep-
// interpolated field (iq's formulation), so procedural relief normals are
// closed-form instead of finite-differenced.
vec4 noised(vec3 x) {
	const vec3 step = vec3(110, 241, 171);
	vec3 i = floor(x);
	vec3 f = fract(x);
	float n = dot(i, step);
	vec3 u = f * f * (3. - 2. * f);
	vec3 du = 6. * f * (1. - f);

	float a = hash(n + dot(step, vec3(0, 0, 0)));
	float b = hash(n + dot(step, vec3(1, 0, 0)));
	float c = hash(n + dot(step, vec3(0, 1, 0)));
	float d = hash(n + dot(step, vec3(1, 1, 0)));
	float e = hash(n + dot(step, vec3(0, 0, 1)));
	float f1 = hash(n + dot(step, vec3(1, 0, 1)));
	float g = hash(n + dot(step, vec3(0, 1, 1)));
	float h = hash(n + dot(step, vec3(1, 1, 1)));

	float k0 = a, k1 = b - a, k2 = c - a, k3 = e - a;
	float k4 = a - b - c + d, k5 = a - c - e + g, k6 = a - b - e + f1;
	float k7 = -a + b + c - d + e - f1 - g + h;

	float val = k0 + k1 * u.x + k2 * u.y + k3 * u.z
	          + k4 * u.x * u.y + k5 * u.y * u.z + k6 * u.z * u.x
	          + k7 * u.x * u.y * u.z;
	vec3 der = du * vec3(
		k1 + k4 * u.y + k6 * u.z + k7 * u.y * u.z,
		k2 + k4 * u.x + k5 * u.z + k7 * u.z * u.x,
		k3 + k5 * u.y + k6 * u.x + k7 * u.x * u.y);
	return vec4(val, der);
}

float fbm(float x) {
	float v = 0.;
	float a = 0.5;
	float shift = 100.;
	for (int i = 0; i < NUM_NOISE_OCTAVES; ++i) {
		v += a * noise(x);
		x = x * 2. + shift;
		a *= 0.5;
	}
	return v;
}

float fbm(vec2 x) {
	float v = 0.;
	float a = 0.5;
	vec2 shift = vec2(100);
	// Rotate to reduce axial bias
	mat2 rot = mat2(cos(0.5), sin(0.5), -sin(0.5), cos(0.5));
	for (int i = 0; i < NUM_NOISE_OCTAVES; ++i) {
		v += a * noise(x);
		x = rot * x * 2. + shift;
		a *= 0.5;
	}
	return v;
}

float fbm(vec3 x) {
	float v = 0.;
	float a = 0.5;
	vec3 shift = vec3(100);
	for (int i = 0; i < NUM_NOISE_OCTAVES; ++i) {
		v += a * noise(x);
		x = x * 2. + shift;
		a *= 0.5;
	}
	return v;
}

// fbm with analytic gradient, matching fbm(vec3)'s octave schedule so a height
// using fbm(vec3) and a normal using fbmd(vec3) share the same field.
vec4 fbmd(vec3 x) {
	float v = 0.;
	vec3 d = vec3(0.);
	float a = 0.5;
	float fscale = 1.;
	vec3 shift = vec3(100);
	for (int i = 0; i < NUM_NOISE_OCTAVES; ++i) {
		vec4 nd = noised(x);
		v += a * nd.x;
		d += a * fscale * nd.yzw;
		x = x * 2. + shift;
		fscale *= 2.;
		a *= 0.5;
	}
	return vec4(v, d);
}

float fbm_1_octave(vec3 x) {
	float v = 0.;
	float a = 0.5;
	vec3 shift = vec3(100);
	for (int i = 0; i < 1; ++i) {
		v += a * noise(x);
		x = x * 2. + shift;
		a *= 0.5;
	}
	return v;
}

float fbm_2_octaves(vec2 x) {
	float v = 0.;
	float a = 0.5;
	vec2 shift = vec2(100);
	for (int i = 0; i < 2; ++i) {
		v += a * noise(x);
		x = x * 2. + shift;
		a *= 0.5;
	}
	return v;
}

float fbm_2_octaves(vec3 x) {
	float v = 0.;
	float a = 0.5;
	vec3 shift = vec3(100);
	for (int i = 0; i < 2; ++i) {
		v += a * noise(x);
		x = x * 2. + shift;
		a *= 0.5;
	}
	return v;
}

float scale_shift(float value, float n) {
	return abs(value) * n - 1.;
}

float ridged_multifractal_noise(vec3 point, float frequency, float lacunarity, float persistence, float attenuation) {
	float result = 0.;
	float weight = 1.;

	point *= frequency;

	int octaves = 2;
	for (int octave = 0; octave < octaves; ++octave) {
		float signal = fbm(point + vec3(float(octave)) * 8.);
		signal = abs(signal);
		signal = 1. - signal;
		signal *= signal;
		signal *= weight;
		weight = signal / attenuation;
		weight = clamp(weight, 0., 1.);
		signal *= pow(persistence, float(octave));
		result += signal;
		point *= lacunarity;
	}

	float scale = 2. - pow(0.5, float(octaves));
	return scale_shift(result, 2. / scale);
}
