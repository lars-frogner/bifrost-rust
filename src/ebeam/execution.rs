//! Execeution of electron beam simulations.

use super::distribution::power_law::acceleration::simple::{
    SimplePowerLawAccelerationConfig, SimplePowerLawAccelerator,
};
use super::distribution::power_law::{PitchAngleDistribution, PowerLawDistributionConfig};
use super::{feb, ElectronBeamSwarm};
use crate::geometry::Dim3;
use crate::grid::hor_regular::HorRegularGrid3;
use crate::grid::Grid3;
use crate::interpolation::poly_fit::PolyFitInterpolator3;
use crate::io::snapshot::{fdt, SnapshotCacher3, SnapshotReader3};
use crate::io::{Endianness, Verbose};
use crate::tracing::seeding::criterion::CriterionSeeder3;
use crate::tracing::seeding::IndexSeeder3;
use crate::tracing::stepping::rkf::rkf23::RKF23StepperFactory3;
use crate::tracing::stepping::rkf::rkf45::RKF45StepperFactory3;
use crate::tracing::stepping::rkf::{RKFStepperConfig, RKFStepperType};
use crate::units::solar::{U_E, U_L, U_T};
use std::path;

/// Convenience object for running offline electron beam simulations.
pub struct ElectronBeamSimulator {
    param_file_path: path::PathBuf,
    /// Whether to use use a normalized version of the reconnection factor when seeding.
    pub use_normalized_reconnection_factor: bool,
    /// Beams will be generated where the reconnection factor value is larger than this.
    pub reconnection_factor_threshold: fdt,
    /// Smallest depth at which electrons will be accelerated [Mm].
    pub minimum_acceleration_depth: fdt,
    /// Largest depth at which electrons will be accelerated [Mm].
    pub maximum_acceleration_depth: fdt,
    /// Configuration parameters for the electron distribution model.
    pub distribution_config: PowerLawDistributionConfig,
    /// Configuration parameters for the acceleration model.
    pub accelerator_config: SimplePowerLawAccelerationConfig,
    /// Duration of the acceleration events [s].
    pub acceleration_duration: feb,
    /// Fraction of the released reconnection energy going into acceleration of electrons.
    pub particle_energy_fraction: feb,
    /// Exponent of the inverse power-law describing the non-thermal electron distribution.
    pub power_law_delta: feb,
    /// Type of pitch angle distribution of the non-thermal electrons.
    pub pitch_angle_distribution: PitchAngleDistribution,
    /// Type of stepper to use.
    pub rkf_stepper_type: RKFStepperType,
    /// Configuration parameters for the stepper.
    pub rkf_stepper_config: RKFStepperConfig,
}

impl ElectronBeamSimulator {
    /// Creates a new electron beam generator with parameters read from
    /// the given parameter (.idl) file.
    pub fn from_param_file<P: AsRef<path::Path>>(param_file_path: P) -> Self {
        let param_file_path = param_file_path.as_ref().to_path_buf();
        let reader = Self::create_reader(&param_file_path.as_path());

        let use_normalized_reconnection_factor =
            Self::read_use_normalized_reconnection_factor(&reader);
        let reconnection_factor_threshold = Self::read_reconnection_factor_threshold(&reader);
        let minimum_acceleration_depth = Self::read_minimum_acceleration_depth(&reader);
        let maximum_acceleration_depth = Self::read_maximum_acceleration_depth(&reader);
        let distribution_config = Self::read_distribution_config(&reader);
        let accelerator_config = Self::read_accelerator_config(&reader);
        let acceleration_duration = Self::read_acceleration_duration(&reader);
        let particle_energy_fraction = Self::read_particle_energy_fraction(&reader);
        let power_law_delta = Self::read_power_law_delta(&reader);
        let pitch_angle_distribution = Self::read_pitch_angle_distribution(&reader);
        let rkf_stepper_type = Self::read_rkf_stepper_type(&reader);
        let rkf_stepper_config = Self::read_rkf_stepper_config(&reader);

        ElectronBeamSimulator {
            param_file_path,
            use_normalized_reconnection_factor,
            reconnection_factor_threshold,
            minimum_acceleration_depth,
            maximum_acceleration_depth,
            distribution_config,
            accelerator_config,
            acceleration_duration,
            particle_energy_fraction,
            power_law_delta,
            pitch_angle_distribution,
            rkf_stepper_type,
            rkf_stepper_config,
        }
    }

