float displacement = 0.;
displacement += (sin(curTimeSeconds * 3.) * 0.5 + 0.5) * 4.;

float highFreqDisplacement = (sin(curTimeSeconds * 32.) * 0.5 + 0.5) * 0.8;
float highFreqDisplacementActivation = pow(sin(curTimeSeconds * 2.) * 0.5 + 0.5, 2.);
displacement = displacement + highFreqDisplacement * highFreqDisplacementActivation;

displacement = displacement * 0.8;
// Only start pulsing once the player gets closer
float distanceActivation = 1. - smoothstep(350., 400., distance(pos, cameraPosition));
// Don't pulse at the top, or at least not for now when we're only pulsing in the Z direction
float heightActivation = 1. - smoothstep(300., 330., pos.y);
vec3 newPosition = vec3(position.x-0.2, position.y+0.2, position.z + position.z * heightActivation * distanceActivation * 0.12);
newPosition = newPosition + vec3(0., 0., sign(position.z)) * displacement * heightActivation * 0.46;
gl_Position = projectionMatrix * modelViewMatrix * vec4( newPosition, 1.0 );
