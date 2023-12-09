use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use flate2::{Decompress, FlushDecompress, Status};
use parking_lot::RwLock;

const DEFLATE_BUF_SIZE: usize = 32;

pub struct DeflateReader {
    file: Arc<RwLock<File>>,
    position: u64,
    start: u64,
    end: u64,
    decompress: Decompress,
    deflate_buf: [u8; DEFLATE_BUF_SIZE],
    deflate_size: usize,
    deflate_position: usize,
}

impl DeflateReader {
    pub fn new(file: Arc<RwLock<File>>, start: u64, end: u64) -> Self {
        Self {
            file,
            position: 0,
            start,
            end,
            decompress: Decompress::new(false),
            deflate_buf: [0u8; DEFLATE_BUF_SIZE],
            deflate_size: 0,
            deflate_position: 0,
        }
    }
}

impl Read for DeflateReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.position >= self.end - self.start {
            return Ok(0);
        }
        loop {
            let (read, consumed, ret, eof);
            {
                if self.deflate_position == self.deflate_size {
                    let from = self.position + self.start;
                    let limit = (self.end - self.start - self.position).min(DEFLATE_BUF_SIZE as u64);
                    {
                        let mut file = &*self.file.write();
                        file.seek(SeekFrom::Start(from))?;
                        self.deflate_size = file.read(&mut self.deflate_buf[0..limit as usize])?;
                    }
                    self.deflate_position = 0;
                }

                let input = &self.deflate_buf[self.deflate_position..self.deflate_size];

                eof = input.is_empty();

                let before_out = self.decompress.total_out();
                let before_in = self.decompress.total_in();
                let flush = if eof {
                    FlushDecompress::Finish
                } else {
                    FlushDecompress::None
                };

                ret = self.decompress.decompress(input, buf, flush);
                read = (self.decompress.total_out() - before_out) as usize;
                consumed = (self.decompress.total_in() - before_in) as usize;
            }
            self.deflate_position += consumed;
            self.position += consumed as u64;

            match ret {
                Ok(Status::Ok | Status::BufError) if read == 0 && !eof && !buf.is_empty() => continue,
                Ok(Status::Ok | Status::BufError | Status::StreamEnd) => return Ok(read),

                Err(..) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "corrupt deflate stream",
                    ));
                }
            }
        }
    }
}

impl Seek for DeflateReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        todo!()
    }
}