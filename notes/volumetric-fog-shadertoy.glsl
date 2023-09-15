// Originally from: https://www.shadertoy.com/view/XlBSRz

/* Hi there!
 * Here is a demo presenting volumetric rendering single with shadowing.
 * Did it quickly so I hope I have not made any big mistakes :)
 *
 * I also added the improved scattering integration I propose in my SIGGRAPH'15 presentation
 * about Frostbite new volumetric system I have developed. See slide 28 at http://www.frostbite.com/2015/08/physically-based-unified-volumetric-rendering-in-frostbite/
 * Basically it improves the scattering integration for each step with respect to extinction
 * The difference is mainly visible for some participating media having a very strong scattering value.
 * I have setup some pre-defined settings for you to checkout below (to present the case it improves):
 * - D_DEMO_SHOW_IMPROVEMENT_xxx: shows improvement (on the right side of the screen). You can still see aliasing due to volumetric shadow and the low amount of sample we take for it.
 * - D_DEMO_SHOW_IMPROVEMENT_xxx_NOVOLUMETRICSHADOW: same as above but without volumetric shadow
 *
 * To increase the volumetric rendering accuracy, I constrain the ray marching steps to a maximum distance.
 *
 * Volumetric shadows are evaluated by raymarching toward the light to evaluate transmittance for each view ray steps (ouch!)
 *
 * Do not hesitate to contact me to discuss about all that :)
 * SebH
 */

/*
 * This are predefined settings you can quickly use
 *    - D_DEMO_FREE play with parameters as you would like
 *    - D_DEMO_SHOW_IMPROVEMENT_FLAT show improved integration on flat surface
 *    - D_DEMO_SHOW_IMPROVEMENT_NOISE show improved integration on noisy surface
 *    - the two previous without volumetric shadows
 */
#define D_DEMO_FREE
//#define D_DEMO_SHOW_IMPROVEMENT_FLAT
//#define D_DEMO_SHOW_IMPROVEMENT_NOISE
//#define D_DEMO_SHOW_IMPROVEMENT_FLAT_NOVOLUMETRICSHADOW
//#define D_DEMO_SHOW_IMPROVEMENT_NOISE_NOVOLUMETRICSHADOW

#ifdef D_DEMO_FREE
	// Apply noise on top of the height fog?
    #define D_FOG_NOISE 1.0

	// Height fog multiplier to show off improvement with new integration formula
    #define D_STRONG_FOG 0.0

    // Enable/disable volumetric shadow (single scattering shadow)
    #define D_VOLUME_SHADOW_ENABLE 1

	// Use imporved scattering?
	// In this mode it is full screen and can be toggle on/off.
	#define D_USE_IMPROVE_INTEGRATION 1

//
// Pre defined setup to show benefit of the new integration. Use D_DEMO_FREE to play with parameters
//
#elif defined(D_DEMO_SHOW_IMPROVEMENT_FLAT)
    #define D_STRONG_FOG 10.0
    #define D_FOG_NOISE 0.0
	#define D_VOLUME_SHADOW_ENABLE 1
#elif defined(D_DEMO_SHOW_IMPROVEMENT_NOISE)
    #define D_STRONG_FOG 5.0
    #define D_FOG_NOISE 1.0
	#define D_VOLUME_SHADOW_ENABLE 1
#elif defined(D_DEMO_SHOW_IMPROVEMENT_FLAT_NOVOLUMETRICSHADOW)
    #define D_STRONG_FOG 10.0
    #define D_FOG_NOISE 0.0
	#define D_VOLUME_SHADOW_ENABLE 0
#elif defined(D_DEMO_SHOW_IMPROVEMENT_NOISE_NOVOLUMETRICSHADOW)
    #define D_STRONG_FOG 3.0
    #define D_FOG_NOISE 1.0
	#define D_VOLUME_SHADOW_ENABLE 0
#endif

/*
 * Other options you can tweak
 */

// Used to control wether transmittance is updated before or after scattering (when not using improved integration)
// If 0 strongly scattering participating media will not be energy conservative
// If 1 participating media will look too dark especially for strong extinction (as compared to what it should be)
// Toggle only visible zhen not using the improved scattering integration.
#define D_UPDATE_TRANS_FIRST 0

