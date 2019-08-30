//! Stepping using Runge–Kutta–Fehlberg methods,
//! a set of RK methods with step size adaptation driven by
//! error estimation through an embedded lower-order step.

pub mod rkf23;
pub mod rkf45;

use num;
use crate::geometry::{Dim3, Point3, Vec3};
use crate::grid::Grid3;
use crate::field::VectorField3;
use crate::interpolation::{InterpResult3, Interpolator3};
use crate::tracing::ftr;
use super::{StepperResult, StoppingCause, StepperInstruction};
use Dim3::{X, Y, Z};

struct RKFStepperState3 {
    /// Configuration parameters for the stepper.
    config: RKFStepperConfig,
    /// PI control parameters for the stepper.
    pi_control: PIControlParams,
    /// Current position of the stepper.
    position: Point3<ftr>,
    /// Field direction at the current position of the stepper.
    direction: Vec3<ftr>,
    /// Current distance of the stepper along the field line.
    distance: ftr,
    /// Step size to use in the next step.
    step_size: ftr,
    /// The estimated error of the step from the previous to the current position.
    error: ftr,
    /// How many consecutive successful steps have been in opposite directions.
    n_sudden_reversals: u32,
    /// The step size used to get from the previous to the current position.
    previous_step_size: ftr,
    /// Position of the stepper directly before the previous step was taken.
    previous_position: Point3<ftr>,
    /// Field direction at the previous position of the stepper.
    previous_direction: Vec3<ftr>,
    /// Intermediate step directions used during the previous step.
    intermediate_directions: Vec<Vec3<ftr>>,
    /// Displacement vector from the previous to the current position
    /// (with periodic boundaries taken into account).
    previous_step_displacement: Vec3<ftr>,
    /// Whether the previous step had to wrap around a periodic boundary.
    previous_step_wrapped: bool,
    /// Distance along the field line where the next dense output position
    /// should be computed.
    next_output_distance: ftr
}

/// Configuration parameters for RKF steppers.
pub struct RKFStepperConfig {
    /// Step size to use for dense (uniform) output positions.
    pub dense_step_size: ftr,
    /// Maximum number of step attempts before terminating.
    pub max_step_attempts: u32,
    /// Absolute error tolerance.
    pub absolute_tolerance: ftr,
    /// Relative error tolerance.
    pub relative_tolerance: ftr,
    /// Scaling factor for the error to reduce oscillations.
    pub safety_factor: ftr,
    /// Smallest allowed scaling of the step size in one step.
    pub min_step_scale: ftr,
    /// Largest allowed scaling of the step size in one step.
    pub max_step_scale: ftr,
    /// Start value for error.
    pub initial_error: ftr,
    /// Initial step size.
    pub initial_step_size: ftr,
    /// Number of sudden direction reversals before the area is considered a sink.
    pub sudden_reversals_for_sink: u32,
    /// Whether to use Proportional Integral (PI) control for stabilizing the stepping.
    pub use_pi_control: bool
}

struct PIControlParams {
    k_i: ftr,
    k_p: ftr
}

enum ComputedDirection3 {
    Standard(Vec3<ftr>),
    WithWrappedPosition((Point3<ftr>, Vec3<ftr>))
}

enum StepError {
    Acceptable(ftr),
    TooLarge(ftr)
}

struct StepAttempt3 {
    next_position: Point3<ftr>,
    next_direction: Vec3<ftr>,
    intermediate_directions: Vec<Vec3<ftr>>,
    step_displacement: Vec3<ftr>,
    step_wrapped: bool
}

trait RKFStepper3 {
    fn state(&self) -> &RKFStepperState3;
    fn state_mut(&mut self) -> &mut RKFStepperState3;

    fn attempt_step<F, G, I>(&self, field: &VectorField3<F, G>, interpolator: &I) -> StepperResult<StepAttempt3>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3;

    fn compute_error_deltas(&self, attempt: &StepAttempt3) -> Vec3<ftr>;

    fn compute_dense_interpolation_coefs(&self) -> Vec<Vec3<ftr>>;

    fn interpolate_dense_position<F, G>(&self, grid: &G, coefs: &[Vec3<ftr>], fraction: ftr) -> Point3<ftr>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone;

