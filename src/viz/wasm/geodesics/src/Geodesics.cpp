#include <emscripten/bind.h>
#include <emscripten/val.h>

using namespace emscripten;

#include "geometrycentral/surface/surface_mesh_factories.h"
#include "geometrycentral/surface/surface_point.h"
#include "geometrycentral/surface/trace_geodesic.h"

using namespace geometrycentral;

#include <Eigen/Core>
#include <algorithm>
#include <cmath>
#include <iostream>
#include <limits>
#include <queue>
#include <stdexcept>
#include <tuple>
#include <unordered_map>
#include <vector>

std::tuple<std::unique_ptr<geometrycentral::surface::ManifoldSurfaceMesh>,
           std::unique_ptr<geometrycentral::surface::VertexPositionGeometry>>
loadMesh(const std::vector<uint32_t> &indices,
         const std::vector<float> &positions) {
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
  return geometrycentral::surface::makeManifoldSurfaceMeshAndGeometry(vMat,
                                                                      fMat);
}

struct ComputeGeodesicsOutput {
  std::vector<float> projectedPositions;

  explicit ComputeGeodesicsOutput(uint32_t pointCount) {
    projectedPositions.reserve(pointCount * 3);
  }
};

/**
 * Normalizes the angle to lie in [-π, π), wrapping around the range as
 * necessary.
 */
double normalizeAngle(double angle) {
  angle = fmod(angle + PI, 2. * PI);
  if (angle < 0.) {
    angle += 2. * PI;
  }
  return angle - PI;
}

/**
 * Compute the difference between two angles, returning a value in [-π, π).
 */
double angleDifference(double angle1, double angle2) {
  double difference = normalizeAngle(angle1 - angle2);
  return difference;
}

double computeDesiredTangentSpaceAngle(double incomingTangentSpaceAngle,
                                       double incoming2DAngle,
                                       double next2DAngle) {
  double angleDiff2D = angleDifference(next2DAngle, incoming2DAngle);
  double desiredAngleTangent = incomingTangentSpaceAngle + angleDiff2D;
  return normalizeAngle(desiredAngleTangent);
}

/**
 * Computes the angle and distance to travel from the start point in the tangent
 * space of that point.
 *
 * We know that `incoming2DAngle` - the angle in the 2D space of the coordinates
 * we're mapping - matches `incomingTangentSpaceAngle` in the tangent space of
 * the start point.
 *
 * Using this, we can compute the difference that needs to be added to the
 * incoming tangent space angle to get the desired tangent space angle to travel
 * in.
 */
std::tuple<double, double>
computeAngleAndDistance(double x, double y, double startX, double startY,
                        double incomingTangentSpaceAngle,
                        double incoming2DAngle) {
  double dx = x - startX;
  double dy = y - startY;
  double angle2D = atan2(dy, dx);
  double distance = sqrt(dx * dx + dy * dy);

  double angle = computeDesiredTangentSpaceAngle(incomingTangentSpaceAngle,
                                                 incoming2DAngle, angle2D);

  return std::make_tuple(angle, distance);
}

struct WalkCoordOutput {
  std::vector<surface::SurfacePoint> pathPoints;
  surface::SurfacePoint pathEndpoint;
  double incomingTangentSpaceAngle;
  double incoming2DAngle;

  WalkCoordOutput(std::vector<surface::SurfacePoint> pathPoints,
                  const surface::SurfacePoint &pathEndpoint,
                  double incomingTangentSpaceAngle, double incoming2DAngle)
      : pathPoints(std::move(pathPoints)), pathEndpoint(pathEndpoint),
        incomingTangentSpaceAngle(incomingTangentSpaceAngle),
        incoming2DAngle(incoming2DAngle) {}
};

