//! Non-thermal electron beam physics in Bifrost simulations.

pub mod distribution;
pub mod accelerator;
pub mod execution;

use std::{io, path, fs, mem};
use std::io::Write;
use std::collections::HashMap;
use serde::Serialize;
use serde::ser::{Serializer, SerializeStruct};
use rayon::prelude::*;
use crate::num::BFloat;
use crate::io::Verbose;
use crate::io::snapshot::{fdt, SnapshotCacher3};
use crate::io::utils;
use crate::geometry::{Vec3, Point3};
use crate::grid::Grid3;
use crate::field::{ScalarField3, VectorField3};
use crate::interpolation::Interpolator3;
use crate::tracing::{self, ftr, TracerResult};
use crate::tracing::seeding::IndexSeeder3;
use crate::tracing::stepping::{StepperInstruction, Stepper3, StepperFactory3};
use self::distribution::{DepletionStatus, PropagationResult, Distribution};
use self::accelerator::Accelerator;

/// Floating-point precision to use for electron beam physics.
#[allow(non_camel_case_types)]
pub type feb = f64;

/// Marker trait for electron beam metadata types.
pub trait ElectronBeamMetadata: Clone + std::fmt::Debug + Serialize + Send + Sync {}

/// A beam of non-thermal electrons propagating through the solar atmosphere.
#[derive(Clone, Debug)]
pub struct ElectronBeam<D: Distribution> {
    trajectory: Vec<Point3<ftr>>,
    fixed_scalar_values: HashMap<String, feb>,
    fixed_vector_values: HashMap<String, Vec3<feb>>,
    varying_scalar_values: HashMap<String, Vec<feb>>,
    varying_vector_values: HashMap<String, Vec<Vec3<feb>>>,
    metadata: D::MetadataType
}

/// A set of non-thermal electron beams propagating through the solar atmosphere.
#[derive(Clone, Debug, Serialize)]
pub struct ElectronBeamSwarm<A: Accelerator> {
    beams: Vec<ElectronBeam<A::DistributionType>>
}

impl<D: Distribution> ElectronBeam<D> {
    /// Generates an electron beam with the given initial distribution,
    /// but does not propagate it.
    ///
    /// # Parameters
    ///
    /// - `distribution`: Initial distribution of the beam electrons.
    ///
    /// # Returns
    ///
    /// A new unpropagated `ElectronBeam`.
    pub fn generate_unpropagated(distribution: D) -> Self {
        let mut fixed_scalar_values = distribution.scalar_properties();
        fixed_scalar_values.insert("total_propagation_distance".to_string(), 0.0);

        let mut varying_scalar_values = HashMap::new();
        varying_scalar_values.insert("deposited_power_density".to_string(), vec![0.0]);

        ElectronBeam{
            trajectory: vec![Point3::from(distribution.acceleration_position())],
            fixed_scalar_values,
            fixed_vector_values:  distribution.vector_properties(),
            varying_scalar_values,
            varying_vector_values: HashMap::new(),
            metadata: distribution.metadata().clone()
        }
    }

