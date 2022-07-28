use super::util::*;
use crate::priority_queue::PriorityQueue;
use std::collections::BTreeMap;


/// build complete graph out of skeleton graph using Dijkstra's algorithm
#[derive(Debug, Clone)]
pub struct CompleteGraph {
    /// number of vertices
    pub vertex_num: usize,
    /// the vertices to run Dijkstra's algorithm
    pub vertices: Vec<CompleteGraphVertex>,
    /// timestamp to invalidate all vertices without iterating them; only invalidating all vertices individually when active_timestamp is usize::MAX
    active_timestamp: FastClearTimestamp,
}

#[derive(Debug, Clone)]
pub struct CompleteGraphVertex {
    /// all skeleton graph edges connected to this vertex
    pub edges: BTreeMap<EdgeIndex, Weight>,
    /// timestamp for Dijkstra's algorithm
    timestamp: FastClearTimestamp,
}

impl CompleteGraph {
    /// create complete graph given skeleton graph
    pub fn new(vertex_num: usize, weighted_edges: &Vec<(usize, usize, Weight)>) -> Self {
        let mut vertices: Vec<CompleteGraphVertex> = (0..vertex_num).map(|_| CompleteGraphVertex { edges: BTreeMap::new(), timestamp: 0, }).collect();
        for &(i, j, weight) in weighted_edges.iter() {
            vertices[i].edges.insert(j, weight);
            vertices[j].edges.insert(i, weight);
        }
        Self {
            vertex_num: vertex_num,
            vertices: vertices,
            active_timestamp: 0,
        }
    }

    /// invalidate Dijkstra's algorithm state from previous call
    pub fn invalidate_previous_dijkstra(&mut self) -> usize {
        if self.active_timestamp == FastClearTimestamp::MAX {  // rarely happens
            self.active_timestamp = 0;
            for i in 0..self.vertex_num {
                self.vertices[i].timestamp = 0;  // refresh all timestamps to avoid conflicts
            }
        }
        self.active_timestamp += 1;  // implicitly invalidate all vertices
        self.active_timestamp
    }

    /// get all complete graph edges from the specific vertex, but will terminate if `terminate` vertex is found
    pub fn all_edges_with_terminate(&mut self, vertex: usize, terminate: usize) -> BTreeMap<usize, (usize, Weight)> {
        let active_timestamp = self.invalidate_previous_dijkstra();
        let mut pq = PriorityQueue::<usize, PriorityElement>::new();
        pq.push(vertex, PriorityElement::new(0, vertex));
        let mut computed_edges = BTreeMap::<usize, (usize, Weight)>::new();  // { peer: (previous, weight) }
        loop {  // until no more elements
            if pq.len() == 0 {
                break
            }
            let (target, PriorityElement { weight, previous }) = pq.pop().unwrap();
            // eprintln!("target: {}, weight: {}, next: {}", target, weight, next);
            debug_assert!({
                !computed_edges.contains_key(&target)  // this entry shouldn't have been set
            });
            // update entry
            self.vertices[target].timestamp = active_timestamp;  // mark as visited
            if target != vertex {
                computed_edges.insert(target, (previous, weight));
                if target == terminate {
                    break  // early terminate
                }
            }
            // add its neighbors to priority queue
            for (&neighbor, &neighbor_weight) in self.vertices[target].edges.iter() {
                let edge_weight = weight + neighbor_weight;
                if let Some(PriorityElement { weight: existing_weight, previous: existing_previous }) = pq.get_priority(&neighbor) {
                    // update the priority if weight is smaller or weight is equal but distance is smaller
                    // this is necessary if the graph has weight-0 edges, which could lead to cycles in the graph and cause deadlock
                    let mut update = &edge_weight < existing_weight;
                    if &edge_weight == existing_weight {
                        let distance = if neighbor > previous { neighbor - previous } else { previous - neighbor };
                        let existing_distance = if &neighbor > existing_previous { neighbor - existing_previous } else { existing_previous - neighbor };
                        // prevent loop by enforcing strong non-descending
                        if distance < existing_distance || (distance == existing_distance && &previous < existing_previous) {
                            update = true;
                        }
                    }
                    if update {
                        pq.change_priority(&neighbor, PriorityElement::new(edge_weight, target));
                    }
                } else {  // insert new entry only if neighbor has not been visited
                    if self.vertices[neighbor].timestamp != active_timestamp {
                        pq.push(neighbor, PriorityElement::new(edge_weight, target));
                    }
                }
            }
        }
        // println!("[debug] computed_edges: {:?}", computed_edges);
        computed_edges
    }

    /// get all complete graph edges from the specific vertex
    pub fn all_edges(&mut self, vertex: VertexIndex) -> BTreeMap<usize, (usize, Weight)> {
        self.all_edges_with_terminate(vertex, VertexIndex::MAX)
    }

    /// get minimum-weight path between any two vertices `a` and `b`, in the order `a -> path[0].0 -> path[1].0 -> .... -> path[-1].0` and it's guaranteed that path[-1].0 = b
    pub fn get_path(&mut self, a: VertexIndex, b: VertexIndex) -> (Vec<(VertexIndex, Weight)>, Weight) {
        assert_ne!(a, b, "cannot get path between the same vertex");
        let edges = self.all_edges_with_terminate(a, b);
        // println!("edges: {:?}", edges);
        let mut vertex = b;
        let mut path = Vec::new();
        loop {
            if vertex == a {
                break
            }
            let &(previous, weight) = &edges[&vertex];
            path.push((vertex, weight));
            if path.len() > 1 {
                let previous_index = path.len() - 2;
                path[previous_index].1 -= weight;
            }
            vertex = previous;
        }
        path.reverse();
        (path, edges[&b].1)
    }
}

#[derive(Eq, Debug)]
pub struct PriorityElement {
    pub weight: Weight,
    pub previous: usize,
}

impl std::cmp::PartialEq for PriorityElement {
    #[inline]
    fn eq(&self, other: &PriorityElement) -> bool {
        self.weight == other.weight
    }
}

impl std::cmp::PartialOrd for PriorityElement {
    #[inline]
    fn partial_cmp(&self, other: &PriorityElement) -> Option<std::cmp::Ordering> {
        other.weight.partial_cmp(&self.weight)  // reverse `self` and `other` to prioritize smaller weight
    }
}

impl std::cmp::Ord for PriorityElement {
    #[inline]
    fn cmp(&self, other: &PriorityElement) -> std::cmp::Ordering {
        other.weight.cmp(&self.weight)  // reverse `self` and `other` to prioritize smaller weight
    }
}

impl PriorityElement {
    pub fn new(weight: Weight, previous: usize) -> Self {
        Self {
            weight: weight,
            previous: previous,
        }
    }
}
