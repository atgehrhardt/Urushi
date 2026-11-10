//! Compiles `proto/urushi/v1/*.proto` into Rust with protox (a pure-Rust compiler, so
//! no `protoc` is needed) and prost.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = "../../proto";
    println!("cargo:rerun-if-changed={root}");
    let descriptors = protox::compile(["urushi/v1/envelope.proto"], [root])?;
    prost_build::Config::new()
        // Rarely set and large: boxed, so the messages that may hold them (and enums of
        // those messages) stay small.
        .boxed(".urushi.v1.Task.issue")
        .boxed(".urushi.v1.Subtask.issue")
        .boxed(".urushi.v1.Board.jira")
        .compile_fds(descriptors)?;
    Ok(())
}
