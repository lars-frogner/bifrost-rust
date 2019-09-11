//! Structured grids with uniform spacing in the horizontal dimensions.

use crate::num::BFloat;
use crate::geometry::{Dim3, Dim2, In3D, In2D, Vec3, Vec2, Coords3, Coords2, CoordRefs3, CoordRefs2};
use super::{CoordLocation, GridType, Grid3, Grid2};
use super::regular::RegularGrid2;
use Dim3::{X, Y, Z};

/// A 3D grid which is regular in x and y but non-uniform in z.
#[derive(Clone, Debug)]
pub struct HorRegularGrid3<F: BFloat> {
    coords: [Coords3<F>; 2],
    regular_z_coords: [Vec<F>; 2],
    is_periodic: In3D<bool>,
    shape: In3D<usize>,
    lower_bounds: Vec3<F>,
    upper_bounds: Vec3<F>,
    extents: Vec3<F>
}

impl<F: BFloat> Grid3<F> for HorRegularGrid3<F> {
    type XSliceGrid = HorRegularGrid2<F>;
    type YSliceGrid = HorRegularGrid2<F>;
    type ZSliceGrid = RegularGrid2<F>;

    const TYPE: GridType = GridType::HorRegular;

    fn from_coords(centers: Coords3<F>, lower_edges: Coords3<F>, is_periodic: In3D<bool>) -> Self {
        assert!(!is_periodic[Z], "This grid type cannot be periodic in the z-direction.");

        let size_x = centers[X].len();
        let size_y = centers[Y].len();
        let size_z = centers[Z].len();

        let (lower_bound_x, upper_bound_x) = super::bounds_from_coords(size_x, &centers[X], &lower_edges[X]);
        let (lower_bound_y, upper_bound_y) = super::bounds_from_coords(size_y, &centers[Y], &lower_edges[Y]);
        let (lower_bound_z, upper_bound_z) = super::bounds_from_coords(size_z, &centers[Z], &lower_edges[Z]);

        let extent_x = super::extent_from_bounds(lower_bound_x, upper_bound_x);
        let extent_y = super::extent_from_bounds(lower_bound_y, upper_bound_y);
        let extent_z = super::extent_from_bounds(lower_bound_z, upper_bound_z);

        let (regular_centers_z, regular_lower_edges_z) = super::regular_coords_from_bounds(size_z, lower_bound_z, upper_bound_z);

        HorRegularGrid3{
            coords: [centers, lower_edges],
            regular_z_coords: [regular_centers_z, regular_lower_edges_z],
            is_periodic,
            shape: In3D::new(size_x, size_y, size_z),
            lower_bounds: Vec3::new(lower_bound_x, lower_bound_y, lower_bound_z),
            upper_bounds: Vec3::new(upper_bound_x, upper_bound_y, upper_bound_z),
            extents: Vec3::new(extent_x, extent_y, extent_z)
        }
    }

    fn shape(&self) -> &In3D<usize> { &self.shape }
    fn is_periodic(&self, dim: Dim3) -> bool { self.is_periodic[dim] }
    fn coords_by_type(&self, location: CoordLocation) -> &Coords3<F> { &self.coords[location as usize] }

    fn regular_centers(&self) -> CoordRefs3<F> {
        let centers = self.centers();
        CoordRefs3::new(
            &centers[X],
            &centers[Y],
            &self.regular_z_coords[0]
        )
    }

    fn regular_lower_edges(&self) -> CoordRefs3<F> {
        let lower_edges = self.lower_edges();
        CoordRefs3::new(
            &lower_edges[X],
            &lower_edges[Y],
            &self.regular_z_coords[1]
        )
    }

    fn lower_bounds(&self) -> &Vec3<F> { &self.lower_bounds }
    fn upper_bounds(&self) -> &Vec3<F> { &self.upper_bounds }
    fn extents(&self) -> &Vec3<F> { &self.extents }
}

/// A 2D grid which is regular in x but non-uniform in y.
#[derive(Clone, Debug)]
pub struct HorRegularGrid2<F: BFloat> {
    coords: [Coords2<F>; 2],
    regular_y_coords: [Vec<F>; 2],
    is_periodic: In2D<bool>,
    shape: In2D<usize>,
    lower_bounds: Vec2<F>,
    upper_bounds: Vec2<F>,
    extents: Vec2<F>
}

impl<F: BFloat> Grid2<F> for HorRegularGrid2<F> {
    const TYPE: GridType = GridType::HorRegular;

