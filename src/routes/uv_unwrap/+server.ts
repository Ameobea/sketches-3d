import { error, type RequestHandler } from '@sveltejs/kit';

// Request is binary and encoded like this:
// u32: vertex count
// u32: index count
// [vtx_count * 3]: vertex data f32 (x, y, z)
// [idx_count]: index data u32 (v0, v1, v2)
const decodeRequestBody = async (
  request: Request
): Promise<{ vertices: Float32Array; indices: Uint32Array }> => {
  const buffer = await request.arrayBuffer();
  if (buffer.byteLength < 8) {
    error(400, 'Invalid request body: must be at least 8 bytes');
  }
  const dataView = new DataView(buffer);
  const vertexCount = dataView.getUint32(0, true);
  const indexCount = dataView.getUint32(4, true);

  // make sure the request body size matches the reported counts
  const expectedSizeBytes = 8 + vertexCount * 12 + indexCount * 4;
  if (buffer.byteLength !== expectedSizeBytes) {
    error(
      400,
      `Invalid request body size: expected ${expectedSizeBytes} bytes, got ${buffer.byteLength} bytes`
    );
  }

  if (vertexCount === 0 || indexCount === 0) {
    error(400, 'Vertex count and index count must be greater than zero');
  }

  if (vertexCount > 1_000_000 || indexCount > 3_000_000) {
    error(400, 'Vertex count or index count exceeds maximum limits');
  }

  const vertices = new Float32Array(buffer, 8, vertexCount * 3);
  const indices = new Uint32Array(buffer, 8 + vertexCount * 12, indexCount);

  return { vertices, indices };
};

// Response format:
// u32: vertex count
// u32: index count
// [vtx_count * 2]: UV data f32 (u, v)
// [vtx_count * 3]: vertex data f32 (x, y, z)
// [idx_count]: index data u32
const encodeResponse = ({
  uvs,
  verts: vertices,
  indices,
}: {
  uvs: Float32Array;
  verts: Float32Array;
  indices: Uint32Array;
}): ArrayBuffer => {
  const vertexCount = vertices.length / 3;
  const indexCount = indices.length;

  const responseBuffer = new ArrayBuffer(8 + uvs.byteLength + vertices.byteLength + indices.byteLength);
  const dataView = new DataView(responseBuffer);

  dataView.setUint32(0, vertexCount, true);
  dataView.setUint32(4, indexCount, true);

  new Float32Array(responseBuffer, 8, uvs.length).set(uvs);
  new Float32Array(responseBuffer, 8 + uvs.byteLength, vertices.length).set(vertices);
  new Uint32Array(responseBuffer, 8 + uvs.byteLength + vertices.byteLength, indexCount).set(indices);

  return responseBuffer;
};

const buildObj = (vertices: Float32Array, indices: Uint32Array): string => {
  const vertexLines = [];
  for (let i = 0; i < vertices.length; i += 3) {
    vertexLines.push(`v ${vertices[i]} ${vertices[i + 1]} ${vertices[i + 2]}`);
  }
  const indexLines = [];
  for (let i = 0; i < indices.length; i += 3) {
    indexLines.push(`f ${indices[i] + 1} ${indices[i + 1] + 1} ${indices[i + 2] + 1}`);
  }
  return [...vertexLines, ...indexLines].join('\n');
};

const extractUVMappingFromObj = (
  objContent: string
): { uvs: Float32Array; verts: Float32Array; indices: Uint32Array } => {
  const positionsTemp: number[] = [];
  const uvsTemp: number[] = [];
  type FaceVertex = { vIdx: number; uvIdx: number };
  const faces: FaceVertex[][] = [];

  const lines = objContent.split('\n');
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const parts = trimmed.split(/\s+/);
    if (parts[0] === 'v') {
      positionsTemp.push(parseFloat(parts[1]), parseFloat(parts[2]), parseFloat(parts[3]));
    } else if (parts[0] === 'vt') {
      uvsTemp.push(parseFloat(parts[1]), parseFloat(parts[2]));
    } else if (parts[0] === 'f') {
      const faceVerts: FaceVertex[] = [];
      for (let i = 1; i < parts.length; i++) {
        const elems = parts[i].split('/');
        const vIdx = parseInt(elems[0], 10);
        const uvIdx = elems[1] ? parseInt(elems[1], 10) : 0;
        faceVerts.push({ vIdx, uvIdx });
      }
      faces.push(faceVerts);
    }
  }

  // .obj files are weird in that they have two sets of indices: one for vertices and one for UVs.
  //
  // This doesn't work for WebGL, so we have to normalize it to only have a single set of indices.
  const vertexMap = new Map<string, number>();
  const finalPositions: number[] = [];
  const finalUvs: number[] = [];
  const finalIndices: number[] = [];

  for (const face of faces) {
    for (const { vIdx, uvIdx } of face) {
      const key = `${vIdx}/${uvIdx}`;
      let outIndex = vertexMap.get(key);
      if (outIndex === undefined) {
        outIndex = finalPositions.length / 3;
        vertexMap.set(key, outIndex);
        const pOff = (vIdx - 1) * 3;
        finalPositions.push(positionsTemp[pOff], positionsTemp[pOff + 1], positionsTemp[pOff + 2]);
        if (uvIdx > 0) {
          const uvOff = (uvIdx - 1) * 2;
          finalUvs.push(uvsTemp[uvOff], uvsTemp[uvOff + 1]);
        } else {
          finalUvs.push(0, 0);
        }
      }
      finalIndices.push(outIndex);
    }
  }

  return {
    uvs: new Float32Array(finalUvs),
    verts: new Float32Array(finalPositions),
    indices: new Uint32Array(finalIndices),
  };
};

const BFF_SERVICE_URL = 'http://localhost:5820';

export const POST: RequestHandler = async ({ request, fetch }) => {
  const { vertices, indices } = await decodeRequestBody(request);
  const objContent = buildObj(vertices, indices);

  // the BFF service accepts .obj files and returns .obj files with a populated UV map
  const formData = new FormData();
  formData.append('file', new Blob([objContent], { type: 'text/plain' }), 'model.obj');
  formData.append('normalize', 'true');
  // TODO: Configurable
  formData.append('cones', '2');
  const bffResponse = await fetch(BFF_SERVICE_URL, {
    method: 'POST',
    body: formData,
  });

  if (!bffResponse.ok) {
    const errorString = await bffResponse.text().catch(() => '<failed to read error message>');
    error(500, `BFF service error: ${bffResponse.status}; ${errorString}`);
  }

  const bffObjContent = await bffResponse.text();
  const extracted = extractUVMappingFromObj(bffObjContent);

  const encodedResponse = encodeResponse(extracted);

  return new Response(encodedResponse as ArrayBuffer, {
    status: 200,
    headers: {
      'Content-Type': 'application/octet-stream',
      'Content-Length': encodedResponse.byteLength.toString(),
      'Cache-Control': 'no-cache, no-store, must-revalidate',
      Pragma: 'no-cache',
      Expires: '0',
    },
  });
};
