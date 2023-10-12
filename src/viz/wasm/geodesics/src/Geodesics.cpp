#include <emscripten/bind.h>

using namespace emscripten;

#include "geometrycentral/surface/surface_mesh_factories.h"
#include "geometrycentral/surface/surface_point.h"
#include "geometrycentral/surface/trace_geodesic.h"

using namespace geometrycentral;

#include <Eigen/Core>
#include <iostream>

std::tuple<
  std::unique_ptr<geometrycentral::surface::ManifoldSurfaceMesh>,
  std::unique_ptr<geometrycentral::surface::VertexPositionGeometry>>
loadMesh(std::vector<uint32_t> indices, std::vector<float> positions) {
  if (indices.size() % 3 != 0) {
    throw std::invalid_argument("indicesLength must be a multiple of 3");
  }
  if (positions.size() % 3 != 0) {
    throw std::invalid_argument("positionsLength must be a multiple of 3");
  }

  uint32_t numVertices = positions.size() / 3;
  uint32_t numFaces = indices.size() / 3;

  Eigen::Matrix<uint32_t, Eigen::Dynamic, 3> fMat(numFaces, 3);
  Eigen::Matrix<float, Eigen::Dynamic, 3> vMat(numVertices, 3);

  for (uint32_t i = 0; i < numFaces; i++) {
    fMat(i, 0) = indices[i * 3 + 0];
    fMat(i, 1) = indices[i * 3 + 1];
    fMat(i, 2) = indices[i * 3 + 2];
  }

  for (uint32_t i = 0; i < numVertices; i++) {
    vMat(i, 0) = positions[i * 3 + 0];
    vMat(i, 1) = positions[i * 3 + 1];
    vMat(i, 2) = positions[i * 3 + 2];
  }

  std::unique_ptr<geometrycentral::surface::ManifoldSurfaceMesh> mesh;
  std::unique_ptr<geometrycentral::surface::VertexPositionGeometry> geometry;
  return geometrycentral::surface::makeManifoldSurfaceMeshAndGeometry(vMat, fMat);
}

class ComputeGeodesicsOutput {
public:
  std::vector<float> projectedPositions;
  std::vector<float> projectedNormals;

  ComputeGeodesicsOutput(uint32_t pointCount) {
    projectedPositions = std::vector<float>();
    projectedPositions.reserve(pointCount * 3);

    projectedNormals = std::vector<float>();
    projectedNormals.reserve(pointCount * 3);
  }
};

/**
 * Given a cartesian coordinate and a cartesian midpoint, compute the angle and distance
 * from the midpoint to the coordinate.
 */
std::tuple<float, float>
computeAngleAndDistance(float x, float y, float midpointX, float midpointY) {
  float dx = x - midpointX;
  float dy = y - midpointY;
  float angle = atan2(dy, dx);
  float distance = sqrt(dx * dx + dy * dy);
  return std::make_tuple(angle, distance);
}

