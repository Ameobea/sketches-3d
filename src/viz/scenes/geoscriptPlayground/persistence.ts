import type { CompositionVersionMetadata } from 'src/geoscript/geotoyAPIClient';
import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
import { DefaultCameraFOV, DefaultCameraPos, DefaultCameraTarget, DefaultCameraZoom } from './types';
import { buildDefaultMaterialDefinitions, type MaterialDefinitions } from 'src/geoscript/materials';
import type { Viz } from 'src/viz';

const DefaultCode = 'box(8) | (box(8) + vec3(4, 4, -4)) | render';

export interface PlaygroundState {
  code: string;
  materials: MaterialDefinitions;
  view: CompositionVersionMetadata['view'];
  lastRunWasSuccessful: boolean;
}

const getLocalStorageKeySuffix = (userData: GeoscriptPlaygroundUserData | undefined): string => {
  const initComposition = userData?.initialComposition;
  if (!initComposition) {
    return '';
  }
  return `-${initComposition.comp.id}-${initComposition.version.id}`;
};

export const loadState = (userData: GeoscriptPlaygroundUserData | undefined): PlaygroundState => {
  const localStorageKeySuffix = getLocalStorageKeySuffix(userData);

  const savedCode = localStorage.getItem(`lastGeoscriptPlaygroundCode${localStorageKeySuffix}`);
  const savedMaterialsRaw = localStorage.getItem(`geoscriptPlaygroundMaterials${localStorageKeySuffix}`);
  const savedView = localStorage.getItem(`geoscriptPlaygroundView${localStorageKeySuffix}`);
  const lastRunWasSuccessful =
    localStorage.getItem(`lastGeoscriptRunCompleted${localStorageKeySuffix}`) !== 'false';

  const serverCode = userData?.initialComposition?.version.source_code;
  const serverMaterials = userData?.initialComposition?.version.metadata?.materials;
  const serverView = userData?.initialComposition?.version.metadata?.view;

  const code = savedCode || serverCode || DefaultCode;
  let materials: MaterialDefinitions;
  let view: CompositionVersionMetadata['view'] = {
    cameraPosition: [DefaultCameraPos.x, DefaultCameraPos.y, DefaultCameraPos.z],
    target: [DefaultCameraTarget.x, DefaultCameraTarget.y, DefaultCameraTarget.z],
    fov: DefaultCameraFOV,
    zoom: DefaultCameraZoom,
  };
  if (savedMaterialsRaw) {
    try {
      materials = JSON.parse(savedMaterialsRaw);
    } catch (err) {
      console.warn('Error parsing saved material definitions:', err);
      materials = serverMaterials ?? buildDefaultMaterialDefinitions();
    }
  } else {
    materials = serverMaterials ?? buildDefaultMaterialDefinitions();
  }
  if (savedView) {
    try {
      view = JSON.parse(savedView);
    } catch (err) {
      console.warn('Error parsing saved view metadata:', err);
      view = serverView ?? view;
    }
  } else if (serverView) {
    view = serverView;
  }

  return {
    code,
    materials,
    view,
    lastRunWasSuccessful,
  };
};

export const getView = (viz: Viz): CompositionVersionMetadata['view'] => ({
  cameraPosition: viz.camera.position.toArray(),
  target: viz.orbitControls?.target.toArray() || DefaultCameraTarget.toArray(),
  fov: 'fov' in viz.camera ? viz.camera.fov : undefined,
  zoom: 'zoom' in viz.camera ? viz.camera.zoom : undefined,
});

export const saveState = (
  state: Omit<PlaygroundState, 'lastRunWasSuccessful'>,
  userData: GeoscriptPlaygroundUserData | undefined
) => {
  const localStorageKeySuffix = getLocalStorageKeySuffix(userData);
  localStorage.setItem(`lastGeoscriptPlaygroundCode${localStorageKeySuffix}`, state.code);
  localStorage.setItem(
    `geoscriptPlaygroundMaterials${localStorageKeySuffix}`,
    JSON.stringify(state.materials)
  );
  localStorage.setItem(`geoscriptPlaygroundView${localStorageKeySuffix}`, JSON.stringify(state.view));
};

/**
 * Not an efficient function; shouldn't be called frequently.
 */
export const getIsDirty = (userData: GeoscriptPlaygroundUserData | undefined): boolean => {
  const localStorageKeySuffix = getLocalStorageKeySuffix(userData);
  const savedCode = localStorage.getItem(`lastGeoscriptPlaygroundCode${localStorageKeySuffix}`);
  const savedMaterialsRaw = localStorage.getItem(`geoscriptPlaygroundMaterials${localStorageKeySuffix}`);

  const serverCode = userData?.initialComposition?.version.source_code || DefaultCode;
  const serverMaterials =
    userData?.initialComposition?.version.metadata?.materials || buildDefaultMaterialDefinitions();

  return (
    (savedCode !== null ? savedCode !== serverCode : false) ||
    (savedMaterialsRaw ? savedMaterialsRaw !== JSON.stringify(serverMaterials) : false)
  );
};

export const getServerState = (userData: GeoscriptPlaygroundUserData | undefined): PlaygroundState => {
  const serverCode = userData?.initialComposition?.version.source_code || DefaultCode;
  const serverMaterials =
    userData?.initialComposition?.version.metadata?.materials || buildDefaultMaterialDefinitions();
  const serverView = userData?.initialComposition?.version.metadata?.view || {
    cameraPosition: [DefaultCameraPos.x, DefaultCameraPos.y, DefaultCameraPos.z],
    target: [DefaultCameraTarget.x, DefaultCameraTarget.y, DefaultCameraTarget.z],
    fov: DefaultCameraFOV,
    zoom: DefaultCameraZoom,
  };

  return {
    code: serverCode,
    materials: serverMaterials,
    view: serverView,
    lastRunWasSuccessful: true,
  };
};

export const clearSavedState = (userData: GeoscriptPlaygroundUserData | undefined) => {
  const localStorageKeySuffix = getLocalStorageKeySuffix(userData);
  localStorage.removeItem(`lastGeoscriptPlaygroundCode${localStorageKeySuffix}`);
  localStorage.removeItem(`geoscriptPlaygroundMaterials${localStorageKeySuffix}`);
};

export const setLastRunWasSuccessful = (
  wasSuccessful: boolean,
  userData: GeoscriptPlaygroundUserData | undefined
) => {
  const localStorageKeySuffix = getLocalStorageKeySuffix(userData);
  localStorage[`lastGeoscriptRunCompleted${localStorageKeySuffix}`] = wasSuccessful ? 'true' : 'false';
};
