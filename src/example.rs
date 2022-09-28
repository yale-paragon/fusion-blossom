//! Example Decoding
//! 
//! This module contains several abstract decoding graph and it's randomized simulator utilities.
//! This helps to debug, but it doesn't corresponds to real error model, nor it's capable of simulating circuit-level noise model.
//! For complex error model and simulator functionality, please see <https://github.com/yuewuo/QEC-Playground>
//! 
//! Note that these examples are not optimized for cache coherency for simplicity.
//! To maximize code efficiency, user should design how to group vertices such that memory coherency is preserved for arbitrary large code distance.
//! 

use super::visualize::*;
use super::util::*;
use std::collections::HashMap;
use crate::serde_json;
use crate::rand_xoshiro::rand_core::SeedableRng;
use crate::derivative::Derivative;
use std::fs::File;
use std::io::{self, BufRead};
use crate::rayon::prelude::*;
use super::pointers::*;


/// Vertex corresponds to a stabilizer measurement bit
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct CodeVertex {
    /// position helps to visualize
    pub position: VisualizePosition,
    /// neighbor edges helps to set find individual edge
    pub neighbor_edges: Vec<usize>,
    /// virtual vertex won't report measurement results
    pub is_virtual: bool,
    /// whether it shows up syndrome, note that virtual nodes should NOT have syndrome
    pub is_syndrome: bool,
}

/// Edge flips the measurement result of two vertices
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct CodeEdge {
    /// the two vertices incident to this edge
    pub vertices: (usize, usize),
    /// probability of flipping the results of these two vertices; do not set p to 0 to remove edge: if desired, create a new code type
    pub p: f64,
    /// probability of having a reported event of error on this edge
    pub pe: f64,
    /// the integer weight of this edge
    pub half_weight: Weight,
    /// whether this edge is erased
    pub is_erasure: bool,
}

impl CodeEdge {
    pub fn new(a: usize, b: usize) -> Self {
        Self {
            vertices: (a, b),
            p: 0.,
            pe: 0.,
            half_weight: 0,
            is_erasure: false,
        }
    }
}

/// default function for computing (pre-scaled) weight from probability
pub fn weight_of_p(p: f64) -> f64 {
    assert!((0. ..=0.5).contains(&p), "p must be a reasonable value between 0 and 50%");
    ((1. - p) / p).ln()
}

pub trait ExampleCode {

    /// get mutable references to vertices and edges
    fn vertices_edges(&mut self) -> (&mut Vec<CodeVertex>, &mut Vec<CodeEdge>);
    fn immutable_vertices_edges(&self) -> (&Vec<CodeVertex>, &Vec<CodeEdge>);

    /// get the number of vertices
    fn vertex_num(&self) -> usize { self.immutable_vertices_edges().0.len() }

    /// generic method that automatically computes integer weights from probabilities,
    /// scales such that the maximum integer weight is 10000 and the minimum is 1
    fn compute_weights(&mut self, max_half_weight: Weight) {
        let (_vertices, edges) = self.vertices_edges();
        let mut max_weight = 0.;
        for edge in edges.iter() {
            let weight = weight_of_p(edge.p);
            if weight > max_weight {
                max_weight = weight;
            }
        }
        assert!(max_weight > 0., "max weight is not expected to be 0.");
        // scale all weights but set the smallest to 1
        for edge in edges.iter_mut() {
            let weight = weight_of_p(edge.p);
            let half_weight: Weight = ((max_half_weight as f64) * weight / max_weight).round() as Weight;
            edge.half_weight = if half_weight == 0 { 1 } else { half_weight };  // weight is required to be even
        }
    }

