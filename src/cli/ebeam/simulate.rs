//! Command line interface for simulating electron beams.

use super::{
    accelerator::simple_power_law::{
        construct_simple_power_law_accelerator_config_from_options,
        create_simple_power_law_accelerator_subcommand,
    },
    detection::{
        manual::{
            construct_manual_reconnection_site_detector_from_options,
            create_manual_reconnection_site_detector_subcommand,
        },
        simple::{
            construct_simple_reconnection_site_detector_config_from_options,
            create_simple_reconnection_site_detector_subcommand,
        },
    },
    distribution::power_law::{
        construct_power_law_distribution_config_from_options,
        create_power_law_distribution_subcommand,
    },
};
use crate::{
    cli::{
        interpolation::poly_fit::{
            construct_poly_fit_interpolator_config_from_options,
            create_poly_fit_interpolator_subcommand,
        },
        snapshot::SnapNumInRange,
        tracing::{
            extract_magnitude_name,
            stepping::rkf::{
                construct_rkf_stepper_config_from_options, create_rkf_stepper_subcommand,
            },
        },
    },
    create_subcommand,
    ebeam::{
        accelerator::Accelerator,
        detection::{
            simple::{SimpleReconnectionSiteDetector, SimpleReconnectionSiteDetectorConfig},
            ReconnectionSiteDetector,
        },
        distribution::{
            power_law::{
                acceleration::simple::{
                    SimplePowerLawAccelerationConfig, SimplePowerLawAccelerator,
                },
                PowerLawDistributionConfig,
            },
            Distribution,
        },
        BeamPropertiesCollection, ElectronBeamSwarm,
    },
    exit_on_error, exit_with_error,
    grid::Grid3,
    interpolation::{
        poly_fit::{PolyFitInterpolator3, PolyFitInterpolatorConfig},
        Interpolator3,
    },
    io::{
        snapshot::{self, fdt, SnapshotCacher3, SnapshotReader3},
        utils::AtomicOutputPath,
    },
    tracing::stepping::rkf::{
        rkf23::RKF23StepperFactory3, rkf45::RKF45StepperFactory3, RKFStepperConfig, RKFStepperType,
    },
};
use clap::{App, Arg, ArgMatches, SubCommand};
use rayon::prelude::*;
use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

