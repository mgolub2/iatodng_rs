extern crate rawler;
use ndarray::parallel::prelude::{IntoParallelRefIterator, ParallelIterator};
use ndarray::{Array1, Dim, OwnedRepr};
use rawler::{
    dng::{rect_to_dng_area, DNG_VERSION_V1_1, DNG_VERSION_V1_6},
    formats::tiff::{
        CompressionMethod, DirectoryWriter, PhotometricInterpretation, Rational, SRational,
        TiffError, TiffWriter, Value,
    },
    imgop::{xyz::Illuminant, Dim2, Point, Rect},
    tags::{DngTag, ExifTag, TiffCommonTag},
};

use std::{fs::File, io::BufWriter, mem::size_of, path::PathBuf};

use crate::sinar_ia::{SinarIAMeta, THUMB_HT, THUMB_WD};

fn scale_1d_f64_u16(image: &Array1<f64>) -> Vec<u16> {
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
    let scale = u16::MAX as f64 / (max - min);
    println!("\tmin: {}, max: {}, scale: {}", min, max, scale);
    image
        .par_iter()
        .map(|i| i - min)
        .map(|i| (i * scale).round() as u16)
        .collect::<Vec<u16>>()
}

fn matrix_to_tiff_value(xyz_to_cam: &Vec<f64>, d: i32) -> Vec<SRational> {
    xyz_to_cam
        .iter()
        .map(|a| SRational::new((a * d as f64) as i32, d))
        .collect()
}

pub(crate) fn write_1d_array_to_dng(
    image: &Array1<f64>,
    thumb: &[u8],
    path: &PathBuf,
    meta: &SinarIAMeta,
) -> Result<(), TiffError> {
    let new_dng = path.join(format!("{}.dng", meta.shutter_count));
    println!("\tWriting DNG to {}", new_dng.display());
    let file = File::create(new_dng).unwrap();
    let mut output = BufWriter::new(file);
    let mut dng = TiffWriter::new(&mut output).unwrap();
    let mut root_ifd = dng.new_directory();
    let wb_coeff = vec![
        Rational::new_f32(1.0, 100000),
        Rational::new_f32(1.0, 100000),
        Rational::new_f32(1.0, 100000),
    ];
    root_ifd.add_tag(
        TiffCommonTag::PhotometricInt,
        PhotometricInterpretation::RGB,
    )?;
    root_ifd.add_tag(TiffCommonTag::NewSubFileType, Value::Long(vec![1]))?;

    root_ifd.add_tag(TiffCommonTag::Orientation, 7_u16)?;
    //356x476 thumbnail
    root_ifd.add_tag(TiffCommonTag::ImageWidth, THUMB_WD)?;
    root_ifd.add_tag(TiffCommonTag::ImageLength, THUMB_HT)?;
    root_ifd.add_tag(TiffCommonTag::Compression, CompressionMethod::None)?;
    root_ifd.add_tag(TiffCommonTag::BitsPerSample, [8_u16, 8, 8])?;
    root_ifd.add_tag(TiffCommonTag::SampleFormat, [1_u16, 1, 1])?;
    root_ifd.add_tag(TiffCommonTag::SamplesPerPixel, 3_u16)?;

    let offset = root_ifd.write_data(&thumb)?;

    root_ifd.add_tag(TiffCommonTag::StripOffsets, offset)?;
    root_ifd.add_tag(TiffCommonTag::StripByteCounts, thumb.len() as u32)?;
    root_ifd.add_tag(TiffCommonTag::RowsPerStrip, 476 as u16)?;

    root_ifd.add_tag(DngTag::AsShotNeutral, &wb_coeff[..])?;

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
                2.0413690, -0.5649464, -0.3446944, -0.9692660, 1.8760108, 0.0415560, 0.0134474,
                -0.1183897, 1.0154096,
            ],
            10_000,
        )
        .as_slice(),
    )?;

    let mut r_ifd = root_ifd.new_directory();
    write_dng_data(&mut r_ifd, meta, image)?;
    let r_off = r_ifd.build()?;
    let mut sub_ifds = Vec::new();
    sub_ifds.push(r_off);
    write_exif_data(&mut root_ifd, meta)?;
    root_ifd.add_tag(TiffCommonTag::SubIFDs, &sub_ifds)?;
    let dng_off = root_ifd.build()?;
    dng.build(dng_off)?;

    Ok(())
}

