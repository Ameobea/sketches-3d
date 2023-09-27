import type { Resizable } from 'postprocessing';
import * as THREE from 'three';

import type { FirstPersonCtx } from '..';

export type TerrainSampler =
  | { type: 'simple'; fn: (x: number, z: number) => number }
  | {
      type: 'batch';
      fn: (
        resolution: [number, number],
        worldSpaceBounds: { mins: [number, number]; maxs: [number, number] }
      ) => Promise<Float32Array>;
    };

interface TerrainParams {
  boundingBox: THREE.Box2;
  minPolygonWidth: number;
  maxPolygonWidth: number;
  sampleHeight: TerrainSampler;
  tileResolution: number;
  maxPixelsPerPolygon: number;
  material: THREE.Material;
  debugLOD?: boolean;
}

interface TileParams {
  bounds: THREE.Box2;
}

type TileState =
  | { type: 'subdivision'; tiles: Tile[] }
  | { type: 'geometry'; geometry: THREE.BufferGeometry };

/**
 * Computes the shortest distance between a point and a `Box2`.
 */
const pointBoxDistance = (
  pointX: number,
  pointY: number,
  pointZ: number,
  boxMinX: number,
  boxMinZ: number,
  boxMaxX: number,
  boxMaxZ: number
) => {
  // Calculate the closest point in 2D space for X and Z
  const closestPointX = Math.max(boxMinX, Math.min(pointX, boxMaxX));
  const closestPointZ = Math.max(boxMinZ, Math.min(pointZ, boxMaxZ));

  // Calculate the 3D distance
  const dx = closestPointX - pointX;
  const dy = pointY; // Since the box is always on the ground, the difference in the y-axis is just the point's y value.
  const dz = closestPointZ - pointZ;

  return Math.sqrt(dx * dx + dy * dy + dz * dz);
};

class Tile extends THREE.Object3D {
  public bounds: THREE.Box2;
  private parentTerrain: LODTerrain;
  private depth: number;
  private tileSize: number;
  private isUpdating = false;

  public state!: TileState;

  constructor(params: TileParams, parentTerrain: LODTerrain, depth: number) {
    super();

    this.parentTerrain = parentTerrain;
    this.bounds = params.bounds;
    this.depth = depth;
    this.tileSize = this.bounds.getSize(new THREE.Vector2()).length();
  }

  /**
   * @returns the width of a pixel in world-space units at the given distance.
   */
  private getApparentSizeAtDistance(distance: number): number {
    const viewportHeight = this.parentTerrain.viewportSize.y;
    // TODO: pre-compute
    const camera = this.parentTerrain.camera;
    const s = (2 * distance * Math.tan((camera.fov / 2) * (Math.PI / 180))) / viewportHeight;
    return s;
  }

  private shouldSubdivide(): boolean {
    const polygonSize = this.tileSize / this.parentTerrain.params.tileResolution;
    if (polygonSize < this.parentTerrain.params.minPolygonWidth) {
      return false;
    }

    const distance = Math.max(
      pointBoxDistance(
        this.parentTerrain.camera.position.x,
        this.parentTerrain.camera.position.y,
        this.parentTerrain.camera.position.z,
        this.bounds.min.x,
        this.bounds.min.y,
        this.bounds.max.x,
        this.bounds.max.y
      ),
      0.0001
    );

    const apparentSize = this.getApparentSizeAtDistance(distance);
    if (apparentSize === 0 || apparentSize === Infinity) {
      throw new Error('apparentSize is 0 or Infinity');
    }

    const screenSpaceSizeOfPolygon = polygonSize / apparentSize;
    return screenSpaceSizeOfPolygon > this.parentTerrain.params.maxPixelsPerPolygon;
  }

  public async update() {
    if (this.isUpdating) {
      return;
    }
    this.isUpdating = true;

    const shouldSubdivide = this.shouldSubdivide();
    if (this.state?.type !== 'geometry' && !shouldSubdivide) {
      await this.generateGeometry();
    } else if (this.state?.type !== 'subdivision' && shouldSubdivide) {
      await this.subdivide();
    }

    if (this.state.type === 'subdivision') {
      const proms = this.state.tiles.map(tile => tile.update());
      await Promise.all(proms);
    }

    this.isUpdating = false;
  }

  private clearChildren() {
    for (let i = this.children.length - 1; i >= 0; i--) {
      this.remove(this.children[i]);
    }
  }