// Apply bump mapping on walls
#define D_DETAILED_WALLS 0

// Use to restrict ray marching length. Needed for volumetric evaluation.
#define D_MAX_STEP_LENGTH_ENABLE 1

// Light position and color
#define LPOS vec3( 20.0+15.0*sin(0.2*iTime), 15.0+12.0*cos(0.2*iTime),-20.0)
#define LCOL (600.0*vec3( 1.0, 0.9, 0.5))

float psrdnoise(vec2 x, vec2 period, float alpha, out vec2 gradient) {
	// Transform to simplex space (axis-aligned hexagonal grid)
	vec2 uv = vec2(x.x + x.y*0.5, x.y);

	// Determine which simplex we're in, with i0 being the "base"
	vec2 i0 = floor(uv);
	vec2 f0 = fract(uv);
	// o1 is the offset in simplex space to the second corner
	float cmp = step(f0.y, f0.x);
	vec2 o1 = vec2(cmp, 1.0-cmp);

	// Enumerate the remaining simplex corners
	vec2 i1 = i0 + o1;
	vec2 i2 = i0 + vec2(1.0, 1.0);

	// Transform corners back to texture space
	vec2 v0 = vec2(i0.x - i0.y * 0.5, i0.y);
	vec2 v1 = vec2(v0.x + o1.x - o1.y * 0.5, v0.y + o1.y);
	vec2 v2 = vec2(v0.x + 0.5, v0.y + 1.0);

	// Compute vectors from v to each of the simplex corners
	vec2 x0 = x - v0;
	vec2 x1 = x - v1;
	vec2 x2 = x - v2;

	vec3 iu, iv;
	vec3 xw, yw;

	// Wrap to periods, if desired
	if(any(greaterThan(period, vec2(0.0)))) {
		xw = vec3(v0.x, v1.x, v2.x);
		yw = vec3(v0.y, v1.y, v2.y);
		if(period.x > 0.0)
			xw = mod(vec3(v0.x, v1.x, v2.x), period.x);
		if(period.y > 0.0)
			yw = mod(vec3(v0.y, v1.y, v2.y), period.y);
		// Transform back to simplex space and fix rounding errors
		iu = floor(xw + 0.5*yw + 0.5);
		iv = floor(yw + 0.5);
	} else { // Shortcut if neither x nor y periods are specified
		iu = vec3(i0.x, i1.x, i2.x);
		iv = vec3(i0.y, i1.y, i2.y);
	}

	// Compute one pseudo-random hash value for each corner
	vec3 hash = mod(iu, 289.0);
	hash = mod((hash*51.0 + 2.0)*hash + iv, 289.0);
	hash = mod((hash*34.0 + 10.0)*hash, 289.0);

	// Pick a pseudo-random angle and add the desired rotation
	vec3 psi = hash * 0.07482 + alpha;
	vec3 gx = cos(psi);
	vec3 gy = sin(psi);

	// Reorganize for dot products below
	vec2 g0 = vec2(gx.x,gy.x);
	vec2 g1 = vec2(gx.y,gy.y);
	vec2 g2 = vec2(gx.z,gy.z);

	// Radial decay with distance from each simplex corner
	vec3 w = 0.8 - vec3(dot(x0, x0), dot(x1, x1), dot(x2, x2));
	w = max(w, 0.0);
	vec3 w2 = w * w;
	vec3 w4 = w2 * w2;

	// The value of the linear ramp from each of the corners
	vec3 gdotx = vec3(dot(g0, x0), dot(g1, x1), dot(g2, x2));

	// Multiply by the radial decay and sum up the noise value
	float n = dot(w4, gdotx);

	// Compute the first order partial derivatives
	vec3 w3 = w2 * w;
	vec3 dw = -8.0 * w3 * gdotx;
	vec2 dn0 = w4.x * g0 + dw.x * x0;
	vec2 dn1 = w4.y * g1 + dw.y * x1;
	vec2 dn2 = w4.z * g2 + dw.z * x2;
	gradient = 10.9 * (dn0 + dn1 + dn2);

	// Scale the return value to fit nicely into the range [-1,1]
	return 10.9 * n;
}