    fn reset_state(&mut self, position: &Point3<ftr>, direction: &Vec3<ftr>) {
        let state = self.state_mut();
        state.position = position.clone();
        state.direction = direction.clone();
        state.distance = 0.0;
        state.step_size = state.config.initial_step_size;
        state.error = state.config.initial_error;
        state.n_sudden_reversals = 0;
        state.previous_step_size = 0.0;
        state.previous_position = position.clone();
        state.previous_direction = direction.clone();
        state.intermediate_directions = Vec::new();
        state.previous_step_displacement = Vec3::zero();
        state.previous_step_wrapped = false;
        state.next_output_distance = state.config.dense_step_size;
    }

    fn place_with_callback<F, G, I, C>(&mut self, field: &VectorField3<F, G>, interpolator: &I, position: &Point3<ftr>, callback: &mut C) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3,
          C: FnMut(&Point3<ftr>) -> StepperInstruction
    {
        let place_result = self.perform_place(field, interpolator, position);
        if let StepperResult::Ok(_) = place_result {
            if let StepperInstruction::Terminate = callback(&self.state().position) {
                return StepperResult::Stopped(StoppingCause::StoppedByCallback)
            }
        }
        place_result
    }

    fn step_with_callback<F, G, I, C>(&mut self, field: &VectorField3<F, G>, interpolator: &I, callback: &mut C) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3,
          C: FnMut(&Point3<ftr>) -> StepperInstruction
    {
        let step_result = self.perform_step(field, interpolator);
        if let StepperResult::Ok(_) = step_result {
            if let StepperInstruction::Terminate = callback(&self.state().position) {
                return StepperResult::Stopped(StoppingCause::StoppedByCallback)
            }
        }
        step_result
    }

    fn step_with_callback_dense_output<F, G, I, C>(&mut self, field: &VectorField3<F, G>, interpolator: &I, callback: &mut C) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3,
          C: FnMut(&Point3<ftr>) -> StepperInstruction
    {
        let step_result = self.perform_step(field, interpolator);
        if let StepperResult::Ok(_) = step_result {
            self.compute_dense_output(field.grid(), callback)
        } else {
            step_result
        }
    }