    /// sanity check to avoid duplicate edges that are hard to debug
    fn sanity_check(&self) -> Result<(), String> {
        let (vertices, edges) = self.immutable_vertices_edges();
        // check the graph is reasonable
        if vertices.is_empty() || edges.is_empty() {
            return Err("empty graph".to_string());
        }
        // check duplicated edges
        let mut existing_edges = HashMap::<(usize, usize), usize>::with_capacity(edges.len() * 2);
        for (idx, edge) in edges.iter().enumerate() {
            let (v1, v2) = edge.vertices;
            let unique_edge = if v1 < v2 { (v1, v2) } else { (v2, v1) };
            if existing_edges.contains_key(&unique_edge) {
                let previous_idx = existing_edges[&unique_edge];
                return Err(format!("duplicate edge {} and {} with incident vertices {} and {}", previous_idx, idx, v1, v2));
            }
            existing_edges.insert(unique_edge, idx);
        }
        // check duplicated referenced edge from each vertex
        for (vertex_idx, vertex) in vertices.iter().enumerate() {
            let mut existing_edges = HashMap::<usize, ()>::new();
            if vertex.neighbor_edges.is_empty() {
                return Err(format!("vertex {} do not have any neighbor edges", vertex_idx));
            }
            for edge_idx in vertex.neighbor_edges.iter() {
                if existing_edges.contains_key(edge_idx) {
                    return Err(format!("duplicate referred edge {} from vertex {}", edge_idx, vertex_idx));
                }
                existing_edges.insert(*edge_idx, ());
            }
        }
        Ok(())
    }

    /// set probability of all edges; user can set individual probabilities
    fn set_probability(&mut self, p: f64) {
        let (_vertices, edges) = self.vertices_edges();
        for edge in edges.iter_mut() {
            edge.p = p;
        }
    }

    /// set erasure probability of all edges; user can set individual probabilities
    fn set_erasure_probability(&mut self, pe: f64) {
        let (_vertices, edges) = self.vertices_edges();
        for edge in edges.iter_mut() {
            edge.pe = pe;
        }
    }

    /// automatically create vertices given edges
    fn fill_vertices(&mut self, vertex_num: usize) {
        let (vertices, edges) = self.vertices_edges();
        vertices.clear();
        vertices.reserve(vertex_num);
        for _ in 0..vertex_num {
            vertices.push(CodeVertex {
                position: VisualizePosition::new(0., 0., 0.),
                neighbor_edges: Vec::new(),
                is_virtual: false,
                is_syndrome: false,
            });
        }
        for (edge_idx, edge) in edges.iter().enumerate() {
            let vertex_1 = &mut vertices[edge.vertices.0];
            vertex_1.neighbor_edges.push(edge_idx);
            let vertex_2 = &mut vertices[edge.vertices.1];
            vertex_2.neighbor_edges.push(edge_idx);
        }
    }

    /// gather all positions of vertices
    fn get_positions(&self) -> Vec<VisualizePosition> {
        let (vertices, _edges) = self.immutable_vertices_edges();
        let mut positions = Vec::with_capacity(vertices.len());
        for vertex in vertices.iter() {
            positions.push(vertex.position.clone());
        }
        positions
    }

    /// generate standard interface to instantiate Fusion blossom solver
    fn get_initializer(&self) -> SolverInitializer {
        let (vertices, edges) = self.immutable_vertices_edges();
        let vertex_num = vertices.len();
        let mut weighted_edges = Vec::with_capacity(edges.len());
        for edge in edges.iter() {
            weighted_edges.push((edge.vertices.0, edge.vertices.1, edge.half_weight * 2));
        }
        let mut virtual_vertices = Vec::new();
        for (vertex_idx, vertex) in vertices.iter().enumerate() {
            if vertex.is_virtual {
                virtual_vertices.push(vertex_idx);
            }
        }
        SolverInitializer {
            vertex_num,
            weighted_edges,
            virtual_vertices,
        }
    }

    /// set syndrome vertices
    fn set_syndrome_vertices(&mut self, syndrome_vertices: &[VertexIndex]) {
        let (vertices, _edges) = self.vertices_edges();
        for vertex in vertices.iter_mut() {
            vertex.is_syndrome = false;
        }
        for vertex_idx in syndrome_vertices.iter() {
            let vertex = &mut vertices[*vertex_idx];
            vertex.is_syndrome = true;
        }
    }

    /// set erasure edges
    fn set_erasures(&mut self, erasures: &[EdgeIndex]) {
        let (_vertices, edges) = self.vertices_edges();
        for edge in edges.iter_mut() {
            edge.is_erasure = false;
        }
        for edge_idx in erasures.iter() {
            let edge = &mut edges[*edge_idx];
            edge.is_erasure = true;
        }
    }