float displacementSimple(vec2 p) {
    vec2 gradient;
    return psrdnoise(p * 40., vec2(0.0), 0.0, gradient);

    float f;
    f = 1.5000 * textureLod(iChannel0, p, 0.0).x;

    return f;
}

vec3 getSceneColor(vec3 p, float material) {
    if(material == 1.0) {
        return vec3(1.0, 0.5, 0.5);
    } else if(material == 2.0) {
        return vec3(0.5, 1.0, 0.5);
    } else if(material == 3.0) {
        return vec3(0.5, 0.5, 1.0);
    }

    return vec3(0.0, 0.0, 0.0);
}

float getClosestDistance(vec3 p, out float material) {
    float d = 0.0;
#if D_MAX_STEP_LENGTH_ENABLE
    float minD = 1.5; // restrict max step for better scattering evaluation
#else
    float minD = 10000000.0;
#endif
    material = 0.0;

    d = max(0.0, p.y);
    if(d < minD) {
        minD = d;
        material = 2.0;
    }

    d = max(0.0, p.x);
    if(d < minD) {
        minD = d;
        material = 1.0;
    }

    d = max(0.0, 40.0 - p.x);
    if(d < minD) {
        minD = d;
        material = 1.0;
    }

    d = max(0.0, -p.z);
    if(d < minD) {
        minD = d;
        material = 3.0;
    }

    return minD;
}

vec3 calcNormal(in vec3 pos) {
    float material = 0.0;
    vec3 eps = vec3(0.3, 0.0, 0.0);
    return normalize(vec3(
        getClosestDistance(pos + eps.xyy, material) - getClosestDistance(pos - eps.xyy, material),
        getClosestDistance(pos + eps.yxy, material) - getClosestDistance(pos - eps.yxy, material),
        getClosestDistance(pos + eps.yyx, material) - getClosestDistance(pos - eps.yyx, material)
    ));
}

vec3 evaluateLight(in vec3 pos) {
    vec3 lightPos = LPOS;
    vec3 lightCol = LCOL;
    vec3 L = lightPos - pos;
    return lightCol * 1.0 / dot(L, L);
}

vec3 evaluateLight(in vec3 pos, in vec3 normal) {
    vec3 lightPos = LPOS;
    vec3 L = lightPos - pos;
    float distanceToL = length(L);
    vec3 Lnorm = L / distanceToL;
    return max(0.0, dot(normal, Lnorm)) * evaluateLight(pos);
}

// To simplify: wavelength independent scattering and extinction
void getParticipatingMedia(out float sigmaS, out float sigmaE, in vec3 pos) {
    float heightFog = 0.;
    if (pos.y < 12. && pos.y > 6.)
        heightFog = 7.0 + D_FOG_NOISE * 3.0 * clamp(displacementSimple(pos.xz * 0.005 + iTime * 0.01), 0.0, 1.0);
    if (pos.y <= 6.) heightFog = 10.;
        heightFog = 0.3 * clamp(heightFog - pos.y, 0.0, 1.0);

    const float fogFactor = 1.0 + D_STRONG_FOG * 5.0;

    const float sphereRadius = 5.0;
    float sphereFog = clamp((sphereRadius - length(pos - vec3(20.0, 19.0, -17.0))) / sphereRadius, 0.0, 1.0);

    const float constantFog = 0.00;

    sigmaS = constantFog + heightFog * fogFactor + sphereFog;

    sigmaE = max(0.000000001, sigmaS); // to avoid division by zero extinction
}

float phaseFunction() {
    return 1.0 / (4.0 * 3.14);
}

float volumetricShadow(in vec3 from, in vec3 to) {
#if D_VOLUME_SHADOW_ENABLE
    // defaulted to 16
    const float numStep = 16.0; // quality control. Bump to avoid shadow alisaing
    float shadow = 1.0;
    float sigmaS = 0.0;
    float sigmaE = 0.0;
    float dd = length(to - from) / numStep;
    for (float s = 0.5; s < (numStep - 0.1); s += 1.0) { // start at 0.5 to sample at center of integral part
        vec3 pos = from + (to - from) * (s / (numStep));
        getParticipatingMedia(sigmaS, sigmaE, pos);
        shadow *= exp(-sigmaE * dd);
    }
    return shadow;
#else
    return 1.0;
#endif
}

