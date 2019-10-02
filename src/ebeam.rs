//! Non-thermal electron beam physics in Bifrost simulations.

pub mod accelerator;
pub mod distribution;
pub mod execution;

use self::accelerator::Accelerator;
use self::distribution::{DepletionStatus, Distribution, PropagationResult};
use crate::field::{ScalarField3, VectorField3};
use crate::geometry::{Dim3, Point3, Vec3};
use crate::grid::Grid3;
use crate::interpolation::Interpolator3;
use crate::io::snapshot::{fdt, SnapshotCacher3};
use crate::io::utils;
use crate::io::Verbose;
use crate::num::BFloat;
use crate::tracing::seeding::IndexSeeder3;
use crate::tracing::stepping::{Stepper3, StepperFactory3, StepperInstruction};
use crate::tracing::{self, ftr, TracerResult};
use rayon::prelude::*;
use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::{fs, io, path};
use Dim3::{X, Y, Z};

/// Floating-point precision to use for electron beam physics.
#[allow(non_camel_case_types)]
pub type feb = f64;

type BeamTrajectory = (Vec<ftr>, Vec<ftr>, Vec<ftr>);
type FixedBeamScalarValues = HashMap<String, Vec<feb>>;
type FixedBeamVectorValues = HashMap<String, Vec<Vec3<feb>>>;
type VaryingBeamScalarValues = HashMap<String, Vec<Vec<feb>>>;
type VaryingBeamVectorValues = HashMap<String, Vec<Vec<Vec3<feb>>>>;

/// Defines the required behaviour of a type representing
/// a collection of objects holding electron beam properties.
pub trait BeamPropertiesCollection: Default + Sync + Send {
    type Item: Send;

    /// Moves the property values into the appropriate entries of the
    /// gives hash maps.
    fn distribute_into_maps(
        self,
        scalar_values: &mut FixedBeamScalarValues,
        vector_values: &mut FixedBeamVectorValues,
    );
}

/// Defines the required behaviour of a type representing
/// a collection of objects holding electron beam metadata.
pub trait BeamMetadataCollection:
    Clone + Default + std::fmt::Debug + Serialize + Sync + Send
{
    type Item: Clone + std::fmt::Debug + Send;
}

/// A set of non-thermal electron beams.
#[derive(Clone, Debug)]
pub struct ElectronBeamSwarm<A: Accelerator> {
    properties: ElectronBeamSwarmProperties,
    metadata: A::MetadataCollectionType,
    verbose: Verbose,
}

#[derive(Clone, Debug)]
struct ElectronBeamSwarmProperties {
    number_of_beams: usize,
    fixed_scalar_values: FixedBeamScalarValues,
    fixed_vector_values: FixedBeamVectorValues,
    varying_scalar_values: VaryingBeamScalarValues,
    varying_vector_values: VaryingBeamVectorValues,
}

struct UnpropagatedElectronBeam<D: Distribution> {
    acceleration_position: Point3<ftr>,
    distribution_properties: <D::PropertiesCollectionType as BeamPropertiesCollection>::Item,
}

struct PropagatedElectronBeam<D: Distribution> {
    trajectory: BeamTrajectory,
    distribution_properties: <D::PropertiesCollectionType as BeamPropertiesCollection>::Item,
    total_propagation_distance: feb,
    deposited_power_densities: Vec<feb>,
}

impl<D> FromParallelIterator<UnpropagatedElectronBeam<D>> for ElectronBeamSwarmProperties
where
    D: Distribution,
    D::PropertiesCollectionType:
        ParallelExtend<<D::PropertiesCollectionType as BeamPropertiesCollection>::Item>,
{
    fn from_par_iter<I>(par_iter: I) -> Self
    where
        I: IntoParallelIterator<Item = UnpropagatedElectronBeam<D>>,
    {
        let nested_tuples_iter = par_iter.into_par_iter().map(|data| {
            (
                data.acceleration_position[X],
                (
                    data.acceleration_position[Y],
                    (data.acceleration_position[Z], data.distribution_properties),
                ),
            )
        });

        // Unzip the iterator of nested tuples into individual collections.
        // The unzipping has to be performed in multiple stages to avoid excessive
        // compilation times.

        let (
            acceleration_positions_x,
            (acceleration_positions_y, (acceleration_positions_z, nested_tuples)),
        ): (Vec<_>, (Vec<_>, (Vec<_>, Vec<_>))) = nested_tuples_iter.unzip();

        let mut distribution_properties = D::PropertiesCollectionType::default();
        distribution_properties.par_extend(nested_tuples.into_par_iter());

        let number_of_beams = acceleration_positions_x.len();
        let mut fixed_scalar_values = HashMap::new();
        let mut fixed_vector_values = HashMap::new();
        let varying_scalar_values = HashMap::new();
        let varying_vector_values = HashMap::new();

        distribution_properties
            .distribute_into_maps(&mut fixed_scalar_values, &mut fixed_vector_values);

        fixed_scalar_values.insert("x0".to_string(), acceleration_positions_x);
        fixed_scalar_values.insert("y0".to_string(), acceleration_positions_y);
        fixed_scalar_values.insert("z0".to_string(), acceleration_positions_z);

        ElectronBeamSwarmProperties {
            number_of_beams,
            fixed_scalar_values,
            fixed_vector_values,
            varying_scalar_values,
            varying_vector_values,
        }
    }
}