    fn perform_place<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I, position: &Point3<ftr>) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3
    {
        match Self::compute_direction(field, interpolator, position) {
            StepperResult::Ok(ComputedDirection3::Standard(direction)) => self.reset_state(position, &direction),
            StepperResult::Ok(ComputedDirection3::WithWrappedPosition((wrapped_position, direction))) => self.reset_state(&wrapped_position, &direction),
            StepperResult::Stopped(cause) => return StepperResult::Stopped(cause)
        };
        StepperResult::Ok(())
    }

    fn perform_step<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3
    {
        let grid = field.grid();
        let mut attempts = 0;

        while attempts < self.state().config.max_step_attempts {
            let step_attempt = match self.attempt_step(field, interpolator) {
                StepperResult::Ok(step_attempt) => step_attempt,
                StepperResult::Stopped(cause) => return StepperResult::Stopped(cause)
            };

            attempts += 1;

            match self.compute_error(grid, &step_attempt) {
                StepError::Acceptable(new_error) => {
                    let mut new_step_size = self.compute_step_size_accepted(new_error);

                    // Don't increase step size if the previous attempt was rejected
                    if attempts > 1 && new_step_size > self.state().step_size {
                        new_step_size = self.state().step_size;
                    }

                    if self.check_for_sink(&step_attempt) {
                        return StepperResult::Stopped(StoppingCause::Sink)
                    }

                    self.apply_step_attempt(step_attempt);
                    self.update_step_size(new_step_size, new_error);
                    break;
                }
                StepError::TooLarge(new_error) => {
                    let new_step_size = self.compute_step_size_rejected(new_error);
                    self.update_step_size(new_step_size, new_error);
                }
            };
        }

        if attempts < self.state().config.max_step_attempts {
            StepperResult::Ok(())
        } else {
            StepperResult::Stopped(StoppingCause::TooManyAttempts)
        }
    }

    fn compute_direction<F, G, I>(field: &VectorField3<F, G>, interpolator: &I, position: &Point3<ftr>) -> StepperResult<ComputedDirection3>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3
    {
        match interpolator.interp_vector_field(field, &Point3::from(position)) {
            InterpResult3::Ok(vec) => {
                if vec.is_zero() {
                    StepperResult::Stopped(StoppingCause::Null)
                } else {
                    let mut direction = Vec3::from(&vec);
                    direction.normalize();
                    StepperResult::Ok(ComputedDirection3::Standard(direction))
                }
            },
            InterpResult3::OutOfBounds(_) => {
                let outside_point = Point3::from(position);
                match field.grid().wrap_point(&outside_point) {
                    Some(wrapped_position) => {
                        match interpolator.interp_vector_field(field, &wrapped_position) {
                            InterpResult3::Ok(vec) => {
                                if vec.is_zero() {
                                    StepperResult::Stopped(StoppingCause::Null)
                                } else {
                                    let mut direction = Vec3::from(&vec);
                                    direction.normalize();
                                    StepperResult::Ok(ComputedDirection3::WithWrappedPosition((Point3::from(&wrapped_position), direction)))
                                }
                            },
                            InterpResult3::OutOfBounds(_) => panic!("Out of bounds after wrapping.")
                        }
                    },
                    None => StepperResult::Stopped(StoppingCause::OutOfBounds)
                }
            }
        }
    }

    fn compute_error<F, G>(&self, grid: &G, attempt: &StepAttempt3) -> StepError
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone
    {
        let state = self.state();
        let error_deltas = self.compute_error_deltas(attempt);

        let grid_extents: Vec3<ftr> = Vec3::from(grid.extents());
        let errors = Vec3::new(error_deltas[X]/(state.config.absolute_tolerance + state.config.relative_tolerance*grid_extents[X]),
                               error_deltas[Y]/(state.config.absolute_tolerance + state.config.relative_tolerance*grid_extents[Y]),
                               error_deltas[Z]/(state.config.absolute_tolerance + state.config.relative_tolerance*grid_extents[Z]));

        let error = ftr::sqrt(0.5*errors.squared_length());

        if error <= 1.0 {
            StepError::Acceptable(error)
        } else {
            StepError::TooLarge(error)
        }
    }

    fn compute_step_size_accepted(&self, new_error: ftr) -> ftr {
        let state = self.state();
        let step_scale = if new_error < 1e-9 {
            // Use max step scale directly for very small error to avoid division by zero
            state.config.max_step_scale
        } else {
            let step_scale = state.config.safety_factor*(state.error.powf(state.pi_control.k_i))/(new_error.powf(state.pi_control.k_p));
            if step_scale < state.config.min_step_scale {
                state.config.min_step_scale
            } else if step_scale > state.config.max_step_scale {
                state.config.max_step_scale
            } else {
                step_scale
            }
        };
        state.step_size*step_scale
    }

    fn compute_step_size_rejected(&self, new_error: ftr) -> ftr {
        let state = self.state();
        ftr::max(state.config.safety_factor/(new_error.powf(state.pi_control.k_p)), state.config.min_step_scale)*state.step_size
    }

    fn check_for_sink(&mut self, attempt: &StepAttempt3) -> bool {
        let state = self.state_mut();
        if attempt.next_direction.dot(&state.direction) < 0.0 {
            state.n_sudden_reversals += 1;
            state.n_sudden_reversals >= state.config.sudden_reversals_for_sink
        } else {
            state.n_sudden_reversals = 0;
            false
        }
    }

    fn apply_step_attempt(&mut self, attempt: StepAttempt3) {
        let state = self.state_mut();
        state.previous_position = state.position.clone();
        state.previous_direction = state.direction.clone();
        state.position = attempt.next_position;
        state.direction = attempt.next_direction;
        state.distance += state.step_size; // Advance distance with step size *prior to* calling `update_step_size`
        state.intermediate_directions = attempt.intermediate_directions;
        state.previous_step_displacement = attempt.step_displacement;
        state.previous_step_wrapped = attempt.step_wrapped;
    }

    fn update_step_size(&mut self, new_step_size: ftr, new_error: ftr) {
        let state = self.state_mut();
        state.previous_step_size = state.step_size;
        state.step_size = new_step_size;
        state.error = new_error;
    }

    fn compute_dense_output<F, G, C>(&mut self, grid: &G, callback: &mut C) -> StepperResult<()>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          C: FnMut(&Point3<ftr>) -> StepperInstruction
    {
        #![allow(clippy::float_cmp)] // Allows the float comparison with zero
        let state = self.state();
        let previous_distance = state.distance - state.previous_step_size;
        debug_assert_ne!(state.previous_step_size, 0.0);
        debug_assert!(state.next_output_distance > previous_distance);

        let mut next_output_distance = state.next_output_distance;
        if next_output_distance <= state.distance {
            let coefs = self.compute_dense_interpolation_coefs();
            loop {
                let fraction = (next_output_distance - previous_distance)/state.previous_step_size;
                let output_position = self.interpolate_dense_position(grid, &coefs, fraction);

                if let StepperInstruction::Terminate = callback(&output_position) {
                    return StepperResult::Stopped(StoppingCause::StoppedByCallback)
                }

                next_output_distance += state.config.dense_step_size;
                if next_output_distance > state.distance {
                    break
                }
            }
        }

        // Reborrow state as mutable to update distance
        let state = self.state_mut();
        state.next_output_distance = next_output_distance;

        StepperResult::Ok(())
    }
}

