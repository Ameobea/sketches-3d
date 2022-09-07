let ammojs: Promise<any> | null = null;

export const getAmmoJS = async () => {
  if (ammojs) return ammojs;
  ammojs = import('../ammojs/ammo.wasm.js').then(mod => mod.Ammo.apply({}));
  return ammojs;
};
