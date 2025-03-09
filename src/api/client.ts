import { BasicGameBackendApi } from './apis';
import { Configuration } from './runtime';

const config = new Configuration({ basePath: 'https://3d.ameo.design/api', credentials: 'include' });
export const API = new BasicGameBackendApi(config);