    /// set syndrome
    fn set_syndrome(&mut self, syndrome_pattern: &SyndromePattern) {
        self.set_syndrome_vertices(&syndrome_pattern.syndrome_vertices);
        self.set_erasures(&syndrome_pattern.erasures);
    }

    /// get current syndrome vertices
    fn get_syndrome_vertices(&self) -> Vec<VertexIndex> {
        let (vertices, _edges) = self.immutable_vertices_edges();
        let mut syndrome = Vec::new();
        for (vertex_idx, vertex) in vertices.iter().enumerate() {
            if vertex.is_syndrome {
                syndrome.push(vertex_idx);
            }
        }
        syndrome
    }

    /// get current erasure edges
    fn get_erasures(&self) -> Vec<EdgeIndex> {
        let (_vertices, edges) = self.immutable_vertices_edges();
        let mut erasures = Vec::new();
        for (edge_idx, edge) in edges.iter().enumerate() {
            if edge.is_erasure {
                erasures.push(edge_idx);
            }
        }
        erasures
    }

    /// get current syndrome
    fn get_syndrome(&self) -> SyndromePattern {
        SyndromePattern::new(self.get_syndrome_vertices(), self.get_erasures())
    }

    /// generate random errors based on the edge probabilities and a seed for pseudo number generator
    fn generate_random_errors(&mut self, seed: u64) -> SyndromePattern {
        let mut rng = DeterministicRng::seed_from_u64(seed);
        let (vertices, edges) = self.vertices_edges();
        for vertex in vertices.iter_mut() {
            vertex.is_syndrome = false;
        }
        for edge in edges.iter_mut() {
            let p = if rng.next_f64() < edge.pe {
                edge.is_erasure = true;
                0.5  // when erasure happens, there are 50% chance of error
            } else {
                edge.is_erasure = false;
                edge.p
            };
            if rng.next_f64() < p {
                let (v1, v2) = edge.vertices;
                let vertex_1 = &mut vertices[v1];
                if !vertex_1.is_virtual {
                    vertex_1.is_syndrome = !vertex_1.is_syndrome;
                }
                let vertex_2 = &mut vertices[v2];
                if !vertex_2.is_virtual {
                    vertex_2.is_syndrome = !vertex_2.is_syndrome;
                }
            }
        }
        self.get_syndrome()
    }

    fn is_virtual(&self, vertex_idx: usize) -> bool {
        let (vertices, _edges) = self.immutable_vertices_edges();
        vertices[vertex_idx].is_virtual
    }

    fn is_syndrome(&self, vertex_idx: usize) -> bool {
        let (vertices, _edges) = self.immutable_vertices_edges();
        vertices[vertex_idx].is_syndrome
    }

    /// reorder the vertices such that new vertices (the indices of the old order) is sequential
    fn reorder_vertices(&mut self, sequential_vertices: &Vec<VertexIndex>) {
        let (vertices, edges) = self.vertices_edges();
        assert_eq!(vertices.len(), sequential_vertices.len(), "amount of vertices must be same");
        let old_to_new = build_old_to_new(sequential_vertices);
        // change the vertices numbering
        *vertices = (0..vertices.len()).map(|new_index| {
            vertices[sequential_vertices[new_index]].clone()
        }).collect();
        for edge in edges.iter_mut() {
            let (old_left, old_right) = edge.vertices;
            edge.vertices = (old_to_new[old_left].unwrap(), old_to_new[old_right].unwrap());
        }
    }

}

impl<T> FusionVisualizer for T where T: ExampleCode {
    fn snapshot(&self, abbrev: bool) -> serde_json::Value {
        let (self_vertices, self_edges) = self.immutable_vertices_edges();
        let mut vertices = Vec::<serde_json::Value>::new();
        for vertex in self_vertices.iter() {
            vertices.push(json!({
                if abbrev { "v" } else { "is_virtual" }: i32::from(vertex.is_virtual),
                if abbrev { "s" } else { "is_syndrome" }: i32::from(vertex.is_syndrome),
            }));
        }
        let mut edges = Vec::<serde_json::Value>::new();
        for edge in self_edges.iter() {
            edges.push(json!({
                if abbrev { "w" } else { "weight" }: edge.half_weight * 2,
                if abbrev { "l" } else { "left" }: edge.vertices.0,
                if abbrev { "r" } else { "right" }: edge.vertices.1,
                // code itself is not capable of calculating growth
            }));
        }
        json!({
            "vertices": vertices,  // TODO: update HTML code to use the same language
            "edges": edges,
        })
    }
}