/// Builds a representation of the `ebeam-simulate` command line subcommand.
pub fn create_simulate_subcommand<'a, 'b>() -> App<'a, 'b> {
    let app = SubCommand::with_name("simulate")
        .about("Simulate electron beams in the snapshot")
        .long_about(
            "Simulate electron beams in the snapshot.\n\
             Each beam originates at a reconnection site, where a non-thermal electron\n\
             distribution is generated by an acceleration mechanism. The distribution\n\
             propagates along the magnetic field and deposits its energy through interactions\n\
             with the surrounding plasma.",
        )
        .after_help(
            "You can use subcommands to configure each action. The subcommands must be specified\n\
             in the order detector -> distribution -> accelerator -> interpolator -> stepper,\n\
             with options for each action directly following the subcommand. Any action(s) can be\n\
             left unspecified, in which case the default implementation and parameters are used\n\
             for that action.",
        )
        .help_message("Print help information")
        .arg(
            Arg::with_name("output-file")
                .value_name("OUTPUT_FILE")
                .help(
                    "Path of the file where the beam data should be saved\n\
                       Writes in the following format based on the file extension:\
                       \n    *.fl: Creates a binary file readable by the backstaff Python package\
                       \n    *.pickle: Creates a Python pickle file\
                       \n    *.json: Creates a JSON file\
                       \n    *.h5part: Creates a H5Part file (requires the hdf5 feature)",
                )
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("overwrite")
                .long("overwrite")
                .help("Automatically overwrite any existing file (unless listed as protected)"),
        )
        .arg(
            Arg::with_name("generate-only")
                .short("g")
                .long("generate-only")
                .help("Do not propagate the generated beams"),
        )
        .arg(
            Arg::with_name("extra-fixed-scalars")
                .long("extra-fixed-scalars")
                .require_equals(true)
                .require_delimiter(true)
                .value_name("NAMES")
                .help(
                    "List of scalar fields to extract at acceleration sites\n \
                     (comma-separated)",
                )
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("extra-fixed-vectors")
                .long("extra-fixed-vectors")
                .require_equals(true)
                .require_delimiter(true)
                .value_name("NAMES")
                .help(
                    "List of vector fields to extract at acceleration sites\n \
                     (comma-separated)",
                )
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("extra-varying-scalars")
                .long("extra-varying-scalars")
                .require_equals(true)
                .require_delimiter(true)
                .value_name("NAMES")
                .help(
                    "List of scalar fields to extract along beam trajectories\n \
                     (comma-separated)",
                )
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("extra-varying-vectors")
                .long("extra-varying-vectors")
                .require_equals(true)
                .require_delimiter(true)
                .value_name("NAMES")
                .help(
                    "List of vector fields to extract along beam trajectories\n \
                     (comma-separated)",
                )
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("drop-h5part-id")
                .long("drop-h5part-id")
                .help(
                    "Reduce H5Part file size by excluding particle IDs required by some tools\n\
                     (e.g. VisIt)",
                ),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("Print status messages while simulating electron beams"),
        )
        .arg(
            Arg::with_name("print-parameter-values")
                .short("p")
                .long("print-parameter-values")
                .help("Prints the values of all the parameters that will be used"),
        );

    app.subcommand(
        create_subcommand!(simulate, simple_reconnection_site_detector)
            .subcommand(
                create_subcommand!(simple_reconnection_site_detector, power_law_distribution)
                    .subcommand(
                        create_subcommand!(power_law_distribution, simple_power_law_accelerator)
                            .subcommand(
                                create_subcommand!(
                                    simple_power_law_accelerator,
                                    poly_fit_interpolator
                                )
                                .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
                            )
                            .subcommand(create_subcommand!(
                                simple_power_law_accelerator,
                                rkf_stepper
                            )),
                    )
                    .subcommand(
                        create_subcommand!(power_law_distribution, poly_fit_interpolator)
                            .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
                    )
                    .subcommand(create_subcommand!(power_law_distribution, rkf_stepper)),
            )
            .subcommand(
                create_subcommand!(
                    simple_reconnection_site_detector,
                    simple_power_law_accelerator
                )
                .subcommand(
                    create_subcommand!(simple_power_law_accelerator, poly_fit_interpolator)
                        .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
                )
                .subcommand(create_subcommand!(
                    simple_power_law_accelerator,
                    rkf_stepper
                )),
            )
            .subcommand(
                create_subcommand!(simple_reconnection_site_detector, poly_fit_interpolator)
                    .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
            )
            .subcommand(create_subcommand!(
                simple_reconnection_site_detector,
                rkf_stepper
            )),
    )
    .subcommand(
        create_subcommand!(simulate, manual_reconnection_site_detector)
            .subcommand(
                create_subcommand!(manual_reconnection_site_detector, power_law_distribution)
                    .subcommand(
                        create_subcommand!(power_law_distribution, simple_power_law_accelerator)
                            .subcommand(
                                create_subcommand!(
                                    simple_power_law_accelerator,
                                    poly_fit_interpolator
                                )
                                .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
                            )
                            .subcommand(create_subcommand!(
                                simple_power_law_accelerator,
                                rkf_stepper
                            )),
                    )
                    .subcommand(
                        create_subcommand!(power_law_distribution, poly_fit_interpolator)
                            .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
                    )
                    .subcommand(create_subcommand!(power_law_distribution, rkf_stepper)),
            )
            .subcommand(
                create_subcommand!(
                    manual_reconnection_site_detector,
                    simple_power_law_accelerator
                )
                .subcommand(
                    create_subcommand!(simple_power_law_accelerator, poly_fit_interpolator)
                        .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
                )
                .subcommand(create_subcommand!(
                    simple_power_law_accelerator,
                    rkf_stepper
                )),
            )
            .subcommand(
                create_subcommand!(manual_reconnection_site_detector, poly_fit_interpolator)
                    .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
            )
            .subcommand(create_subcommand!(
                manual_reconnection_site_detector,
                rkf_stepper
            )),
    )
    .subcommand(
        create_subcommand!(simulate, power_law_distribution)
            .subcommand(
                create_subcommand!(power_law_distribution, simple_power_law_accelerator)
                    .subcommand(
                        create_subcommand!(simple_power_law_accelerator, poly_fit_interpolator)
                            .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
                    )
                    .subcommand(create_subcommand!(
                        simple_power_law_accelerator,
                        rkf_stepper
                    )),
            )
            .subcommand(
                create_subcommand!(power_law_distribution, poly_fit_interpolator)
                    .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
            )
            .subcommand(create_subcommand!(power_law_distribution, rkf_stepper)),
    )
    .subcommand(
        create_subcommand!(simulate, simple_power_law_accelerator)
            .subcommand(
                create_subcommand!(simple_power_law_accelerator, poly_fit_interpolator)
                    .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
            )
            .subcommand(create_subcommand!(
                simple_power_law_accelerator,
                rkf_stepper
            )),
    )
    .subcommand(
        create_subcommand!(simulate, poly_fit_interpolator)
            .subcommand(create_subcommand!(poly_fit_interpolator, rkf_stepper)),
    )
    .subcommand(create_subcommand!(simulate, rkf_stepper))
}

