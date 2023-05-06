extern crate ndarray;

use crate::pwad;
use ndarray::parallel::prelude::{IntoParallelRefIterator, ParallelIterator};
use ndarray::{Array1, Array2, Zip};
use phf::phf_map;
use rawler::dng::{rect_to_dng_area, DNG_VERSION_V1_1, DNG_VERSION_V1_6};
use rawler::formats::tiff::{
    CompressionMethod, PhotometricInterpretation, SRational, TiffError, TiffWriter, Rational,
};
use rawler::imgop::xyz::Illuminant;
use rawler::imgop::{Dim2, Point, Rect};
use rawler::tags::{DngTag, ExifTag, TiffCommonTag};
use std::convert::TryInto;
use std::fs::File;
use std::io::BufWriter;
use std::mem::size_of;
use std::path::PathBuf;
use std::str;

//Contants for parsing the IA file
pub const META_KEY: &str = "META\0\0\0\0";
pub const RAW_KEY: &str = "RAW0\00\0\0";
pub const THUMB_KEY: &str = "THUMB\0\0\0";
pub const BLACK0_KEY: &str = "BLACK0\0\0";
pub const BLACK1_KEY: &str = "BLACK1\0\0";
pub const WHITE_KEY: &str = "WHITE\0\0\0";
pub const CROP: u32 = 8;

pub static MODEL_NAMES: phf::Map<&'static str, &'static str> = phf_map! {
    "e22" => "Emotion 22",
    "e75" => "Emotion 75",
};

pub static MODEL_TO_SIZE: phf::Map<&'static str, (u32, u32)> = phf_map! {
    "e22" => (5344, 4008),
    "e75" => (6668, 4992),
};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum WhiteBalance {
    Manual = 7,
    Flash = 0,
    Neon = 1,
    Tungsten = 2,
    Shadow = 3,
    Sun = 4,
    Cloudy = 5,
    Unknown = 6,
}

impl WhiteBalance {
    pub fn from(value: u16) -> Self {
        match value {
            0 => WhiteBalance::Flash,
            1 => WhiteBalance::Neon,
            2 => WhiteBalance::Tungsten,
            3 => WhiteBalance::Shadow,
            4 => WhiteBalance::Sun,
            5 => WhiteBalance::Cloudy,
            6 => WhiteBalance::Unknown,
            7 => WhiteBalance::Manual,
            _ => WhiteBalance::Unknown,
        }
    }
}

#[derive(Debug)]
pub struct SinarIAMeta {
    pub shutter_count: u32,
    pub camera: String,
    pub measured_shutter_us: u32,
    pub req_shutter_us: u32,
    pub f_stop: f32,
    pub black_ref: String,
    pub iso: u32,
    pub serial: String,
    pub model: String,
    pub height: u32,
    pub width: u32,
    pub white_balance_name: WhiteBalance,
    pub focal_length: f32,
    pub white_ref: String,
}

impl SinarIAMeta {
    pub fn process_meta(meta: &[u8]) -> Self {
        let shutter_count = u32::from_le_bytes(meta[4..8].try_into().unwrap());
        let camera = str::from_utf8(&meta[20..64])
            .unwrap()
            .trim_end_matches('\x00')
            .to_string();
        let white_balance_name =
            WhiteBalance::from(u16::from_le_bytes(meta[100..102].try_into().unwrap()));
        let shutter_time_us = u32::from_le_bytes(meta[104..108].try_into().unwrap());
        let black_ref = str::from_utf8(&meta[108 + 14..172])
            .unwrap()
            .trim_end_matches('\x00')
            .to_string();
        let white_ref = str::from_utf8(&meta[172 + 14..236])
            .unwrap()
            .trim_end_matches('\x00')
            .to_string();
        let iso = u32::from_le_bytes(meta[252..256].try_into().unwrap());
        let serial = str::from_utf8(&meta[272..288])
            .unwrap()
            .trim_end_matches('\x00')
            .to_string();
        let shutter_time_us_2 = u32::from_le_bytes(meta[344..348].try_into().unwrap());
        let f_stop = (u16::from_le_bytes(meta[352..354].try_into().unwrap()) as f32) / 256.0;
        let focal_length =
            u32::from_le_bytes(meta[356..360].try_into().unwrap()) as f32 / 1000.0;
        let short_model = serial.split('-').next().unwrap();
        let model = MODEL_NAMES[short_model];
        let (height, width) = MODEL_TO_SIZE[short_model];

        SinarIAMeta {
            shutter_count,
            camera,
            measured_shutter_us: shutter_time_us,
            req_shutter_us: shutter_time_us_2,
            f_stop,
            black_ref,
            white_ref,
            iso,
            serial,
            model: model.to_string(),
            height,
            width,
            white_balance_name,
            focal_length,
        }
    }
}

