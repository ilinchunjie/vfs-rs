use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use byteorder::{LittleEndian, ReadBytesExt};
use parking_lot::RwLock;
use crate::zip::cp437::FromCp437;
use crate::zip::deflate::DeflateReader;
use crate::zip::plaintext::PlaintextReader;
use crate::zip::result::{ZipError, ZipResult};
use crate::zip::spec;
use crate::zip::spec::{AesMode, AesVendorVersion, CompressionMethod};

pub struct ZipFile {
    reader: ZipFileReader,
    data: Arc<ZipFileData>,
}

impl ZipFile {
    pub fn new(reader: ZipFileReader, data: Arc<ZipFileData>) -> Self {
        Self {
            reader,
            data,
        }
    }

    pub fn len(&self) -> u64 {
        return self.data.compressed_size;
    }
}

impl Read for ZipFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl Seek for ZipFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.reader.seek(pos)
    }
}

pub struct ZipFileData {
    pub compression_method: CompressionMethod,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub file_name: String,
    pub extra_field: Vec<u8>,
    pub header_start: u64,
    pub central_header_start: u64,
    pub large_file: bool,
    pub aes_mode: Option<(AesMode, AesVendorVersion)>,
}

pub fn central_header_to_zip_file_inner<R: Read>(reader: &mut R, archive_offset: u64, central_header_start: u64) -> ZipResult<ZipFileData> {
    let version_made_by = reader.read_u16::<LittleEndian>()?;
    let _version_to_extract = reader.read_u16::<LittleEndian>()?;
    let flags = reader.read_u16::<LittleEndian>()?;
    let encrypted = flags & 1 == 1;
    let is_utf8 = flags & (1 << 11) != 0;
    let using_data_descriptor = flags & (1 << 3) != 0;
    let compression_method = reader.read_u16::<LittleEndian>()?;
    let last_mod_time = reader.read_u16::<LittleEndian>()?;
    let last_mod_date = reader.read_u16::<LittleEndian>()?;
    let crc32 = reader.read_u32::<LittleEndian>()?;
    let compressed_size = reader.read_u32::<LittleEndian>()?;
    let uncompressed_size = reader.read_u32::<LittleEndian>()?;
    let file_name_length = reader.read_u16::<LittleEndian>()? as usize;
    let extra_field_length = reader.read_u16::<LittleEndian>()? as usize;
    let file_comment_length = reader.read_u16::<LittleEndian>()? as usize;
    let _disk_number = reader.read_u16::<LittleEndian>()?;
    let _internal_file_attributes = reader.read_u16::<LittleEndian>()?;
    let external_file_attributes = reader.read_u32::<LittleEndian>()?;
    let offset = reader.read_u32::<LittleEndian>()? as u64;
    let mut file_name_raw = vec![0; file_name_length];
    reader.read_exact(&mut file_name_raw)?;
    let mut extra_field = vec![0; extra_field_length];
    reader.read_exact(&mut extra_field)?;
    let mut file_comment_raw = vec![0; file_comment_length];
    reader.read_exact(&mut file_comment_raw)?;

    let file_name = match is_utf8 {
        true => String::from_utf8_lossy(&file_name_raw).into_owned(),
        false => file_name_raw.clone().from_cp437(),
    };
    let file_comment = match is_utf8 {
        true => String::from_utf8_lossy(&file_comment_raw).into_owned(),
        false => file_comment_raw.from_cp437(),
    };

    let mut result = ZipFileData {
        compression_method: {
            CompressionMethod::from_u16(compression_method)
        },
        compressed_size: compressed_size as u64,
        uncompressed_size: uncompressed_size as u64,
        file_name,
        extra_field,
        header_start: offset,
        central_header_start,
        large_file: false,
        aes_mode: None,
    };

    match parse_extra_field(&mut result) {
        Ok(..) | Err(ZipError::Io(..)) => {}
        Err(e) => return Err(e),
    }

    match result.compression_method {
        CompressionMethod::Unsupported(method) => {
            return Err(ZipError::UnsupportedCompressionMethod(method));
        }
        _ => {}
    }

    // Account for shifted zip offsets.
    result.header_start = result
        .header_start
        .checked_add(archive_offset)
        .ok_or(ZipError::InvalidArchive("Archive header is too large"))?;

    Ok(result)
}

