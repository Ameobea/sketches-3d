export const DEP_BIT_GEODESICS = 1 << 0;
export const DEP_BIT_CGAL = 1 << 1;
export const DEP_BIT_CLIPPER2 = 1 << 2;
export const DEP_BIT_TEXT2PATH = 1 << 3;

export const bitmaskToAsyncDepNames = (bitmask: number): string[] => {
  const deps: string[] = [];
  if (bitmask & DEP_BIT_GEODESICS) {
    deps.push('geodesics');
  }
  if (bitmask & DEP_BIT_CGAL) {
    deps.push('cgal');
  }
  if (bitmask & DEP_BIT_CLIPPER2) {
    deps.push('clipper2');
  }
  if (bitmask & DEP_BIT_TEXT2PATH) {
    deps.push('text_to_path');
  }
  return deps;
};
