# IAtoDNG.rs

A rust utility to convert Sinar IA images to DNG files.

## Installation

Make sure you have cargo / rust installed:
    
     
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh


Then: 

    git clone https://github.com/mgolub2/iatodng_rs.git
    cargo install --path .

This will install `iatodng` and a utility called `pwad` to your cargo bin directory. 
`iatodng` is the utility to convert IA files to DNGs, and `pwad` is a utility to print information about IA, BR, and WR PWAD files.

It might work on other doom patch wad files, but I haven't tested it.

## Usage

```bash
Usage: iatodng <SINAR_AI_DIR> <OUTPUT_DIR>

Arguments:
  <SINAR_AI_DIR>  The path to the file or directory to read
  <OUTPUT_DIR>    The directory to output DNGs to

Options:
  -h, --help     Print help
  -V, --version  Print version
```