void
walkCoord(
  float x,
  float y,
  surface::VertexPositionGeometry& targetGeometry,
  surface::SurfacePoint startSurfacePoint,
  float midpointX,
  float midpointY,
  ComputeGeodesicsOutput& output,
  surface::TraceOptions& traceOptions
) {
  float angle, distance;
  std::tie(angle, distance) = computeAngleAndDistance(x, y, midpointX, midpointY);

  Vector2 traceVec = distance * Vector2::fromAngle(angle);
  auto traceRes = traceGeodesic(targetGeometry, startSurfacePoint, traceVec);
  if (traceRes.hitBoundary) {
    throw std::invalid_argument("hit boundary");
  }

  auto vertexCount = targetGeometry.inputVertexPositions.size();
  for (size_t i = 0; i < vertexCount; i += 1) {
    auto vertex = targetGeometry.mesh.vertex(i);
    auto vertexCoords = targetGeometry.inputVertexPositions[vertex];
  }

  surface::SurfacePoint pathEndpoint = traceRes.endPoint;
  Vector3 normal;
  float cartX, cartY, cartZ;

  if (pathEndpoint.type == surface::SurfacePointType::Face) {
    // location inside that face, as barycentric coordinates (numbered according to the iteration order of vertices
    // about the face)
    auto coords = pathEndpoint.faceCoords;
    size_t faceIx = pathEndpoint.face.getIndex();
    auto face = pathEndpoint.face;
    normal = targetGeometry.faceNormal(face);

    auto i = 0;
    surface::Vertex v0, v1, v2;
    for (auto v : face.adjacentVertices()) {
      if (i == 0) {
        v0 = v;
      } else if (i == 1) {
        v1 = v;
      } else if (i == 2) {
        v2 = v;
      } else {
        throw std::invalid_argument("face has more than 3 vertices");
      }
      i += 1;
    }

    auto v0Pos = targetGeometry.inputVertexPositions[v0];
    auto v1Pos = targetGeometry.inputVertexPositions[v1];
    auto v2Pos = targetGeometry.inputVertexPositions[v2];

    // cartesian coordinates of the point in the triangle
    cartX = coords.x * v0Pos[0] + coords.y * v1Pos[0] + coords.z * v2Pos[0];
    cartY = coords.x * v0Pos[1] + coords.y * v1Pos[1] + coords.z * v2Pos[1];
    cartZ = coords.x * v0Pos[2] + coords.y * v1Pos[2] + coords.z * v2Pos[2];
  } else if (pathEndpoint.type == surface::SurfacePointType::Vertex) {
    auto vertex = pathEndpoint.vertex;
    auto coords = targetGeometry.inputVertexPositions[vertex];
    cartX = coords.x;
    cartY = coords.y;
    cartZ = coords.z;
    normal = Vector3::constant(1.);
  } else if (pathEndpoint.type == surface::SurfacePointType::Edge) {
    throw std::invalid_argument("path endpoint is an edge at index " + std::to_string(pathEndpoint.edge.getIndex()));
  } else {
    throw std::invalid_argument("unknown path endpoint type");
  }

  normal = normal.normalize();

  output.projectedPositions.push_back(cartX);
  output.projectedPositions.push_back(cartY);
  output.projectedPositions.push_back(cartZ);

  output.projectedNormals.push_back(normal.x);
  output.projectedNormals.push_back(normal.y);
  output.projectedNormals.push_back(normal.z);
}

ComputeGeodesicsOutput
computeGeodesics(
  std::vector<uint32_t> targetMeshIndices,
  std::vector<float> targetMeshPositions,
  std::vector<float> coordsToWalk,
  float midpointX,
  float midpointY
) {
  std::unique_ptr<geometrycentral::surface::ManifoldSurfaceMesh> targetMesh;
  std::unique_ptr<geometrycentral::surface::VertexPositionGeometry> targetGeometry;
  std::tie(targetMesh, targetGeometry) = loadMesh(targetMeshIndices, targetMeshPositions);

  uint32_t coordCount = coordsToWalk.size() / 2;
  ComputeGeodesicsOutput output = ComputeGeodesicsOutput(coordCount);

  auto startVertex = targetMesh->vertex(0);
  auto startSurfacePoint = geometrycentral::surface::SurfacePoint(startVertex);

  surface::TraceOptions traceOptions;
  traceOptions.errorOnProblem = true;
  traceOptions.includePath = false;

  for (uint32_t ptIx = 0; ptIx < coordCount; ptIx += 1) {
    float x = coordsToWalk[ptIx * 2 + 0];
    float y = coordsToWalk[ptIx * 2 + 1];

    walkCoord(x, y, *targetGeometry, startSurfacePoint, midpointX, midpointY, output, traceOptions);
  }

  return output;
}

template<typename T>
uint32_t
getVecDataPtr(std::vector<T>& vec) {
  return reinterpret_cast<uint32_t>(vec.data());
}

template<typename T>
class_<std::vector<T>>
register_vector_custom(const char* name) {
  typedef std::vector<T> VecType;

  // void (VecType::*push_back)(const T&) = &VecType::push_back;
  void (VecType::*resize)(const size_t, const T&) = &VecType::resize;
  void (VecType::*reserve)(const size_t) = &VecType::reserve;
  size_t (VecType::*size)() const = &VecType::size;
  return class_<std::vector<T>>(name)
    .template constructor<>()
    // .function("push_back", push_back)
    .function("resize", resize)
    .function("size", size)
    // .function("reserve", reserve)
    // .function("get", &internal::VectorAccess<VecType>::get)
    // .function("set", &internal::VectorAccess<VecType>::set)
    .function("data", &getVecDataPtr<T>, allow_raw_pointers());
}

EMSCRIPTEN_BINDINGS(my_module) {
  function("computeGeodesics", &computeGeodesics);

  class_<ComputeGeodesicsOutput>("ComputeGeodesicsOutput")
    .property("projectedPositions", &ComputeGeodesicsOutput::projectedPositions)
    .property("projectedNormals", &ComputeGeodesicsOutput::projectedNormals);

  register_vector_custom<float>("vector<float>");
  register_vector_custom<uint32_t>("vector<uint32_t>");
}