  public async subdivide() {
    const centerX = (this.bounds.min.x + this.bounds.max.x) / 2;
    const centerZ = (this.bounds.min.y + this.bounds.max.y) / 2;

    const tiles = [
      // Top-left
      this.parentTerrain.getTile(
        new THREE.Box2(this.bounds.min, new THREE.Vector2(centerX, centerZ)),
        this.depth + 1
      ),
      // Top-right
      this.parentTerrain.getTile(
        new THREE.Box2(
          new THREE.Vector2(centerX, this.bounds.min.y),
          new THREE.Vector2(this.bounds.max.x, centerZ)
        ),
        this.depth + 1
      ),
      // Bottom-left
      this.parentTerrain.getTile(
        new THREE.Box2(
          new THREE.Vector2(this.bounds.min.x, centerZ),
          new THREE.Vector2(centerX, this.bounds.max.y)
        ),
        this.depth + 1
      ),
      // Bottom-right
      this.parentTerrain.getTile(
        new THREE.Box2(new THREE.Vector2(centerX, centerZ), this.bounds.max),
        this.depth + 1
      ),
    ];

    await Promise.all(tiles.map(tile => tile.update()));

    this.clearChildren();
    this.add(...tiles);

    this.state = { type: 'subdivision', tiles };
  }

  public async generateGeometry() {
    const segments = this.parentTerrain.params.tileResolution;

    // Calculate the step for X and Z based on the tile's size and desired segments.
    const stepX = (this.bounds.max.x - this.bounds.min.x) / segments;
    const stepZ = (this.bounds.max.y - this.bounds.min.y) / segments;

    const vertices = new Float32Array((segments + 1) * (segments + 1) * 3);
    const indexCount = segments * segments * 6;
    const u16Max = 65_535;
    const indices = indexCount > u16Max ? new Uint32Array(indexCount) : new Uint16Array(indexCount);

    // Generate vertices
    if (this.parentTerrain.params.sampleHeight.type === 'simple') {
      const sampleHeight = this.parentTerrain.params.sampleHeight.fn;
      for (let i = 0; i <= segments; i += 1) {
        for (let j = 0; j <= segments; j += 1) {
          const x = this.bounds.min.x + i * stepX;
          const z = this.bounds.min.y + j * stepZ;
          const y = sampleHeight(x, z);

          vertices[i * (segments + 1) * 3 + j * 3] = x;
          vertices[i * (segments + 1) * 3 + j * 3 + 1] = y;
          vertices[i * (segments + 1) * 3 + j * 3 + 2] = z;
        }
      }
    } else if (this.parentTerrain.params.sampleHeight.type === 'batch') {
      const heightmap = await this.parentTerrain.params.sampleHeight.fn([segments + 1, segments + 1], {
        mins: [this.bounds.min.x, this.bounds.min.y],
        maxs: [this.bounds.max.x, this.bounds.max.y],
      });

      for (let zIx = 0; zIx <= segments; zIx += 1) {
        for (let xIx = 0; xIx <= segments; xIx += 1) {
          const x = this.bounds.min.x + xIx * stepX;
          const z = this.bounds.min.y + zIx * stepZ;
          const y = heightmap[zIx * (segments + 1) + xIx];

          vertices[zIx * (segments + 1) * 3 + xIx * 3] = x;
          vertices[zIx * (segments + 1) * 3 + xIx * 3 + 1] = y;
          vertices[zIx * (segments + 1) * 3 + xIx * 3 + 2] = z;
        }
      }
    } else {
      throw new Error('Invalid sampleHeight type');
    }

    // Generate indices
    for (let i = 0; i < segments; i++) {
      for (let j = 0; j < segments; j++) {
        const topLeft = i * (segments + 1) + j;
        const topRight = topLeft + 1;
        const bottomLeft = (i + 1) * (segments + 1) + j;
        const bottomRight = bottomLeft + 1;

        // Two triangles for the quad
        indices[i * segments * 6 + j * 6] = topLeft;
        indices[i * segments * 6 + j * 6 + 1] = bottomLeft;
        indices[i * segments * 6 + j * 6 + 2] = topRight;
        indices[i * segments * 6 + j * 6 + 3] = topRight;
        indices[i * segments * 6 + j * 6 + 4] = bottomLeft;
        indices[i * segments * 6 + j * 6 + 5] = bottomRight;
      }
    }

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.Float32BufferAttribute(vertices, 3));
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    geometry.computeVertexNormals();

