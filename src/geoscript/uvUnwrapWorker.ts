import * as Comlink from 'comlink';

import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import type { UVUnwrapWorker } from './uvUnwrapWorker.worker';

const UVUnwrapWorker = new AsyncOnce(() =>
  import('./uvUnwrapWorker.worker?worker').then(mod => Comlink.wrap<UVUnwrapWorker>(new mod.default()))
);

export const getUVUnwrapWorker = (): Promise<Comlink.Remote<UVUnwrapWorker>> => UVUnwrapWorker.get();
