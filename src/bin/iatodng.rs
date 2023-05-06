extern crate iatodng;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// The path to the file or directory to read
    pub sinar_ai_dir: PathBuf,
    /// The directory to output DNGs to
    pub output_dir: PathBuf,
}

fn main() {
    let args = Cli::parse();
    // make output directory if it doesn't exist
    if !args.output_dir.exists() {
        std::fs::create_dir(&args.output_dir).unwrap();
    }
    for entry in std::fs::read_dir(&args.sinar_ai_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "IA" {
                    iatodng::sinar_ia::process_ia(&path, &args.output_dir);
                }
            }
        }
    }
}
