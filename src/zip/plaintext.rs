use std::fs::File;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom};
use std::sync::Arc;
use parking_lot::RwLock;

pub struct PlaintextReader {
    pub file: Arc<RwLock<File>>,
    pub start: u64,
    pub end: u64,
    pub position: u64,
}

impl PlaintextReader {
    pub fn new(file: Arc<RwLock<File>>, start: u64, end: u64) -> Self {
        Self {
            file,
            start,
            end,
            position: 0,
        }
    }
}

impl Read for PlaintextReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.position >= self.end - self.start {
            return Ok(0);
        }
        let from = self.position + self.start;
        let len = self.end - self.start - self.position;
        let limit = (len as usize).min(buf.len());
        let mut size = {
            let mut file = &*self.file.write();
            file.seek(SeekFrom::Start(from))?;
            file.read(&mut buf[0..limit])?
        };

        self.position += size as u64;

        Ok(size)
    }
}

impl Seek for PlaintextReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let mut position = 0u64;
        match pos {
            SeekFrom::Start(pos) => position = pos,
            SeekFrom::End(pos) => position = (self.end as i64 - self.start as i64 - pos) as u64,
            SeekFrom::Current(pos) => position = (self.position as i64 + pos) as u64,
        }
        if position < 0 || position > self.end - self.start {
            return Err(Error::new(ErrorKind::InvalidInput, "Invalid seek input"));
        }
        self.position = position;
        Ok(self.position)
    }
}