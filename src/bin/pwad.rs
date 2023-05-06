/*
Prints a Sinar IA/WR/BR meta lump strucure.
*/
extern crate iatodng;

//Clap CLI parser
use clap::Parser;
//Clap strucure for CLI args
#[derive(Parser)]
struct Cli {
    /// The path to the file or directory to read
    file: std::path::PathBuf,
}

fn main() {
    //Parse CLI args
    let args = Cli::parse();
    //Open file
    let pwad = iatodng::pwad::Pwad::from_file(&args.file.to_str().unwrap()).unwrap();
    //Print pwad struct data
    println!("{:?}", pwad);
    //Read meta lump
    let meta = pwad.read_lump_by_tag(iatodng::sinar_ia::META_KEY).unwrap();
    //Print meta lump
    let metadata = iatodng::sinar_ia::SinarIAMeta::process_meta(&meta);
    println!("{:?}", &metadata);
    let parent_dir = &args.file.parent().unwrap();
    println!(
        "black_ref exists: {}",
        parent_dir.join(metadata.black_ref).exists()
    );
    println!(
        "white_ref exists: {}",
        parent_dir.join(metadata.white_ref).exists()
    );
}
