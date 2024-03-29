#include <emscripten/bind.h>

using namespace emscripten;

#include "geometrycentral/surface/surface_mesh_factories.h"
#include "geometrycentral/surface/surface_point.h"
#include "geometrycentral/surface/trace_geodesic.h"

using namespace geometrycentral;

#include <Eigen/Core>
#include <cmath>
#include <iostream>
#include <queue>

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

  ComputeGeodesicsOutput(uint32_t pointCount) {
    projectedPositions = std::vector<float>();
    projectedPositions.reserve(pointCount * 3);
  }
};

/**
 * Normalizes the angle to lie in [-π, π), wrapping around the range as necessary.
 */
double
normalizeAngle(double angle) {
  while (angle < -PI) {
    angle += 2.0 * PI;
  }
  while (angle >= PI) {
    angle -= 2.0 * PI;
  }
  return angle;
}

/**
 * Compute the difference between two angles, returning a value in [-π, π).
 */
double
angleDifference(double angle1, double angle2) {
  double difference = normalizeAngle(angle1 - angle2);
  return difference;
}

double
computeDesiredTangentSpaceAngle(double incomingTangentSpaceAngle, double incoming2DAngle, double next2DAngle) {
  double angleDiff2D = angleDifference(next2DAngle, incoming2DAngle);
  double desiredAngleTangent = incomingTangentSpaceAngle + angleDiff2D;
  return normalizeAngle(desiredAngleTangent);
}

/**
 * Computes the angle and distance to travel from the start point in the tangent space of that point.
 *
 * We know that `incoming2DAngle` - the angle in the 2D space of the coordinates we're mapping - matches
 * `incomingTangentSpaceAngle` in the tangent space of the start point.
 *
 * Using this, we can compute the difference that needs to be added to the incoming tangent space angle to get the
 * desired tangent space angle to travel in.
 */
std::tuple<double, double>
computeAngleAndDistance(
  double x,
  double y,
  double startX,
  double startY,
  double incomingTangentSpaceAngle,
  double incoming2DAngle
) {
  double dx = x - startX;
  double dy = y - startY;
  double angle2D = atan2(dy, dx);
  double distance = sqrt(dx * dx + dy * dy);

  double angle = computeDesiredTangentSpaceAngle(incomingTangentSpaceAngle, incoming2DAngle, angle2D);

  return std::make_tuple(angle, distance);
}

class WalkCoordOutput {
public:
  surface::SurfacePoint pathEndpoint;
  float cartX;
  float cartY;
  float cartZ;
  double incomingTangentSpaceAngle;
  double incoming2DAngle;

  WalkCoordOutput(
    float cartX,
    float cartY,
    float cartZ,
    surface::SurfacePoint pathEndpoint,
    double incomingTangentSpaceAngle,
    double incoming2DAngle
  ) {
    this->cartX = cartX;
    this->cartY = cartY;
    this->cartZ = cartZ;
    this->pathEndpoint = pathEndpoint;
    this->incomingTangentSpaceAngle = incomingTangentSpaceAngle;
    this->incoming2DAngle = incoming2DAngle;
  }
};