fn buffer_to_1d_array_f64(buffer: &Vec<u8>, width: usize, height: usize) -> Array1<f64> {
    assert_eq!(buffer.len(), width * height * 2);

    let mut array = Array1::zeros(height * width);

    {
        let mut array_view = array.view_mut();
        for (i, chunk) in buffer.chunks_exact(2).enumerate() {
            let value = u16::from_le_bytes(chunk.try_into().unwrap()) as f64 / (u16::MAX as f64);
            array_view[i] = value;
        }
    }

    array
}

//unused
#[allow(dead_code)]
fn subract_black_ref_st(
    image: &Array2<u16>,
    black_ref0: &Array2<u16>,
    black_ref1: &Array2<u16>,
) -> Array2<u16> {
    assert_eq!(image.shape(), black_ref0.shape());

    let mut result = Array2::zeros(image.raw_dim());

    {
        let mut result_view = result.view_mut();
        let image_view = image.view();
        let black_ref_view0 = black_ref0.view();
        let black_ref_view1 = black_ref1.view();

        for (i, _) in image.iter().enumerate() {
            let row = i / image.shape()[1];
            let col = i % image.shape()[1];
            let value = (image_view[[row, col]] - black_ref_view0[[row, col]])
                - (black_ref_view1[[row, col]] - black_ref_view0[[row, col]]);
            result_view[[row, col]] = value;
        }
    }

    result
}

fn scale_1d_f64_to_pix_u16(image: &Array1<f64>) -> Vec<u16> {
    let mut min = 0.0;
    let mut max = 0.0;
    for i in image.iter() {
        if i < &min {
            min = *i;
        }
        if i > &max {
            max = *i;
        }
    }
    let scale = u16::MAX as f64 / (max-min);
    println!("\tmin: {}, max: {}, scale: {}", min, max, scale);
    image
        .par_iter()
        .map(|i| i-min)
        .map(|i| (i * scale).round() as u16)
        .collect::<Vec<u16>>()
}

fn subract_black_ref_mut(
    image: &mut Array1<f64>,
    black_ref0: &Array1<f64>,
    black_ref1: &Array1<f64>,
) {
    assert_eq!(image.shape(), black_ref0.shape());
    {
        Zip::from(image)
            .and(black_ref0)
            .and(black_ref1)
            .par_for_each(|i, &b0, &b1| {
                *i = (*i-b0) - (b1 - b0);
            });
    }
}

fn apply_white_ref_mut(image: &mut Array1<f64>, white_ref: &Array1<f64>) {
    assert_eq!(image.shape(), white_ref.shape());
    {
        Zip::from(image).and(white_ref).par_for_each(|i, &w| {
            *i /= w;
        });
    }
}

fn matrix_to_tiff_value(xyz_to_cam: &Vec<f64>, d: i32) -> Vec<SRational> {
    xyz_to_cam
        .iter()
        .map(|a| SRational::new((a * d as f64) as i32, d))
        .collect()
}

