/*
 * Code from N8 Programs: https://n8python.github.io/goodGodRays/EffectCompositer.js
 *
 * With cleanup and minor changes
 */

#include <common>

uniform sampler2D godrays;
uniform sampler2D sceneDiffuse;
uniform sampler2D sceneDepth;
uniform float edgeStrength;
uniform float edgeRadius;
uniform vec2 resolution;
uniform vec3 color;
varying vec2 vUv;

#define DITHERING
#include <dithering_pars_fragment>

float linearize_depth(float d,float zNear,float zFar)
{
    return zNear * zFar / (zFar + d * (zNear - zFar));
}

void main() {
    vec4 diffuse = texture2D(sceneDiffuse, vUv);
    float rawDepth = texture2D(sceneDepth, vUv).x;
    // gl_FragColor = vec4(rawDepth< 1. ? 0. : 1., rawDepth,rawDepth, 1.0);
    // return;
    float correctDepth = linearize_depth(rawDepth, 0.1, 1000.0);
    float minDist = 100000.0;
    vec2 pushDir = vec2(0.0);
    float count = 0.0;

    for (float x = -edgeRadius; x <= edgeRadius; x++) {
        for (float y = -edgeRadius; y <= edgeRadius; y++) {
            vec2 sampleUv = (vUv * resolution + vec2(x, y)) / resolution;
            float sampleDepth = linearize_depth(texture2D(sceneDepth, sampleUv).x, 0.1, 1000.0);
            if (abs(sampleDepth - correctDepth) < 0.05 * correctDepth) {
                pushDir += vec2(x, y);
                count += 1.0;
            }
        }
    }

    if (count == 0.0) {
        count = 1.0;
    }

    pushDir /= count;
    pushDir = normalize(pushDir);
    vec2 sampleUv = length(pushDir) > 0.0 ? vUv + edgeStrength * (pushDir / resolution) : vUv;
    float bestChoice = texture2D(godrays, sampleUv).x;

    gl_FragColor = vec4(mix(diffuse.rgb, color, bestChoice), 1.0);
    #include <dithering_fragment>
}