std::tuple<float, float, float>
getSurfacePointCoords(surface::VertexPositionGeometry& targetGeometry, surface::SurfacePoint surfacePoint) {
  if (surfacePoint.type == surface::SurfacePointType::Face) {
    // location inside that face, as barycentric coordinates (numbered according to the iteration order of vertices
    // about the face)
    auto coords = surfacePoint.faceCoords;
    size_t faceIx = surfacePoint.face.getIndex();
    auto face = surfacePoint.face;

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

    Vector3& v0Pos = targetGeometry.inputVertexPositions[v0];
    Vector3& v1Pos = targetGeometry.inputVertexPositions[v1];
    Vector3& v2Pos = targetGeometry.inputVertexPositions[v2];

    // cartesian coordinates of the point in the triangle
    float cartX = coords.x * v0Pos.x + coords.y * v1Pos.x + coords.z * v2Pos.x;
    float cartY = coords.x * v0Pos.y + coords.y * v1Pos.y + coords.z * v2Pos.y;
    float cartZ = coords.x * v0Pos.z + coords.y * v1Pos.z + coords.z * v2Pos.z;

    return std::make_tuple(cartX, cartY, cartZ);
  } else if (surfacePoint.type == surface::SurfacePointType::Vertex) {
    surface::Vertex& vertex = surfacePoint.vertex;
    Vector3& coords = targetGeometry.inputVertexPositions[vertex];
    return std::make_tuple(coords.x, coords.y, coords.z);
  } else if (surfacePoint.type == surface::SurfacePointType::Edge) {
    // throw std::invalid_argument("path endpoint is an edge at index " + std::to_string(surfacePoint.edge.getIndex()));
    auto edge = surfacePoint.edge;
    double t = surfacePoint.tEdge;
    auto firstVertex = edge.firstVertex();
    auto secondVertex = edge.secondVertex();
    auto firstCoords = targetGeometry.inputVertexPositions[firstVertex];
    auto secondCoords = targetGeometry.inputVertexPositions[secondVertex];

    auto coords = (1 - t) * firstCoords + t * secondCoords;
    return std::make_tuple(coords.x, coords.y, coords.z);
  } else {
    throw std::invalid_argument("unknown path endpoint type");
  }
}

/**
 * `startDir` should be a vector in the canonical tangent space of that point (represented as a vector in that tangent
 * space)
 */
WalkCoordOutput
walkCoord(
  float x,
  float y,
  surface::VertexPositionGeometry& targetGeometry,
  surface::SurfacePoint startSurfacePoint,
  float startX,
  float startY,
  double incomingTangentSpaceAngle,
  double incoming2DAngle,
  surface::TraceOptions& traceOptions
) {
  double angle, distance;
  std::tie(angle, distance) = computeAngleAndDistance(x, y, startX, startY, incomingTangentSpaceAngle, incoming2DAngle);

  if (distance == 0.) {
    float cartX, cartY, cartZ;
    std::tie(cartX, cartY, cartZ) = getSurfacePointCoords(targetGeometry, startSurfacePoint);
    return WalkCoordOutput(cartX, cartY, cartZ, startSurfacePoint, incomingTangentSpaceAngle, incoming2DAngle);
  }

  Vector2 traceVec = distance * Vector2::fromAngle(angle);
  auto traceRes = traceGeodesic(targetGeometry, startSurfacePoint, traceVec);

  surface::SurfacePoint pathEndpoint = traceRes.endPoint;
  float cartX, cartY, cartZ;
  std::tie(cartX, cartY, cartZ) = getSurfacePointCoords(targetGeometry, pathEndpoint);

  // if (traceRes.hitBoundary) {
  //   throw std::invalid_argument("hit boundary at index " + std::to_string(traceRes.hitBoundaryIndex));
  // }

  Vector2 endDir = traceRes.endingDir;
  double newIncomingTangentSpaceAngle = atan2(endDir.y, endDir.x);
  double newIncoming2DAngle = atan2(y - startY, x - startX);
  return WalkCoordOutput(cartX, cartY, cartZ, pathEndpoint, newIncomingTangentSpaceAngle, newIncoming2DAngle);
}

using Graph = std::vector<std::vector<uint32_t>>;

Graph
buildGraph(const std::vector<uint32_t>& indicesToWalk) {
  Graph graph;
  auto maxVertexIdx = *std::max_element(indicesToWalk.begin(), indicesToWalk.end());
  graph.resize(maxVertexIdx + 1);

  for (size_t i = 0; i < indicesToWalk.size(); i += 3) {
    uint32_t a = indicesToWalk[i];
    uint32_t b = indicesToWalk[i + 1];
    uint32_t c = indicesToWalk[i + 2];

    graph[a].push_back(b);
    graph[a].push_back(c);
    graph[b].push_back(a);
    graph[b].push_back(c);
    graph[c].push_back(a);
    graph[c].push_back(b);
  }

  // remove duplicates
  for (auto& neighbors : graph) {
    std::sort(neighbors.begin(), neighbors.end());
    neighbors.erase(std::unique(neighbors.begin(), neighbors.end()), neighbors.end());
  }

  return graph;
}