pub fn find_reader(file: &Arc<RwLock<File>>, data: &ZipFileData, position: u64) -> ZipResult<ZipFileReader> {
    let data_start = {
        let mut file = &*file.write();
        file.seek(io::SeekFrom::Start(position + 22))?;
        let file_name_length = file.read_u16::<LittleEndian>()? as u64;
        let extra_field_length = file.read_u16::<LittleEndian>()? as u64;
        let magic_and_header = 4 + 22 + 2 + 2;
        data.header_start + magic_and_header + file_name_length + extra_field_length
    };

    match data.compression_method {
        CompressionMethod::Stored => {
            return Ok(ZipFileReader::Stored(PlaintextReader::new(file.clone(), data_start, data_start + data.compressed_size)));
        }
        CompressionMethod::Deflate => {
            return Ok(ZipFileReader::Deflate(DeflateReader::new(file.clone(), data_start, data_start + data.compressed_size)));
        }
        CompressionMethod::Unsupported(method) => {
            return Err(ZipError::UnsupportedCompressionMethod(method));
        }
    }
}

fn parse_extra_field(file: &mut ZipFileData) -> ZipResult<()> {
    let mut reader = io::Cursor::new(&file.extra_field);

    while (reader.position() as usize) < file.extra_field.len() {
        let kind = reader.read_u16::<LittleEndian>()?;
        let len = reader.read_u16::<LittleEndian>()?;
        let mut len_left = len as i64;
        match kind {
            0x0001 => {
                if file.uncompressed_size == spec::ZIP64_BYTES_THR {
                    file.large_file = true;
                    file.uncompressed_size = reader.read_u64::<LittleEndian>()?;
                    len_left -= 8;
                }
                if file.compressed_size == spec::ZIP64_BYTES_THR {
                    file.large_file = true;
                    file.compressed_size = reader.read_u64::<LittleEndian>()?;
                    len_left -= 8;
                }
                if file.header_start == spec::ZIP64_BYTES_THR {
                    file.header_start = reader.read_u64::<LittleEndian>()?;
                    len_left -= 8;
                }
            }
            0x9901 => {
                // AES
                if len != 7 {
                    return Err(ZipError::UnsupportedAesExtraData);
                }
                let vendor_version = reader.read_u16::<LittleEndian>()?;
                let vendor_id = reader.read_u16::<LittleEndian>()?;
                let aes_mode = reader.read_u8()?;
                let compression_method = reader.read_u16::<LittleEndian>()?;

                if vendor_id != 0x4541 {
                    return Err(ZipError::InvalidArchive("Invalid AES vendor"));
                }
                let vendor_version = match vendor_version {
                    0x0001 => AesVendorVersion::Ae1,
                    0x0002 => AesVendorVersion::Ae2,
                    _ => return Err(ZipError::InvalidArchive("Invalid AES vendor version")),
                };
                match aes_mode {
                    0x01 => file.aes_mode = Some((AesMode::Aes128, vendor_version)),
                    0x02 => file.aes_mode = Some((AesMode::Aes192, vendor_version)),
                    0x03 => file.aes_mode = Some((AesMode::Aes256, vendor_version)),
                    _ => return Err(ZipError::InvalidArchive("Invalid AES encryption strength")),
                };
                file.compression_method = {
                    #[allow(deprecated)]
                    CompressionMethod::from_u16(compression_method)
                };
            }
            _ => {
                // Other fields are ignored
            }
        }

        // We could also check for < 0 to check for errors
        if len_left > 0 {
            reader.seek(io::SeekFrom::Current(len_left))?;
        }
    }
    Ok(())
}

pub enum ZipFileReader {
    Stored(PlaintextReader),
    Deflate(DeflateReader),
}

impl Read for ZipFileReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        return match self {
            ZipFileReader::Stored(r) => r.read(buf),
            ZipFileReader::Deflate(r) => r.read(buf),
        }
    }
}

impl Seek for ZipFileReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        return match self {
            ZipFileReader::Stored(r) => r.seek(pos),
            ZipFileReader::Deflate(r) => r.seek(pos),
        }
    }
}