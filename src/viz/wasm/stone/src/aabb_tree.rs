use std::{num::NonZeroUsize, ops::ControlFlow};

#[derive(Clone, Copy, Debug)]
pub struct AABB {
  pub min: [f32; 2],
  pub max: [f32; 2],
}

impl AABB {
  pub fn intersects(&self, other: &AABB) -> bool {
    self.min[0] <= other.max[0]
      && self.max[0] >= other.min[0]
      && self.min[1] <= other.max[1]
      && self.max[1] >= other.min[1]
  }

  pub fn merge(a: &AABB, b: &AABB) -> AABB {
    AABB {
      min: [a.min[0].min(b.min[0]), a.min[1].min(b.min[1])],
      max: [a.max[0].max(b.max[0]), a.max[1].max(b.max[1])],
    }
  }
}

#[derive(Clone)]
enum Node {
  Leaf {
    data_index: Option<NonZeroUsize>,
    aabb: AABB,
  },
  Internal {
    left: NodeIx,
    right: NodeIx,
    aabb: AABB,
  },
}

impl Node {
  fn aabb(&self) -> &AABB {
    match self {
      Node::Leaf { aabb, .. } => aabb,
      Node::Internal { aabb, .. } => aabb,
    }
  }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct NodeIx(usize);

pub struct AABBTree<T> {
  root: NodeIx,
  nodes: Vec<Node>,
  data: Vec<T>,
  stack_scratch: Vec<NodeIx>,
}

#[allow(dead_code)]
pub struct AABBTreeDebug {
  /// (aabb, depth)
  pub internal_nodes: Vec<(AABB, usize)>,
  pub leaf_nodes: Vec<AABB>,
}

impl<T> AABBTree<T> {
  pub fn new() -> Self {
    let mut data = Vec::with_capacity(8);
    // burn first slot to facilitate `NonZeroUsize` optimization
    unsafe {
      data.set_len(1);
    }

    Self {
      root: NodeIx(0),
      nodes: vec![Node::Leaf {
        data_index: None,
        aabb: AABB {
          min: [0.0, 0.0],
          max: [0.0, 0.0],
        },
      }],
      data,
      stack_scratch: Vec::with_capacity(16),
    }
  }

  pub fn insert(&mut self, aabb: AABB, data: T) {
    let data_ix = NonZeroUsize::new(self.data.len()).unwrap();
    self.data.push(data);
    // self.insert_recursive(self.root, aabb, data_ix);
    self.insert_iterative(aabb, data_ix);
  }

  pub fn insert_iterative(&mut self, aabb: AABB, data_ix: NonZeroUsize) {
    let stack = &mut self.stack_scratch;
    stack.clear();
    stack.push(self.root);

    while let Some(node_ix) = stack.pop() {
      let node = self.nodes[node_ix.0].clone();
      match node {
        Node::Leaf {
          data_index,
          aabb: leaf_aabb,
        } => {
          let new_node = if let Some(existing_data_ix) = data_index {
            // Convert the leaf to an internal node.
            let left_ix = self.nodes.len();
            self.nodes.push(Node::Leaf {
              data_index: Some(existing_data_ix),
              aabb: leaf_aabb,
            });

            let right_ix = self.nodes.len();
            self.nodes.push(Node::Leaf {
              data_index: Some(data_ix),
              aabb,
            });

            Node::Internal {
              left: NodeIx(left_ix),
              right: NodeIx(right_ix),
              aabb: AABB::merge(&leaf_aabb, &aabb),
            }
          } else {
            // If the leaf node is empty, just set the data index and adjust the AABB.
            Node::Leaf {
              data_index: Some(data_ix),
              aabb,
            }
          };
          self.nodes[node_ix.0] = new_node;
        }
        Node::Internal {
          left,
          right,
          aabb: node_aabb,
        } => {
          let left_aabb = self.nodes[left.0].aabb();
          let right_aabb = self.nodes[right.0].aabb();

          let merged_left = AABB::merge(left_aabb, &aabb);
          let merged_right = AABB::merge(right_aabb, &aabb);

          let left_increase = (merged_left.max[0] - merged_left.min[0])
            * (merged_left.max[1] - merged_left.min[1])
            - (left_aabb.max[0] - left_aabb.min[0]) * (left_aabb.max[1] - left_aabb.min[1]);

          let right_increase = (merged_right.max[0] - merged_right.min[0])
            * (merged_right.max[1] - merged_right.min[1])
            - (right_aabb.max[0] - right_aabb.min[0]) * (right_aabb.max[1] - right_aabb.min[1]);

          if left_increase <= right_increase {
            stack.push(left);
          } else {
            stack.push(right);
          }

          // Adjust the bounding box of the current node.
          let updated_node = Node::Internal {
            left,
            right,
            aabb: AABB::merge(&node_aabb, &aabb),
          };
          self.nodes[node_ix.0] = updated_node;
        }
      }
    }
  }

