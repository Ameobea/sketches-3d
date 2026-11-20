// Widens the unit-cylinder beam to a minimum angular radius so its soft edge profile stays
// resolvable at any distance; `vObjectNormal` carries the smooth radial direction.
float beamRadius = length(modelMatrix[0].xyz);
vec3 beamAxisPos = (modelMatrix * vec4(0., position.y, 0., 1.)).xyz;
float minRadius = 0.003 * distance(beamAxisPos, cameraPosition) / projectionMatrix[1][1];
float inflate = max(1., minRadius / beamRadius);
vec4 beamWorldPos = modelMatrix * vec4(position.x * inflate, position.y, position.z * inflate, 1.);
gl_Position = projectionMatrix * viewMatrix * beamWorldPos;
vWorldPos = beamWorldPos.xyz;
vObjectNormal = vec3(position.x, 0., position.z);
