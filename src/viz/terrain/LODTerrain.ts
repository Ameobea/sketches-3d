import * as THREE from 'three';

interface TerrainParams {
  boundingBox: THREE.Box2;
  minPolygonWidth: number;
  maxPolygonWidth: number;
  sampleHeight: (point: THREE.Vector2) => number;
  tileResolution: number;
}

interface TileParams {
  bounds: THREE.Box2;
}

type TileState =
  | { type: 'subdivision'; tiles: Tile[] }
  | { type: 'geometry'; geometry: THREE.BufferGeometry };

class Tile extends THREE.Object3D {
  public bounds: THREE.Box2;
  private parentTerrain: LODTerrain;
  private depth: number;

  public state!: TileState;

  constructor(params: TileParams, parentTerrain: LODTerrain, depth: number) {
    super();

    this.parentTerrain = parentTerrain;
    this.bounds = params.bounds;
    this.depth = depth;

    this.update();
  }

  private shouldSubdivide(): boolean {
    const tileCenter = new THREE.Vector2();
    this.bounds.getCenter(tileCenter);

    const distance = this.parentTerrain.camera.position.distanceTo(
      new THREE.Vector3(tileCenter.x, 0, tileCenter.y)
    );

    const tileSize = this.bounds.getSize(new THREE.Vector2()).length();
    const polygonSize = tileSize / this.parentTerrain.params.tileResolution;

    return distance < 5000 && polygonSize > this.parentTerrain.params.minPolygonWidth;
  }

  public update() {
    const shouldSubdivide = this.shouldSubdivide();
    if (this.state?.type !== 'geometry' && !shouldSubdivide) {
      console.log('generate geometry');
      this.generateGeometry();
    } else if (this.state?.type !== 'subdivision' && shouldSubdivide) {
      console.log('subdivide');
      this.subdivide();
    }

    if (this.state.type === 'subdivision') {
      // TODO: only update tiles that might need to change
      for (const tile of this.state.tiles) {
        tile.update();
      }
    }
  }

  private clearChildren() {
    // for (const child of this.children) {
    //   this.remove(child);
    // }
    for (let i = this.children.length - 1; i >= 0; i--) {
      this.remove(this.children[i]);
    }
  }

  public subdivide() {
    this.clearChildren();

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

    this.add(...tiles);

    this.state = { type: 'subdivision', tiles };
  }

  public generateGeometry() {
    this.clearChildren();

    const segments = this.parentTerrain.params.tileResolution;

    // Calculate the step for X and Z based on the tile's size and desired segments.
    const stepX = (this.bounds.max.x - this.bounds.min.x) / segments;
    const stepZ = (this.bounds.max.y - this.bounds.min.y) / segments;

    const vertices: Float32Array = new Float32Array((segments + 1) * (segments + 1) * 3);
    const indices: Float32Array = new Float32Array(segments * segments * 6);

    // Generate vertices
    for (let i = 0; i <= segments; i++) {
      for (let j = 0; j <= segments; j++) {
        const x = this.bounds.min.x + i * stepX;
        const z = this.bounds.min.y + j * stepZ;
        const y = this.parentTerrain.params.sampleHeight(new THREE.Vector2(x, z));

        vertices[i * (segments + 1) * 3 + j * 3] = x;
        vertices[i * (segments + 1) * 3 + j * 3 + 1] = y;
        vertices[i * (segments + 1) * 3 + j * 3 + 2] = z;
      }
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

    // We create a simple mesh with the generated geometry and some basic material.
    const material = new THREE.MeshBasicMaterial({
      color: (() => {
        // color based on depth to debug
        const colors = [0xff0000, 0x00ff00, 0x0000ff, 0xffff00, 0xff00ff];
        return colors[this.depth % colors.length];
      })(),
      wireframe: true,
    }); // For visualization.
    const mesh = new THREE.Mesh(geometry, material);

    this.add(mesh);

    this.state = { type: 'geometry', geometry };
  }
}

export class LODTerrain extends THREE.Group {
  public camera: THREE.Camera;
  public params: TerrainParams;
  // TODO: swap this out for a LRU cache
  // TODO: only cache leaf tiles
  private tileCache: Map<string, Tile> = new Map();
  private rootTile: Tile;

  constructor(camera: THREE.Camera, params: TerrainParams) {
    super();

    this.camera = camera;
    this.params = params;

    this.rootTile = new Tile({ bounds: this.params.boundingBox }, this, 0);
    this.add(this.rootTile);
  }

  /**
   * Update the tiles based on camera position or other factors that determine the LOD.
   */
  public update() {
    this.rootTile.update();
  }

  public getTile(bounds: THREE.Box2, depth: number): Tile {
    // TODO: cache integration

    return new Tile({ bounds }, this, depth);
  }
}
