use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{Read, Seek};
use std::path::Path;
use std::sync::{Arc};
use byteorder::{LittleEndian, ReadBytesExt};
use parking_lot::RwLock;
use crate::zip::result::{ZipError, ZipResult};
use crate::zip::spec;
use crate::zip::zip_file::*;

pub struct ZipArchive {
    pub file: Arc<RwLock<File>>,
    pub entries: HashMap<String, Arc<ZipFileData>>,
}

impl ZipArchive {
    pub fn new(path: impl AsRef<Path>) -> ZipResult<ZipArchive> {
        let mut file = OpenOptions::new().read(true).open(path)?;

        let (footer, cde_start_pos) = spec::CentralDirectoryEnd::find_and_parse(&mut file)?;

        if !footer.record_too_small() && footer.disk_number != footer.disk_with_central_directory {
            return Err(ZipError::UnsupportedArchive);
        }

        let (archive_offset, directory_start, number_of_files) =
            Self::get_directory_counts(&mut file, &footer, cde_start_pos)?;

        let file_capacity = if number_of_files > cde_start_pos as usize {
            0
        } else {
            number_of_files
        };

        let mut entries = HashMap::with_capacity(file_capacity);

        if file.seek(io::SeekFrom::Start(directory_start)).is_err() {
            return Err(ZipError::InvalidArchive(
                "Could not seek to start of central directory",
            ));
        }

        for _ in 0..number_of_files {
            match central_header_to_zip_file(&mut file, archive_offset) {
                Ok(zip_fil_data) => {
                    entries.insert(zip_fil_data.file_name.clone(), Arc::new(zip_fil_data));
                }
                Err(e) => {
                    match e {
                        ZipError::UnsupportedCompressionMethod(method) => continue,
                        _ => return Err(e),
                    }
                }
            }
        }

        let file = Arc::new(RwLock::new(file));

        Ok(ZipArchive { file, entries })
    }

    fn get_directory_counts<T: Read + io::Seek>(
        reader: &mut T,
        footer: &spec::CentralDirectoryEnd,
        cde_start_pos: u64,
    ) -> ZipResult<(u64, u64, usize)> {
        let zip64locator = if reader
            .seek(io::SeekFrom::End(
                -(20 + 22 + footer.zip_file_comment.len() as i64),
            ))
            .is_ok()
        {
            match spec::Zip64CentralDirectoryEndLocator::parse(reader) {
                Ok(loc) => Some(loc),
                Err(ZipError::InvalidArchive(_)) => {
                    None
                }
                Err(e) => {
                    return Err(e);
                }
            }
        } else {
            None
        };

        match zip64locator {
            None => {
                let archive_offset = cde_start_pos
                    .checked_sub(footer.central_directory_size as u64)
                    .and_then(|x| x.checked_sub(footer.central_directory_offset as u64))
                    .ok_or(ZipError::InvalidArchive(
                        "Invalid central directory size or offset",
                    ))?;

                let directory_start = footer.central_directory_offset as u64 + archive_offset;
                let number_of_files = footer.number_of_files_on_this_disk as usize;
                Ok((archive_offset, directory_start, number_of_files))
            }
            Some(locator64) => {
                if !footer.record_too_small()
                    && footer.disk_number as u32 != locator64.disk_with_central_directory
                {
                    return Err(ZipError::UnsupportedArchive);
                }

                let search_upper_bound = cde_start_pos
                    .checked_sub(60) // minimum size of Zip64CentralDirectoryEnd + Zip64CentralDirectoryEndLocator
                    .ok_or(ZipError::InvalidArchive(
                        "File cannot contain ZIP64 central directory end",
                    ))?;
                let (footer, archive_offset) = spec::Zip64CentralDirectoryEnd::find_and_parse(
                    reader,
                    locator64.end_of_central_directory_offset,
                    search_upper_bound,
                )?;

                if footer.disk_number != footer.disk_with_central_directory {
                    return Err(ZipError::UnsupportedArchive);
                }

                let directory_start = footer
                    .central_directory_offset
                    .checked_add(archive_offset)
                    .ok_or({
                        ZipError::InvalidArchive("Invalid central directory size or offset")
                    })?;

                Ok((
                    archive_offset,
                    directory_start,
                    footer.number_of_files as usize,
                ))
            }
        }
    }

    pub fn by_name(&mut self, name: &str) -> ZipResult<ZipFile> {
        let data = self
            .entries
            .get(name)
            .ok_or(ZipError::FileNotFound)?;

        let position = {
            let mut file = &*self.file.write();
            file.seek(io::SeekFrom::Start(data.header_start))?;
            let signature = file.read_u32::<LittleEndian>()?;
            if signature != spec::LOCAL_FILE_HEADER_SIGNATURE {
                return Err(ZipError::InvalidArchive("Invalid local file header"));
            }

            file.seek(io::SeekFrom::Current(0))?
        };


        let reader = find_reader(&self.file, &data, position)?;

        Ok(ZipFile::new(reader, data.clone()))
    }
}

pub fn central_header_to_zip_file<R: Read + Seek>(
    reader: &mut R,
    archive_offset: u64,
) -> ZipResult<ZipFileData> {
    let central_header_start = reader.stream_position()?;

    let signature = reader.read_u32::<LittleEndian>()?;
    if signature != spec::CENTRAL_DIRECTORY_HEADER_SIGNATURE {
        Err(ZipError::InvalidArchive("Invalid Central Directory header"))
    } else {
        central_header_to_zip_file_inner(reader, archive_offset, central_header_start)
    }
}