/// perfect quantum repetition code
#[derive(Clone)]
pub struct CodeCapacityRepetitionCode {
    /// vertices in the code
    pub vertices: Vec<CodeVertex>,
    /// nearest-neighbor edges in the decoding graph
    pub edges: Vec<CodeEdge>,
}

impl ExampleCode for CodeCapacityRepetitionCode {
    fn vertices_edges(&mut self) -> (&mut Vec<CodeVertex>, &mut Vec<CodeEdge>) { (&mut self.vertices, &mut self.edges) }
    fn immutable_vertices_edges(&self) -> (&Vec<CodeVertex>, &Vec<CodeEdge>) { (&self.vertices, &self.edges) }
}

impl CodeCapacityRepetitionCode {

    pub fn new(d: usize, p: f64, max_half_weight: Weight) -> Self {
        let mut code = Self::create_code(d);
        code.set_probability(p);
        code.compute_weights(max_half_weight);
        code
    }

    pub fn create_code(d: usize) -> Self {
        assert!(d >= 3 && d % 2 == 1, "d must be odd integer >= 3");
        let vertex_num = (d - 1) + 2;  // two virtual vertices at left and right
        // create edges
        let mut edges = Vec::new();
        for i in 0..d-1 {
            edges.push(CodeEdge::new(i, i+1));
        }
        edges.push(CodeEdge::new(0, d));  // tje left-most edge
        let mut code = Self {
            vertices: Vec::new(),
            edges,
        };
        // create vertices
        code.fill_vertices(vertex_num);
        code.vertices[d-1].is_virtual = true;
        code.vertices[d].is_virtual = true;
        let mut positions = Vec::new();
        for i in 0..d {
            positions.push(VisualizePosition::new(0., i as f64, 0.));
        }
        positions.push(VisualizePosition::new(0., -1., 0.));
        for (i, position) in positions.into_iter().enumerate() {
            code.vertices[i].position = position;
        }
        code
    }

}

/// code capacity noise model is a single measurement round with perfect stabilizer measurements;
/// e.g. this is the decoding graph of a CSS surface code (standard one, not rotated one) with X-type stabilizers
#[derive(Clone)]
pub struct CodeCapacityPlanarCode {
    /// vertices in the code
    pub vertices: Vec<CodeVertex>,
    /// nearest-neighbor edges in the decoding graph
    pub edges: Vec<CodeEdge>,
}

impl ExampleCode for CodeCapacityPlanarCode {
    fn vertices_edges(&mut self) -> (&mut Vec<CodeVertex>, &mut Vec<CodeEdge>) { (&mut self.vertices, &mut self.edges) }
    fn immutable_vertices_edges(&self) -> (&Vec<CodeVertex>, &Vec<CodeEdge>) { (&self.vertices, &self.edges) }
}

impl CodeCapacityPlanarCode {

    pub fn new(d: usize, p: f64, max_half_weight: Weight) -> Self {
        let mut code = Self::create_code(d);
        code.set_probability(p);
        code.compute_weights(max_half_weight);
        code
    }

    pub fn create_code(d: usize) -> Self {
        assert!(d >= 3 && d % 2 == 1, "d must be odd integer >= 3");
        let row_vertex_num = (d-1) + 2;  // two virtual nodes at left and right
        let vertex_num = row_vertex_num * d;  // `d` rows
        // create edges
        let mut edges = Vec::new();
        for row in 0..d {
            let bias = row * row_vertex_num;
            for i in 0..d-1 {
                edges.push(CodeEdge::new(bias + i, bias + i+1));
            }
            edges.push(CodeEdge::new(bias, bias + d));  // left most edge
            if row + 1 < d {
                for i in 0..d-1 {
                    edges.push(CodeEdge::new(bias + i, bias + i + row_vertex_num));
                }
            }
        }
        let mut code = Self {
            vertices: Vec::new(),
            edges,
        };
        // create vertices
        code.fill_vertices(vertex_num);
        for row in 0..d {
            let bias = row * row_vertex_num;
            code.vertices[bias + d - 1].is_virtual = true;
            code.vertices[bias + d].is_virtual = true;
        }
        let mut positions = Vec::new();
        for row in 0..d {
            let pos_i = row as f64;
            for i in 0..d {
                positions.push(VisualizePosition::new(pos_i, i as f64, 0.));
            }
            positions.push(VisualizePosition::new(pos_i, -1., 0.));
        }
        for (i, position) in positions.into_iter().enumerate() {
            code.vertices[i].position = position;
        }
        code
    }

}