void traceScene(bool improvedScattering, vec3 rO, vec3 rD, inout vec3 finalPos, inout vec3 normal, inout vec3 albedo, inout vec4 scatTrans) {
    const int numIter = 50;

    float sigmaS = 0.0;
    float sigmaE = 0.0;

    vec3 lightPos = LPOS;

    // Initialise volumetric scattering integration (to view)
    float transmittance = 1.0;
    vec3 scatteredLight = vec3(0.0, 0.0, 0.0);

    float d = 1.0; // hack: always have a first step of 1 unit to go further
    float material = 0.0;
    vec3 p = vec3(0.0, 0.0, 0.0);
    float dd = 0.0;
    for (int i = 0; i < numIter; ++i) {
        vec3 p = rO + d * rD;

        getParticipatingMedia(sigmaS, sigmaE, p);

#ifdef D_DEMO_FREE
        if(D_USE_IMPROVE_INTEGRATION > 0) { // freedom/tweakable version
#else
            if(improvedScattering) {
#endif
                // See slide 28 at http://www.frostbite.com/2015/08/physically-based-unified-volumetric-rendering-in-frostbite/
                vec3 S = evaluateLight(p) * sigmaS * phaseFunction() * volumetricShadow(p, lightPos);// incoming light
                vec3 Sint = (S - S * exp(-sigmaE * dd)) / sigmaE; // integrate along the current step segment
                scatteredLight += transmittance * Sint; // accumulate and also take into account the transmittance from previous steps

                // Evaluate transmittance to view independentely
                transmittance *= exp(-sigmaE * dd);
            } else {
            // Basic scatering/transmittance integration
        #if D_UPDATE_TRANS_FIRST
                transmittance *= exp(-sigmaE * dd);
        #endif
                scatteredLight += sigmaS * evaluateLight(p) * phaseFunction() * volumetricShadow(p, lightPos) * transmittance * dd;
        #if !D_UPDATE_TRANS_FIRST
                transmittance *= exp(-sigmaE * dd);
        #endif
            }

        dd = getClosestDistance(p, material);
        if(dd < 0.2)
            break; // give back a lot of performance without too much visual loss
        d += dd;
    }

    albedo = getSceneColor(p, material);

    finalPos = rO + d * rD;

    normal = calcNormal(finalPos);

    scatTrans = vec4(scatteredLight, transmittance);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    float hfactor = float(iResolution.y) / float(iResolution.x); // make it screen ratio independent
    vec2 uv2 = vec2(2.0, 2.0 * hfactor) * fragCoord.xy / iResolution.xy - vec2(1.0, hfactor);

    vec3 camPos = vec3(20.0, 18.0, -50.0);
    if(iMouse.x + iMouse.y > 0.0) // to handle first loading and see somthing on screen
        camPos += vec3(0.05, 0.12, 0.0) * (vec3(iMouse.x, iMouse.y, 0.0) - vec3(iResolution.xy * 0.5, 0.0));
    vec3 camX = vec3(1.0, 0.0, 0.0);
    vec3 camY = vec3(0.0, 1.0, 0.0);
    vec3 camZ = vec3(0.0, 0.0, 1.0);

    vec3 rO = camPos;
    vec3 rD = normalize(uv2.x * camX + uv2.y * camY + camZ);
    vec3 finalPos = rO;
    vec3 albedo = vec3(0.0, 0.0, 0.0);
    vec3 normal = vec3(0.0, 0.0, 0.0);
    vec4 scatTrans = vec4(0.0, 0.0, 0.0, 0.0);
    traceScene(fragCoord.x > (iResolution.x / 2.0), rO, rD, finalPos, normal, albedo, scatTrans);

    //lighting
    vec3 color = (albedo / 3.14) * evaluateLight(finalPos, normal) * volumetricShadow(finalPos, LPOS);
    // Apply scattering/transmittance
    color = color * scatTrans.w + scatTrans.xyz;

    // Gamma correction
    color = pow(color, vec3(1.0 / 2.2)); // simple linear to gamma, exposure of 1.0

#ifndef D_DEMO_FREE
    // Separation line
    if(abs(fragCoord.x - (iResolution.x * 0.5)) < 0.6)
        color.r = 0.5;
#endif

    fragColor = vec4(color, 1.0);
}
