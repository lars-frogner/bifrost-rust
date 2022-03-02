//! Command line interface for producing regularly spaced seed points in a volume of a 3D grid.

use crate::{
    cli::utils,
    geometry::{In3D, Point3, Vec3},
    grid::regular::RegularGrid3,
    io::snapshot::fdt,
    seeding::volume::VolumeSeeder3,
};
use clap::{App, Arg, ArgMatches, SubCommand};

/// Creates a subcommand for using the regular volume seeder.
pub fn create_regular_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("regular")
        .about("Use the regular volume seeder")
        .long_about(
            "Use the regular volume seeder.\n\
             Seed points are produced at the cell centers of a regular 3D grid.",
        )
        .help_message("Print help information")
        .arg(
            Arg::with_name("shape")
                .short("s")
                .long("shape")
                .require_equals(true)
                .require_delimiter(true)
                .value_names(&["X", "Y", "Z"])
                .help("Number of seed points to generate in each dimension")
                .required(true)
                .takes_value(true)
                .number_of_values(3),
        )
}

/// Creates a regular volume seeder based on the provided arguments.
pub fn create_regular_volume_seeder_from_arguments<S>(
    arguments: &ArgMatches,
    lower_bounds: Vec3<fdt>,
    upper_bounds: Vec3<fdt>,
    satisfies_constraints: &S,
) -> VolumeSeeder3
where
    S: Fn(&Point3<fdt>) -> bool + Sync,
{
    let shape = utils::get_values_from_required_parseable_argument::<usize>(arguments, "shape");

    let grid = RegularGrid3::from_bounds(
        In3D::new(shape[0], shape[1], shape[2]),
        lower_bounds,
        upper_bounds,
        In3D::same(false),
    );

    VolumeSeeder3::regular(&grid, satisfies_constraints)
}