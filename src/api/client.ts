import { derived } from 'svelte/store';
import { BasicGameBackendApi } from './apis';
import { Configuration } from './runtime';
import * as metrics from './metrics.js';
import { rwritable } from 'src/viz/util/TransparentWritable.js';

const config = new Configuration({ basePath: '/api', credentials: 'include' });
export const API = new BasicGameBackendApi(config);

export const LoggedInUserID = rwritable<string | null>(null);
export const IsLoggedIn = derived(LoggedInUserID, $LoggedInUserID => $LoggedInUserID !== null);

export const checkLogin = () =>
  API.getPlayer()
    .then(player => {
      LoggedInUserID.set(player.id ?? null);
      return player.id;
    })
    .catch(() => LoggedInUserID.set(null));

export const MetricsAPI = metrics;
