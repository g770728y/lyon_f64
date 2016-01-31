//!
//! Y-monotone decomposition and triangulation of shapes.
//!
//! This module provides the tools to generate triangles from arbitrary shapes with connectivity
//! information (using a half-edge connectivity kernel).
//!
//! The implementation inspired by the book Computational Geometry, Algorithms And Applications 3rd edition.
//!
//! Note that a lot of the comments and variable labels in this module assume a coordinate
//! system where y is pointing downwards

use std::cmp::{Ordering, PartialOrd};
use std::collections::HashMap;
use std::mem::swap;
use std::f32::consts::PI;

use half_edge::kernel::*;
use half_edge::iterators::{ DirectedEdgeCirculator};
use half_edge::vectors::{ Position2D, Vec2, vec2_sub, directed_angle };

use tesselation::vertex_builder::{ VertexBufferBuilder };
use tesselation::polygon::*;
use tesselation::path::WindingOrder;
use tesselation::polygon_partition::{ Diagonals, partition_polygon };

use vodk_alloc::*;
use vodk_id::*;
use vodk_id::id_vector::*;

#[derive(Debug, Copy, Clone)]
enum VertexType {
    Start,
    End,
    Split,
    Merge,
    Left,
    Right,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DecompositionError {
    OpenPath,
    WrongWindingOrder,
    MissingFace,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TriangulationError {
    NotMonotone,
    InvalidPath,
    MissingFace,
}

fn is_below(a: Vec2, b: Vec2) -> bool { a.y() > b.y() || (a.y() == b.y() && a.x() > b.x()) }

fn get_vertex_type(prev: Vec2, current: Vec2, next: Vec2) -> VertexType {
    // assuming clockwise vertex_positions winding order
    let interrior_angle = directed_angle(vec2_sub(prev, current), vec2_sub(next, current));

    // If the interrior angle is exactly 0 we'll have degenerate (invisible 0-area) triangles
    // which is yucks but we can live with it for the sake of being robust against degenerate
    // inputs. So special-case them so that they don't get considered as Merge ot Split vertices
    // otherwise there can be no monotone decomposition of a shape where all points are on the
    // same line.

    if is_below(current, prev) && is_below(current, next) {
        if interrior_angle < PI && interrior_angle != 0.0 {
            return VertexType::Merge;
        } else {
            return VertexType::End;
        }
    }

    if !is_below(current, prev) && !is_below(current, next) {
        if interrior_angle < PI && interrior_angle != 0.0 {
            return VertexType::Split;
        } else {
            return VertexType::Start;
        }
    }

    if prev.y() == next.y() {
        return if prev.x() < next.x() { VertexType::Right } else { VertexType::Left };
    }
    return if prev.y() < next.y() { VertexType::Right } else { VertexType::Left };
}

fn intersect_segment_with_horizontal(a: [f32;2], b: [f32;2], y: f32) -> f32 {
    let vx = b.x() - a.x();
    let vy = b.y() - a.y();
    if vy == 0.0 {
        // If the segment is horizontal, pick the biggest x value (the right-most point).
        // That's an arbitrary decision that serves the purpose of y-monotone decomposition
        return a.x().max(b.x());
    }
    return a.x() + (y - a.y()) * vx / vy;
}

struct SweepLineBuilder<'l, P:'l+Position2D> {
    polygon: &'l ComplexPolygon,
    vertices: IdSlice<'l, VertexId, P>,
    current_vertex: Vec2
}

impl<'l, P: 'l+Position2D> SweepLineBuilder<'l, P> {
    fn vertex_position(&self, e: ComplexPointId) -> Vec2 {
        return self.vertices[self.polygon.vertex(e)].position();
    }

    fn add(&self, sweep_line: &mut Vec<ComplexPointId>, e: ComplexPointId) {
        sweep_line.push(e);
        // sort from left to right (increasing x values)
        sweep_line.sort_by(|ea, eb| {
            let a1 = self.vertex_position(*ea);
            let a2 = self.vertex_position(self.polygon.next(*ea));
            let b1 = self.vertex_position(*eb);
            let b2 = self.vertex_position(self.polygon.next(*eb));
            let xa = intersect_segment_with_horizontal(a1, a2, self.current_vertex.y());
            let xb = intersect_segment_with_horizontal(b1, b2, self.current_vertex.y());
            return xa.partial_cmp(&xb).unwrap();
        });
        //println!(" sweep status is: {:?}", sweep_line);
    }

    fn remove(&self, sweep_line: &mut Vec<ComplexPointId>, e: ComplexPointId) {
        //println!(" remove {:?} from sweep line", e);
        sweep_line.retain(|item|{ *item != e });
    }

    // Search the sweep status to find the edge directly to the right of the current vertex.
    fn find_right_of_current_vertex(&self, sweep_line: &Vec<ComplexPointId>) -> ComplexPointId {
        for &e in sweep_line {
            let a = self.vertex_position(e);
            let b = self.vertex_position(self.polygon.next(e));
            let x = intersect_segment_with_horizontal(a, b, self.current_vertex.y());
            //println!(" -- search sweep status {:?} x: {}", e, x);

            if x >= self.current_vertex.x() {
                return e;
            }
        }
        panic!("Could not find the edge directly right of e on the sweep line");
    }
}

fn connect_with_helper_if_merge_vertex(current_edge: ComplexPointId,
                                       helper_edge: ComplexPointId,
                                       helpers: &mut HashMap<ComplexPointId, (ComplexPointId, VertexType)>,
                                       diagonals: &mut Diagonals<ComplexPolygon>) {
    if let Some(&(h, VertexType::Merge)) = helpers.get(&helper_edge) {
        diagonals.add_diagonal(h, current_edge);
        //println!("      helper {:?} of {:?} is a merge vertex", h, helper_edge);
        //println!(" **** connection {:?}->{:?}", h, current_edge);
    }
}

/// Can perform y-monotone decomposition on a connectivity kernel.
///
/// This object holds on to the memory that was allocated during previous
/// decompositions in order to avoid allocating during the next decompositions
/// if possible.
pub struct DecompositionContext {
    sorted_edges_storage: Allocation,
    // list of edges that intercept the sweep line, sorted by increasing x coordinate
    sweep_state_storage: Allocation,
    helper: HashMap<ComplexPointId, (ComplexPointId, VertexType)>,
}

impl DecompositionContext {
    pub fn new() -> DecompositionContext {
        DecompositionContext {
            sorted_edges_storage: Allocation::empty(),
            sweep_state_storage: Allocation::empty(),
            helper: HashMap::new(),
        }
    }

