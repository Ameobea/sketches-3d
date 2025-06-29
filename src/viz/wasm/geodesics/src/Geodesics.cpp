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
#include <algorithm>
#include <stdexcept>
#include <tuple>
#include <vector>
#include <limits>

std::tuple<
  std::unique_ptr<geometrycentral::surface::ManifoldSurfaceMesh>,
  std::unique_ptr<geometrycentral::surface::VertexPositionGeometry>>
loadMesh(const std::vector<uint32_t>& indices, const std::vector<float>& positions) {
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

struct ComputeGeodesicsOutput {
  std::vector<float> projectedPositions;

  explicit ComputeGeodesicsOutput(uint32_t pointCount) {
    projectedPositions.reserve(pointCount * 3);
  }
};

/**
 * Normalizes the angle to lie in [-π, π), wrapping around the range as necessary.
 */
double
normalizeAngle(double angle) {
  angle = fmod(angle + PI, 2. * PI);
  if (angle < 0.0) {
    angle += 2.0 * PI;
  }
  return angle - PI;
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

struct WalkCoordOutput {
  std::vector<surface::SurfacePoint> pathPoints;
  surface::SurfacePoint pathEndpoint;
  double incomingTangentSpaceAngle;
  double incoming2DAngle;

  WalkCoordOutput(
    std::vector<surface::SurfacePoint> pathPoints,
    const surface::SurfacePoint& pathEndpoint,
    double incomingTangentSpaceAngle,
    double incoming2DAngle
  ) : pathPoints(std::move(pathPoints)),
      pathEndpoint(pathEndpoint),
      incomingTangentSpaceAngle(incomingTangentSpaceAngle),
      incoming2DAngle(incoming2DAngle) {}
};

std::tuple<float, float, float>
getSurfacePointCoords(surface::VertexPositionGeometry& targetGeometry, const surface::SurfacePoint& surfacePoint) {
  if (surfacePoint.type == surface::SurfacePointType::Face) {
    // location inside that face, as barycentric coordinates (numbered according to the iteration order of vertices
    // about the face)
    auto coords = surfacePoint.faceCoords;
    auto face = surfacePoint.face;

    std::vector<surface::Vertex> face_vertices;
    face_vertices.reserve(3);
    for (auto v : face.adjacentVertices()) {
      face_vertices.push_back(v);
    }

    if (face_vertices.size() != 3) {
      throw std::invalid_argument("face has more than 3 vertices");
    }

    Vector3& v0Pos = targetGeometry.inputVertexPositions[face_vertices[0]];
    Vector3& v1Pos = targetGeometry.inputVertexPositions[face_vertices[1]];
    Vector3& v2Pos = targetGeometry.inputVertexPositions[face_vertices[2]];

    // cartesian coordinates of the point in the triangle
    float cartX = coords.x * v0Pos.x + coords.y * v1Pos.x + coords.z * v2Pos.x;
    float cartY = coords.x * v0Pos.y + coords.y * v1Pos.y + coords.z * v2Pos.y;
    float cartZ = coords.x * v0Pos.z + coords.y * v1Pos.z + coords.z * v2Pos.z;

    return std::make_tuple(cartX, cartY, cartZ);
  } else if (surfacePoint.type == surface::SurfacePointType::Vertex) {
    const surface::Vertex& vertex = surfacePoint.vertex;
    const Vector3& coords = targetGeometry.inputVertexPositions[vertex];
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
  const surface::SurfacePoint& startSurfacePoint,
  float startX,
  float startY,
  double incomingTangentSpaceAngle,
  double incoming2DAngle,
  surface::TraceOptions& traceOptions
) {
  double angle, distance;
  std::tie(angle, distance) = computeAngleAndDistance(x, y, startX, startY, incomingTangentSpaceAngle, incoming2DAngle);

  if (distance == 0.) {
    return WalkCoordOutput(
      { startSurfacePoint },
      startSurfacePoint,
      incomingTangentSpaceAngle,
      incoming2DAngle
    );
  }

  Vector2 traceVec = distance * Vector2::fromAngle(angle);
  auto traceRes = traceGeodesic(targetGeometry, startSurfacePoint, traceVec, traceOptions);

  surface::SurfacePoint pathEndpoint = traceRes.endPoint;

  // if (traceRes.hitBoundary) {
  //   throw std::invalid_argument("hit boundary at index " + std::to_string(traceRes.hitBoundaryIndex));
  // }

  Vector2 endDir = traceRes.endingDir;
  double newIncomingTangentSpaceAngle = atan2(endDir.y, endDir.x);
  double newIncoming2DAngle = atan2(y - startY, x - startX);
  return WalkCoordOutput(std::move(traceRes.pathPoints), pathEndpoint, newIncomingTangentSpaceAngle, newIncoming2DAngle);
}

using Graph = std::vector<std::vector<uint32_t>>;

Graph
buildGraph(const std::vector<uint32_t>& indicesToWalk) {
  Graph graph;
  if (indicesToWalk.empty()) {
    return graph;
  }
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

struct BFSQueueEntry {
  uint32_t vertexIdx;
  surface::SurfacePoint surfacePoint;
  float x;
  float y;
  double incomingTangentSpaceAngle;
  double incoming2DAngle;

  BFSQueueEntry(
    uint32_t vertexIdx,
    const surface::SurfacePoint& surfacePoint,
    float x,
    float y,
    double incomingTangentSpaceAngle,
    double incoming2DAngle
  ) : vertexIdx(vertexIdx),
      surfacePoint(surfacePoint),
      x(x),
      y(y),
      incomingTangentSpaceAngle(incomingTangentSpaceAngle),
      incoming2DAngle(incoming2DAngle) {}

  // default constructor for vector resizing
  BFSQueueEntry() : vertexIdx(INVALID_VERTEX_IX) {}
};

class KDTree {
private:
  struct Point {
    float x, y;
    uint32_t data;
  };

  struct Node {
    Point pt;
    Node *left = nullptr;
    Node *right = nullptr;
  };

  std::vector<Point> points_copy;
  std::vector<Node> node_pool;
  size_t next_node_idx = 0;
  Node* root = nullptr;

  Node* newNode(const Point& pt) {
    if (next_node_idx >= node_pool.size()) {
      throw std::runtime_error("KDTree node pool exhausted");
    }
    Node* node = &node_pool[next_node_idx++];
    node->pt = pt;
    node->left = nullptr;
    node->right = nullptr;
    return node;
  }

  Node* build(typename std::vector<Point>::iterator begin, typename std::vector<Point>::iterator end, int depth) {
    if (begin >= end) {
      return nullptr;
    }

    int axis = depth % 2;
    size_t len = std::distance(begin, end);
    auto mid = begin + len / 2;

    std::nth_element(begin, mid, end, [axis](const Point& a, const Point& b) {
      return axis == 0 ? a.x < b.x : a.y < b.y;
    });

    Node* node = newNode(*mid);
    node->left = build(begin, mid, depth + 1);
    node->right = build(mid + 1, end, depth + 1);
    return node;
  }

  void findClosest(Node* node, const Point& target, Point& best, float& min_dist_sq, int depth) const {
    if (!node) {
      return;
    }

    float dx_node = target.x - node->pt.x;
    float dy_node = target.y - node->pt.y;
    float d_sq = dx_node * dx_node + dy_node * dy_node;

    if (d_sq < min_dist_sq) {
      min_dist_sq = d_sq;
      best = node->pt;
    }

    int axis = depth % 2;
    float delta = axis == 0 ? dx_node : dy_node;

    Node *near_child = delta < 0 ? node->left : node->right;
    Node *far_child = delta < 0 ? node->right : node->left;

    findClosest(near_child, target, best, min_dist_sq, depth + 1);

    if (delta * delta < min_dist_sq) {
      findClosest(far_child, target, best, min_dist_sq, depth + 1);
    }
  }

public:
  explicit KDTree(const std::vector<std::tuple<float, float, uint32_t>>& initial_points) {
    if (initial_points.empty()) {
      return;
    }

    points_copy.reserve(initial_points.size());
    for(const auto& [x, y, data] : initial_points) {
      points_copy.push_back({x, y, data});
    }

    node_pool.resize(points_copy.size());
    next_node_idx = 0;
    root = build(points_copy.begin(), points_copy.end(), 0);
  }

  uint32_t findClosestPoint(float x, float y) const {
    if (!root) {
      throw std::runtime_error("No point found within the search radius");
    }

    Point target = {x, y, 0};
    Point best = root->pt;
    float dx_root = target.x - root->pt.x;
    float dy_root = target.y - root->pt.y;
    float min_dist_sq = dx_root * dx_root + dy_root * dy_root;

    findClosest(root, target, best, min_dist_sq, 0);
    return best.data;
  }
};

ComputeGeodesicsOutput
computeGeodesics(
  const std::vector<uint32_t>& targetMeshIndices,
  const std::vector<float>& targetMeshPositions,
  const std::vector<float>& coordsToWalk,
  const std::vector<uint32_t>& indicesToWalk,
  bool fullPath
) {
  std::unique_ptr<geometrycentral::surface::ManifoldSurfaceMesh> targetMesh;
  std::unique_ptr<geometrycentral::surface::VertexPositionGeometry> targetGeometry;
  std::tie(targetMesh, targetGeometry) = loadMesh(targetMeshIndices, targetMeshPositions);

  auto startFace = targetMesh->face(0);
  auto origSurfacePoint = geometrycentral::surface::SurfacePoint(startFace, Vector3{ 0.3, 0.3, 0.4 });

  surface::TraceOptions traceOptions;
  traceOptions.errorOnProblem = true;
  traceOptions.includePath = fullPath;

  uint32_t coordCount = coordsToWalk.size() / 2;
  ComputeGeodesicsOutput output(coordCount);
  if (!fullPath) {
    output.projectedPositions.resize(coordCount * 3);
  }

  Graph graph = buildGraph(indicesToWalk);

  std::vector<std::tuple<float, float, uint32_t>> processedCoords;

  // Start BFS from the first vertex
  std::queue<BFSQueueEntry> bfsQueue;
  std::vector<BFSQueueEntry> visited;
  visited.resize(coordCount);
  float startX = 0.001;
  float startY = 0.001;
  bfsQueue.push({ 0, origSurfacePoint, startX, startY, 0., 0. });

  while (true) {
    while (!bfsQueue.empty()) {
      uint32_t inVtxIx = bfsQueue.front().vertexIdx;
      if (visited[inVtxIx].vertexIdx != INVALID_VERTEX_IX) {
        bfsQueue.pop();
        continue;
      }

      BFSQueueEntry entry = bfsQueue.front();
      surface::SurfacePoint startSurfacePoint = entry.surfacePoint;
      bfsQueue.pop();

      float startX = entry.x;
      float startY = entry.y;
      float x = coordsToWalk[inVtxIx * 2 + 0];
      float y = coordsToWalk[inVtxIx * 2 + 1];

      WalkCoordOutput walkOutput = walkCoord(
        x, y, *targetGeometry, startSurfacePoint, startX, startY, entry.incomingTangentSpaceAngle,
        entry.incoming2DAngle, traceOptions
      );

      if (fullPath) {
        for (const auto& pathPoint : walkOutput.pathPoints) {
          float cartX, cartY, cartZ;
          std::tie(cartX, cartY, cartZ) = getSurfacePointCoords(*targetGeometry, pathPoint);
          output.projectedPositions.push_back(cartX);
          output.projectedPositions.push_back(cartY);
          output.projectedPositions.push_back(cartZ);
        }
      } else {
        float cartX, cartY, cartZ;
        std::tie(cartX, cartY, cartZ) = getSurfacePointCoords(*targetGeometry, walkOutput.pathEndpoint);
        output.projectedPositions[inVtxIx * 3 + 0] = cartX;
        output.projectedPositions[inVtxIx * 3 + 1] = cartY;
        output.projectedPositions[inVtxIx * 3 + 2] = cartZ;
      }

      auto endpoint = walkOutput.pathEndpoint;
      visited[inVtxIx] =
        BFSQueueEntry(inVtxIx, endpoint, x, y, walkOutput.incomingTangentSpaceAngle, walkOutput.incoming2DAngle);
      processedCoords.emplace_back(x, y, inVtxIx);

      for (auto neighbor : graph[inVtxIx]) {
        if (visited[neighbor].vertexIdx != INVALID_VERTEX_IX) {
          continue;
        }
        bfsQueue.push(
          BFSQueueEntry(neighbor, endpoint, x, y, walkOutput.incomingTangentSpaceAngle, walkOutput.incoming2DAngle)
        );
      }
    }

    // Find the closest unvisited vertex
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

    KDTree processedCoordsTree(processedCoords);
    uint32_t closestVertexIx = processedCoordsTree.findClosestPoint(unvisitedX, unvisitedY);

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
