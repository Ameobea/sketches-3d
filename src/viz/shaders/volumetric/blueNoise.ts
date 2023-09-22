import * as THREE from 'three';

export const getBlueNoiseTexture = async (loader: THREE.TextureLoader): Promise<THREE.Texture> => {
  const url = 'https://i.ameo.link/bhb.png';
  const blueNoiseTexture = await loader.loadAsync(url);

  blueNoiseTexture.wrapS = THREE.RepeatWrapping;
  blueNoiseTexture.wrapT = THREE.RepeatWrapping;
  blueNoiseTexture.magFilter = THREE.NearestFilter;
  blueNoiseTexture.minFilter = THREE.NearestFilter;
  blueNoiseTexture.generateMipmaps = false;
  return blueNoiseTexture;
};