    /// Applies an y_monotone decomposition of a face in a connectivity kernel.
    ///
    /// This operation will add faces and edges to the connectivity kernel.
    pub fn y_monotone_polygon_decomposition<'l,
        P: Position2D
    >(
        &mut self,
        polygon: &'l ComplexPolygon,
        vertex_positions: IdSlice<'l, VertexId, P>,
        diagonals: &'l mut Diagonals<ComplexPolygon>
    ) -> Result<(), DecompositionError> {
        self.helper.clear();

        let mut storage = Allocation::empty();
        swap(&mut self.sweep_state_storage, &mut storage);
        let mut sweep_state: Vec<ComplexPointId> = create_vec_from(storage);

        let mut storage = Allocation::empty();
        swap(&mut self.sorted_edges_storage, &mut storage);
        let mut sorted_edges: Vec<ComplexPointId> = create_vec_from(storage);

        for sub_poly in polygon.polygon_ids() {
            if sub_poly != polygon_id(0) {
                let winding = compute_winding_order(polygon.get_sub_polygon(sub_poly).unwrap(), vertex_positions);
                debug_assert_eq!(winding, Some(WindingOrder::CounterClockwise)
                );
            }
            sorted_edges.extend(polygon.point_ids(sub_poly));
        }
        debug_assert!(sorted_edges.len() == polygon.num_vertices());

        // sort indices by increasing y coordinate of the corresponding vertex
        sorted_edges.sort_by(|a, b| {
            let va = vertex_positions[polygon.vertex(*a)].position();
            let vb = vertex_positions[polygon.vertex(*b)].position();
            if va.y() > vb.y() { return Ordering::Greater; }
            if va.y() < vb.y() { return Ordering::Less; }
            if va.x() > vb.x() { return Ordering::Greater; }
            if va.x() < vb.x() { return Ordering::Less; }
            return Ordering::Equal;
        });

        for &e in &sorted_edges {
            //let edge = kernel[e];
            let prev = polygon.previous(e);
            let next = polygon.next(e);
            let current_vertex = vertex_positions[polygon.vertex(e)].position();
            let previous_vertex = vertex_positions[polygon.vertex(prev)].position();
            let next_vertex = vertex_positions[polygon.vertex(next)].position();
            let vertex_type = get_vertex_type(previous_vertex, current_vertex, next_vertex);
            let sweep_line = SweepLineBuilder {
                polygon: polygon,
                vertices: vertex_positions,
                current_vertex: current_vertex,
            };
            match vertex_type {
                VertexType::Start => {
                    sweep_line.add(&mut sweep_state, e);
                    self.helper.insert(e, (e, vertex_type));
                }
                VertexType::End => {
                    connect_with_helper_if_merge_vertex(e, prev, &mut self.helper, diagonals);
                    sweep_line.remove(&mut sweep_state, prev);
                }
                VertexType::Split => {
                    let ej = sweep_line.find_right_of_current_vertex(&sweep_state);
                    if let Some(&(helper_edge,_)) = self.helper.get(&ej) {
                        diagonals.add_diagonal(e, helper_edge);
                        //println!(" **** connection {:?}->{:?}", e, helper_edge);
                    } else {
                        panic!();
                    }
                    self.helper.insert(ej, (e, vertex_type));

                    sweep_line.add(&mut sweep_state, e);
                    self.helper.insert(e, (e, vertex_type));
                }
                VertexType::Merge => {
                    connect_with_helper_if_merge_vertex(e, prev, &mut self.helper, diagonals);
                    sweep_line.remove(&mut sweep_state, prev);

                    let ej = sweep_line.find_right_of_current_vertex(&sweep_state);
                    connect_with_helper_if_merge_vertex(e, ej, &mut self.helper, diagonals);
                    self.helper.insert(ej, (e, vertex_type));
                }
                VertexType::Right => {
                    // TODO remove helper(edge.prev) ?
                    connect_with_helper_if_merge_vertex(e, prev, &mut self.helper, diagonals);
                    self.helper.remove(&prev);
                    sweep_line.remove(&mut sweep_state, prev);

                    sweep_line.add(&mut sweep_state, e);
                    self.helper.insert(e, (e, vertex_type));
                }
                VertexType::Left => {
                    let ej = sweep_line.find_right_of_current_vertex(&sweep_state);
                    connect_with_helper_if_merge_vertex(e, ej, &mut self.helper, diagonals);

                    self.helper.insert(ej, (e, vertex_type));
                }
            }
        }

        // Keep the buffers to avoid reallocating it next time, if possible.
        self.sweep_state_storage = vec::recycle(sweep_state);
        self.sorted_edges_storage = vec::recycle(sorted_edges);

        return Ok(());
    }
}

/// Returns true if the face is y-monotone in O(n).
pub fn is_y_monotone<'l, Pos: Position2D>(
    polygon: PolygonView<'l>,
    vertex_positions: IdSlice<'l, VertexId, Pos>,
) -> bool {
    for point in polygon.point_ids() {
        let previous = vertex_positions[polygon.previous_vertex(point)].position();
        let current = vertex_positions[polygon.vertex(point)].position();
        let next = vertex_positions[polygon.next_vertex(point)].position();

        match get_vertex_type(previous, current, next) {
            VertexType::Split | VertexType::Merge => {
                return false;
            }
            _ => {}
        }
    }
    return true;
}