std::tuple<float, float, float>
getSurfacePointCoords(surface::VertexPositionGeometry &targetGeometry,
                      const surface::SurfacePoint &surfacePoint) {
  if (surfacePoint.type == surface::SurfacePointType::Face) {
    // location inside that face, as barycentric coordinates (numbered according
    // to the iteration order of vertices about the face)
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

    Vector3 &v0Pos = targetGeometry.inputVertexPositions[face_vertices[0]];
    Vector3 &v1Pos = targetGeometry.inputVertexPositions[face_vertices[1]];
    Vector3 &v2Pos = targetGeometry.inputVertexPositions[face_vertices[2]];

    // cartesian coordinates of the point in the triangle
    float cartX = coords.x * v0Pos.x + coords.y * v1Pos.x + coords.z * v2Pos.x;
    float cartY = coords.x * v0Pos.y + coords.y * v1Pos.y + coords.z * v2Pos.y;
    float cartZ = coords.x * v0Pos.z + coords.y * v1Pos.z + coords.z * v2Pos.z;

    return std::make_tuple(cartX, cartY, cartZ);
  } else if (surfacePoint.type == surface::SurfacePointType::Vertex) {
    const surface::Vertex &vertex = surfacePoint.vertex;
    const Vector3 &coords = targetGeometry.inputVertexPositions[vertex];
    return std::make_tuple(coords.x, coords.y, coords.z);
  } else if (surfacePoint.type == surface::SurfacePointType::Edge) {
    // throw std::invalid_argument("path endpoint is an edge at index " +
    // std::to_string(surfacePoint.edge.getIndex()));
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
 * `startDir` should be a vector in the canonical tangent space of that point
 * (represented as a vector in that tangent space)
 */
WalkCoordOutput
walkCoord(float x, float y, surface::VertexPositionGeometry &targetGeometry,
          const surface::SurfacePoint &startSurfacePoint, float startX,
          float startY, double incomingTangentSpaceAngle,
          double incoming2DAngle, surface::TraceOptions &traceOptions) {
  double angle, distance;
  std::tie(angle, distance) = computeAngleAndDistance(
      x, y, startX, startY, incomingTangentSpaceAngle, incoming2DAngle);

  if (distance == 0.) {
    return WalkCoordOutput({startSurfacePoint}, startSurfacePoint,
                           incomingTangentSpaceAngle, incoming2DAngle);
  }

  Vector2 traceVec = distance * Vector2::fromAngle(angle);
  auto traceRes =
      traceGeodesic(targetGeometry, startSurfacePoint, traceVec, traceOptions);

  surface::SurfacePoint pathEndpoint = traceRes.endPoint;

  // if (traceRes.hitBoundary) {
  //   throw std::invalid_argument("hit boundary at index " +
  //   std::to_string(traceRes.hitBoundaryIndex));
  // }

  Vector2 endDir = traceRes.endingDir;
  double newIncomingTangentSpaceAngle = atan2(endDir.y, endDir.x);
  double newIncoming2DAngle = atan2(y - startY, x - startX);
  return WalkCoordOutput(std::move(traceRes.pathPoints), pathEndpoint,
                         newIncomingTangentSpaceAngle, newIncoming2DAngle);
}

using Graph = std::vector<std::vector<uint32_t>>;

Graph buildGraph(const std::vector<uint32_t> &indicesToWalk) {
  Graph graph;
  if (indicesToWalk.empty()) {
    return graph;
  }
  auto maxVertexIdx =
      *std::max_element(indicesToWalk.begin(), indicesToWalk.end());
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
  for (auto &neighbors : graph) {
    std::sort(neighbors.begin(), neighbors.end());
    neighbors.erase(std::unique(neighbors.begin(), neighbors.end()),
                    neighbors.end());
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

  BFSQueueEntry(uint32_t vertexIdx, const surface::SurfacePoint &surfacePoint,
                float x, float y, double incomingTangentSpaceAngle,
                double incoming2DAngle)
      : vertexIdx(vertexIdx), surfacePoint(surfacePoint), x(x), y(y),
        incomingTangentSpaceAngle(incomingTangentSpaceAngle),
        incoming2DAngle(incoming2DAngle) {}

  // default constructor for vector resizing
  BFSQueueEntry() : vertexIdx(INVALID_VERTEX_IX) {}
};

class GridPoint {
public:
  float x, y;
  uint32_t data;

  GridPoint(float x, float y, uint32_t data) : x(x), y(y), data(data) {}

  float distance(const GridPoint &other) const {
    float dx = x - other.x;
    float dy = y - other.y;
    return std::sqrt(dx * dx + dy * dy);
  }
};

class Grid {
private:
  std::unordered_map<int, std::unordered_map<int, std::vector<GridPoint>>> grid;
  float bucketSize;

  int getBucketIndex(float coordinate) const {
    return static_cast<int>(std::floor(coordinate / bucketSize));
  }

public:
  Grid(float bucketSize) : bucketSize(bucketSize) {}

  void addPoint(float x, float y, uint32_t data) {
    int bx = getBucketIndex(x);
    int by = getBucketIndex(y);
    grid[bx][by].emplace_back(x, y, data);
  }

  uint32_t findClosestPoint(float x, float y) const {
    int bx = getBucketIndex(x);
    int by = getBucketIndex(y);

    float minDistance = std::numeric_limits<float>::max();
    uint32_t closestData = INVALID_VERTEX_IX;
    bool pointFound = false;

    for (int radius = 1; radius <= 16 && !pointFound; ++radius) {
      int startX = bx - radius;
      int endX = bx + radius;
      int startY = by - radius;
      int endY = by + radius;

      for (int i = startX; i <= endX; ++i) {
        for (int j = startY; j <= endY; ++j) {
          if (i < startX + radius && i > endX - radius && j < startY + radius &&
              j > endY - radius) {
            // Skip the inner buckets which have already been checked
            continue;
          }

          auto itX = grid.find(i);
          if (itX == grid.end())
            continue;
          auto itY = itX->second.find(j);
          if (itY == itX->second.end())
            continue;

          for (const GridPoint &point : itY->second) {
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

using namespace geometrycentral::surface;

// From "Real-Time Collision Detection": http://realtimecollisiondetection.net
std::tuple<Vector3, Vector3> closestPointOnTriangle(const Vector3 &p,
                                                    const Vector3 &a,
                                                    const Vector3 &b,
                                                    const Vector3 &c) {
  Vector3 ab = b - a;
  Vector3 ac = c - a;
  Vector3 ap = p - a;
  double d1 = dot(ab, ap);
  double d2 = dot(ac, ap);
  if (d1 <= 0. && d2 <= 0.)
    return {a, {1., 0., 0.}};

  Vector3 bp = p - b;
  double d3 = dot(ab, bp);
  double d4 = dot(ac, bp);
  if (d3 >= 0. && d4 <= d3)
    return {b, {0., 1., 0.}};

  double vc = d1 * d4 - d3 * d2;
  if (vc <= 0. && d1 >= 0. && d3 <= 0.) {
    double v = d1 / (d1 - d3);
    return {a + v * ab, {1. - v, v, 0.}};
  }

  Vector3 cp = p - c;
  double d5 = dot(ab, cp);
  double d6 = dot(ac, cp);
  if (d6 >= 0. && d5 <= d6)
    return {c, {0., 0., 1.}};

  double vb = d5 * d2 - d1 * d6;
  if (vb <= 0. && d2 >= 0. && d6 <= 0.) {
    double w = d2 / (d2 - d6);
    return {a + w * ac, {1. - w, 0., w}};
  }

  double va = d3 * d6 - d5 * d4;
  if (va <= 0. && (d4 - d3) >= 0. && (d5 - d6) >= 0.) {
    double w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
    return {b + w * (c - b), {0., 1. - w, w}};
  }

  double denom = 1. / (va + vb + vc);
  double v = vb * denom;
  double w = vc * denom;
  return {a + ab * v + ac * w, {1. - v - w, v, w}};
}

Edge edgeBetween(Vertex v1, Vertex v2) {
  for (const Halfedge &he : v1.outgoingHalfedges()) {
    if (he.tipVertex() == v2) {
      return he.edge();
    }
  }
  for (const Halfedge &he : v1.incomingHalfedges()) {
    if (he.tailVertex() == v2) {
      return he.edge();
    }
  }
  throw std::runtime_error("Could not find edge between vertices");
}

SurfacePoint findClosestPointOnMesh(const Vector3 &queryPoint,
                                    ManifoldSurfaceMesh &mesh,
                                    VertexPositionGeometry &geometry) {
  double min_dist_sq = std::numeric_limits<double>::max();
  SurfacePoint best_sp;

  for (Face f : mesh.faces()) {
    std::vector<Vertex> face_vertices;
    face_vertices.reserve(3);
    for (auto v : f.adjacentVertices()) {
      face_vertices.push_back(v);
    }

    if (face_vertices.size() != 3) {
      continue;
    }

    Vector3 p0 = geometry.inputVertexPositions[face_vertices[0]];
    Vector3 p1 = geometry.inputVertexPositions[face_vertices[1]];
    Vector3 p2 = geometry.inputVertexPositions[face_vertices[2]];

    auto [closest_pt, bary_coords] =
        closestPointOnTriangle(queryPoint, p0, p1, p2);

    double dist_sq = (queryPoint - closest_pt).norm2();

    if (dist_sq < min_dist_sq) {
      min_dist_sq = dist_sq;

      const double epsilon = 1e-8;
      if (bary_coords.x > 1. - epsilon) {
        best_sp = SurfacePoint(face_vertices[0]);
      } else if (bary_coords.y > 1. - epsilon) {
        best_sp = SurfacePoint(face_vertices[1]);
      } else if (bary_coords.z > 1. - epsilon) {
        best_sp = SurfacePoint(face_vertices[2]);
      } else if (std::abs(bary_coords.z) < epsilon) {
        Edge e = edgeBetween(face_vertices[0], face_vertices[1]);
        double t = (e.firstVertex() == face_vertices[0]) ? bary_coords.y
                                                         : 1. - bary_coords.y;
        best_sp = SurfacePoint(e, t);
      } else if (std::abs(bary_coords.y) < epsilon) {
        Edge e = edgeBetween(face_vertices[0], face_vertices[2]);
        double t = (e.firstVertex() == face_vertices[0]) ? bary_coords.z
                                                         : 1. - bary_coords.z;
        best_sp = SurfacePoint(e, t);
      } else if (std::abs(bary_coords.x) < epsilon) {
        Edge e = edgeBetween(face_vertices[1], face_vertices[2]);
        double t = (e.firstVertex() == face_vertices[1]) ? bary_coords.z
                                                         : 1. - bary_coords.z;
        best_sp = SurfacePoint(e, t);
      } else {
        best_sp =
            SurfacePoint(f, {bary_coords.x, bary_coords.y, bary_coords.z});
      }
    }
  }
  return best_sp;
}

ComputeGeodesicsOutput
computeGeodesics(const std::vector<uint32_t> &targetMeshIndices,
                 const std::vector<float> &targetMeshPositions,
                 const std::vector<float> &coordsToWalk,
                 const std::vector<uint32_t> &indicesToWalk, bool fullPath,
                 const std::vector<float> &startPointWorld,
                 const std::vector<float> &upDirectionWorld) {
  std::unique_ptr<geometrycentral::surface::ManifoldSurfaceMesh> targetMesh;
  std::unique_ptr<geometrycentral::surface::VertexPositionGeometry>
      targetGeometry;
  std::tie(targetMesh, targetGeometry) =
      loadMesh(targetMeshIndices, targetMeshPositions);

  surface::SurfacePoint origSurfacePoint;
  if (!startPointWorld.empty()) {
    if (startPointWorld.size() != 3) {
      throw std::invalid_argument("startPointWorld must have 3 elements");
    }
    Vector3 start_p = {startPointWorld[0], startPointWorld[1],
                       startPointWorld[2]};
    origSurfacePoint =
        findClosestPointOnMesh(start_p, *targetMesh, *targetGeometry);
  } else {
    auto startFace = targetMesh->face(0);
    origSurfacePoint = geometrycentral::surface::SurfacePoint(
        startFace, Vector3{0.3, 0.3, 0.4});
  }

  double initialAngle = 0.;
  if (!upDirectionWorld.empty()) {
    if (upDirectionWorld.size() != 3) {
      throw std::invalid_argument("upDirectionWorld must have 3 elements");
    }
    Vector3 up_dir_world = {upDirectionWorld[0], upDirectionWorld[1],
                            upDirectionWorld[2]};
    up_dir_world = up_dir_world.normalize();
    // fix any nans in the up direction
    if (std::isnan(up_dir_world.x) || std::isnan(up_dir_world.y) ||
        std::isnan(up_dir_world.z)) {
      up_dir_world = Vector3{0., 1., 0.};
    }

    origSurfacePoint = origSurfacePoint.inSomeFace();
    auto startFace = origSurfacePoint.face;

    targetGeometry->requireFaceTangentBasis();
    Vector3 normal = targetGeometry->faceNormals[startFace];
    // if the normal and up direction are almost parallel, perturb it slightly
    // to avoid nans
    if (std::abs(dot(normal, up_dir_world)) > 0.9999) {
      // perturb the up direction slightly
      Vector3 perturbation = Vector3::constant(1.).normalize();
      up_dir_world = (up_dir_world + perturbation * 1e-6).normalize();
    }
    Vector3 proj_dir = up_dir_world - dot(up_dir_world, normal) * normal;
    proj_dir = proj_dir.normalize();

    const auto &tangent_basis = targetGeometry->faceTangentBasis[startFace];
    const Vector3 &tangent_basis_x = tangent_basis[0];
    const Vector3 &tangent_basis_y = tangent_basis[1];
    initialAngle =
        atan2(dot(proj_dir, tangent_basis_y), dot(proj_dir, tangent_basis_x));
    if (std::isnan(initialAngle)) {
      initialAngle = 0.;
    }
  }

  surface::TraceOptions traceOptions;
  traceOptions.errorOnProblem = true;
  traceOptions.includePath = fullPath;

  uint32_t coordCount = coordsToWalk.size() / 2;
  ComputeGeodesicsOutput output(coordCount);
  if (!fullPath) {
    output.projectedPositions.resize(coordCount * 3);
  }

  Graph graph = buildGraph(indicesToWalk);

  Grid processedCoordsGrid(5.);

  // Start BFS from the first vertex
  std::queue<BFSQueueEntry> bfsQueue;
  std::vector<BFSQueueEntry> visited;
  visited.resize(coordCount);
  bfsQueue.push({0, origSurfacePoint, 0., 0., initialAngle, PI / 2.});

  float lastCartX = std::numeric_limits<float>::max();
  float lastCartY = std::numeric_limits<float>::max();
  float lastCartZ = std::numeric_limits<float>::max();
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
          x, y, *targetGeometry, startSurfacePoint, startX, startY,
          entry.incomingTangentSpaceAngle, entry.incoming2DAngle, traceOptions);

      if (fullPath) {
        for (const auto &pathPoint : walkOutput.pathPoints) {
          float cartX, cartY, cartZ;
          std::tie(cartX, cartY, cartZ) =
              getSurfacePointCoords(*targetGeometry, pathPoint);
          if (std::abs(cartX - lastCartX) < 1e-6 &&
              std::abs(cartY - lastCartY) < 1e-6 &&
              std::abs(cartZ - lastCartZ) < 1e-6) {
            continue;
          }
          output.projectedPositions.push_back(cartX);
          output.projectedPositions.push_back(cartY);
          output.projectedPositions.push_back(cartZ);

          lastCartX = cartX;
          lastCartY = cartY;
          lastCartZ = cartZ;
        }
      } else {
        float cartX, cartY, cartZ;
        std::tie(cartX, cartY, cartZ) =
            getSurfacePointCoords(*targetGeometry, walkOutput.pathEndpoint);
        output.projectedPositions[inVtxIx * 3 + 0] = cartX;
        output.projectedPositions[inVtxIx * 3 + 1] = cartY;
        output.projectedPositions[inVtxIx * 3 + 2] = cartZ;
      }

      auto endpoint = walkOutput.pathEndpoint;
      visited[inVtxIx] = BFSQueueEntry(inVtxIx, endpoint, x, y,
                                       walkOutput.incomingTangentSpaceAngle,
                                       walkOutput.incoming2DAngle);
      processedCoordsGrid.addPoint(x, y, inVtxIx);

      for (auto neighbor : graph[inVtxIx]) {
        if (visited[neighbor].vertexIdx != INVALID_VERTEX_IX) {
          continue;
        }
        bfsQueue.push(BFSQueueEntry(neighbor, endpoint, x, y,
                                    walkOutput.incomingTangentSpaceAngle,
                                    walkOutput.incoming2DAngle));
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

    uint32_t closestVertexIx =
        processedCoordsGrid.findClosestPoint(unvisitedX, unvisitedY);

    if (closestVertexIx == INVALID_VERTEX_IX) {
      throw std::invalid_argument("no closest vertex found");
    }

    // walk from the closest visited vertex to the unvisited vertex
    BFSQueueEntry &closestEntry = visited[closestVertexIx];
    if (closestEntry.vertexIdx != closestVertexIx) {
      throw std::invalid_argument("closest entry vertex index does not match");
    }

    auto newEntry = BFSQueueEntry(unvisitedVertexIx, closestEntry.surfacePoint,
                                  closestEntry.x, closestEntry.y,
                                  closestEntry.incomingTangentSpaceAngle,
                                  closestEntry.incoming2DAngle);
    bfsQueue.push(newEntry);
  }

  return output;
}

template <typename T> uint32_t getVecDataPtr(std::vector<T> &vec) {
  return reinterpret_cast<uint32_t>(vec.data());
}

template <typename T>
class_<std::vector<T>> register_vector_custom(const char *name) {
  typedef std::vector<T> VecType;

  // void (VecType::*push_back)(const T&) = &VecType::push_back;
  void (VecType::*resize)(const size_t, const T &) = &VecType::resize;
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
      .property("projectedPositions",
                &ComputeGeodesicsOutput::projectedPositions);

  register_vector_custom<float>("vector<float>");
  register_vector_custom<uint32_t>("vector<uint32_t>");
}
