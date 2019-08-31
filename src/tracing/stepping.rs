//! Stepping along field lines of a Bifrost vector field.

pub mod rkf;

use crate::geometry::{Vec3, Point3};
use crate::grid::Grid3;
use crate::field::VectorField3;
use crate::interpolation::Interpolator3;
use super::ftr;

/// Stepping along the field line in the same direction as the field or opposite.
#[derive(Copy, Clone, Debug)]
pub enum SteppingSense {
    Same,
    Opposite
}

/// A stepper result which is either OK (with an an abitrary value) or stopped (with a cause).
#[derive(Clone, Debug)]
pub enum StepperResult<T> {
    Ok(T),
    Stopped(StoppingCause)
}

/// Reason for terminating stepping.
#[derive(Copy, Clone, Debug)]
pub enum StoppingCause {
    Null,
    Sink,
    OutOfBounds,
    TooManyAttempts,
    StoppedByCallback
}

/// Lets the stepper callback communicate whether tracing should
/// continue or terminate.
#[derive(Copy, Clone, Debug)]
pub enum StepperInstruction {
    Continue,
    Terminate
}

/// Defines the properties of a stepping scheme.
pub trait Stepper3 {
    /// Places the stepper inside the field.
    ///
    /// # Parameters
    ///
    /// - `field`: Vector field to step in.
    /// - `interpolator`: Interpolator to use.
    /// - `direction_computer`: Closure used to compute a stepping direction from a field vector.
    /// - `position`: Position where the stepper should be placed.
    /// - `callback`: Closure that will be called with the placed position if successful.
    ///
    /// # Returns
    ///
    /// A `StepperResult<()>` which is either:
    ///
    /// - `Ok`: Stepper placement succeeded.
    /// - `Stopped`: Contains a `StoppingCause` indicating why stepper placement failed.
    ///
    /// # Type parameters
    ///
    /// - `F`: Floating point type of the field data.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    /// - `D`: Function type taking a mutable reference to a field vector.
    /// - `C`: Mutable function type taking a reference to a position and returning a `StepperInstruction`.
    fn place<F, G, I, D, C>(&mut self, field: &VectorField3<F, G>, interpolator: &I, direction_computer: &D, position: &Point3<ftr>, callback: &mut C) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3,
          D: Fn(&mut Vec3<ftr>),
          C: FnMut(&Point3<ftr>) -> StepperInstruction;

    /// Performs a step.
    ///
    /// # Parameters
    ///
    /// - `field`: Vector field to step in.
    /// - `interpolator`: Interpolator to use.
    /// - `direction_computer`: Closure used to compute a stepping direction from a field vector.
    /// - `callback`: Closure that will be called with the resulting position if successful.
    ///
    /// # Returns
    ///
    /// A `StepperResult<()>` which is either:
    ///
    /// - `Ok`: Stepper placement succeeded.
    /// - `Stopped`: Contains a `StoppingCause` indicating why the step failed.
    ///
    /// # Type parameters
    ///
    /// - `F`: Floating point type of the field data.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    /// - `D`: Function type taking a mutable reference to a field vector.
    /// - `C`: Mutable function type taking a reference to a position and returning a `StepperInstruction`.
    fn step<F, G, I, D, C>(&mut self, field: &VectorField3<F, G>, interpolator: &I, direction_computer: &D, callback: &mut C) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3,
          D: Fn(&mut Vec3<ftr>),
          C: FnMut(&Point3<ftr>) -> StepperInstruction;

    /// Performs a step, producing regularly spaced output positions.
    ///
    /// # Parameters
    ///
    /// - `field`: Vector field to step in.
    /// - `interpolator`: Interpolator to use.
    /// - `direction_computer`: Closure used to compute a stepping direction from a field vector.
    /// - `callback`: Closure that will be called with the resulting dense position if successful.
    ///
    /// # Returns
    ///
    /// A `StepperResult<()>` which is either:
    ///
    /// - `Ok`: Stepper placement succeeded.
    /// - `Stopped`: Contains a `StoppingCause` indicating why the step failed.
    ///
    /// # Type parameters
    ///
    /// - `F`: Floating point type of the field data.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    /// - `D`: Function type taking a mutable reference to a field vector.
    /// - `C`: Mutable function type taking a reference to a position and returning a `StepperInstruction`.
    fn step_dense_output<F, G, I, D, C>(&mut self, field: &VectorField3<F, G>, interpolator: &I, direction_computer: &D, callback: &mut C) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3,
          D: Fn(&mut Vec3<ftr>),
          C: FnMut(&Point3<ftr>) -> StepperInstruction;

    /// Returns a reference to the current stepper position.
    fn position(&self) -> &Point3<ftr>;

    /// Retuns the current distance of the stepper along the field line.
    fn distance(&self) -> ftr;
}