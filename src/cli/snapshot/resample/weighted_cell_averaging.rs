//! Command line interface for resampling a snapshot using weighted cell averaging.

use crate::{
    add_subcommand_combinations,
    cli::snapshot::{derive::create_derive_subcommand, write::create_write_subcommand},
};
use clap::Command;

/// Builds a representation of the `snapshot-resample-weighted_cell_averaging` command line subcommand.
pub fn create_weighted_cell_averaging_subcommand(
    parent_command_name: &'static str,
) -> Command<'static> {
    let command_name = "weighted_cell_averaging";

    crate::cli::command_graph::insert_command_graph_edge(parent_command_name, command_name);

    let command = Command::new(command_name)
        .about("Use the weighted cell averaging method")
        .long_about(
            "Use the weighted cell averaging method.\n\
             For each new grid cell, the values of all overlapped original grid cells are\n\
             averaged with weights according to the intersected volumes.\n\
             This method is suited for downsampling. It is faster than weighted sample\n\
             averaging, but slightly less accurate.",
        );

    add_subcommand_combinations!(command, command_name, true; derive, write)
}
