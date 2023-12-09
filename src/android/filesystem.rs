use crate::file::File;
use crate::FileSystem;

pub struct VirtualFileSystemAndroid {

}

impl FileSystem for VirtualFileSystemAndroid {
    fn mount(path: &str, priority: u32) -> bool {
        todo!()
    }

    fn exist(file_name: &str) -> bool {
        todo!()
    }

    fn open(file_name: &str) -> crate::result::Result<File> {
        todo!()
    }
}