/// Runs the actions for the `ebeam-simulate` subcommand using the given arguments.
pub fn run_simulate_subcommand<G, R>(
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_in_range: &Option<SnapNumInRange>,
    protected_file_types: &[&str],
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
{
    run_with_selected_detector(arguments, snapshot, snap_num_in_range, protected_file_types);
}

#[derive(Copy, Clone, Debug)]
enum OutputType {
    Fl,
    Pickle,
    JSON,
    #[cfg(feature = "hdf5")]
    H5Part,
}

impl OutputType {
    fn from_path<P: AsRef<Path>>(file_path: P) -> Self {
        Self::from_extension(
            file_path
                .as_ref()
                .extension()
                .unwrap_or_else(|| {
                    exit_with_error!(
                        "Error: Missing extension for output file\n\
                         Valid extensions are: {}",
                        Self::valid_extensions_string()
                    )
                })
                .to_string_lossy()
                .as_ref(),
        )
    }

    fn from_extension(extension: &str) -> Self {
        match extension {
            "fl" => Self::Fl,
            "pickle" => Self::Pickle,
            "json" => Self::JSON,
            "h5part" => {
                #[cfg(feature = "hdf5")]
                {
                    Self::H5Part
                }
                #[cfg(not(feature = "hdf5"))]
                exit_with_error!("Error: Compile with hdf5 feature in order to write H5Part files\n\
                                  Tip: Use cargo flag --features=hdf5 and make sure the HDF5 library is available");
            }
            invalid => exit_with_error!(
                "Error: Invalid extension {} for output file\n\
                 Valid extensions are: {}",
                invalid,
                Self::valid_extensions_string()
            ),
        }
    }

    fn valid_extensions_string() -> String {
        format!(
            "fl, pickle, json{}",
            if cfg!(feature = "hdf5") {
                ", h5part"
            } else {
                ""
            }
        )
    }
}

impl fmt::Display for OutputType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Fl => "fl",
                Self::Pickle => "pickle",
                Self::JSON => "json",
                #[cfg(feature = "hdf5")]
                Self::H5Part => "h5part",
            }
        )
    }
}

