mod common;
mod regression;

use common::run;
use regression::{Actual, Expected, RegressionTest};

#[test]
fn regular_mesh_is_correct() {
    let test = RegressionTest::for_output_file("regular.mesh");
    run([
        "create_mesh",
        test.output_path(),
        "--overwrite",
        "regular",
        "--shape=3,4,5",
        "--x-bounds=0,1",
        "--y-bounds=-1,2",
        "--z-bounds=1,1.5",
    ]);
    test.assert_mesh_files_equal();
}

#[test]
fn hor_regular_mesh_is_correct() {
    let test = RegressionTest::for_output_file("hor_regular.mesh");
    run([
        "create_mesh",
        test.output_path(),
        "--overwrite",
        "horizontally_regular",
        "--shape=3,4,25",
        "--x-bounds=0,1",
        "--y-bounds=-1,2",
        "--z-bounds=-15,2.5",
        "--boundary-dz-scales=80,20",
        "--interior-z=-2.5,0",
        "--interior-dz-scales=10,10",
    ]);
    test.assert_mesh_files_equal();
}
