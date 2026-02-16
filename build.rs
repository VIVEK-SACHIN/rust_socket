use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    //out_dir is the directory where the generated code will be saved else it will be saved ..
    // /home/instavc/Desktop/socketserver_project/rust_socket/target/debug/build/rust_socket-526eb674674253c1/out/messages.rs
    let out_dir = "src/generated";
    // Check if folder exists, if not create it
    if !Path::new(out_dir).exists() {
        fs::create_dir_all(out_dir)?;
    }
    prost_build::Config::new()
        .out_dir(out_dir)
        .compile_protos(&["proto/messages.proto"], &["proto/"])?;
      //for first argument, we pass the path to the proto file
        //for second argument, we pass the path to the directory containing the proto file
        //this is because the proto file is not in the same directory as the build.rs file
        //so we need to pass the path to the directory containing the proto file
        //? means error → return error immediately | success → continue
    Ok(())
}