    /// Generates an electron beam with the given initial distribution
    /// and propagates it through the atmosphere in the given snapshot.
    ///
    /// # Parameters
    ///
    /// - `distribution`: Initial distribution of the beam electrons.
    /// - `snapshot`: Snapshot representing the atmosphere.
    /// - `interpolator`: Interpolator to use.
    /// - `stepper`: Stepper to use (will be consumed).
    ///
    /// # Returns
    ///
    /// An `Option` which is either:
    ///
    /// - `Some`: Contains a new propagated `ElectronBeam`.
    /// - `None`: No electron beam was generated.
    ///
    /// # Type parameters
    ///
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    /// - `S`: Type of stepper.
    pub fn generate_propagated<G, I, S>(mut distribution: D, snapshot: &SnapshotCacher3<G>, interpolator: &I, stepper: S) -> Option<Self>
    where G: Grid3<fdt>,
          I: Interpolator3,
          S: Stepper3
    {
        let mut trajectory = Vec::new();
        let mut deposited_power_densities = Vec::new();
        let mut total_propagation_distance = 0.0;

        let magnetic_field = snapshot.cached_vector_field("b");
        let start_position = Point3::from(distribution.acceleration_position());

        let tracer_result = tracing::trace_3d_field_line_dense(magnetic_field, interpolator, stepper, &start_position, distribution.propagation_sense(),
            &mut |displacement, position, distance| {
                let PropagationResult{
                    deposited_power_density,
                    deposition_position,
                    depletion_status
                } = distribution.propagate(snapshot, interpolator, displacement, position);

                trajectory.push(deposition_position);
                deposited_power_densities.push(deposited_power_density);
                total_propagation_distance = distance;

                match depletion_status {
                    DepletionStatus::Undepleted => StepperInstruction::Continue,
                    DepletionStatus::Depleted => StepperInstruction::Terminate
                }
            }
        );
        let mut fixed_scalar_values = distribution.scalar_properties();
        fixed_scalar_values.insert("total_propagation_distance".to_string(), total_propagation_distance);

        let fixed_vector_values = distribution.vector_properties();

        let mut varying_scalar_values = HashMap::new();
        varying_scalar_values.insert("deposited_power_density".to_string(), deposited_power_densities);

        let varying_vector_values = HashMap::new();

        let metadata = distribution.metadata().clone();

        match tracer_result {
            TracerResult::Ok(_) => Some(ElectronBeam{
                trajectory,
                fixed_scalar_values,
                fixed_vector_values,
                varying_scalar_values,
                varying_vector_values,
                metadata
            }),
            TracerResult::Void => None
        }
    }

    /// Returns a reference to the positions making up the beam trajectory.
    pub fn trajectory(&self) -> &Vec<Point3<ftr>> { &self.trajectory }

    /// Returns the number of points making up the electron beam.
    pub fn number_of_points(&self) -> usize { self.trajectory.len() }

    /// Extracts and stores the value of the given scalar field at the initial position of the beam.
    pub fn extract_fixed_scalar<F, G, I>(&mut self, field: &ScalarField3<F, G>, interpolator: &I)
    where F: BFloat,
          G: Grid3<F>,
          I: Interpolator3
    {
        let value = interpolator.interp_scalar_field(field, &Point3::from(&self.trajectory[0])).expect_inside();
        self.fixed_scalar_values.insert(field.name().to_string(), num::NumCast::from(value).expect("Conversion failed."));
    }

    /// Extracts and stores the value of the given vector field at the initial position of the beam.
    pub fn extract_fixed_vector<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I)
    where F: BFloat,
          G: Grid3<F>,
          I: Interpolator3
    {
        let vector = interpolator.interp_vector_field(field, &Point3::from(&self.trajectory[0])).expect_inside();
        self.fixed_vector_values.insert(field.name().to_string(), Vec3::from(&vector));
    }

    /// Extracts and stores the value of the given scalar field at each position of the beam.
    pub fn extract_varying_scalars<F, G, I>(&mut self, field: &ScalarField3<F, G>, interpolator: &I)
    where F: BFloat,
          G: Grid3<F>,
          I: Interpolator3
    {
        let mut values = Vec::with_capacity(self.number_of_points());
        for pos in &self.trajectory {
            let value = interpolator.interp_scalar_field(field, &Point3::from(pos)).expect_inside();
            values.push(num::NumCast::from(value).expect("Conversion failed."));
        }
        self.varying_scalar_values.insert(field.name().to_string(), values);
    }

    /// Extracts and stores the value of the given vector field at each position of the beam.
    pub fn extract_varying_vectors<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I)
    where F: BFloat,
          G: Grid3<F>,
          I: Interpolator3
    {
        let mut values = Vec::with_capacity(self.number_of_points());
        for pos in &self.trajectory {
            let value = interpolator.interp_vector_field(field, &Point3::from(pos)).expect_inside();
            values.push(Vec3::from(&value));
        }
        self.varying_vector_values.insert(field.name().to_string(), values);
    }

    /// Serializes the electron beam data into pickle format and saves at the given path.
    pub fn save_as_pickle<P: AsRef<path::Path>>(&self, file_path: P) -> io::Result<()> {
        utils::save_data_as_pickle(file_path, &self)
    }
}