/// phenomenological noise model is multiple measurement rounds adding only measurement errors
/// e.g. this is the decoding graph of a CSS surface code (standard one, not rotated one) with X-type stabilizers
#[derive(Clone)]
pub struct PhenomenologicalPlanarCode {
    /// vertices in the code
    pub vertices: Vec<CodeVertex>,
    /// nearest-neighbor edges in the decoding graph
    pub edges: Vec<CodeEdge>,
}

impl ExampleCode for PhenomenologicalPlanarCode {
    fn vertices_edges(&mut self) -> (&mut Vec<CodeVertex>, &mut Vec<CodeEdge>) { (&mut self.vertices, &mut self.edges) }
    fn immutable_vertices_edges(&self) -> (&Vec<CodeVertex>, &Vec<CodeEdge>) { (&self.vertices, &self.edges) }
}

impl PhenomenologicalPlanarCode {

    pub fn new(d: usize, noisy_measurements: usize, p: f64, max_half_weight: Weight) -> Self {
        let mut code = Self::create_code(d, noisy_measurements);
        code.set_probability(p);
        code.compute_weights(max_half_weight);
        code
    }

    pub fn create_code(d: usize, noisy_measurements: usize) -> Self {
        assert!(d >= 3 && d % 2 == 1, "d must be odd integer >= 3");
        let row_vertex_num = (d-1) + 2;  // two virtual nodes at left and right
        let t_vertex_num = row_vertex_num * d;  // `d` rows
        let td = noisy_measurements + 1;  // a perfect measurement round is capped at the end
        let vertex_num = t_vertex_num * td;  // `td` layers
        // create edges
        let mut edges = Vec::new();
        for t in 0..td {
            let t_bias = t * t_vertex_num;
            for row in 0..d {
                let bias = t_bias + row * row_vertex_num;
                for i in 0..d-1 {
                    edges.push(CodeEdge::new(bias + i, bias + i+1));
                }
                edges.push(CodeEdge::new(bias, bias + d));  // left most edge
                if row + 1 < d {
                    for i in 0..d-1 {
                        edges.push(CodeEdge::new(bias + i, bias + i + row_vertex_num));
                    }
                }
            }
            // inter-layer connection
            if t + 1 < td {
                for row in 0..d {
                    let bias = t_bias + row * row_vertex_num;
                    for i in 0..d-1 {
                        edges.push(CodeEdge::new(bias + i, bias + i + t_vertex_num));
                    }
                }
            }
        }
        let mut code = Self {
            vertices: Vec::new(),
            edges,
        };
        // create vertices
        code.fill_vertices(vertex_num);
        for t in 0..td {
            let t_bias = t * t_vertex_num;
            for row in 0..d {
                let bias = t_bias + row * row_vertex_num;
                code.vertices[bias + d - 1].is_virtual = true;
                code.vertices[bias + d].is_virtual = true;
            }
        }
        let mut positions = Vec::new();
        for t in 0..td {
            let pos_t = t as f64;
            for row in 0..d {
                let pos_i = row as f64;
                for i in 0..d {
                    positions.push(VisualizePosition::new(pos_i, i as f64 + 0.5, pos_t));
                }
                positions.push(VisualizePosition::new(pos_i, -1. + 0.5, pos_t));
            }
        }
        for (i, position) in positions.into_iter().enumerate() {
            code.vertices[i].position = position;
        }
        code
    }

}