pub trait Write<T> { fn write(&mut self, data: T); }

/// A dummy implementation that doesn't write anything. Useful when ignoring the output
/// of an algorithm.
impl<T> Write<T> for () { fn write(&mut self, _data: T) {} }

/// Write into a Vec.
impl<T> Write<T> for Vec<T> { fn write(&mut self, data: T) { self.push(data) } }

/// Writes triangles as indices in a &[u16].
pub struct SliceTriangleWriter<'l> {
    indices: &'l mut[u16],
    offset: usize,
}

impl<'l> Write<[VertexId; 3]> for SliceTriangleWriter<'l> {
    fn write(&mut self, indices: [VertexId; 3]) {
        debug_assert!(indices[0] != indices[1]);
        debug_assert!(indices[0] != indices[2]);
        debug_assert!(indices[1] != indices[2]);
        self.indices[self.offset] = indices[0].to_index() as u16;
        self.indices[self.offset+1] = indices[1].to_index() as u16;
        self.indices[self.offset+2] = indices[2].to_index() as u16;
        self.offset += 3;
    }
}

impl<'l> SliceTriangleWriter<'l> {
    pub fn new(buffer: &'l mut[u16]) -> SliceTriangleWriter {
        SliceTriangleWriter {
            indices: buffer,
            offset: 0,
        }
    }
}