impl<D> FromParallelIterator<PropagatedElectronBeam<D>> for ElectronBeamSwarmProperties
where
    D: Distribution,
    D::PropertiesCollectionType:
        ParallelExtend<<D::PropertiesCollectionType as BeamPropertiesCollection>::Item>,
{
    fn from_par_iter<I>(par_iter: I) -> Self
    where
        I: IntoParallelIterator<Item = PropagatedElectronBeam<D>>,
    {
        let nested_tuples_iter = par_iter.into_par_iter().map(|data| {
            (
                data.trajectory.0,
                (
                    data.trajectory.1,
                    (
                        data.trajectory.2,
                        (
                            data.distribution_properties,
                            (
                                data.total_propagation_distance,
                                data.deposited_power_densities,
                            ),
                        ),
                    ),
                ),
            )
        });

        // Unzip the iterator of nested tuples into individual collections.
        // The unzipping has to be performed in multiple stages to avoid excessive
        // compilation times.

        let (trajectories_x, (trajectories_y, (trajectories_z, nested_tuples))): (
            Vec<_>,
            (Vec<_>, (Vec<_>, Vec<_>)),
        ) = nested_tuples_iter.unzip();

        let (distribution_properties, nested_tuples): (D::PropertiesCollectionType, Vec<_>) =
            nested_tuples.into_par_iter().unzip();

        let (total_propagation_distances, deposited_power_densities): (Vec<_>, Vec<_>) =
            nested_tuples.into_par_iter().unzip();

        let number_of_beams = trajectories_x.len();
        let mut fixed_scalar_values = HashMap::new();
        let mut fixed_vector_values = HashMap::new();
        let mut varying_scalar_values = HashMap::new();
        let varying_vector_values = HashMap::new();

        distribution_properties
            .distribute_into_maps(&mut fixed_scalar_values, &mut fixed_vector_values);

        fixed_scalar_values.insert(
            "x0".to_string(),
            trajectories_x
                .par_iter()
                .map(|trajectory_x| trajectory_x[0])
                .collect(),
        );
        fixed_scalar_values.insert(
            "y0".to_string(),
            trajectories_y
                .par_iter()
                .map(|trajectory_y| trajectory_y[0])
                .collect(),
        );
        fixed_scalar_values.insert(
            "z0".to_string(),
            trajectories_z
                .par_iter()
                .map(|trajectory_z| trajectory_z[0])
                .collect(),
        );
        fixed_scalar_values.insert(
            "total_propagation_distance".to_string(),
            total_propagation_distances,
        );

        varying_scalar_values.insert("x".to_string(), trajectories_x);
        varying_scalar_values.insert("y".to_string(), trajectories_y);
        varying_scalar_values.insert("z".to_string(), trajectories_z);
        varying_scalar_values.insert(
            "deposited_power_density".to_string(),
            deposited_power_densities,
        );

        ElectronBeamSwarmProperties {
            number_of_beams,
            fixed_scalar_values,
            fixed_vector_values,
            varying_scalar_values,
            varying_vector_values,
        }
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
    /// A new `ElectronBeamSwarm` with unpropagated electron beams.
    ///
    /// # Type parameters
    ///
    /// - `Sd`: Type of index seeder.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    pub fn generate_unpropagated<Sd, G, I>(seeder: Sd, snapshot: &mut SnapshotCacher3<G>, accelerator: A, interpolator: &I, verbose: Verbose) -> Self
    where Sd: IndexSeeder3,
          G: Grid3<fdt>,
          A: Accelerator + Sync,
          A::DistributionType: Send,
          <A::DistributionType as Distribution>::PropertiesCollectionType: ParallelExtend<<<A::DistributionType as Distribution>::PropertiesCollectionType as BeamPropertiesCollection>::Item>,
          I: Interpolator3
    {
        if verbose.is_yes() {
            println!("Found {} acceleration sites", seeder.number_of_indices());
        }

        let (distributions, metadata) = accelerator
            .generate_distributions(seeder, snapshot, interpolator, verbose)
            .unwrap_or_else(|err| panic!("Could not read field from snapshot: {}", err));

        let properties: ElectronBeamSwarmProperties = distributions
            .into_par_iter()
            .map(UnpropagatedElectronBeam::<A::DistributionType>::generate)
            .collect();

        ElectronBeamSwarm {
            properties,
            metadata,
            verbose,
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
    /// A new `ElectronBeamSwarm` with propagated electron beams.
    ///
    /// # Type parameters
    ///
    /// - `Sd`: Type of index seeder.
    /// - `G`: Type of grid.
    /// - `I`: Type of interpolator.
    /// - `StF`: Type of stepper factory.
    pub fn generate_propagated<Sd, G, I, StF>(seeder: Sd, snapshot: &mut SnapshotCacher3<G>, accelerator: A, interpolator: &I, stepper_factory: StF, verbose: Verbose) -> Self
    where Sd: IndexSeeder3,
          G: Grid3<fdt>,
          A: Accelerator + Sync + Send,
          A::DistributionType: Send,
          <A::DistributionType as Distribution>::PropertiesCollectionType: ParallelExtend<<<A::DistributionType as Distribution>::PropertiesCollectionType as BeamPropertiesCollection>::Item>,
          I: Interpolator3,
          StF: StepperFactory3 + Sync
    {
        if verbose.is_yes() {
            println!("Found {} acceleration sites", seeder.number_of_indices());
        }

        let (distributions, metadata) = accelerator
            .generate_distributions(seeder, snapshot, interpolator, verbose)
            .unwrap_or_else(|err| panic!("Could not read field from snapshot: {}", err));

        if verbose.is_yes() {
            println!(
                "Attempting to propagate {} electron beams",
                distributions.len()
            );
        }

        let properties: ElectronBeamSwarmProperties = distributions
            .into_par_iter()
            .filter_map(|distribution| {
                PropagatedElectronBeam::<A::DistributionType>::generate(
                    distribution,
                    snapshot,
                    interpolator,
                    stepper_factory.produce(),
                )
            })
            .collect();

        if verbose.is_yes() {
            println!(
                "Successfully propagated {} electron beams",
                properties.number_of_beams
            );
        }

        ElectronBeamSwarm {
            properties,
            metadata,
            verbose,
        }
    }

    /// Returns the number of beams making up the electron beam set.
    pub fn number_of_beams(&self) -> usize {
        self.properties.number_of_beams
    }

    /// Extracts and stores the value of the given scalar field at the initial position for each beam.
    pub fn extract_fixed_scalars<F, G, I>(&mut self, field: &ScalarField3<F, G>, interpolator: &I)
    where
        F: BFloat,
        G: Grid3<F>,
        I: Interpolator3,
    {
        if self.verbose.is_yes() {
            println!("Extracting {} at acceleration sites", field.name());
        }
        let initial_coords_x = &self.properties.fixed_scalar_values["x0"];
        let initial_coords_y = &self.properties.fixed_scalar_values["y0"];
        let initial_coords_z = &self.properties.fixed_scalar_values["z0"];
        let values = initial_coords_x
            .into_par_iter()
            .zip(initial_coords_y)
            .zip(initial_coords_z)
            .map(|((&beam_x0, &beam_y0), &beam_z0)| {
                let acceleration_position = Point3::from_components(beam_x0, beam_y0, beam_z0);
                let value = interpolator
                    .interp_scalar_field(field, &acceleration_position)
                    .expect_inside();
                num::NumCast::from(value).expect("Conversion failed.")
            })
            .collect();
        self.properties
            .fixed_scalar_values
            .insert(field.name().to_string(), values);
    }

    /// Extracts and stores the value of the given vector field at the initial position for each beam.
    pub fn extract_fixed_vectors<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I)
    where
        F: BFloat,
        G: Grid3<F>,
        I: Interpolator3,
    {
        if self.verbose.is_yes() {
            println!("Extracting {} at acceleration sites", field.name());
        }
        let initial_coords_x = &self.properties.fixed_scalar_values["x0"];
        let initial_coords_y = &self.properties.fixed_scalar_values["y0"];
        let initial_coords_z = &self.properties.fixed_scalar_values["z0"];
        let vectors = initial_coords_x
            .into_par_iter()
            .zip(initial_coords_y)
            .zip(initial_coords_z)
            .map(|((&beam_x0, &beam_y0), &beam_z0)| {
                let acceleration_position = Point3::from_components(beam_x0, beam_y0, beam_z0);
                let vector = interpolator
                    .interp_vector_field(field, &acceleration_position)
                    .expect_inside();
                Vec3::from(&vector)
            })
            .collect();
        self.properties
            .fixed_vector_values
            .insert(field.name().to_string(), vectors);
    }

    /// Extracts and stores the value of the given scalar field at each position for each beam.
    pub fn extract_varying_scalars<F, G, I>(&mut self, field: &ScalarField3<F, G>, interpolator: &I)
    where
        F: BFloat,
        G: Grid3<F>,
        I: Interpolator3,
    {
        if self.verbose.is_yes() {
            println!("Extracting {} along beam trajectories", field.name());
        }
        let coords_x = &self.properties.varying_scalar_values["x"];
        let coords_y = &self.properties.varying_scalar_values["y"];
        let coords_z = &self.properties.varying_scalar_values["z"];
        let values = coords_x
            .into_par_iter()
            .zip(coords_y)
            .zip(coords_z)
            .map(|((beam_coords_x, beam_coords_y), beam_coords_z)| {
                beam_coords_x
                    .iter()
                    .zip(beam_coords_y)
                    .zip(beam_coords_z)
                    .map(|((&beam_x, &beam_y), &beam_z)| {
                        let position = Point3::from_components(beam_x, beam_y, beam_z);
                        let value = interpolator
                            .interp_scalar_field(field, &position)
                            .expect_inside();
                        num::NumCast::from(value).expect("Conversion failed.")
                    })
                    .collect()
            })
            .collect();
        self.properties
            .varying_scalar_values
            .insert(field.name().to_string(), values);
    }

    /// Extracts and stores the value of the given vector field at each position for each beam.
    pub fn extract_varying_vectors<F, G, I>(&mut self, field: &VectorField3<F, G>, interpolator: &I)
    where
        F: BFloat,
        G: Grid3<F>,
        I: Interpolator3,
    {
        if self.verbose.is_yes() {
            println!("Extracting {} along beam trajectories", field.name());
        }
        let coords_x = &self.properties.varying_scalar_values["x"];
        let coords_y = &self.properties.varying_scalar_values["y"];
        let coords_z = &self.properties.varying_scalar_values["z"];
        let vectors = coords_x
            .into_par_iter()
            .zip(coords_y)
            .zip(coords_z)
            .map(|((beam_coords_x, beam_coords_y), beam_coords_z)| {
                beam_coords_x
                    .iter()
                    .zip(beam_coords_y)
                    .zip(beam_coords_z)
                    .map(|((&beam_x, &beam_y), &beam_z)| {
                        let position = Point3::from_components(beam_x, beam_y, beam_z);
                        let vector = interpolator
                            .interp_vector_field(field, &position)
                            .expect_inside();
                        Vec3::from(&vector)
                    })
                    .collect()
            })
            .collect();
        self.properties
            .varying_vector_values
            .insert(field.name().to_string(), vectors);
    }

    /// Serializes the electron beam data into JSON format and saves at the given path.
    pub fn save_as_json<P: AsRef<path::Path>>(&self, file_path: P) -> io::Result<()> {
        if self.verbose.is_yes() {
            println!(
                "Saving beam data in JSON format in {}",
                file_path.as_ref().display()
            );
        }
        utils::save_data_as_json(file_path, &self)
    }

    /// Serializes the electron beam data into pickle format and saves at the given path.
    ///
    /// All the electron beam data is saved as a single pickled structure.
    pub fn save_as_pickle<P: AsRef<path::Path>>(&self, file_path: P) -> io::Result<()> {
        if self.verbose.is_yes() {
            println!(
                "Saving beams as single pickle object in {}",
                file_path.as_ref().display()
            );
        }
        utils::save_data_as_pickle(file_path, &self)
    }

    /// Serializes the electron beam data fields in parallel into pickle format and saves at the given path.
    ///
    /// The data fields are saved as separate pickle objects in the same file.
    pub fn save_as_combined_pickles<P: AsRef<path::Path>>(&self, file_path: P) -> io::Result<()> {
        if self.verbose.is_yes() {
            println!("Saving beams in {}", file_path.as_ref().display());
        }
        let mut buffer_1 = Vec::new();
        utils::write_data_as_pickle(&mut buffer_1, &self.number_of_beams())?;

        let (mut result_2, mut result_3, mut result_4, mut result_5, mut result_6) =
            (Ok(()), Ok(()), Ok(()), Ok(()), Ok(()));
        let (mut buffer_2, mut buffer_3, mut buffer_4, mut buffer_5, mut buffer_6) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        rayon::scope(|s| {
            s.spawn(|_| {
                result_2 =
                    utils::write_data_as_pickle(&mut buffer_2, &self.properties.fixed_scalar_values)
            });
            s.spawn(|_| {
                result_3 =
                    utils::write_data_as_pickle(&mut buffer_3, &self.properties.fixed_vector_values)
            });
            s.spawn(|_| {
                result_4 = utils::write_data_as_pickle(
                    &mut buffer_4,
                    &self.properties.varying_scalar_values,
                )
            });
            s.spawn(|_| {
                result_5 = utils::write_data_as_pickle(
                    &mut buffer_5,
                    &self.properties.varying_vector_values,
                )
            });
            s.spawn(|_| result_6 = utils::write_data_as_pickle(&mut buffer_6, &self.metadata));
        });
        result_2?;
        result_3?;
        result_4?;
        result_5?;
        result_6?;

        let mut file = fs::File::create(file_path)?;
        file.write_all(&[buffer_1, buffer_2, buffer_3, buffer_4, buffer_5, buffer_6].concat())?;
        Ok(())
    }
}

impl<D: Distribution> UnpropagatedElectronBeam<D> {
    fn generate(distribution: D) -> Self {
        let acceleration_position = Point3::from(distribution.acceleration_position());
        UnpropagatedElectronBeam {
            acceleration_position,
            distribution_properties: distribution.properties(),
        }
    }
}

impl<D: Distribution> PropagatedElectronBeam<D> {
    fn generate<G, I, S>(
        mut distribution: D,
        snapshot: &SnapshotCacher3<G>,
        interpolator: &I,
        stepper: S,
    ) -> Option<Self>
    where
        G: Grid3<fdt>,
        I: Interpolator3,
        S: Stepper3,
    {
        let mut trajectory = (Vec::new(), Vec::new(), Vec::new());
        let mut deposited_power_densities = Vec::new();
        let mut total_propagation_distance = 0.0;

        let magnetic_field = snapshot.cached_vector_field("b");
        let start_position = Point3::from(distribution.acceleration_position());

        let tracer_result = tracing::trace_3d_field_line_dense(
            magnetic_field,
            interpolator,
            stepper,
            &start_position,
            distribution.propagation_sense(),
            &mut |displacement, position, distance| {
                let PropagationResult {
                    deposited_power_density,
                    deposition_position,
                    depletion_status,
                } = distribution.propagate(snapshot, interpolator, displacement, position);

                trajectory.0.push(deposition_position[X]);
                trajectory.1.push(deposition_position[Y]);
                trajectory.2.push(deposition_position[Z]);
                deposited_power_densities.push(deposited_power_density);
                total_propagation_distance = distance;

                match depletion_status {
                    DepletionStatus::Undepleted => StepperInstruction::Continue,
                    DepletionStatus::Depleted => StepperInstruction::Terminate,
                }
            },
        );

        let distribution_properties = distribution.properties();

        match tracer_result {
            TracerResult::Ok(_) => Some(PropagatedElectronBeam {
                trajectory,
                distribution_properties,
                total_propagation_distance,
                deposited_power_densities,
            }),
            TracerResult::Void => None,
        }
    }
}

impl<A: Accelerator> Serialize for ElectronBeamSwarm<A> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("ElectronBeamSwarm", 6)?;
        s.serialize_field("number_of_beams", &self.number_of_beams())?;
        s.serialize_field("fixed_scalar_values", &self.properties.fixed_scalar_values)?;
        s.serialize_field("fixed_vector_values", &self.properties.fixed_vector_values)?;
        s.serialize_field(
            "varying_scalar_values",
            &self.properties.varying_scalar_values,
        )?;
        s.serialize_field(
            "varying_vector_values",
            &self.properties.varying_vector_values,
        )?;
        s.serialize_field("metadata", &self.metadata)?;
        s.end()
    }
}
