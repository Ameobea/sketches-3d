float quantize(float val, float interval) {
  return round(val / interval) * interval;
}

vec2 quantize(vec2 val, float interval) {
  val.x = quantize(val.x, interval);
  val.y = quantize(val.y, interval);
  return val;
}

vec3 quantize(vec3 val, float interval) {
  val.x = quantize(val.x, interval);
  val.y = quantize(val.y, interval);
  val.z = quantize(val.z, interval);
  return val;
}

float fmod(float x, float y) {
  return x - y * floor(x / y);
}
