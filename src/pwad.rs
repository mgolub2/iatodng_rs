/*
Generic PWAD reader

*/

extern crate byteorder;

use byteorder::{LittleEndian, ReadBytesExt};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug)]
pub struct Pwad {
    pub header: WadHeader,
    pub filename: PathBuf,
    pub directory: Vec<LumpDirectoryEntry>,
}

#[derive(Debug)]
pub struct WadHeader {
    pub identification: String,
    pub num_lumps: u32,
    pub directory_offset: u32,
}

#[derive(Debug)]
pub struct LumpDirectoryEntry {
    pub offset: u32,
    pub size: u32,
    pub name: String,
}

impl Pwad {
    pub fn from_file(file_path: &str) -> io::Result<Self> {
        let mut file = File::open(file_path)?;
        let header = read_wad_header(&mut file)?;
        let directory = read_lump_directory(&mut file, header.directory_offset, header.num_lumps)?;

        Ok(Pwad {
            header,
            directory,
            filename: PathBuf::from_str(file_path).unwrap(),
        })
    }

    pub fn read_lump_by_tag(&self, tag: &str) -> io::Result<Vec<u8>> {
        let lump = self
            .directory
            .iter()
            .find(|entry| entry.name.starts_with(tag));
        match lump {
            Some(lump) => {
                let mut file = File::open(&self.filename)?;
                file.seek(SeekFrom::Start(lump.offset as u64))?;
                let mut buffer = vec![0; lump.size as usize];
                file.read_exact(&mut buffer)?;
                Ok(buffer)
            }
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Lump with tag '{}' not found", tag),
            )),
        }
    }
}

fn read_wad_header<R: Read + Seek>(reader: &mut R) -> io::Result<WadHeader> {
    let mut identification = vec![0; 4];
    reader.read_exact(&mut identification)?;
    let identification = String::from_utf8(identification).unwrap();

    let num_lumps = reader.read_u32::<LittleEndian>()?;
    let directory_offset = reader.read_u32::<LittleEndian>()?;

    Ok(WadHeader {
        identification,
        num_lumps,
        directory_offset,
    })
}

fn read_lump_directory<R: Read + Seek>(
    reader: &mut R,
    directory_offset: u32,
    num_lumps: u32,
) -> io::Result<Vec<LumpDirectoryEntry>> {
    reader.seek(SeekFrom::Start(directory_offset as u64))?;

    let mut directory = Vec::with_capacity(num_lumps as usize);

    for _ in 0..num_lumps {
        let offset = reader.read_u32::<LittleEndian>()?;
        let size = reader.read_u32::<LittleEndian>()?;

        let mut name = vec![0; 8];
        reader.read_exact(&mut name)?;
        let name = String::from_utf8_lossy(&name).to_string();

        directory.push(LumpDirectoryEntry { offset, size, name });
    }

    Ok(directory)
}