const uint32_t INVALID_VERTEX_IX = 4294967295;

class BFSQueueEntry {
public:
  uint32_t vertexIdx;
  surface::SurfacePoint surfacePoint;
  float x;
  float y;
  double incomingTangentSpaceAngle;
  double incoming2DAngle;

  BFSQueueEntry(
    uint32_t vertexIdx,
    surface::SurfacePoint surfacePoint,
    float x,
    float y,
    double incomingTangentSpaceAngle,
    double incoming2DAngle
  ) {
    this->vertexIdx = vertexIdx;
    this->surfacePoint = surfacePoint;
    this->x = x;
    this->y = y;
    this->incomingTangentSpaceAngle = incomingTangentSpaceAngle;
    this->incoming2DAngle = incoming2DAngle;
  }

  // default constructor
  BFSQueueEntry() { this->vertexIdx = INVALID_VERTEX_IX; }
};

#include <cmath>
#include <limits>
#include <unordered_map>
#include <vector>

class GridPoint {
public:
  float x, y;
  uint32_t data;

  GridPoint(float x, float y, uint32_t data)
    : x(x)
    , y(y)
    , data(data) {}

  float distance(const GridPoint& other) const {
    float dx = x - other.x;
    float dy = y - other.y;
    return std::sqrt(dx * dx + dy * dy);
  }
};

class Grid {
private:
  std::unordered_map<int, std::unordered_map<int, std::vector<GridPoint>>> grid;
  float bucketSize;

  int getBucketIndex(float coordinate) const { return static_cast<int>(std::floor(coordinate / bucketSize)); }

public:
  Grid(float bucketSize)
    : bucketSize(bucketSize) {}

  void addPoint(float x, float y, uint32_t data) {
    int bx = getBucketIndex(x);
    int by = getBucketIndex(y);
    grid[bx][by].emplace_back(x, y, data);
  }

  uint32_t findClosestPoint(float x, float y) {
    int bx = getBucketIndex(x);
    int by = getBucketIndex(y);

    float minDistance = std::numeric_limits<float>::max();
    uint32_t closestData;
    bool pointFound = false;

    for (int radius = 1; radius <= 16 && !pointFound; ++radius) {
      int startX = bx - radius;
      int endX = bx + radius;
      int startY = by - radius;
      int endY = by + radius;

      for (int i = startX; i <= endX; ++i) {
        for (int j = startY; j <= endY; ++j) {
          if (i < startX + radius && i > endX - radius && j < startY + radius && j > endY - radius) {
            // Skip the inner buckets which have already been checked
            continue;
          }

          for (const GridPoint& point : grid[i][j]) {
            float dist = point.distance(GridPoint(x, y, 0));
            if (dist < minDistance) {
              minDistance = dist;
              closestData = point.data;
              pointFound = true;
            }
          }
        }
      }
    }

    if (!pointFound) {
      throw std::runtime_error("No point found within the search radius");
    }

    return closestData;
  }
};