/// (not accurate) circuit-level noise model is multiple measurement rounds with errors between each two-qubit gates
/// e.g. this is the decoding graph of a CSS surface code (standard one, not rotated one) with X-type stabilizers
#[derive(Clone)]
pub struct CircuitLevelPlanarCode {
    /// vertices in the code
    pub vertices: Vec<CodeVertex>,
    /// nearest-neighbor edges in the decoding graph
    pub edges: Vec<CodeEdge>,
}

impl ExampleCode for CircuitLevelPlanarCode {
    fn vertices_edges(&mut self) -> (&mut Vec<CodeVertex>, &mut Vec<CodeEdge>) { (&mut self.vertices, &mut self.edges) }
    fn immutable_vertices_edges(&self) -> (&Vec<CodeVertex>, &Vec<CodeEdge>) { (&self.vertices, &self.edges) }
}

impl CircuitLevelPlanarCode {

    /// by default diagonal edge has error rate p/3 to mimic the behavior of unequal weights
    pub fn new(d: usize, noisy_measurements: usize, p: f64, max_half_weight: Weight) -> Self {
        Self::new_diagonal(d, noisy_measurements, p, max_half_weight, p/3.)
    }

    pub fn new_diagonal(d: usize, noisy_measurements: usize, p: f64, max_half_weight: Weight, diagonal_p: f64) -> Self {
        let mut code = Self::create_code(d, noisy_measurements);
        code.set_probability(p);
        if diagonal_p != p {
            let (vertices, edges) = code.vertices_edges();
            for edge in edges.iter_mut() {
                let (v1, v2) = edge.vertices;
                let v1p = &vertices[v1].position;
                let v2p = &vertices[v2].position;
                let manhattan_distance = (v1p.i - v2p.i).abs() + (v1p.j - v2p.j).abs() + (v1p.t - v2p.t).abs();
                if manhattan_distance > 1. {
                    edge.p = diagonal_p;
                }
            }
        }
        code.compute_weights(max_half_weight);
        code
    }

    pub fn create_code(d: usize, noisy_measurements: usize) -> Self {
        assert!(d >= 3 && d % 2 == 1, "d must be odd integer >= 3");
        let row_vertex_num = (d-1) + 2;  // two virtual nodes at left and right
        let t_vertex_num = row_vertex_num * d;  // `d` rows
        let td = noisy_measurements + 1;  // a perfect measurement round is capped at the end
        let vertex_num = t_vertex_num * td;  // `td` layers
        // create edges
        let mut edges = Vec::new();
        for t in 0..td {
            let t_bias = t * t_vertex_num;
            for row in 0..d {
                let bias = t_bias + row * row_vertex_num;
                for i in 0..d-1 {
                    edges.push(CodeEdge::new(bias + i, bias + i+1));
                }
                edges.push(CodeEdge::new(bias, bias + d));  // left most edge
                if row + 1 < d {
                    for i in 0..d-1 {
                        edges.push(CodeEdge::new(bias + i, bias + i + row_vertex_num));
                    }
                }
            }
            // inter-layer connection
            if t + 1 < td {
                for row in 0..d {
                    let bias = t_bias + row * row_vertex_num;
                    for i in 0..d-1 {
                        edges.push(CodeEdge::new(bias + i, bias + i + t_vertex_num));
                        let diagonal_diffs: Vec<(isize, isize)> = vec![(0, 1), (1, 0), (1, 1)];
                        for (di, dj) in diagonal_diffs {
                            let new_row = row as isize + di;  // row corresponds to `i`
                            let new_i = i as isize + dj;  // i corresponds to `j`
                            if new_row >= 0 && new_i >= 0 && new_row < d as isize && new_i < (d-1) as isize {
                                let new_bias = t_bias + (new_row as usize) * row_vertex_num + t_vertex_num;
                                edges.push(CodeEdge::new(bias + i, new_bias + new_i as usize));
                            }
                        }
                    }
                }
            }
        }
        let mut code = Self {
            vertices: Vec::new(),
            edges,
        };
        // create vertices
        code.fill_vertices(vertex_num);
        for t in 0..td {
            let t_bias = t * t_vertex_num;
            for row in 0..d {
                let bias = t_bias + row * row_vertex_num;
                code.vertices[bias + d - 1].is_virtual = true;
                code.vertices[bias + d].is_virtual = true;
            }
        }
        let mut positions = Vec::new();
        for t in 0..td {
            let pos_t = t as f64;
            for row in 0..d {
                let pos_i = row as f64;
                for i in 0..d {
                    positions.push(VisualizePosition::new(pos_i, i as f64 + 0.5, pos_t));
                }
                positions.push(VisualizePosition::new(pos_i, -1. + 0.5, pos_t));
            }
        }
        for (i, position) in positions.into_iter().enumerate() {
            code.vertices[i].position = position;
        }
        code
    }

}