impl<A: Accelerator> ElectronBeamSwarm<A> {
    /// Generates a set of electron beams using the given seeder and accelerator
    /// but does not propagate them.
    ///
    /// # Parameters
    ///
    /// - `seeder`: Seeder to use for generating acceleration positions.
    /// - `snapshot`: Snapshot representing the atmosphere.
    /// - `accelerator`: Accelerator to use for generating electron distributions.
    /// - `interpolator`: Interpolator to use.
    /// - `verbose`: Whether to print status messages.
    ///
    /// # Returns
    ///
    /// An `Option` which is either:
    ///
    /// - `Some`: Contains a new `ElectronBeamSwarm` with unpropagated electron beams.
    /// - `None`: No electron beams were generated.
    ///
    /// # Type parameters
    ///
    /// - `Sd`: Type of index seeder.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    pub fn generate_unpropagated<Sd, G, I>(seeder: Sd, snapshot: &mut SnapshotCacher3<G>, accelerator: A, interpolator: &I, verbose: Verbose) -> Option<Self>
    where Sd: IndexSeeder3,
          G: Grid3<fdt>,
          A: Accelerator + Sync + Send,
          A::DistributionType: Send,
          I: Interpolator3
    {
        A::prepare_snapshot_for_generation(snapshot).unwrap_or_else(|err| panic!("Snapshot preparation failed: {}", err));

        if verbose.is_yes() { println!("Generating electron distributions at {} acceleration sites", seeder.number_of_indices()); }
        let seed_iter = seeder.into_par_iter();
        let beams: Vec<_> = seed_iter.filter_map(
            |indices| {
                accelerator.generate_distribution(snapshot, interpolator, &indices)
                           .map(ElectronBeam::generate_unpropagated)
            }
        ).collect();

        if beams.is_empty() {
            None
        } else {
            Some(ElectronBeamSwarm{ beams })
        }
    }

    /// Generates a set of electron beams using the given seeder and accelerator,
    /// and propagates them through the atmosphere in the given snapshot.
    ///
    /// # Parameters
    ///
    /// - `seeder`: Seeder to use for generating start positions.
    /// - `snapshot`: Snapshot representing the atmosphere.
    /// - `accelerator`: Accelerator to use for generating initial electron distributions.
    /// - `interpolator`: Interpolator to use.
    /// - `stepper_factory`: Factory structure to use for producing steppers.
    /// - `verbose`: Whether to print status messages.
    ///
    /// # Returns
    ///
    /// An `Option` which is either:
    ///
    /// - `Some`: Contains a new `ElectronBeamSwarm` with propagated electron beams.
    /// - `None`: No electron beams were generated.
    ///
    /// # Type parameters
    ///
    /// - `Sd`: Type of index seeder.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    /// - `StF`: Type of stepper factory.
    pub fn generate_propagated<Sd, G, I, StF>(seeder: Sd, snapshot: &mut SnapshotCacher3<G>, accelerator: A, interpolator: &I, stepper_factory: StF, verbose: Verbose) -> Option<Self>
    where Sd: IndexSeeder3,
          G: Grid3<fdt>,
          A: Accelerator + Sync + Send,
          A::DistributionType: Send,
          I: Interpolator3,
          StF: StepperFactory3 + Sync
    {
        A::prepare_snapshot_for_generation(snapshot).unwrap_or_else(|err| panic!("Snapshot preparation failed: {}", err));

        if verbose.is_yes() { println!("Generating electron distributions at {} acceleration sites", seeder.number_of_indices()); }
        let seed_iter = seeder.into_par_iter();
        let distributions: Vec<_> = seed_iter.filter_map(
            |indices| {
                accelerator.generate_distribution(snapshot, interpolator, &indices)
            }
        ).collect();

        A::prepare_snapshot_for_propagation(snapshot).unwrap_or_else(|err| panic!("Snapshot preparation failed: {}", err));

        if verbose.is_yes() { println!("Attempting to propagate {} electron distributions", distributions.len()); }
        let beams: Vec<_> = distributions.into_par_iter().filter_map(
            |distribution| ElectronBeam::generate_propagated(distribution, snapshot, interpolator, stepper_factory.produce())
        ).collect();
        if verbose.is_yes() { println!("Successfully propagated {} electron distributions", beams.len()); }

        if beams.is_empty() {
            None
        } else {
            Some(ElectronBeamSwarm{ beams })
        }
    }

