import { derived } from 'svelte/store';
import { BasicGameBackendApi } from './apis';
import { Configuration } from './runtime';
import * as metrics from './metrics.js';
import { rwritable } from 'src/viz/util/TransparentWritable.js';
import type { StrippedPlayer } from './models/StrippedPlayer.js';

const config = new Configuration({ basePath: '/api', credentials: 'include' });
export const API = new BasicGameBackendApi(config);

const LoggedInUser = rwritable<StrippedPlayer | null>(null);
export const IsLoggedIn = derived(LoggedInUser, $LoggedInUserID => $LoggedInUserID !== null);

let checkLoginPromise: Promise<StrippedPlayer | null> | null = null;
const checkLogin = () =>
  API.getPlayer()
    .then(player => {
      LoggedInUser.set(player);
      return player;
    })
    .catch(() => {
      LoggedInUser.set(null);
      return null;
    });

export const setUserLoggedOut = () => {
  LoggedInUser.set(null);
  checkLoginPromise = Promise.resolve(null);
};

export const getUser = async () => {
  if (LoggedInUser.current) {
    return LoggedInUser.current;
  }

  if (!checkLoginPromise) {
    checkLoginPromise = checkLogin();
  }

  return checkLoginPromise;
};

export const refetchUser = () => {
  LoggedInUser.set(null);
  checkLoginPromise = null;
  return getUser();
};

export const getUserID = () => getUser().then(player => player?.id ?? null);

export const MetricsAPI = metrics;
