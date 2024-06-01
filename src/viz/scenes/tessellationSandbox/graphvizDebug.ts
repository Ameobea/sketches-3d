import * as graphvizBuilder from 'graphviz-builder';

export const build_linked_mesh_graphviz = (rawConnections: string): string => {
  // Work around bug in `graphviz-builder`: https://github.com/prantlf/graphviz-builder/issues/1
  (window as any).l = undefined;

  const connections = rawConnections
    .split('\n')
    .map(line => line.trim())
    .filter(line => line.length > 0)
    .map(
      line =>
        line
          .split('::')
          .map(s =>
            s.replaceAll('VertexKey', 'Vtx').replaceAll('EdgeKey', 'Edge').replaceAll('FaceKey', 'Face')
          ) as [string, string]
    );

  const g = graphvizBuilder.digraph('G');
  const allNodes = new Set<string>();
  connections.forEach(([from, to]) => {
    if (!allNodes.has(from)) {
      allNodes.add(from);
      g.addNode(from);
    }
    if (!allNodes.has(to)) {
      allNodes.add(to);
      g.addNode(to);
    }
    g.addEdge(from, to);
  });

  const dot = g.to_dot();
  console.log(dot);
  return dot;
};
