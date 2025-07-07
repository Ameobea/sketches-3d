export const getProxiedThumbnailURL = (originalURL: string) => `/geotoy/thumbnail_proxy/${btoa(originalURL)}`;