impl RKFStepperConfig {
    const DEFAULT_DENSE_STEP_SIZE: ftr = 1e-2;
    const DEFAULT_MAX_STEP_ATTEMPTS: u32 = 16;
    const DEFAULT_ABSOLUTE_TOLERANCE: ftr = 1e-6;
    const DEFAULT_RELATIVE_TOLERANCE: ftr = 1e-6;
    const DEFAULT_SAFETY_FACTOR: ftr = 0.9;
    const DEFAULT_MIN_STEP_SCALE: ftr = 0.2;
    const DEFAULT_MAX_STEP_SCALE: ftr = 10.0;
    const DEFAULT_INITIAL_ERROR: ftr = 1e-4;
    const DEFAULT_INITIAL_STEP_SIZE: ftr = 1e-4;
    const DEFAULT_SUDDEN_REVERSALS_FOR_SINK: u32 = 3;

    /// Creates a new configuration struct with the default values.
    pub fn default() -> Self {
        RKFStepperConfig {
            dense_step_size: Self::DEFAULT_DENSE_STEP_SIZE,
            max_step_attempts: Self::DEFAULT_MAX_STEP_ATTEMPTS,
            absolute_tolerance: Self::DEFAULT_ABSOLUTE_TOLERANCE,
            relative_tolerance: Self::DEFAULT_RELATIVE_TOLERANCE,
            safety_factor: Self::DEFAULT_SAFETY_FACTOR,
            min_step_scale: Self::DEFAULT_MAX_STEP_SCALE,
            max_step_scale: Self::DEFAULT_MIN_STEP_SCALE,
            initial_step_size: Self::DEFAULT_INITIAL_STEP_SIZE,
            initial_error: Self::DEFAULT_INITIAL_ERROR,
            sudden_reversals_for_sink: Self::DEFAULT_SUDDEN_REVERSALS_FOR_SINK,
            use_pi_control: true
        }
    }

    fn validate(&self) {
        assert!(self.dense_step_size > 0.0, "Dense step size must be larger than zero.");
        assert!(self.max_step_attempts > 0, "Maximum number of step attempts must be larger than zero.");
        assert!(self.absolute_tolerance > 0.0, "Absolute error tolerance must be larger than zero.");
        assert!(self.relative_tolerance >= 0.0, "Relative error tolerance must be larger than or equal to zero.");
        assert!(self.safety_factor > 0.0 && self.safety_factor <= 1.0, "Safety factor must be in the range (0, 1].");
        assert!(self.min_step_scale > 0.0, "Minimum step scale must be larger than zero.");
        assert!(self.max_step_scale >= self.min_step_scale, "Maximum step scale must be larger than or equal to the minimum step scale.");
        assert!(self.initial_step_size > 0.0, "Initial step size must be larger than zero.");
        assert!(self.initial_error > 0.0 && self.initial_error <= 1.0, "Initial error must be in the range (0, 1].");
        assert!(self.sudden_reversals_for_sink > 0, "Number of sudden reversals for sink must be larger than zero.");
    }
}

impl PIControlParams {
    fn activated(scheme_order: u8) -> Self {
        #[allow(clippy::cast_lossless)]
        let order = scheme_order as ftr;
        let k_i = 0.4/order;
        let k_p = 1.0/order - 0.75*k_i;
        PIControlParams{ k_i, k_p }
    }

    fn deactivated(scheme_order: u8) -> Self {
        #[allow(clippy::cast_lossless)]
        let order = scheme_order as ftr;
        let k_i = 0.0;
        let k_p = 1.0/order;
        PIControlParams{ k_i, k_p }
    }
}