pub(crate) fn write_dng_data(
    r_ifd: &mut DirectoryWriter,
    meta: &SinarIAMeta,
    image: &ndarray::ArrayBase<OwnedRepr<f64>, Dim<[usize; 1]>>,
) -> Result<(), TiffError> {
    let full_size = Rect::new(
        Point::new(0, 0),
        Dim2::new(meta.width as usize, meta.height as usize),
    );
    r_ifd.add_tag(TiffCommonTag::NewSubFileType, Value::Long(vec![0]))?;
    r_ifd.add_tag(DngTag::ActiveArea, rect_to_dng_area(&full_size))?;
    r_ifd.add_tag(ExifTag::PlanarConfiguration, 1_u16)?;
    r_ifd.add_tag(
        TiffCommonTag::PhotometricInt,
        PhotometricInterpretation::CFA,
    )?;
    r_ifd.add_tag(TiffCommonTag::ImageWidth, meta.width as u32)?;
    r_ifd.add_tag(TiffCommonTag::ImageLength, meta.height as u32)?;
    r_ifd.add_tag(TiffCommonTag::SamplesPerPixel, 1_u16)?;
    r_ifd.add_tag(TiffCommonTag::BitsPerSample, [16_u16])?;
    r_ifd.add_tag(DngTag::CFALayout, 1_u16)?;
    r_ifd.add_tag(TiffCommonTag::CFAPattern, [0u8, 1u8, 1u8, 2u8])?;
    r_ifd.add_tag(TiffCommonTag::CFARepeatPatternDim, [2u16, 2u16])?;
    r_ifd.add_tag(DngTag::CFAPlaneColor, [0u8, 1u8, 2u8])?;
    r_ifd.add_tag(TiffCommonTag::Compression, CompressionMethod::None)?;
    let mut strip_offsets: Vec<u32> = Vec::new();
    let mut strip_sizes: Vec<u32> = Vec::new();
    let mut strip_rows: Vec<u32> = Vec::new();
    let rows_per_strip = meta.height / 1;
    let u16_image = scale_1d_f64_u16(image);
    for strip in u16_image
        .chunks((rows_per_strip * meta.width) as usize)
        .into_iter()
    {
        let offset = r_ifd.write_data_u16_be(strip)?;
        strip_offsets.push(offset);
        strip_sizes.push((strip.len() * size_of::<u16>()) as u32);
        strip_rows.push((strip.len() / meta.width as usize) as u32);
    }
    r_ifd.add_tag(TiffCommonTag::StripOffsets, &strip_offsets)?;
    r_ifd.add_tag(TiffCommonTag::StripByteCounts, &strip_sizes)?;
    r_ifd.add_tag(TiffCommonTag::RowsPerStrip, &strip_rows)?;
    Ok(())
}

pub(crate) fn write_exif_data(
    root_ifd: &mut rawler::formats::tiff::DirectoryWriter,
    meta: &SinarIAMeta,
) -> Result<(), TiffError> {
    let exif_offset = {
        let mut exif_ifd = root_ifd.new_directory();
        // Add EXIF version 0220
        exif_ifd.add_tag_undefined(ExifTag::ExifVersion, vec![48, 50, 50, 48])?;
        fill_exif_ifd(&mut exif_ifd, meta)?;
        exif_ifd.build()?
    };
    root_ifd.add_tag(TiffCommonTag::ExifIFDPointer, exif_offset)?;
    Ok(())
}

pub(crate) fn fill_exif_ifd(
    exif_ifd: &mut rawler::formats::tiff::DirectoryWriter,
    meta: &SinarIAMeta,
) -> Result<(), TiffError> {
    exif_ifd.add_tag(ExifTag::FNumber, Rational::new_f32(meta.f_stop, 10_000))?;
    //exif_ifd.add_tag(ExifTag::ApertureValue, Rational::new_f32(meta.f_stop, 10_000))?;
    exif_ifd.add_tag(
        ExifTag::ISOSpeedRatings,
        Value::Short(vec![meta.iso as u16]),
    )?;
    exif_ifd.add_tag(
        ExifTag::FocalLength,
        Rational::new_f32(meta.focal_length, 10_00),
    )?;
    //add serial number
    exif_ifd.add_tag(ExifTag::SerialNumber, meta.serial.clone())?;
    //add shutter time
    exif_ifd.add_tag(
        ExifTag::ExposureTime,
        Rational::new(meta.measured_shutter_us, 1_000_000),
    )?;
    Ok(())
}
