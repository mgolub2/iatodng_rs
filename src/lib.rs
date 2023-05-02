pub mod pwad;
pub mod sinar_ia;

#[cfg(test)]
mod tests {
    use crate::{
        pwad::Pwad,
        sinar_ia::{self, META_KEY},
    };

    #[test]
    fn test_meta_info() {
        let pwad = Pwad::from_file("src/6C486AFC.IA").unwrap();
        println!("{:?}", pwad);
        let metab = pwad.read_lump_by_tag(META_KEY).unwrap();
        assert!(metab.len() > 0);
        let meta = sinar_ia::SinarIAMeta::process_meta(&metab);
        assert!(meta.camera == "Sinar Hy6");
    }
}