    /// Generates a new set of electron beams using the current parameter values.
    pub fn generate_beams(
        &self,
        propagate_beams: bool,
        extra_fixed_scalars: Option<&Vec<&str>>,
        extra_varying_scalars: Option<&Vec<&str>>,
        verbose: Verbose,
    ) -> ElectronBeamSwarm<SimplePowerLawAccelerator> {
        let mut snapshot = self.create_cacher();
        let seeder = self.create_seeder(&mut snapshot);
        let accelerator = self.create_accelerator();
        let interpolator = self.create_interpolator();
        let mut beams = if propagate_beams {
            match self.rkf_stepper_type {
                RKFStepperType::RKF23 => {
                    let stepper_factory = self.create_rkf23_stepper_factory();
                    ElectronBeamSwarm::generate_propagated(
                        seeder,
                        &mut snapshot,
                        accelerator,
                        &interpolator,
                        stepper_factory,
                        verbose,
                    )
                }
                RKFStepperType::RKF45 => {
                    let stepper_factory = self.create_rkf45_stepper_factory();
                    ElectronBeamSwarm::generate_propagated(
                        seeder,
                        &mut snapshot,
                        accelerator,
                        &interpolator,
                        stepper_factory,
                        verbose,
                    )
                }
            }
        } else {
            ElectronBeamSwarm::generate_unpropagated(
                seeder,
                &mut snapshot,
                accelerator,
                &interpolator,
                verbose,
            )
        };
        snapshot.drop_all_fields();

        if let Some(extra_fixed_scalars) = extra_fixed_scalars {
            for name in extra_fixed_scalars {
                beams.extract_fixed_scalars(
                    snapshot.obtain_scalar_field(name).unwrap_or_else(|err| {
                        panic!("Could not read {} from snapshot: {}", name, err)
                    }),
                    &interpolator,
                );
                snapshot.drop_scalar_field(name);
            }
        }
        if let Some(extra_varying_scalars) = extra_varying_scalars {
            for name in extra_varying_scalars {
                beams.extract_varying_scalars(
                    snapshot.obtain_scalar_field(name).unwrap_or_else(|err| {
                        panic!("Could not read {} from snapshot: {}", name, err)
                    }),
                    &interpolator,
                );
                snapshot.drop_scalar_field(name);
            }
        }
        beams
    }

    fn read_use_normalized_reconnection_factor<G: Grid3<fdt>>(reader: &SnapshotReader3<G>) -> bool {
        let use_normalized_reconnection_factor: u8 = reader
            .get_numerical_param("norm_krec")
            .unwrap_or_else(|err| panic!("{}", err));
        use_normalized_reconnection_factor > 0
    }

    fn read_reconnection_factor_threshold<G: Grid3<fdt>>(reader: &SnapshotReader3<G>) -> fdt {
        reader
            .get_numerical_param("krec_lim")
            .unwrap_or_else(|err| panic!("{}", err))
    }

    fn read_minimum_acceleration_depth<G: Grid3<fdt>>(reader: &SnapshotReader3<G>) -> fdt {
        reader
            .get_numerical_param("z_rec_ulim")
            .unwrap_or_else(|err| panic!("{}", err))
    }

    fn read_maximum_acceleration_depth<G: Grid3<fdt>>(reader: &SnapshotReader3<G>) -> fdt {
        reader
            .get_numerical_param("z_rec_llim")
            .unwrap_or_else(|err| panic!("{}", err))
    }

    fn read_distribution_config<G: Grid3<fdt>>(
        reader: &SnapshotReader3<G>,
    ) -> PowerLawDistributionConfig {
        let min_remaining_power_density = reader
            .get_numerical_param::<feb>("min_stop_en")
            .unwrap_or_else(|err| panic!("{}", err))
            * U_E
            / U_T;

        PowerLawDistributionConfig {
            min_remaining_power_density,
        }
    }

    fn read_accelerator_config<G: Grid3<fdt>>(
        reader: &SnapshotReader3<G>,
    ) -> SimplePowerLawAccelerationConfig {
        let min_total_power_density = reader
            .get_numerical_param::<feb>("min_beam_en")
            .unwrap_or_else(|err| panic!("{}", err))
            * U_E
            / U_T;

        let min_estimated_depletion_distance = reader
            .get_numerical_param::<feb>("min_stop_dist")
            .unwrap_or_else(|err| panic!("{}", err))
            * U_L;

        // Online version always uses 20 degrees
        let min_acceleration_angle = 20.0;

        // Online version always uses 4 keV
        let initial_cutoff_energy_guess = 4.0;

        let acceptable_root_finding_error = 1e-3;
        let max_root_finding_iterations = 100;

        SimplePowerLawAccelerationConfig {
            enforce_rejection: true,
            min_total_power_density,
            min_estimated_depletion_distance,
            min_acceleration_angle,
            initial_cutoff_energy_guess,
            acceptable_root_finding_error,
            max_root_finding_iterations,
        }
    }