fn run_with_selected_detector<G, R>(
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_in_range: &Option<SnapNumInRange>,
    protected_file_types: &[&str],
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
{
    if let Some(detector_arguments) = arguments.subcommand_matches("manual_detector") {
        let detector = construct_manual_reconnection_site_detector_from_options(detector_arguments);
        run_with_selected_accelerator(
            arguments,
            detector_arguments,
            snapshot,
            snap_num_in_range,
            detector,
            protected_file_types,
        );
    } else {
        let (detector_config, detector_arguments) =
            if let Some(detector_arguments) = arguments.subcommand_matches("simple_detector") {
                (
                    construct_simple_reconnection_site_detector_config_from_options(
                        detector_arguments,
                        snapshot.reader(),
                    ),
                    detector_arguments,
                )
            } else {
                (
                    SimpleReconnectionSiteDetectorConfig::with_defaults_from_param_file(
                        snapshot.reader(),
                    ),
                    arguments,
                )
            };

        if arguments.is_present("print-parameter-values") {
            println!("{:#?}", detector_config);
        }

        let detector = SimpleReconnectionSiteDetector::new(detector_config);

        run_with_selected_accelerator(
            arguments,
            detector_arguments,
            snapshot,
            snap_num_in_range,
            detector,
            protected_file_types,
        );
    }
}

fn run_with_selected_accelerator<G, R, D>(
    root_arguments: &ArgMatches,
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_in_range: &Option<SnapNumInRange>,
    detector: D,
    protected_file_types: &[&str],
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
    D: ReconnectionSiteDetector,
{
    let (distribution_config, distribution_arguments) = if let Some(distribution_arguments) =
        arguments.subcommand_matches("power_law_distribution")
    {
        (
            construct_power_law_distribution_config_from_options(
                distribution_arguments,
                snapshot.reader(),
            ),
            distribution_arguments,
        )
    } else {
        (
            PowerLawDistributionConfig::with_defaults_from_param_file(snapshot.reader()),
            arguments,
        )
    };

    if root_arguments.is_present("print-parameter-values") {
        println!("{:#?}", distribution_config);
    }

    if let Some(_accelerator_arguments) =
        distribution_arguments.subcommand_matches("dc_power_law_accelerator")
    {
        unimplemented!()
    } else if let Some(accelerator_arguments) =
        distribution_arguments.subcommand_matches("simple_power_law_accelerator")
    {
        let accelerator_config = construct_simple_power_law_accelerator_config_from_options(
            accelerator_arguments,
            snapshot.reader(),
        );
        if root_arguments.is_present("print-parameter-values") {
            println!("{:#?}", accelerator_config);
        }
        let accelerator = SimplePowerLawAccelerator::new(distribution_config, accelerator_config);
        run_with_selected_interpolator(
            root_arguments,
            accelerator_arguments,
            snapshot,
            snap_num_in_range,
            detector,
            accelerator,
            protected_file_types,
        );
    } else {
        let accelerator_config =
            SimplePowerLawAccelerationConfig::with_defaults_from_param_file(snapshot.reader());
        if root_arguments.is_present("print-parameter-values") {
            println!("{:#?}", accelerator_config);
        }
        let accelerator = SimplePowerLawAccelerator::new(distribution_config, accelerator_config);
        run_with_selected_interpolator(
            root_arguments,
            distribution_arguments,
            snapshot,
            snap_num_in_range,
            detector,
            accelerator,
            protected_file_types,
        );
    };
}

fn run_with_selected_interpolator<G, R, D, A>(
    root_arguments: &ArgMatches,
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_in_range: &Option<SnapNumInRange>,
    detector: D,
    accelerator: A,
    protected_file_types: &[&str])
where G: Grid3<fdt>,
      R: SnapshotReader3<G> + Sync,
      D: ReconnectionSiteDetector,
      A: Accelerator + Sync + Send,
      <A::DistributionType as Distribution>::PropertiesCollectionType: ParallelExtend<<<A::DistributionType as Distribution>::PropertiesCollectionType as BeamPropertiesCollection>::Item>,
      A::DistributionType: Send,
{
    let (interpolator_config, interpolator_arguments) = if let Some(interpolator_arguments) =
        arguments.subcommand_matches("poly_fit_interpolator")
    {
        (
            construct_poly_fit_interpolator_config_from_options(interpolator_arguments),
            interpolator_arguments,
        )
    } else {
        (PolyFitInterpolatorConfig::default(), arguments)
    };

    if root_arguments.is_present("print-parameter-values") {
        println!("{:#?}", interpolator_config);
    }

    let interpolator = PolyFitInterpolator3::new(interpolator_config);

    run_with_selected_stepper_factory(
        root_arguments,
        interpolator_arguments,
        snapshot,
        snap_num_in_range,
        detector,
        accelerator,
        interpolator,
        protected_file_types,
    );
}

fn run_with_selected_stepper_factory<G, R, D, A, I>(
    root_arguments: &ArgMatches,
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_in_range: &Option<SnapNumInRange>,
    detector: D,
    accelerator: A,
    interpolator: I,
    protected_file_types: &[&str])
where G: Grid3<fdt>,
      R: SnapshotReader3<G> + Sync,
      D: ReconnectionSiteDetector,
      A: Accelerator + Sync + Send,
      A::DistributionType: Send,
      <A::DistributionType as Distribution>::PropertiesCollectionType: ParallelExtend<<<A::DistributionType as Distribution>::PropertiesCollectionType as BeamPropertiesCollection>::Item>,
      I: Interpolator3
{
    let (stepper_type, stepper_config) =
        if let Some(stepper_arguments) = arguments.subcommand_matches("rkf_stepper") {
            construct_rkf_stepper_config_from_options(stepper_arguments)
        } else {
            (RKFStepperType::RKF45, RKFStepperConfig::default())
        };

    if root_arguments.is_present("print-parameter-values") {
        println!("{:#?}\nstepper_type: {:?}", stepper_config, stepper_type);
    }
    let mut output_file_path = exit_on_error!(
        PathBuf::from_str(
            root_arguments
                .value_of("output-file")
                .expect("No value for required argument"),
        ),
        "Error: Could not interpret path to output file: {}"
    );

    let output_type = OutputType::from_path(&output_file_path);

    if let Some(snap_num_in_range) = snap_num_in_range {
        output_file_path.set_file_name(snapshot::create_new_snapshot_file_name_from_path(
            &output_file_path,
            snap_num_in_range.offset(),
            &output_type.to_string(),
            true,
        ));
    }

    let automatic_overwrite = root_arguments.is_present("overwrite");

    let atomic_output_path = exit_on_error!(
        AtomicOutputPath::new(output_file_path),
        "Error: Could not create temporary output file: {}"
    );

    if atomic_output_path.write_should_be_skipped(automatic_overwrite, protected_file_types) {
        return;
    }

    let extra_atomic_output_path = match output_type {
        #[cfg(feature = "hdf5")]
        OutputType::H5Part => {
            let extra_atomic_output_path = exit_on_error!(
                AtomicOutputPath::new(
                    atomic_output_path
                        .target_path()
                        .with_extension("sites.h5part")
                ),
                "Error: Could not create temporary output file: {}"
            );
            if extra_atomic_output_path
                .write_should_be_skipped(automatic_overwrite, protected_file_types)
            {
                return;
            }
            Some(extra_atomic_output_path)
        }
        _ => None,
    };

    let verbose = root_arguments.is_present("verbose").into();
    let beams = match stepper_type {
        RKFStepperType::RKF23 => {
            let stepper_factory = RKF23StepperFactory3::new(stepper_config);
            if root_arguments.is_present("generate-only") {
                ElectronBeamSwarm::generate_unpropagated(
                    snapshot,
                    detector,
                    accelerator,
                    &interpolator,
                    &stepper_factory,
                    verbose,
                )
            } else {
                ElectronBeamSwarm::generate_propagated(
                    snapshot,
                    detector,
                    accelerator,
                    &interpolator,
                    &stepper_factory,
                    verbose,
                )
            }
        }
        RKFStepperType::RKF45 => {
            let stepper_factory = RKF45StepperFactory3::new(stepper_config);
            if root_arguments.is_present("generate-only") {
                ElectronBeamSwarm::generate_unpropagated(
                    snapshot,
                    detector,
                    accelerator,
                    &interpolator,
                    &stepper_factory,
                    verbose,
                )
            } else {
                ElectronBeamSwarm::generate_propagated(
                    snapshot,
                    detector,
                    accelerator,
                    &interpolator,
                    &stepper_factory,
                    verbose,
                )
            }
        }
    };
    snapshot.drop_all_fields();
    perform_post_simulation_actions(
        root_arguments,
        output_type,
        atomic_output_path,
        extra_atomic_output_path,
        snapshot,
        interpolator,
        beams,
    );
}

fn perform_post_simulation_actions<G, R, A, I>(
    root_arguments: &ArgMatches,
    output_type: OutputType,
    atomic_output_path: AtomicOutputPath,
    extra_atomic_output_path: Option<AtomicOutputPath>,
    snapshot: &mut SnapshotCacher3<G, R>,
    interpolator: I,
    mut beams: ElectronBeamSwarm<A>,
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G>,
    A: Accelerator,
    I: Interpolator3,
{
    if let Some(extra_fixed_scalars) = root_arguments
        .values_of("extra-fixed-scalars")
        .map(|values| values.collect::<Vec<_>>())
    {
        for name in extra_fixed_scalars {
            beams.extract_fixed_scalars(
                exit_on_error!(
                    snapshot.obtain_scalar_field(name),
                    "Error: Could not read quantity {0} from snapshot: {1}",
                    name
                ),
                &interpolator,
            );
            snapshot.drop_scalar_field(name);
        }
    }
    if let Some(extra_fixed_vectors) = root_arguments
        .values_of("extra-fixed-vectors")
        .map(|values| values.collect::<Vec<_>>())
    {
        for name in extra_fixed_vectors {
            beams.extract_fixed_vectors(
                exit_on_error!(
                    snapshot.obtain_vector_field(name),
                    "Error: Could not read quantity {0} from snapshot: {1}",
                    name
                ),
                &interpolator,
            );
            snapshot.drop_vector_field(name);
        }
    }
    if let Some(extra_varying_scalars) = root_arguments
        .values_of("extra-varying-scalars")
        .map(|values| values.collect::<Vec<_>>())
    {
        for name in extra_varying_scalars {
            if let Some(name) = extract_magnitude_name(name) {
                beams.extract_varying_vector_magnitudes(
                    exit_on_error!(
                        snapshot.obtain_vector_field(name),
                        "Error: Could not read quantity {0} from snapshot: {1}",
                        name
                    ),
                    &interpolator,
                );
                snapshot.drop_vector_field(name);
            } else {
                beams.extract_varying_scalars(
                    exit_on_error!(
                        snapshot.obtain_scalar_field(name),
                        "Error: Could not read quantity {0} from snapshot: {1}",
                        name
                    ),
                    &interpolator,
                );
                snapshot.drop_scalar_field(name);
            }
        }
    }
    if let Some(extra_varying_vectors) = root_arguments
        .values_of("extra-varying-vectors")
        .map(|values| values.collect::<Vec<_>>())
    {
        for name in extra_varying_vectors {
            beams.extract_varying_vectors(
                exit_on_error!(
                    snapshot.obtain_vector_field(name),
                    "Error: Could not read quantity {0} from snapshot: {1}",
                    name
                ),
                &interpolator,
            );
            snapshot.drop_vector_field(name);
        }
    }

    if beams.verbose().is_yes() {
        println!(
            "Saving beams in {}",
            atomic_output_path
                .target_path()
                .file_name()
                .unwrap()
                .to_string_lossy()
        );
    }

    exit_on_error!(
        match output_type {
            OutputType::Fl => beams.save_into_custom_binary(atomic_output_path.temporary_path()),
            OutputType::Pickle =>
                beams.save_as_combined_pickles(atomic_output_path.temporary_path()),
            OutputType::JSON => beams.save_as_json(atomic_output_path.temporary_path()),
            #[cfg(feature = "hdf5")]
            OutputType::H5Part => beams.save_as_h5part(
                atomic_output_path.temporary_path(),
                extra_atomic_output_path.as_ref().unwrap().temporary_path(),
                root_arguments.is_present("drop-h5part-id"),
            ),
        },
        "Error: Could not save output data: {}"
    );

    exit_on_error!(
        atomic_output_path.perform_replace(),
        "Error: Could not move temporary output file to target path: {}"
    );
    if let Some(extra_atomic_output_path) = extra_atomic_output_path {
        exit_on_error!(
            extra_atomic_output_path.perform_replace(),
            "Error: Could not move temporary output file to target path: {}"
        );
    }
}