/// read from file, including the error patterns;
/// the point is to avoid bad cache performance, because generating random error requires iterating over a large memory space,
/// invalidating all cache. also, this can reduce the time of decoding by prepare the data before hand and could be shared between
/// different partition configurations
pub struct ErrorPatternReader {
    /// vertices in the code
    pub vertices: Vec<CodeVertex>,
    /// nearest-neighbor edges in the decoding graph
    pub edges: Vec<CodeEdge>,
    /// pre-generated syndrome patterns
    pub syndrome_patterns: Vec<SyndromePattern>,
    /// cursor of current errors
    pub syndrome_index: usize,
}

impl ExampleCode for ErrorPatternReader {
    fn vertices_edges(&mut self) -> (&mut Vec<CodeVertex>, &mut Vec<CodeEdge>) { (&mut self.vertices, &mut self.edges) }
    fn immutable_vertices_edges(&self) -> (&Vec<CodeVertex>, &Vec<CodeEdge>) { (&self.vertices, &self.edges) }
    fn generate_random_errors(&mut self, _seed: u64) -> SyndromePattern {
        assert!(self.syndrome_index < self.syndrome_patterns.len(), "reading syndrome pattern more than in the file, consider generate the file with more data points");
        let syndrome_pattern = self.syndrome_patterns[self.syndrome_index].clone();
        self.syndrome_index += 1;
        syndrome_pattern
    }
}

impl ErrorPatternReader {

    pub fn new(mut config: serde_json::Value) -> Self {
        let mut filename = "tmp/syndrome_patterns.txt".to_string();
        let config = config.as_object_mut().expect("config must be JSON object");
        if let Some(value) = config.remove("filename") {
            filename = value.as_str().expect("filename string").to_string();
        }
        if !config.is_empty() { panic!("unknown config keys: {:?}", config.keys().collect::<Vec<&String>>()); }
        let file = File::open(filename).unwrap();
        let mut syndrome_patterns = vec![];
        let mut initializer: Option<SolverInitializer> = None;
        let mut positions: Option<Vec<VisualizePosition>> = None;
        for (line_index, line) in io::BufReader::new(file).lines().enumerate() {
            if let Ok(value) = line {
                match line_index {
                    0 => {
                        assert!(value.starts_with("Syndrome Pattern v1.0 "), "incompatible file version");
                    },
                    1 => {
                        initializer = Some(serde_json::from_str(&value).unwrap());
                    },
                    2 => {
                        positions = Some(serde_json::from_str(&value).unwrap());
                    },
                    _ => {
                        let syndrome_pattern: SyndromePattern = serde_json::from_str(&value).unwrap();
                        syndrome_patterns.push(syndrome_pattern);
                    }
                }
            }
        }
        let initializer = initializer.expect("initializer not present in file");
        let positions = positions.expect("positions not present in file");
        assert_eq!(positions.len(), initializer.vertex_num);
        let mut code = Self {
            vertices: Vec::with_capacity(initializer.vertex_num),
            edges: Vec::with_capacity(initializer.weighted_edges.len()),
            syndrome_patterns,
            syndrome_index: 0,
        };
        for (left_vertex, right_vertex, weight) in initializer.weighted_edges.iter() {
            assert!(weight % 2 == 0, "weight must be even number");
            code.edges.push(CodeEdge {
                vertices: (*left_vertex, *right_vertex),
                p: 0.,  // doesn't matter
                pe: 0.,  // doesn't matter
                half_weight: weight / 2,
                is_erasure: false,  // doesn't matter
            });
        }
        // automatically create the vertices and nearest-neighbor connection
        code.fill_vertices(initializer.vertex_num);
        // set virtual vertices and positions
        for (vertex_index, position) in positions.into_iter().enumerate() {
            code.vertices[vertex_index].position = position;
        }
        for vertex_index in initializer.virtual_vertices {
            code.vertices[vertex_index].is_virtual = true;
        }
        code
    }

}

