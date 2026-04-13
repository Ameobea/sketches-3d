import type { ObjectDef, ObjectGroupDef } from 'src/viz/levelDef/types';
import { isObjectGroup } from 'src/viz/levelDef/levelDefTreeUtils';

type AnyNode = ObjectDef | ObjectGroupDef;

/**
 * Insert a node into the tree at a specific placement.
 *
 * If `parentId` is undefined, the node is inserted into the root `objects`
 * array at `index`. If `parentId` is given, the matching group's `children`
 * array is the insertion target.
 *
 * The index is clamped to the length of the target array so out-of-range
 * values (e.g. from a concurrent edit) always produce a valid append.
 *
 * Returns true on success, false if the parent group was not found.
 */
export function insertNodeAtPlacement(
  objects: AnyNode[],
  node: AnyNode,
  parentId: string | undefined,
  index: number
): boolean {
  if (!parentId) {
    objects.splice(Math.min(index, objects.length), 0, node);
    return true;
  }
  for (const obj of objects) {
    if (isObjectGroup(obj)) {
      if (obj.id === parentId) {
        obj.children.splice(Math.min(index, obj.children.length), 0, node);
        return true;
      }
      if (insertNodeAtPlacement(obj.children, node, parentId, index)) return true;
    }
  }
  return false;
}
