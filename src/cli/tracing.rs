//! Command line interface for field line tracing.

pub mod field_line;
pub mod seeding;
pub mod stepping;

use self::{
    field_line::basic::{
        construct_basic_field_line_tracer_config_from_options,
        create_basic_field_line_tracer_subcommand,
    },
    seeding::{
        manual::{create_manual_seeder_from_arguments, create_manual_seeder_subcommand},
        slice::{create_slice_seeder_from_arguments, create_slice_seeder_subcommand},
    },
    stepping::rkf::{construct_rkf_stepper_config_from_options, create_rkf_stepper_subcommand},
};
use crate::{
    cli::interpolation::poly_fit::{
        construct_poly_fit_interpolator_config_from_options,
        create_poly_fit_interpolator_subcommand,
    },
    create_subcommand, exit_on_error, exit_with_error,
    grid::Grid3,
    interpolation::{
        poly_fit::{PolyFitInterpolator3, PolyFitInterpolatorConfig},
        Interpolator3,
    },
    io::{
        snapshot::{self, fdt, SnapshotCacher3, SnapshotReader3},
        utils,
    },
    tracing::{
        field_line::{
            basic::{BasicFieldLineTracer3, BasicFieldLineTracerConfig},
            FieldLineSet3, FieldLineSetProperties3, FieldLineTracer3,
        },
        seeding::Seeder3,
        stepping::{
            rkf::{
                rkf23::RKF23StepperFactory3, rkf45::RKF45StepperFactory3, RKFStepperConfig,
                RKFStepperType,
            },
            StepperFactory3,
        },
    },
};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use rayon::prelude::*;
use std::{path::PathBuf, str::FromStr};