/// generate error patterns in parallel by hold multiple instances of the same code type
pub struct ExampleCodeParallel<CodeType: ExampleCode + Sync + Send + Clone> {
    /// used to provide graph
    pub example: CodeType,
    /// list of codes
    pub codes: Vec<ArcRwLock<CodeType>>,
    /// syndrome patterns generated by individual code
    pub syndrome_patterns: Vec<SyndromePattern>,
    /// currently using code
    pub code_index: usize,
}

impl<CodeType: ExampleCode + Sync + Send + Clone> ExampleCodeParallel<CodeType> {
    pub fn new(example: CodeType, code_count: usize) -> Self {
        let mut codes = vec![];
        for _ in 0..code_count {
            codes.push(ArcRwLock::<CodeType>::new_value(example.clone()));
        }
        Self {
            example,
            codes,
            syndrome_patterns: vec![],
            code_index: 0,
        }
    }
}

impl<CodeType: ExampleCode + Sync + Send + Clone> ExampleCode for ExampleCodeParallel<CodeType> {
    fn vertices_edges(&mut self) -> (&mut Vec<CodeVertex>, &mut Vec<CodeEdge>) { self.example.vertices_edges() }
    fn immutable_vertices_edges(&self) -> (&Vec<CodeVertex>, &Vec<CodeEdge>) { self.example.immutable_vertices_edges() }
    fn generate_random_errors(&mut self, seed: u64) -> SyndromePattern {
        if self.code_index == 0 {
            // run generator in parallel
            (0..self.codes.len()).into_par_iter().map(|code_index| {
                self.codes[code_index].write().generate_random_errors(seed + (code_index * 1_000_000_000) as u64)
            }).collect_into_vec(&mut self.syndrome_patterns);
        }
        let syndrome_pattern = self.syndrome_patterns[self.code_index].clone();
        self.code_index = (self.code_index + 1) % self.codes.len();
        syndrome_pattern
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn visualize_code(code: &mut impl ExampleCode, visualize_filename: String) {
        print_visualize_link(&visualize_filename);
        let mut visualizer = Visualizer::new(Some(visualize_data_folder() + visualize_filename.as_str())).unwrap();
        visualizer.set_positions(code.get_positions(), true);  // automatic center all nodes
        visualizer.snapshot(format!("code"), code).unwrap();
        for round in 0..3 {
            code.generate_random_errors(round);
            visualizer.snapshot(format!("syndrome {}", round + 1), code).unwrap();
        }
    }

    #[test]
    fn example_code_capacity_repetition_code() {  // cargo test example_code_capacity_repetition_code -- --nocapture
        let mut code = CodeCapacityRepetitionCode::new(7, 0.2, 500);
        code.sanity_check().unwrap();
        visualize_code(&mut code, format!("example_code_capacity_repetition_code.json"));
    }

    #[test]
    fn example_code_capacity_planar_code() {  // cargo test example_code_capacity_planar_code -- --nocapture
        let mut code = CodeCapacityPlanarCode::new(7, 0.1, 500);
        code.sanity_check().unwrap();
        visualize_code(&mut code, format!("example_code_capacity_planar_code.json"));
    }

    #[test]
    fn example_phenomenological_planar_code() {  // cargo test example_phenomenological_planar_code -- --nocapture
        let mut code = PhenomenologicalPlanarCode::new(7, 7, 0.01, 500);
        code.sanity_check().unwrap();
        visualize_code(&mut code, format!("example_phenomenological_planar_code.json"));
    }

    #[test]
    fn example_circuit_level_planar_code() {  // cargo test example_circuit_level_planar_code -- --nocapture
        let mut code = CircuitLevelPlanarCode::new(7, 7, 0.01, 500);
        code.sanity_check().unwrap();
        visualize_code(&mut code, format!("example_circuit_level_planar_code.json"));
    }

}
