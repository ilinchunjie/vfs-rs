use crate::file::File;
use crate::result::Result;

mod zip;
mod file;
mod result;
mod android;

pub trait FileSystem {
    fn mount(path: &str, priority: u32) -> bool;
    fn exist(file_name: &str) -> bool;
    fn open(file_name: &str) -> Result<File>;
}

pub struct VirtualFileSystem {}

#[cfg(test)]
mod test {
    use crate::zip::zip_archive::ZipArchive;

    #[test]
    fn test_zip_file() {
        let archive = ZipArchive::new("");
    }
}