  pub fn query(&mut self, aabb: &AABB, mut visitor: impl FnMut(&AABB, &T) -> ControlFlow<(), ()>) {
    if self.data.is_empty() {
      return;
    }

    let stack = &mut self.stack_scratch;
    stack.push(self.root);
    while let Some(node_ix) = stack.pop() {
      match &self.nodes[node_ix.0] {
        Node::Leaf {
          data_index,
          aabb: leaf_aabb,
        } => {
          if let Some(data_index) = data_index {
            let data = &self.data[data_index.get()];
            if aabb.intersects(leaf_aabb) {
              match visitor(leaf_aabb, data) {
                ControlFlow::Continue(()) => (),
                ControlFlow::Break(()) => break,
              }
            }
          }
        }
        Node::Internal {
          left,
          right,
          aabb: internal_aabb,
        } => {
          if !aabb.intersects(internal_aabb) {
            continue;
          }
          stack.push(*left);
          stack.push(*right);
        }
      }
    }

    stack.clear();
  }

  pub fn balance(&mut self) {
    let leaves: Vec<(AABB, NonZeroUsize)> = self
      .nodes
      .iter()
      .filter_map(|node| {
        if let Node::Leaf {
          data_index: Some(data_ix),
          aabb,
        } = node
        {
          Some((*aabb, *data_ix))
        } else {
          None
        }
      })
      .collect();

    self.nodes.clear();
    let new_root = self.build_balanced(&leaves);
    self.root = new_root;
  }

  fn build_balanced(&mut self, leaves: &[(AABB, NonZeroUsize)]) -> NodeIx {
    if leaves.is_empty() {
      let ix = self.nodes.len();
      self.nodes.push(Node::Leaf {
        data_index: None,
        aabb: AABB {
          min: [0.0, 0.0],
          max: [0.0, 0.0],
        },
      });
      return NodeIx(ix);
    } else if leaves.len() == 1 {
      let ix = self.nodes.len();
      self.nodes.push(Node::Leaf {
        data_index: Some(leaves[0].1),
        aabb: leaves[0].0,
      });
      return NodeIx(ix);
    }

    let encompassing_aabb = leaves.iter().fold(
      AABB {
        min: [f32::MAX; 2],
        max: [f32::MIN; 2],
      },
      |acc, &(aabb, _)| AABB::merge(&acc, &aabb),
    );

    // Split along the longest axis
    let axis = if encompassing_aabb.max[0] - encompassing_aabb.min[0]
      > encompassing_aabb.max[1] - encompassing_aabb.min[1]
    {
      0
    } else {
      1
    };

    // Sort the items by the centroids of their AABBs along the chosen axis
    let mut sorted_items = leaves.to_vec();
    sorted_items
      .sort_unstable_by(|&(a, _), &(b, _)| a.min[axis].partial_cmp(&b.min[axis]).unwrap());

    // Split the items into two roughly equal-sized sets
    let mid = sorted_items.len() / 2;
    let (left_items, right_items) = sorted_items.split_at(mid);

    // Recursively build the left and right subtrees
    let left = self.build_balanced(left_items);
    let right = self.build_balanced(right_items);

    let ix = self.nodes.len();
    self.nodes.push(Node::Internal {
      left,
      right,
      aabb: encompassing_aabb,
    });
    NodeIx(ix)
  }

  #[allow(dead_code)]
  pub fn debug(&self) -> AABBTreeDebug {
    let mut internal_nodes = Vec::new();
    let mut leaf_nodes = Vec::new();
    self.debug_recursive(self.root, 0, &mut internal_nodes, &mut leaf_nodes);
    AABBTreeDebug {
      internal_nodes,
      leaf_nodes,
    }
  }

  #[allow(dead_code)]
  fn debug_recursive(
    &self,
    node_ix: NodeIx,
    depth: usize,
    internal_nodes: &mut Vec<(AABB, usize)>,
    leaf_nodes: &mut Vec<AABB>,
  ) {
    let node = &self.nodes[node_ix.0];
    match node {
      Node::Leaf { aabb, .. } => {
        leaf_nodes.push(*aabb);
      }
      Node::Internal { left, right, aabb } => {
        internal_nodes.push((*aabb, depth));
        self.debug_recursive(*left, depth + 1, internal_nodes, leaf_nodes);
        self.debug_recursive(*right, depth + 1, internal_nodes, leaf_nodes);
      }
    }
  }
}