/// Can perform y-monotone triangulation on a connectivity kernel.
///
/// This object holds on to the memory that was allocated during previous
/// triangulations, in order to avoid allocating during the next triangulations
/// if possible.
pub struct TriangulationContext {
    vertex_stack_storage: Allocation,
}

impl TriangulationContext {
    /// Constructor.
    pub fn new() -> TriangulationContext {
        TriangulationContext {
            vertex_stack_storage: Allocation::empty()
        }
    }

    /// Computes an y-monotone triangulation of a face in the connectivity kernel,
    /// outputing triangles by pack of 3 vertex indices in a TriangleStream.
    ///
    /// Returns the number of indices that were added to the stream.
    pub fn y_monotone_triangulation<'l,
        P: Position2D,
        Output: VertexBufferBuilder<Vec2>
    >(
        &mut self,
        polygon: PolygonView<'l>,
        vertex_positions: IdSlice<'l, VertexId, P>,
        output: &mut Output,
    ) -> Result<(), TriangulationError> {

        // for convenience
        let vertex = |circ: Circulator| { vertex_positions[polygon.vertex(circ.point)].position() };
        let next = |circ: Circulator| { Circulator { point: polygon.advance(circ.point, circ.direction), direction: circ.direction } };
        let previous = |circ: Circulator| { Circulator { point: polygon.advance(circ.point, circ.direction.reverse()), direction: circ.direction } };

        #[derive(Copy, Clone, Debug, PartialEq)]
        struct Circulator {
            point: PointId,
            direction: Direction,
        }

        let mut up = Circulator { point: polygon.first_point(), direction: Direction::Forward };
        let mut down = up.clone();

        loop {
            down = next(down);
            if vertex(up).y() != vertex(down).y() {
                break;
            }
            if down == up {
                // Avoid an infnite loop in the degenerate case where all vertices are in the same position.
                break;
            }
        }

        up.direction = if is_below(vertex(up), vertex(down)) { Direction::Forward }
                       else { Direction::Backward };

        down.direction = up.direction.reverse();

        // Find the bottom-most vertex (with the highest y value)
        let mut big_y = vertex(down);
        let guard = down;
        loop {
            down = next(down);
            let new_y = vertex(down);
            if is_below(big_y, new_y) {
                down = previous(down);
                break;
            }
            big_y = new_y;
            if down == guard {
                // We have looped through all vertices already because of
                // a degenerate input, avoid looping infinitely.
                break;
            }
        }
        // find the top-most vertex (with the smallest y value)
        let mut small_y = vertex(up);
        let guard = up;
        loop {
            up = next(up);
            let new_y = vertex(up);
            if is_below(new_y, small_y) {
                up = previous(up);
                break;
            }
            small_y = new_y;
            if up == guard {
                // We have looped through all vertices already because of
                // a degenerate input, avoid looping infinitely.
                break;
            }
        }

        // now that we have the top-most vertex, we will circulate simulataneously
        // from the left and right chains until we reach the bottom-most vertex

        // main chain
        let mut m = up.clone();

        // opposite chain
        let mut o = up.clone();
        m.direction = Direction::Forward;
        o.direction = Direction::Backward;

        m = next(m);
        o = next(o);

        if is_below(vertex(m), vertex(o)) {
            swap(&mut m, &mut o);
        }

        m = previous(m);
        // previous
        let mut p = m;

        // vertices already visited, waiting to be connected
        let mut storage = Allocation::empty();
        swap(&mut storage, &mut self.vertex_stack_storage);
        let mut vertex_stack: Vec<Circulator> = create_vec_from(storage);

        let mut triangle_count = 0;
        let mut i: i32 = 0;

        loop {
            //println!("   -- m: {:?}  o: {:?}", m.point, o.point);

            // walk edges from top to bottom, alternating between the left and
            // right chains. The chain we are currently iterating over is the
            // main chain (m) and the other one the opposite chain (o).
            // p is the previous iteration, regardless of which chain it is on.
            if is_below(vertex(m), vertex(o)) || m == down {
                swap(&mut m, &mut o);
            }

            if i < 2 {
                vertex_stack.push(m);
            } else {
                if vertex_stack.len() > 0 && m.direction != vertex_stack[vertex_stack.len()-1].direction {
                    for i in 0..vertex_stack.len() - 1 {
                        let id_1 = polygon.vertex(vertex_stack[i].point);
                        let id_2 = polygon.vertex(vertex_stack[i+1].point);
                        let id_opp = polygon.vertex(m.point);

                        output.push_indices(id_opp.handle, id_1.handle, id_2.handle);
                        triangle_count += 1;
                    }

                    vertex_stack.clear();

                    vertex_stack.push(p);
                    vertex_stack.push(m);

                } else {

                    let mut last_popped = vertex_stack.pop();

                    loop {
                        if vertex_stack.len() < 1 {
                            break;
                        }
                        let mut id_1 = polygon.vertex(vertex_stack[vertex_stack.len()-1].point);
                        let id_2 = polygon.vertex(last_popped.unwrap().point);
                        let mut id_3 = polygon.vertex(m.point);

                        if m.direction == Direction::Backward {
                            swap(&mut id_1, &mut id_3);
                        }

                        let v1 = vertex_positions[id_1].position();
                        let v2 = vertex_positions[id_2].position();
                        let v3 = vertex_positions[id_3].position();
                        if directed_angle(vec2_sub(v1, v2), vec2_sub(v3, v2)) > PI {
                            output.push_indices(id_1.handle, id_2.handle, id_3.handle);
                            triangle_count += 1;

                            last_popped = vertex_stack.pop();

                        } else {
                            break;
                        }
                    } // loop 2

                    if let Some(item) = last_popped {
                        vertex_stack.push(item);
                    }
                    vertex_stack.push(m);

                }
            }

            if m.point == down.point {
                if o.point == down.point {
                    break;
                }
            }

            i += 1;
            p = m;
            m = next(m);
            debug_assert!(!is_below(vertex(p), vertex(m)));
        }
        debug_assert_eq!(triangle_count, polygon.num_vertices() as usize - 2);

        // Keep the buffer to avoid reallocating it next time, if possible.
        self.vertex_stack_storage = vec::recycle(vertex_stack);
        return Ok(());
    }
}

