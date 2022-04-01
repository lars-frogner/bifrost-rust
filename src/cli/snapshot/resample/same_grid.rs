//! Command line interface for resampling a snapshot to
//! the original grid (but with a potentially different
//! sample location within the grid cells).

use crate::{
    cli::{
        interpolation::poly_fit::create_poly_fit_interpolator_subcommand,
        snapshot::{write::create_write_subcommand, SnapNumInRange},
    },
    create_subcommand,
    field::{ResampledCoordLocation, ResamplingMethod},
    geometry::In3D,
    grid::Grid3,
    interpolation::Interpolator3,
    io::snapshot::{fdt, SnapshotReader3},
};
use clap::{ArgMatches, Command};

/// Builds a representation of the `snapshot-resample-same_grid` command line subcommand.
pub fn create_same_grid_subcommand() -> Command<'static> {
    Command::new("same_grid")
        .about("Resample to the original grid")
        .long_about(
            "Resample to the original grid.\n\
             This is useful if you only want to change the sample location within the grid cells\n\
             (e.g. to center all quantities using --sample-location=center).",
        )
        .after_help(
            "You can use a subcommand to configure the interpolator. If left unspecified,\n\
             the default interpolator implementation and parameters are used.",
        )
        .subcommand_required(true)
        .subcommand(
            create_subcommand!(resample, poly_fit_interpolator)
                .subcommand_required(true)
                .subcommand(create_subcommand!(poly_fit_interpolator, write)),
        )
        .subcommand(create_subcommand!(resample, write))
}

pub fn run_resampling_for_same_grid<G, R, I>(
    arguments: &ArgMatches,
    reader: &R,
    snap_num_in_range: &Option<SnapNumInRange>,
    resampled_locations: &In3D<ResampledCoordLocation>,
    resampling_method: ResamplingMethod,
    continue_on_warnings: bool,
    is_verbose: bool,
    interpolator: I,
    protected_file_types: &[&str],
) where
    G: Grid3<fdt>,
    R: SnapshotReader3<G>,
    I: Interpolator3,
{
    let write_arguments = arguments.subcommand_matches("write").unwrap();

    super::resample_to_same_or_reshaped_grid(
        write_arguments,
        None,
        reader,
        snap_num_in_range,
        resampled_locations,
        resampling_method,
        continue_on_warnings,
        is_verbose,
        interpolator,
        protected_file_types,
    );
}