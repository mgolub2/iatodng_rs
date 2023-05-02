extern crate iatodng;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// The path to the file or directory to read
    pub sinar_ai_dir: PathBuf,
}

fn main() {
    //Find all .IA files in a directory and print their meta info:
    let args = Cli::parse();
    //let mut pwads = Vec::new();
    for entry in std::fs::read_dir(&args.sinar_ai_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "IA" {
                    iatodng::sinar_ia::process_ia(&path);
                    break;
                }
            }
        }
    }
}