#[cfg(test)]
struct TestShape<'l> {
    label: &'l str,
    main: &'l[Vec2],
    holes: &'l[&'l[Vec2]],
}

#[cfg(test)]
fn test_shape(shape: &TestShape, angle: f32) {
    use std::iter::FromIterator;
    use tesselation::vertex_builder::{ VertexBuffers, simple_vertex_builder, };

    let mut vertices: Vec<Vec2> = Vec::new();
    vertices.extend(shape.main.iter());
    for hole in shape.holes {
        vertices.extend(hole.iter());
    }

    println!("vertices: {:?}", vertices);

    for ref mut v in &mut vertices[..] {
        // rotate all points around (0, 0).
        let cos = angle.cos();
        let sin = angle.sin();
        let (x, y) = (v.x(), v.y());
        v[0] = x*cos + y*sin;
        v[1] = y*cos - x*sin;
    }

    println!("transformed vertices: {:?}", vertices);

    let mut polygon = ComplexPolygon {
        main: Polygon::from_vertices(vertex_id_range(0, shape.main.len() as u16)),
        holes: Vec::new(),
    };

    let mut from = shape.main.len() as u16;
    for hole in shape.holes {
        let to = from + hole.len() as u16;
        println!(" -- range from {} to {}", from, to);
        polygon.holes.push(Polygon::from_vertices(ReverseIdRange::new(vertex_id_range(from, to))));
        from = to;
    }

    println!("\n\n -- poly main {:?}", polygon.main.vertices);
    for h in &polygon.holes {
        println!("    hole {:?}", h.vertices);
    }


    let vertex_positions = IdSlice::new(&vertices[..]);
    let mut ctx = DecompositionContext::new();
    let mut diagonals = Diagonals::new();
    let res = ctx.y_monotone_polygon_decomposition(&polygon, vertex_positions, &mut diagonals);
    assert_eq!(res, Ok(()));

    let mut y_monotone_polygons = Vec::new();
    partition_polygon(&polygon, vertex_positions, &mut diagonals, &mut y_monotone_polygons);

    let mut triangulator = TriangulationContext::new();
    let mut buffers: VertexBuffers<Vec2> = VertexBuffers::new();

    println!("\n\n -- There are {:?} monotone polygons", y_monotone_polygons.len());
    for poly in y_monotone_polygons {
        println!("\n\n -- Triangulating polygon with vertices {:?}", poly.vertices);
        let mut i = 0;
        for &p in &poly.vertices {
            println!("     -> point {} vertex {:?} position {:?}", i, p, vertex_positions[p].position());
            i += 1;
        }
        assert!(is_y_monotone(poly.view(), vertex_positions));
        let res = triangulator.y_monotone_triangulation(
            poly.view(),
            vertex_positions,
            &mut simple_vertex_builder(&mut buffers)
        );
        assert_eq!(res, Ok(()));
    }

    for n in 0 .. buffers.indices.len()/3 {
        println!(" ===> {} {} {}", buffers.indices[n*3], buffers.indices[n*3+1], buffers.indices[n*3+2]);
    }
}