    // We create a simple mesh with the generated geometry and some basic material.
    const material = this.parentTerrain.params.debugLOD
      ? new THREE.MeshBasicMaterial({
          color: (() => {
            // color based on depth to debug
            const colors = [0xff0000, 0x00ff00, 0x0000ff, 0xffff00, 0xff00ff];
            return colors[this.depth % colors.length];
          })(),
          wireframe: true,
        })
      : this.parentTerrain.params.material;
    const mesh = new THREE.Mesh(geometry, material);
    mesh.castShadow = true;
    mesh.receiveShadow = true;

    this.clearChildren();
    this.add(mesh);

    this.state = { type: 'geometry', geometry };
  }
}

export class LODTerrain extends THREE.Group implements Resizable {
  public camera: THREE.PerspectiveCamera;
  public viewportSize: THREE.Vector2;
  public params: TerrainParams;
  private tileCache: Map<string, Tile> = new Map();
  private rootTile: Tile;

  constructor(camera: THREE.PerspectiveCamera, params: TerrainParams, viewportSize: THREE.Vector2) {
    super();

    this.camera = camera;
    this.params = params;
    this.viewportSize = viewportSize;

    this.rootTile = new Tile({ bounds: this.params.boundingBox }, this, 0);
    this.add(this.rootTile);
  }

  public async initializeCollision(fpCtx: FirstPersonCtx) {
    // TODO: make this configurable
    const heightmapResolution = 1024 * 1;
    const heightmapData = new Float32Array(heightmapResolution * heightmapResolution);

    let heightmap: Float32Array;
    if (this.params.sampleHeight.type === 'simple') {
      heightmap = new Float32Array(heightmapResolution * heightmapResolution);
      for (let yIx = 0; yIx < heightmapResolution; yIx++) {
        for (let xIx = 0; xIx < heightmapResolution; xIx++) {
          const x = THREE.MathUtils.lerp(
            this.params.boundingBox.min.x,
            this.params.boundingBox.max.x,
            xIx / (heightmapResolution - 1)
          );
          const y = THREE.MathUtils.lerp(
            this.params.boundingBox.min.y,
            this.params.boundingBox.max.y,
            yIx / (heightmapResolution - 1)
          );
          const z = this.params.sampleHeight.fn(x, y);

          heightmap[yIx * heightmapResolution + xIx] = z;
        }
      }
    } else if (this.params.sampleHeight.type === 'batch') {
      heightmap = await this.params.sampleHeight.fn([heightmapResolution, heightmapResolution], {
        mins: [this.params.boundingBox.min.x, this.params.boundingBox.min.y],
        maxs: [this.params.boundingBox.max.x, this.params.boundingBox.max.y],
      });
    } else {
      throw new Error('Invalid sampleHeight type');
    }

    let minHeight = Infinity;
    let maxHeight = -Infinity;
    for (let yIx = 0; yIx < heightmapResolution; yIx++) {
      for (let xIx = 0; xIx < heightmapResolution; xIx++) {
        const x = THREE.MathUtils.lerp(
          this.params.boundingBox.min.x,
          this.params.boundingBox.max.x,
          xIx / (heightmapResolution - 1)
        );
        const y = THREE.MathUtils.lerp(
          this.params.boundingBox.min.y,
          this.params.boundingBox.max.y,
          yIx / (heightmapResolution - 1)
        );
        const z = heightmap[yIx * heightmapResolution + xIx];

        minHeight = Math.min(minHeight, z);
        maxHeight = Math.max(maxHeight, z);

        heightmapData[yIx * heightmapResolution + xIx] = z;
      }
    }

    const bboxSize = this.params.boundingBox.getSize(new THREE.Vector2());
    const worldSpaceWidth = bboxSize.x;
    const worldSpaceLength = bboxSize.y;
    fpCtx.addHeightmapTerrain(
      heightmapData,
      minHeight,
      maxHeight,
      heightmapResolution,
      heightmapResolution,
      worldSpaceWidth,
      worldSpaceLength
    );
  }

  setSize(width: number, height: number) {
    this.viewportSize.set(width, height);
  }

  /**
   * Update the tiles based on camera position or other factors that determine the LOD.
   */
  public async update() {
    await this.rootTile.update();
  }

  public getTile(bounds: THREE.Box2, depth: number): Tile {
    // TODO: cache integration

    const tile = new Tile({ bounds }, this, depth);
    return tile;
  }
}
