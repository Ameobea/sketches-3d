import { AsyncOnce } from './util/AsyncOnce';

export const LoadOrbitControls = new AsyncOnce(() => import('three/examples/jsm/controls/OrbitControls.js'));
