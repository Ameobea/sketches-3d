// float displacement = sin(curTimeSeconds * 4.) * sin(curTimeSeconds * 1.) * 0.5 + 0.5;
float displacement = 0.;
displacement += sin(position.y * 20. + curTimeSeconds * 3.) * 4.;

float highFreqDisplacement = sin(position.y * 124. + curTimeSeconds * 32.) * 0.8;
float highFreqDisplacementActivation = pow(sin(curTimeSeconds * 2.) * 0.5 + 0.5, 2.);
displacement = displacement + highFreqDisplacement * highFreqDisplacementActivation;

displacement = displacement * 0.3;
vec3 newPosition = position + normal * displacement;
gl_Position = projectionMatrix * modelViewMatrix * vec4( newPosition, 1.0 );
