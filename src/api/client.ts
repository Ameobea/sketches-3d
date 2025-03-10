import { writable } from 'svelte/store';
import { BasicGameBackendApi } from './apis';
import { Configuration } from './runtime';

const config = new Configuration({ basePath: '/api', credentials: 'include' });
export const API = new BasicGameBackendApi(config);

export const IsLoggedIn = writable<boolean | null>(null);