fn write_1d_array_to_dng(
    image: &Array1<f64>,
    path: &PathBuf,
    meta: &SinarIAMeta,
) -> Result<(), TiffError> {
    let new_dng = path.to_str().unwrap();
    println!("\tWriting DNG to {}", new_dng);
    let file = File::create(new_dng).unwrap();
    let mut output = BufWriter::new(file);
    let mut dng = TiffWriter::new(&mut output).unwrap();
    let mut root_ifd = dng.new_directory();
    let wb_coeff = vec![
        Rational::new_f32(1.0, 100000),
        Rational::new_f32(1.0, 100000),
        Rational::new_f32(1.0, 100000),  
    ];
    root_ifd.add_tag(TiffCommonTag::PhotometricInt, PhotometricInterpretation::CFA)?;
    root_ifd.add_tag(DngTag::AsShotNeutral, &wb_coeff[..])?;
    root_ifd.add_tag(TiffCommonTag::NewSubFileType, 0_u16)?;
    root_ifd.add_tag(TiffCommonTag::ImageWidth, meta.width as u32)?;
    root_ifd.add_tag(TiffCommonTag::ImageLength, meta.height as u32)?;
    root_ifd.add_tag(TiffCommonTag::Software, "iatodng_rs v1.0")?;
    root_ifd.add_tag(DngTag::DNGVersion, &DNG_VERSION_V1_6[..])?;
    root_ifd.add_tag(DngTag::DNGBackwardVersion, &DNG_VERSION_V1_1[..])?;
    root_ifd.add_tag(TiffCommonTag::Model, meta.model.as_str())?;
    root_ifd.add_tag(TiffCommonTag::Make, meta.camera.as_str())?;
    let uq_model = format!("{} on {}", meta.model, meta.camera);
    root_ifd.add_tag(DngTag::UniqueCameraModel, uq_model.as_str())?;
    root_ifd.add_tag(
        ExifTag::ModifyDate,
        chrono::Local::now().format("%Y:%m:%d %H:%M:%S").to_string(),
    )?;
    root_ifd.add_tag(DngTag::CalibrationIlluminant1, u16::from(Illuminant::D50))?;
    root_ifd.add_tag(
        DngTag::ColorMatrix1,
        matrix_to_tiff_value(
            &vec![
                2.0413690, -0.5649464, -0.3446944,
                -0.9692660,  1.8760108,  0.0415560,
                 0.0134474, -0.1183897,  1.0154096
            ],
            10_000,
        )
        .as_slice(),
    )?;
    
    let full_size = Rect::new(
        Point::new(0, 0),
        Dim2::new(meta.width as usize, meta.height as usize),
    );

    root_ifd.add_tag(DngTag::ActiveArea, rect_to_dng_area(&full_size))?;
    root_ifd.add_tag(ExifTag::PlanarConfiguration, 1_u16)?;
    root_ifd.add_tag(
        TiffCommonTag::PhotometricInt,
        PhotometricInterpretation::CFA,
    )?;
    root_ifd.add_tag(TiffCommonTag::SamplesPerPixel, 1_u16)?;
    root_ifd.add_tag(TiffCommonTag::BitsPerSample, [16_u16])?;
    root_ifd.add_tag(DngTag::CFALayout, 1_u16)?; // Square layout
    root_ifd.add_tag(TiffCommonTag::CFAPattern, [0u8, 1u8, 1u8, 2u8])?; // RGGB
    root_ifd.add_tag(TiffCommonTag::CFARepeatPatternDim, [2u16, 2u16])?;
    root_ifd.add_tag(DngTag::CFAPlaneColor, [0u8, 1u8, 2u8])?; // RGGB
    root_ifd.add_tag(TiffCommonTag::Compression, CompressionMethod::None)?;
    let mut strip_offsets: Vec<u32> = Vec::new();
    let mut strip_sizes: Vec<u32> = Vec::new();
    let mut strip_rows: Vec<u32> = Vec::new();

    // 8 Strips
    let rows_per_strip = meta.height / 1;
    let u16_image = scale_1d_f64_to_pix_u16(image);
    for strip in u16_image
        .chunks((rows_per_strip * meta.width) as usize)
        .into_iter()
    {
        let offset = root_ifd.write_data_u16_be(strip)?;
        strip_offsets.push(offset);
        strip_sizes.push((strip.len() * size_of::<u16>()) as u32);
        strip_rows.push((strip.len() / meta.width as usize) as u32);
    }

    root_ifd.add_tag(TiffCommonTag::StripOffsets, &strip_offsets)?;
    root_ifd.add_tag(TiffCommonTag::StripByteCounts, &strip_sizes)?;
    root_ifd.add_tag(TiffCommonTag::RowsPerStrip, &strip_rows)?;
    root_ifd.add_tag(TiffCommonTag::Orientation, 7_u16)?;

    // Add EXIF information
    let exif_offset = {
        let mut exif_ifd = root_ifd.new_directory();
        // Add EXIF version 0220
        exif_ifd.add_tag_undefined(ExifTag::ExifVersion, vec![48, 50, 50, 48])?;
        fill_exif_ifd(&mut exif_ifd, meta)?;
        exif_ifd.build()?
    };
    root_ifd.add_tag(TiffCommonTag::ExifIFDPointer, exif_offset)?;

    let ifd0_offset = root_ifd.build()?;
    dng.build(ifd0_offset)?;

    Ok(())
}