    /// Returns the number of beams making up the electron beam set.
    pub fn number_of_beams(&self) -> usize { self.beams.len() }

    /// Extracts and stores the value of the given scalar field at the initial position for each beam.
    pub fn extract_fixed_scalars<F, G, I>(&mut self, field: &ScalarField3<F, G>, interpolator: &I)
    where F: BFloat,
          G: Grid3<F>,
          I: Interpolator3
    {
        self.beams.par_iter_mut().for_each(|field_line| field_line.extract_fixed_scalar(field, interpolator));
    }

    /// Extracts and stores the value of the given vector field at the initial position for each beam.
    pub fn extract_fixed_vectors<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I)
    where F: BFloat,
          G: Grid3<F>,
          I: Interpolator3
    {
        self.beams.par_iter_mut().for_each(|field_line| field_line.extract_fixed_vector(field, interpolator));
    }

    /// Extracts and stores the value of the given scalar field at each position for each beam.
    pub fn extract_varying_scalars<F, G, I>(&mut self, field: &ScalarField3<F, G>, interpolator: &I)
    where F: BFloat,
          G: Grid3<F>,
          I: Interpolator3
    {
        self.beams.par_iter_mut().for_each(|field_line| field_line.extract_varying_scalars(field, interpolator));
    }

    /// Extracts and stores the value of the given vector field at each position for each beam.
    pub fn extract_varying_vectors<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I)
    where F: BFloat,
          G: Grid3<F>,
          I: Interpolator3
    {
        self.beams.par_iter_mut().for_each(|field_line| field_line.extract_varying_vectors(field, interpolator));
    }

    /// Serializes the electron beam data into pickle format and saves at the given path.
    ///
    /// All the electron beam data is saved as a single pickled structure.
    pub fn save_as_pickle<P: AsRef<path::Path>>(&self, file_path: P) -> io::Result<()> {
        utils::save_data_as_pickle(file_path, &self)
    }

    /// Serializes the electron beam data in parallel into pickle format and saves at the given path.
    ///
    /// The data is saved in a file containing a separate pickled structure for each electron beam.
    pub fn save_as_combined_pickles<P: AsRef<path::Path>>(&self, file_path: P) -> io::Result<()> {
        let write_to_buffer = |beam: &ElectronBeam<A::DistributionType>| {
            let mut buffer = Vec::with_capacity(beam.number_of_points()*mem::size_of::<ftr>());
            utils::write_data_as_pickle(&mut buffer, beam)?;
            Ok(buffer)
        };
        let buffers = self.beams.par_iter().map(write_to_buffer).collect::<io::Result<Vec<Vec<u8>>>>()?;

        let mut file = fs::File::create(file_path)?;
        file.write_all(&buffers.concat())?;
        Ok(())
    }
}

impl<D: Distribution> Serialize for ElectronBeam<D> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("ElectronBeam", 6)?;
        s.serialize_field("trajectory", &self.trajectory)?;
        s.serialize_field("fixed_scalar_values", &self.fixed_scalar_values)?;
        s.serialize_field("fixed_vector_values", &self.fixed_vector_values)?;
        s.serialize_field("varying_scalar_values", &self.varying_scalar_values)?;
        s.serialize_field("varying_vector_values", &self.varying_vector_values)?;
        s.serialize_field("metadata", &self.metadata)?;
        s.end()
    }
}