    fn read_acceleration_duration<G: Grid3<fdt>>(reader: &SnapshotReader3<G>) -> feb {
        reader
            .get_numerical_param::<feb>("dt")
            .unwrap_or_else(|err| panic!("{}", err))
            * U_T
    }

    fn read_particle_energy_fraction<G: Grid3<fdt>>(reader: &SnapshotReader3<G>) -> feb {
        reader
            .get_numerical_param("qjoule_acc_frac")
            .unwrap_or_else(|err| panic!("{}", err))
    }

    fn read_power_law_delta<G: Grid3<fdt>>(reader: &SnapshotReader3<G>) -> feb {
        reader
            .get_numerical_param("power_law_index")
            .unwrap_or_else(|err| panic!("{}", err))
    }

    fn read_pitch_angle_distribution<G: Grid3<fdt>>(
        _reader: &SnapshotReader3<G>,
    ) -> PitchAngleDistribution {
        // Online version always uses a peaked distribution
        PitchAngleDistribution::Peaked
    }

    fn read_rkf_stepper_type<G: Grid3<fdt>>(_reader: &SnapshotReader3<G>) -> RKFStepperType {
        // Online version typically uses 5th order stepper
        RKFStepperType::RKF45
    }

    fn read_rkf_stepper_config<G: Grid3<fdt>>(reader: &SnapshotReader3<G>) -> RKFStepperConfig {
        let dense_step_length = reader
            .get_numerical_param("ds_out")
            .unwrap_or_else(|err| panic!("{}", err));

        // The following values are always used in the online version
        let max_step_attempts = 16;
        let absolute_tolerance = 1e-6;
        let relative_tolerance = 1e-6;
        let safety_factor = 0.9;
        let min_step_scale = 0.2;
        let max_step_scale = 10.0;
        let initial_step_length = 1e-4;
        let initial_error = 1e-4;
        let sudden_reversals_for_sink = 3;

        let use_pi_control: u8 = reader
            .get_numerical_param("use_pi_ctrl")
            .unwrap_or_else(|err| panic!("{}", err));
        let use_pi_control = use_pi_control > 0;

        RKFStepperConfig {
            dense_step_length,
            max_step_attempts,
            absolute_tolerance,
            relative_tolerance,
            safety_factor,
            min_step_scale,
            max_step_scale,
            initial_step_length,
            initial_error,
            sudden_reversals_for_sink,
            use_pi_control,
        }
    }

    fn create_reader(param_file_path: &path::Path) -> SnapshotReader3<HorRegularGrid3<fdt>> {
        SnapshotReader3::new(param_file_path, Endianness::Little)
            .unwrap_or_else(|err| panic!("Could not create snapshot reader: {}", err))
    }

    fn create_cacher(&self) -> SnapshotCacher3<HorRegularGrid3<fdt>> {
        Self::create_reader(&self.param_file_path.as_path()).into_cacher()
    }

    fn create_seeder<G: Grid3<fdt>>(&self, snapshot: &mut SnapshotCacher3<G>) -> CriterionSeeder3 {
        let reconnection_factor_variable = if self.use_normalized_reconnection_factor {
            "krec_norm"
        } else {
            "krec"
        };
        let reconnection_factor_field = snapshot
            .obtain_scalar_field(reconnection_factor_variable)
            .unwrap_or_else(|err| panic!("Could not obtain reconnection factor field: {}", err));

        let mut seeder = CriterionSeeder3::on_scalar_field_values(
            reconnection_factor_field,
            &|reconnection_factor| reconnection_factor >= self.reconnection_factor_threshold,
        );

        snapshot.drop_scalar_field(reconnection_factor_variable);

        let z_coordinates = &snapshot.reader().grid().centers()[Dim3::Z];
        seeder.retain_indices(|indices| {
            z_coordinates[indices[Dim3::Z]] >= self.minimum_acceleration_depth
                && z_coordinates[indices[Dim3::Z]] <= self.maximum_acceleration_depth
        });
        seeder
    }

    fn create_accelerator(&self) -> SimplePowerLawAccelerator {
        SimplePowerLawAccelerator::new(
            self.distribution_config.clone(),
            self.accelerator_config.clone(),
            self.acceleration_duration,
            self.particle_energy_fraction,
            self.power_law_delta,
            self.pitch_angle_distribution,
        )
    }

    fn create_interpolator(&self) -> PolyFitInterpolator3 {
        PolyFitInterpolator3
    }

    fn create_rkf23_stepper_factory(&self) -> RKF23StepperFactory3 {
        RKF23StepperFactory3::new(self.rkf_stepper_config.clone())
    }

    fn create_rkf45_stepper_factory(&self) -> RKF45StepperFactory3 {
        RKF45StepperFactory3::new(self.rkf_stepper_config.clone())
    }
}
