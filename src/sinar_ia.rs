extern crate ndarray;

use crate::{iadng, pwad};
use ndarray::{Array1, Array2, Zip};
use phf::phf_map;
use std::convert::TryInto;
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
pub const THUMB_WD: u32 = 356;
pub const THUMB_HT: u32 = 476;

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
        let focal_length = u32::from_le_bytes(meta[356..360].try_into().unwrap()) as f32 / 1000.0;
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

fn bufferu8_u16_to_1d_array_f64(buffer: &Vec<u8>, width: usize, height: usize) -> Array1<f64> {
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
                *i = (*i - b0) - (b1 - b0);
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
                    let mut raw = bufferu8_u16_to_1d_array_f64(
                        &metadata.read_lump_by_tag(RAW_KEY)?,
                        ia.width as usize,
                        ia.height as usize,
                    );
                    let black_ref0 = bufferu8_u16_to_1d_array_f64(
                        &black.read_lump_by_tag(BLACK0_KEY)?,
                        ia.width as usize,
                        ia.height as usize,
                    );
                    let black_ref1 = bufferu8_u16_to_1d_array_f64(
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
                                    &bufferu8_u16_to_1d_array_f64(
                                        &white.read_lump_by_tag(WHITE_KEY)?,
                                        ia.width as usize,
                                        ia.height as usize,
                                    ),
                                );
                            })
                        })
                        .ok();
                    iadng::write_1d_array_to_dng(
                        &raw,
                        &metadata.read_lump_by_tag(THUMB_KEY)?,
                        &output_dir,
                        &ia,
                    )
                    .unwrap()
                })
            })
        })
        .map_err(|e| println!("Error: {}", e))
        .ok()
}
