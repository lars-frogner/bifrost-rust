//! Non-thermal electron distributions.

pub mod power_law;

use super::{feb, BeamPropertiesCollection};
use crate::geometry::{Point3, Vec3};
use crate::grid::Grid3;
use crate::interpolation::Interpolator3;
use crate::io::snapshot::{fdt, SnapshotCacher3};
use crate::tracing::ftr;
use crate::tracing::stepping::SteppingSense;

/// Whether or not a distribution is depleted.
#[derive(Clone, Copy, Debug)]
pub enum DepletionStatus {
    Undepleted,
    Depleted,
}

/// Holds the result of propagating the electron distribution.
#[derive(Clone, Debug)]
pub struct PropagationResult {
    /// Total power density deposited during propagation.
    pub deposited_power_density: feb,
    /// Average position where the power density was deposited.
    pub deposition_position: Point3<ftr>,
    /// Whether or not the distribution is now depleted.
    pub depletion_status: DepletionStatus,
}

/// Defines the properties of a non-thermal electron distribution.
pub trait Distribution {
    type PropertiesCollectionType: BeamPropertiesCollection;

    /// Returns the position where the distribution originates.
    fn acceleration_position(&self) -> &Point3<fdt>;

    /// Returns the direction of propagation of the electrons relative to the magnetic field direction.
    fn propagation_sense(&self) -> SteppingSense;

    /// Returns the maximum distance the distribution can propagate before propagation should be terminated.
    fn max_propagation_distance(&self) -> ftr;

    /// Returns an object holding properties associated with the distribution.
    fn properties(&self) -> <Self::PropertiesCollectionType as BeamPropertiesCollection>::Item;

    /// Propagates the electron distribution for the given displacement
    /// and returns the power density deposited during the propagation.
    fn propagate<G, I>(
        &mut self,
        snapshot: &SnapshotCacher3<G>,
        interpolator: &I,
        displacement: &Vec3<ftr>,
        new_position: &Point3<ftr>,
    ) -> PropagationResult
    where
        G: Grid3<fdt>,
        I: Interpolator3;
}