ComputeGeodesicsOutput
computeGeodesics(
  std::vector<uint32_t> targetMeshIndices,
  std::vector<float> targetMeshPositions,
  std::vector<float> coordsToWalk,
  std::vector<uint32_t> indicesToWalk,
  float midpointX,
  float midpointY
) {
  std::unique_ptr<geometrycentral::surface::ManifoldSurfaceMesh> targetMesh;
  std::unique_ptr<geometrycentral::surface::VertexPositionGeometry> targetGeometry;
  std::tie(targetMesh, targetGeometry) = loadMesh(targetMeshIndices, targetMeshPositions);

  auto startFace = targetMesh->face(0);
  auto origSurfacePoint = geometrycentral::surface::SurfacePoint(startFace, Vector3{ 0.3, 0.3, 0.4 });

  surface::TraceOptions traceOptions;
  traceOptions.errorOnProblem = true;
  traceOptions.includePath = false;

  uint32_t coordCount = coordsToWalk.size() / 2;
  ComputeGeodesicsOutput output = ComputeGeodesicsOutput(coordCount);
  output.projectedPositions.resize(coordCount * 3);

  Graph graph = buildGraph(indicesToWalk);

  Grid processedCoordsGrid(5.);

  // Start BFS from the first vertex
  std::queue<BFSQueueEntry> bfsQueue;
  std::vector<BFSQueueEntry> visited;
  visited.resize(coordCount);
  float startX = 0.001;
  float startY = 0.001;
  bfsQueue.push({ 0, origSurfacePoint, startX, startY, 0., 0. });

  while (true) {
    while (!bfsQueue.empty()) {
      uint32_t curVertexIdx = bfsQueue.front().vertexIdx;
      if (visited[curVertexIdx].vertexIdx != INVALID_VERTEX_IX) {
        bfsQueue.pop();
        continue;
      }

      BFSQueueEntry entry = bfsQueue.front();
      surface::SurfacePoint startSurfacePoint = entry.surfacePoint;
      bfsQueue.pop();

      float startX = entry.x;
      float startY = entry.y;
      float x = coordsToWalk[curVertexIdx * 2 + 0];
      float y = coordsToWalk[curVertexIdx * 2 + 1];

      WalkCoordOutput walkOutput = walkCoord(
        x, y, *targetGeometry, startSurfacePoint, startX, startY, entry.incomingTangentSpaceAngle,
        entry.incoming2DAngle, traceOptions
      );
      output.projectedPositions[curVertexIdx * 3 + 0] = walkOutput.cartX;
      output.projectedPositions[curVertexIdx * 3 + 1] = walkOutput.cartY;
      output.projectedPositions[curVertexIdx * 3 + 2] = walkOutput.cartZ;
      auto endpoint = walkOutput.pathEndpoint;
      visited[curVertexIdx] =
        BFSQueueEntry(curVertexIdx, endpoint, x, y, walkOutput.incomingTangentSpaceAngle, walkOutput.incoming2DAngle);
      processedCoordsGrid.addPoint(x, y, curVertexIdx);

      for (auto neighbor : graph[curVertexIdx]) {
        if (visited[neighbor].vertexIdx != INVALID_VERTEX_IX) {
          continue;
        }
        bfsQueue.push(
          BFSQueueEntry(neighbor, endpoint, x, y, walkOutput.incomingTangentSpaceAngle, walkOutput.incoming2DAngle)
        );
      }
    }

    // TODO: use a better method to find closest unvisited vertex
    uint32_t unvisitedVertexIx = INVALID_VERTEX_IX;
    for (uint32_t i = 0; i < coordCount; i += 1) {
      if (visited[i].vertexIdx == INVALID_VERTEX_IX) {
        unvisitedVertexIx = i;
        break;
      }
    }

    if (unvisitedVertexIx == INVALID_VERTEX_IX) {
      break;
    }

    auto unvisitedX = coordsToWalk[unvisitedVertexIx * 2 + 0];
    auto unvisitedY = coordsToWalk[unvisitedVertexIx * 2 + 1];

    uint32_t closestVertexIx = processedCoordsGrid.findClosestPoint(unvisitedX, unvisitedY);

    if (closestVertexIx == INVALID_VERTEX_IX) {
      throw std::invalid_argument("no closest vertex found");
    }

    // walk from the closest visited vertex to the unvisited vertex
    BFSQueueEntry& closestEntry = visited[closestVertexIx];
    if (closestEntry.vertexIdx != closestVertexIx) {
      throw std::invalid_argument("closest entry vertex index does not match");
    }

    auto newEntry = BFSQueueEntry(
      unvisitedVertexIx, closestEntry.surfacePoint, closestEntry.x, closestEntry.y,
      closestEntry.incomingTangentSpaceAngle, closestEntry.incoming2DAngle
    );
    bfsQueue.push(newEntry);
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
  // void (VecType::*reserve)(const size_t) = &VecType::reserve;
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
    .property("projectedPositions", &ComputeGeodesicsOutput::projectedPositions);

  register_vector_custom<float>("vector<float>");
  register_vector_custom<uint32_t>("vector<uint32_t>");
}