fn fill_exif_ifd(exif_ifd: &mut rawler::formats::tiff::DirectoryWriter, meta: &SinarIAMeta) -> Result<(), TiffError>{
    exif_ifd.add_tag(ExifTag::FNumber, Rational::new_f32(meta.f_stop, 10_000))?;
    //exif_ifd.add_tag(ExifTag::ApertureValue, Rational::new_f32(meta.f_stop, 10_000))?;
    exif_ifd.add_tag(ExifTag::ISOSpeed, meta.iso)?;
    exif_ifd.add_tag(ExifTag::FocalLength, Rational::new_f32(meta.focal_length, 10_00))?;
    //add serial number
    exif_ifd.add_tag(ExifTag::SerialNumber, meta.serial.clone())?;
    //add shutter time
    exif_ifd.add_tag(ExifTag::ExposureTime, Rational::new(meta.measured_shutter_us, 1_000_000))?;
    Ok(())
}

pub fn process_ia(path: &PathBuf, output_dir: &PathBuf) -> Option<()> {
    let metadata = pwad::Pwad::from_file(path.to_str()?).unwrap();
    metadata
        .read_lump_by_tag(META_KEY)
        .and_then(|meta| {
            let ia = SinarIAMeta::process_meta(&meta);
            let black_full_path = path.parent().unwrap().join(&ia.black_ref);
            let white_full_path: PathBuf = path.parent().unwrap().join(&ia.white_ref);
            println!(
                "Processing IA: {}...\n\tblack: {}\n\twhite: {}",
                path.to_str().unwrap(),
                black_full_path.to_str().unwrap(),
                white_full_path.to_str().unwrap()
            );
            pwad::Pwad::from_file(black_full_path.to_str().unwrap()).and_then(|black| {
                Ok({
                    let mut raw = buffer_to_1d_array_f64(
                        &metadata.read_lump_by_tag(RAW_KEY)?,
                        ia.width as usize,
                        ia.height as usize,
                    );
                    let black_ref0 = buffer_to_1d_array_f64(
                        &black.read_lump_by_tag(BLACK0_KEY)?,
                        ia.width as usize,
                        ia.height as usize,
                    );
                    let black_ref1 = buffer_to_1d_array_f64(
                        &black.read_lump_by_tag(BLACK1_KEY)?,
                        ia.width as usize,
                        ia.height as usize,
                    );
                    subract_black_ref_mut(&mut raw, &black_ref0, &black_ref1);
                    pwad::Pwad::from_file(white_full_path.to_str().unwrap())
                        .and_then(|white| {
                            Ok({
                                apply_white_ref_mut(
                                    &mut raw,
                                    &buffer_to_1d_array_f64(
                                        &white.read_lump_by_tag(WHITE_KEY)?,
                                        ia.width as usize,
                                        ia.height as usize,
                                    ),
                                );
                            })
                        })
                        .ok();
                    write_1d_array_to_dng(&raw, &output_dir.join(path.with_extension("dng").file_name().unwrap()), &ia).unwrap()
                })
            })
        })
        .map_err(|e| println!("Error: {}", e))
        .ok()
}