    fn from_coords(centers: Coords2<F>, lower_edges: Coords2<F>, is_periodic: In2D<bool>) -> Self {
        assert!(!is_periodic[Dim2::Y], "This grid type cannot be periodic in the y-direction.");

        let size_x = centers[Dim2::X].len();
        let size_y = centers[Dim2::Y].len();

        let (lower_bound_x, upper_bound_x) = super::bounds_from_coords(size_x, &centers[Dim2::X], &lower_edges[Dim2::X]);
        let (lower_bound_y, upper_bound_y) = super::bounds_from_coords(size_y, &centers[Dim2::Y], &lower_edges[Dim2::Y]);

        let extent_x = super::extent_from_bounds(lower_bound_x, upper_bound_x);
        let extent_y = super::extent_from_bounds(lower_bound_y, upper_bound_y);

        let (regular_centers_y, regular_lower_edges_y) = super::regular_coords_from_bounds(size_y, lower_bound_y, upper_bound_y);

        HorRegularGrid2{
            coords: [centers, lower_edges],
            regular_y_coords: [regular_centers_y, regular_lower_edges_y],
            is_periodic,
            shape: In2D::new(size_x, size_y),
            lower_bounds: Vec2::new(lower_bound_x, lower_bound_y),
            upper_bounds: Vec2::new(upper_bound_x, upper_bound_y),
            extents: Vec2::new(extent_x, extent_y)
        }
    }

    fn shape(&self) -> &In2D<usize> { &self.shape }
    fn is_periodic(&self, dim: Dim2) -> bool { self.is_periodic[dim] }
    fn coords_by_type(&self, location: CoordLocation) -> &Coords2<F> { &self.coords[location as usize] }

    fn regular_centers(&self) -> CoordRefs2<F> {
        let centers = self.centers();
        CoordRefs2::new(
            &centers[Dim2::X],
            &self.regular_y_coords[0]
        )
    }

    fn regular_lower_edges(&self) -> CoordRefs2<F> {
        let lower_edges = self.lower_edges();
        CoordRefs2::new(
            &lower_edges[Dim2::X],
            &self.regular_y_coords[1]
        )
    }

    fn lower_bounds(&self) -> &Vec2<F> { &self.lower_bounds }
    fn upper_bounds(&self) -> &Vec2<F> { &self.upper_bounds }
    fn extents(&self) -> &Vec2<F> { &self.extents }
}

#[cfg(test)]
mod tests {

    use super::*;
    use ndarray::prelude::*;
    use ndarray::s;
    use crate::geometry::{Point3, Idx3};
    use crate::grid::GridPointQuery3;

    #[test]
    fn varying_z_grid_index_search_works() {
        #![allow(clippy::deref_addrof)] // Mutes warning due to workings of s! macro
        let (mx, my, mz) = (17, 5, 29);

        let xc = Array::linspace(-1.0,  1.0, mx);
        let yc = Array::linspace( 1.0,  5.2, my);

        let (dx, dy) = (xc[1] - xc[0], yc[1] - yc[0]);

        let xdn = Array::linspace(xc[0] - dx/2.0, xc[mx-1] - dx/2.0, mx);
        let ydn = Array::linspace(yc[0] - dy/2.0, yc[my-1] - dy/2.0, my);

        let zdn = Array::linspace(-2.0, 2.0, mz+1) + Array::linspace(1.0, 2.0, mz+1).mapv(|a| a*a*a*a);
        let zc = (zdn.slice(s![1..]).into_owned() + zdn.slice(s![..mz]))*0.5;
        let zdn = zdn.slice(s![..mz]).into_owned();

        let z_max = 2.0*zc[mz-1] - zdn[mz-1];

        let centers = Coords3::new(xc.to_vec(), yc.to_vec(), zc.to_vec());
        let lower_edges = Coords3::new(xdn.to_vec(), ydn.to_vec(), zdn.to_vec());

        let grid = HorRegularGrid3::from_coords(centers, lower_edges, In3D::new(false, false, false));
        assert_eq!(grid.find_grid_cell(&Point3::new(xdn[mx-1] + dx + 1e-12, ydn[my-1] + dy + 1e-12, z_max + 1e-12)), GridPointQuery3::Outside);
        assert_eq!(grid.find_grid_cell(&Point3::new(xdn[0] + 1e-12, ydn[0] + 1e-12, zdn[0] + 1e-12)), GridPointQuery3::Inside(Idx3::new(0, 0, 0)));
        assert_eq!(grid.find_grid_cell(&Point3::new(xdn[0] + 1e-12, ydn[0] + 1e-12, zdn[0] - 1e-9)), GridPointQuery3::Outside);
        assert_eq!(grid.find_grid_cell(&Point3::new(-0.68751, 1.5249, 3.0)), GridPointQuery3::Inside(Idx3::new(2, 0, 10)));
        assert_eq!(grid.find_grid_cell(&Point3::new(0.0, 2.0, 16.7)), GridPointQuery3::Inside(Idx3::new(8, 1, 27)));
        assert_eq!(grid.find_grid_cell(&Point3::new(0.0, 2.0, -0.7)), GridPointQuery3::Inside(Idx3::new(8, 1, 1)));
    }
}
