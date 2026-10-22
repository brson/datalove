//! Worldfiles of several script units, run in one session by `script-world`.

#[path = "support/script_world.rs"]
mod script_world;

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), script_world::run_script_world)
        .fixture_subdir("world")
        .file_extension("world")
        .allow_errors(false)
        .run();
}