/// Builds a representation of the `trace` command line subcommand.
pub fn create_trace_subcommand<'a, 'b>() -> App<'a, 'b> {
    let app = SubCommand::with_name("trace")
        .about("Trace field lines of a vector field in the snapshot")
        .after_help(
            "You can use subcommands to configure each action. The subcommands must be\n\
             specified in the order tracer -> stepper -> interpolator -> seeder, with options\n\
             for each action directly following the subcommand. Any action(s) except seeding\n\
             can be left unspecified, in which case the default implementation and parameters\n\
             are used for that action.",
        )
        .help_message("Print help information")
        .arg(
            Arg::with_name("output-file")
                .value_name("OUTPUT_FILE")
                .help(
                    "Path of the file where the field line data should be saved\n\
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
                .help("Automatically overwrite any existing file"),
        )
        .arg(
            Arg::with_name("vector-quantity")
                .short("q")
                .long("vector-quantity")
                .require_equals(true)
                .value_name("NAME")
                .help("Vector field from the snapshot to trace")
                .takes_value(true)
                .default_value("b"),
        )
        .arg(
            Arg::with_name("extracted-quantities")
                .long("extracted-quantities")
                .require_equals(true)
                .require_delimiter(true)
                .value_name("NAMES")
                .help("List of quantities to extract along field line paths (comma-separated)")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("extracted-seed-quantities")
                .long("extracted-seed-quantities")
                .require_equals(true)
                .require_delimiter(true)
                .value_name("NAMES")
                .help("List of quantities to extract at seed positions (comma-separated)")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("Print status messages while tracing field lines"),
        )
        .arg(
            Arg::with_name("print-parameter-values")
                .short("p")
                .long("print-parameter-values")
                .help("Prints the values of all the parameters that will be used")
                .hidden(true),
        );

    app.setting(AppSettings::SubcommandRequired)
        .subcommand(
            create_subcommand!(trace, basic_field_line_tracer)
                .setting(AppSettings::SubcommandRequired)
                .subcommand(
                    create_subcommand!(basic_field_line_tracer, rkf_stepper)
                        .setting(AppSettings::SubcommandRequired)
                        .subcommand(
                            create_subcommand!(rkf_stepper, poly_fit_interpolator)
                                .setting(AppSettings::SubcommandRequired)
                                .subcommand(create_subcommand!(poly_fit_interpolator, slice_seeder))
                                .subcommand(create_subcommand!(
                                    poly_fit_interpolator,
                                    manual_seeder
                                )),
                        )
                        .subcommand(create_subcommand!(rkf_stepper, slice_seeder))
                        .subcommand(create_subcommand!(rkf_stepper, manual_seeder)),
                )
                .subcommand(
                    create_subcommand!(basic_field_line_tracer, poly_fit_interpolator)
                        .setting(AppSettings::SubcommandRequired)
                        .subcommand(create_subcommand!(poly_fit_interpolator, slice_seeder))
                        .subcommand(create_subcommand!(poly_fit_interpolator, manual_seeder)),
                )
                .subcommand(create_subcommand!(basic_field_line_tracer, slice_seeder))
                .subcommand(create_subcommand!(basic_field_line_tracer, manual_seeder)),
        )
        .subcommand(
            create_subcommand!(trace, rkf_stepper)
                .setting(AppSettings::SubcommandRequired)
                .subcommand(
                    create_subcommand!(rkf_stepper, poly_fit_interpolator)
                        .setting(AppSettings::SubcommandRequired)
                        .subcommand(create_subcommand!(poly_fit_interpolator, slice_seeder))
                        .subcommand(create_subcommand!(poly_fit_interpolator, manual_seeder)),
                )
                .subcommand(create_subcommand!(rkf_stepper, slice_seeder))
                .subcommand(create_subcommand!(rkf_stepper, manual_seeder)),
        )
        .subcommand(
            create_subcommand!(trace, poly_fit_interpolator)
                .setting(AppSettings::SubcommandRequired)
                .subcommand(create_subcommand!(poly_fit_interpolator, slice_seeder))
                .subcommand(create_subcommand!(poly_fit_interpolator, manual_seeder)),
        )
        .subcommand(create_subcommand!(trace, slice_seeder))
        .subcommand(create_subcommand!(trace, manual_seeder))
}

/// Runs the actions for the `trace` subcommand using the given arguments.
pub fn run_trace_subcommand<G, R>(
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_offset: Option<u32>,
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
{
    run_with_selected_tracer(arguments, snapshot, snap_num_offset);
}

fn run_with_selected_tracer<G, R>(
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_offset: Option<u32>,
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
{
    let (tracer_config, tracer_arguments) =
        if let Some(tracer_arguments) = arguments.subcommand_matches("basic_tracer") {
            (
                construct_basic_field_line_tracer_config_from_options(tracer_arguments),
                tracer_arguments,
            )
        } else {
            (BasicFieldLineTracerConfig::default(), arguments)
        };

    if arguments.is_present("print-parameter-values") {
        println!("{:#?}", tracer_config);
    }

    let tracer = BasicFieldLineTracer3::new(tracer_config);

    run_with_selected_stepper_factory(
        arguments,
        tracer_arguments,
        snapshot,
        snap_num_offset,
        tracer,
    );
}

fn run_with_selected_stepper_factory<G, R, Tr>(
    root_arguments: &ArgMatches,
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_offset: Option<u32>,
    tracer: Tr,
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
    Tr: FieldLineTracer3 + Sync,
    <Tr as FieldLineTracer3>::Data: Send,
    FieldLineSetProperties3: FromParallelIterator<<Tr as FieldLineTracer3>::Data>,
{
    let ((stepper_type, stepper_config), stepper_arguments) =
        if let Some(stepper_arguments) = arguments.subcommand_matches("rkf_stepper") {
            (
                construct_rkf_stepper_config_from_options(stepper_arguments),
                stepper_arguments,
            )
        } else {
            (
                (RKFStepperType::RKF45, RKFStepperConfig::default()),
                arguments,
            )
        };

    if root_arguments.is_present("print-parameter-values") {
        println!("{:#?}\nstepper_type: {:?}", stepper_config, stepper_type);
    }

    match stepper_type {
        RKFStepperType::RKF23 => run_with_selected_interpolator(
            root_arguments,
            stepper_arguments,
            snapshot,
            snap_num_offset,
            tracer,
            RKF23StepperFactory3::new(stepper_config),
        ),
        RKFStepperType::RKF45 => run_with_selected_interpolator(
            root_arguments,
            stepper_arguments,
            snapshot,
            snap_num_offset,
            tracer,
            RKF45StepperFactory3::new(stepper_config),
        ),
    }
}

fn run_with_selected_interpolator<G, R, Tr, StF>(
    root_arguments: &ArgMatches,
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_offset: Option<u32>,
    tracer: Tr,
    stepper_factory: StF,
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
    Tr: FieldLineTracer3 + Sync,
    <Tr as FieldLineTracer3>::Data: Send,
    FieldLineSetProperties3: FromParallelIterator<<Tr as FieldLineTracer3>::Data>,
    StF: StepperFactory3 + Sync,
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

    run_with_selected_seeder(
        root_arguments,
        interpolator_arguments,
        snapshot,
        snap_num_offset,
        tracer,
        stepper_factory,
        interpolator,
    );
}

fn run_with_selected_seeder<G, R, Tr, StF, I>(
    root_arguments: &ArgMatches,
    arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_offset: Option<u32>,
    tracer: Tr,
    stepper_factory: StF,
    interpolator: I,
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
    Tr: FieldLineTracer3 + Sync,
    <Tr as FieldLineTracer3>::Data: Send,
    FieldLineSetProperties3: FromParallelIterator<<Tr as FieldLineTracer3>::Data>,
    StF: StepperFactory3 + Sync,
    I: Interpolator3,
{
    if let Some(seeder_arguments) = arguments.subcommand_matches("slice_seeder") {
        let seeder = create_slice_seeder_from_arguments(seeder_arguments, snapshot, &interpolator);
        run_tracing(
            root_arguments,
            snapshot,
            snap_num_offset,
            tracer,
            stepper_factory,
            interpolator,
            seeder,
        );
    } else if let Some(seeder_arguments) = arguments.subcommand_matches("manual_seeder") {
        let seeder = create_manual_seeder_from_arguments(seeder_arguments);
        run_tracing(
            root_arguments,
            snapshot,
            snap_num_offset,
            tracer,
            stepper_factory,
            interpolator,
            seeder,
        );
    } else {
        exit_with_error!("Error: No seeder specified")
    };
}

fn run_tracing<G, R, Tr, StF, I, Sd>(
    root_arguments: &ArgMatches,
    snapshot: &mut SnapshotCacher3<G, R>,
    snap_num_offset: Option<u32>,
    tracer: Tr,
    stepper_factory: StF,
    interpolator: I,
    seeder: Sd,
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G> + Sync,
    Tr: FieldLineTracer3 + Sync,
    <Tr as FieldLineTracer3>::Data: Send,
    FieldLineSetProperties3: FromParallelIterator<<Tr as FieldLineTracer3>::Data>,
    StF: StepperFactory3 + Sync,
    I: Interpolator3,
    Sd: Seeder3,
{
    let mut output_file_path = exit_on_error!(
        PathBuf::from_str(
            root_arguments
                .value_of("output-file")
                .expect("Required argument not present."),
        ),
        "Error: Could not interpret path to output file: {}"
    );

    let output_extension = output_file_path
        .extension()
        .unwrap_or_else(|| exit_with_error!("Error: Missing extension for output-file"))
        .to_string_lossy()
        .to_string();

    if let Some(snap_num_offset) = snap_num_offset {
        let (output_base_name, output_existing_num) =
            snapshot::extract_name_and_num_from_snapshot_path(&output_file_path);
        let output_existing_num = output_existing_num.unwrap_or(snapshot::FALLBACK_SNAP_NUM);
        output_file_path.set_file_name(snapshot::create_snapshot_file_name(
            &output_base_name,
            output_existing_num + snap_num_offset,
            &output_extension,
        ));
    }

    let force_overwrite = root_arguments.is_present("overwrite");

    if !force_overwrite && !utils::write_allowed(&output_file_path) {
        exit_with_error!("Aborted");
    }

    let quantity = root_arguments
        .value_of("vector-quantity")
        .expect("No value for argument with default.");
    exit_on_error!(
        snapshot.cache_vector_field(quantity),
        "Error: Could not read quantity {0} in snapshot: {1}",
        quantity
    );

    let field_lines = FieldLineSet3::trace(
        quantity,
        snapshot,
        seeder,
        &tracer,
        &interpolator,
        &stepper_factory,
        root_arguments.is_present("verbose").into(),
    );
    snapshot.drop_all_fields();
    perform_post_tracing_actions(
        root_arguments,
        &output_extension,
        output_file_path,
        snapshot,
        interpolator,
        field_lines,
    );
}

fn perform_post_tracing_actions<G, R, I>(
    root_arguments: &ArgMatches,
    output_extension: &str,
    output_file_path: PathBuf,
    snapshot: &mut SnapshotCacher3<G, R>,
    interpolator: I,
    mut field_lines: FieldLineSet3,
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G>,
    I: Interpolator3,
{
    if let Some(extra_fixed_scalars) = root_arguments
        .values_of("extracted-seed-quantities")
        .map(|values| values.collect::<Vec<_>>())
    {
        for name in extra_fixed_scalars {
            field_lines.extract_fixed_scalars(
                exit_on_error!(
                    snapshot.obtain_scalar_field(name),
                    "Error: Could not read quantity {0} in snapshot: {1}",
                    name
                ),
                &interpolator,
            );
            snapshot.drop_scalar_field(name);
        }
    }
    if let Some(extra_varying_scalars) = root_arguments
        .values_of("extracted-quantities")
        .map(|values| values.collect::<Vec<_>>())
    {
        for name in extra_varying_scalars {
            field_lines.extract_varying_scalars(
                exit_on_error!(
                    snapshot.obtain_scalar_field(name),
                    "Error: Could not read quantity {0} in snapshot: {1}",
                    name
                ),
                &interpolator,
            );
            snapshot.drop_scalar_field(name);
        }
    }

    exit_on_error!(
        match output_extension {
            "fl" => field_lines.save_into_custom_binary(output_file_path),
            "pickle" => field_lines.save_as_combined_pickles(output_file_path),
            "json" => field_lines.save_as_json(output_file_path),
            "h5part" => {
                #[cfg(feature = "hdf5")]
                {
                    field_lines.save_as_h5part(output_file_path)
                }
                #[cfg(not(feature = "hdf5"))]
                exit_with_error!("Error: Compile with hdf5 feature in order to write H5Part files\n\
                                  Tip: Use cargo flag --features=hdf5 and make sure the HDF5 library is available");
            }
            invalid => exit_with_error!("Error: Invalid extension {} for output-file", invalid),
        },
        "Error: Could not save output data: {}"
    );
}
