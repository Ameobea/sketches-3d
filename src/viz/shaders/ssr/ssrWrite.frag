// TODO: This isn't taking normal map into account, but using `normal` here produces
// incorrect results.
vec3 worldSpaceNormal = normalize((modelMatrix * vec4(vNormalAbsolute, 0.)).xyz);
writeSSRData(worldSpaceNormal);
// writeSSRData(normal);