#[cfg(test)]
fn test_all_shapes(tests: &[TestShape]) {
    let mut angle = 0.0;
    while angle < 2.0*PI {
        for shape in tests {
            println!("\n\n\n   -- shape: {} (angle {:?})", shape.label, angle);
            test_shape(shape, angle);
        }
        angle += 0.005;
    }
}

#[test]
fn test_triangulate() {
    test_all_shapes(&[
        TestShape {
            label: &"Simple triangle",
            main: &[
                [-10.0, 5.0],
                [0.0, -5.0],
                [10.0, 5.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"Simple triangle",
            main: &[
                [1.0, 2.0],
                [1.5, 3.0],
                [0.0, 4.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"Simple rectangle",
            main: &[
                [1.0, 2.0],
                [1.5, 3.0],
                [0.0, 4.0],
                [-1.0, 1.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"",
            main: &[
                [0.0, 0.0],
                [3.0, 0.0],
                [2.0, 1.0],
                [3.0, 2.0],
                [2.0, 3.0],
                [0.0, 2.0],
                [1.0, 1.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"",
            main: &[
                [0.0, 0.0],
                [1.0, 1.0],
                [2.0, 0.0],
                [2.0, 4.0],
                [1.0, 3.0],
                [0.0, 4.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"",
            main: &[
                [0.0, 2.0],
                [1.0, 2.0],
                [0.0, 1.0],
                [2.0, 0.0],
                [3.0, 1.0],// 4
                [4.0, 0.0],
                [3.0, 2.0],
                [2.0, 1.0],// 7
                [3.0, 3.0],
                [2.0, 4.0]
            ],
            holes: &[],
        },
        TestShape {
            label: &"",
            main: &[
                [0.0, 0.0],
                [1.0, 0.0],
                [2.0, 0.0],
                [3.0, 0.0],
                [3.0, 1.0],
                [3.0, 2.0],
                [3.0, 3.0],
                [2.0, 3.0],
                [1.0, 3.0],
                [0.0, 3.0],
                [0.0, 2.0],
                [0.0, 1.0],
            ],
            holes: &[],
        },
    ]);
}

#[test]
fn test_triangulate_holes() {
    test_all_shapes(&[
        TestShape {
            label: &"Triangle with triangle hole",
            main: &[
                [-11.0, 5.0],
                [0.0, -5.0],
                [10.0, 5.0],
            ],
            holes: &[
                &[
                    [-5.0, 2.0],
                    [0.0, -2.0],
                    [4.0, 2.0],
                ]
            ]
        },
        TestShape {
            label: &"Square with triangle hole",
            main: &[
                [-10.0, -10.0],
                [ 10.0, -10.0],
                [ 10.0,  10.0],
                [-10.0,  10.0],
            ],
            holes: &[
                &[
                    [-4.0, 2.0],
                    [0.0, -2.0],
                    [4.0, 2.0],
                ]
            ],
        },
        TestShape {
            label: &"Square with two holes",
            main: &[
                [-10.0, -10.0],
                [ 10.0, -10.0],
                [ 10.0,  10.0],
                [-10.0,  10.0],
            ],
            holes: &[
                &[
                    [-8.0, -8.0],
                    [-4.0, -8.0],
                    [4.0, 8.0],
                    [-8.0, 8.0],
                ],
                &[
                    [8.0, -8.0],
                    [6.0, 7.0],
                    [-2.0, -8.0],
                ]
            ],
        },
        TestShape {
            label: &"",
            main: &[
                [0.0, 0.0],
                [1.0, 1.0],
                [2.0, 1.0],
                [3.0, 0.0],
                [4.0, 0.0],
                [5.0, 0.0],
                [3.0, 4.0],
                [1.0, 4.0],
            ],
            holes: &[
                &[
                    [2.0, 2.0],
                    [3.0, 2.0],
                    [2.5, 3.0],
                ]
            ],
        },
    ]);
}

#[test]
#[ignore]
fn test_triangulate_degenerate() {
    test_all_shapes(&[
        TestShape {
            label: &"3 points on the same line (1)",
            main: &[
                [0.0, 0.0],
                [0.0, 1.0],
                [0.0, 2.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"3 points on the same line (2)",
            main: &[
                [0.0, 0.0],
                [0.0, 2.0],
                [0.0, 1.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"All points in the same place (1)",
            main: &[
                [0.0, 0.0],
                [0.0, 0.0],
                [0.0, 0.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"All points in the same place (2)",
            main: &[
                [0.0, 0.0],
                [0.0, 0.0],
                [0.0, 0.0],
                [0.0, 0.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"Geometry comes back along a line on the y axis",
            main: &[
                [0.0, 0.0],
                [0.0, 2.0],
                [0.0, 1.0],
                [-1.0, 0.0],
            ],
            holes: &[],
        },
    ]);
}

#[test]
#[ignore]
fn test_triangulate_failures() {
    // Test cases that are known to fail but we want to make work eventually.
    test_all_shapes(&[
        TestShape {
            label: &"Duplicate point (1)",
            main: &[
                [0.0, 0.0],
                [1.0, 0.0],
                [1.0, 0.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"Duplicate point (2)",
            main: &[
                [0.0, 0.0],
                [1.0, 0.0],
                [1.0, 0.0],
                [1.0, 1.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"Duplicate point (3)",
            main: &[
              [0.0, 0.0],
              [1.0, 0.0],
              [1.0, 0.0],
              [1.0, 0.0],
              [1.0, 1.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"Geometry comes back along a line on the x axis",
            main: &[
                [0.0, 0.0],
                [2.0, 0.0],
                [1.0, 0.0],
                [0.0, 1.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"Geometry comes back along lines",
            main: &[
            // a mix of the previous 2 cases
                [0.0, 0.0],
                [2.0, 0.0],
                [1.0, 0.0],
                [1.0, 2.0],
                [1.0, 1.0],
                [-1.0, 1.0],
                [0.0, 1.0],
                [0.0, -1.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"...->A->B->A->...",
            main: &[
                // outer
                [0.0, 0.0],
                [1.0, 1.0], // <--
                [2.0, 1.0],
                [3.0, 0.0],
                [4.0, 0.0],
                [5.0, 0.0],
                [3.0, 4.0],
                [1.0, 4.0],
                [1.0, 1.0], // <--
            ],
            holes: &[
                &[
                    [2.0, 2.0],
                    [3.0, 2.0],
                    [2.5, 3.0],
                ]
            ],
        },
        TestShape {
            label: &"zero-area geometry shaped like a cross going back to the same position at the center",
            main: &[
                [1.0, 1.0],
                [2.0, 1.0],
                [1.0, 1.0],
                [2.0, 1.0],
                [1.0, 1.0],
                [0.0, 1.0],
                [1.0, 1.0],
                [1.0, 0.0],
            ],
            holes: &[],
        },
        TestShape {
            label: &"Self-intersection",
            main: &[
                [0.0, 0.0],
                [1.0, 0.0],
                [1.0, 1.0],
                [0.0, 1.0],
                [3.0, 0.0],
                [3.0, 1.0],
            ],
            holes: &[],
        },
    ]);
}

#[cfg(test)]
fn assert_almost_eq(a: f32, b:f32) {
    if (a - b).abs() < 0.0001 { return; }
    println!("expected {} and {} to be equal", a, b);
    panic!();
}

#[test]
fn test_intersect_segment_horizontal() {
    assert_almost_eq(intersect_segment_with_horizontal([0.0, 0.0], [0.0, 2.0], 1.0), 0.0);
    assert_almost_eq(intersect_segment_with_horizontal([0.0, 2.0], [2.0, 0.0], 1.0), 1.0);
    assert_almost_eq(intersect_segment_with_horizontal([0.0, 1.0], [3.0, 0.0], 0.0), 3.0);
}
