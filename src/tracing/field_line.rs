//! Field lines in Bifrost vector fields.

pub mod natural;
pub mod regular;

use std::{io, path, fs};
use std::collections::HashMap;
use serde::Serialize;
use crate::io::utils::{save_data_as_pickle, write_data_as_pickle_to_file};
use crate::geometry::{Vec3, Point3};
use crate::grid::Grid3;
use crate::field::{ScalarField3, VectorField3};
use crate::interpolation::Interpolator3;
use super::stepping::{StepperFactory3, Stepper3};
use super::seeding::Seeder3;
use super::{ftr, TracerResult};

/// Data associated with a 3D field line.
#[derive(Default, Serialize)]
pub struct FieldLineData3 {
    positions: Vec<Point3<ftr>>,
    scalar_values: HashMap<String, Vec<ftr>>,
    vector_values: HashMap<String, Vec<Vec3<ftr>>>
}

/// Collection of 3D field lines.
#[derive(Default)]
pub struct FieldLineSet3<L: FieldLine3> {
    field_lines: Vec<L>
}

/// Defines the properties of a field line of a 3D vector field.
pub trait FieldLine3 {
    type Data: Serialize;

    /// Returns a reference to the field line data structure.
    fn data(&self) -> &Self::Data;

    /// Returns a reference to the positions making up the field line.
    fn positions(&self) -> &Vec<Point3<ftr>>;

    /// Traces the field line through a 3D vector field.
    ///
    /// # Parameters
    ///
    /// - `field`: Vector field to trace.
    /// - `interpolator`: Interpolator to use.
    /// - `stepper`: Stepper to use (will be consumed).
    /// - `start_position`: Position where the tracing should start.
    ///
    /// # Returns
    ///
    /// A `TracerResult` which is either:
    ///
    /// - `Ok`: Contains an `Option<StoppingCause>`, possibly indicating why tracing was terminated.
    /// - `Void`: No field line was traced.
    ///
    /// # Type parameters
    ///
    /// - `F`: Floating point type of the field data.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    /// - `St`: Type of stepper.
    fn trace<F, G, I, St>(&mut self, field: &VectorField3<F, G>, interpolator: &I, stepper: St, start_position: &Point3<ftr>) -> TracerResult
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3,
          St: Stepper3;

    /// Stores the given scalar values for the field line points.
    fn add_scalar_values(&mut self, field_name: String, values: Vec<ftr>);

    /// Stores the given vector values for the field line points.
    fn add_vector_values(&mut self, field_name: String, values: Vec<Vec3<ftr>>);

    /// Returns the number of points making up the field line.
    fn number_of_points(&self) -> usize { self.positions().len() }

    /// Extracts and stores the value of the given scalar field at each field line point.
    fn extract_scalars<F, G, I>(&mut self, field: &ScalarField3<F, G>, interpolator: &I)
    where F: num::Float + std::fmt::Display,
            G: Grid3<F> + Clone,
            I: Interpolator3
    {
        let mut values = Vec::with_capacity(self.number_of_points());
        for pos in self.positions() {
            let value = interpolator.interp_scalar_field(field, &Point3::from(pos)).unwrap();
            values.push(num::NumCast::from(value).unwrap());
        }
        self.add_scalar_values(field.name().to_string(), values);
    }

    /// Extracts and stores the value of the given vector field at each field line point.
    fn extract_vectors<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I)
    where F: num::Float + std::fmt::Display,
            G: Grid3<F> + Clone,
            I: Interpolator3
    {
        let mut values = Vec::with_capacity(self.number_of_points());
        for pos in self.positions() {
            let value = interpolator.interp_vector_field(field, &Point3::from(pos)).unwrap();
            values.push(Vec3::from(&value));
        }
        self.add_vector_values(field.name().to_string(), values);
    }

    /// Serializes the field line data into pickle format and save at the given path.
    fn save_as_pickle(&self, file_path: &path::Path) -> io::Result<()> {
        save_data_as_pickle(file_path, self.data())
    }
}

impl FieldLineData3 {
    fn new() -> Self {
        FieldLineData3{
            positions: Vec::new(),
            scalar_values: HashMap::new(),
            vector_values: HashMap::new()
        }
    }
}

impl<L: FieldLine3> FieldLineSet3<L> {
    /// Traces all the field lines in the set from positions generated by the given seeder.
    ///
    /// # Parameters
    ///
    /// - `field`: Vector field to trace.
    /// - `interpolator`: Interpolator to use.
    /// - `stepper_factory`: Factory structure to use for producing steppers.
    /// - `seeder`: Seeder to use for generating start positions.
    /// - `field_line_initializer`: Closure for initializing empty field lines.
    ///
    /// # Returns
    ///
    /// An `Option` which is either:
    ///
    /// - `Some`: Contains a new `FieldLineSet3` with traced field lines.
    /// - `None`: No field lines were traced.
    ///
    /// # Type parameters
    ///
    /// - `F`: Floating point type of the field data.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    /// - `StF`: Type of stepper factory.
    /// - `Sd`: Type of seeder.
    /// - `FI`: Function type with no parameters returning a value of type `L`.
    pub fn trace<F, G, I, StF, Sd, FI>(field: &VectorField3<F, G>, interpolator: &I, stepper_factory: StF, seeder: Sd, field_line_initializer: &FI) -> Option<Self>
    where F: num::Float + std::fmt::Display,
          G: Grid3<F> + Clone,
          I: Interpolator3,
          StF: StepperFactory3,
          Sd: Seeder3,
          FI: Fn() -> L
    {
        let seed_iter = seeder.into_iter();
        let mut field_lines = match seed_iter.size_hint() {
            (lower, None) => Vec::with_capacity(lower),
            (_, Some(upper)) => Vec::with_capacity(upper)
        };
        for start_position in seed_iter {
            let mut field_line = field_line_initializer();
            if let TracerResult::Ok(_) = field_line.trace(field, interpolator, stepper_factory.produce(), &start_position) {
                field_lines.push(field_line);
            }
        }
        if field_lines.is_empty() {
            None
        } else {
            Some(FieldLineSet3{ field_lines })
        }
    }

    /// Serializes the data of each field line into pickle format and save at the given path.
    pub fn save_as_pickle(&self, file_path: &path::Path) -> io::Result<()> {
        let mut file = fs::File::create(file_path)?;
        for field_line in &self.field_lines {
            write_data_as_pickle_to_file(&mut file, field_line.data())?
        }
        Ok(())
    }
}