use core::{fmt, ops::Deref, str};

use serde::Serialize;

pub struct ByteBuffer<const N: usize> {
    buf: [u8; N],
    cursor: usize,
}

impl<const N: usize> ByteBuffer<N> {
    pub fn new() -> Self {
        ByteBuffer {
            buf: [0; N],
            cursor: 0,
        }
    }

    pub fn write(&mut self, buf: &[u8]) {
        self.buf[self.cursor..self.cursor + buf.len()].copy_from_slice(buf);
        self.cursor += buf.len();
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buf[0..self.cursor]
    }

    pub fn serialize<T>(&mut self, value: &T) -> serde_json_core::ser::Result<()>
    where
        T: Serialize + ?Sized,
    {
        let len = serde_json_core::to_slice(value, &mut self.buf[self.cursor..])?;
        self.cursor += len;
        Ok(())
    }

    pub fn as_str(&self) -> &str {
        str::from_utf8(self.buffer()).unwrap()
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }
}

impl<const N: usize> Deref for ByteBuffer<N> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.buffer()
    }
}

impl<const N: usize> fmt::Write for ByteBuffer<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let cap = self.capacity();
        for (i, &b) in self.buf[self.cursor..cap]
            .iter_mut()
            .zip(s.as_bytes().iter())
        {
            *i = b;
        }
        self.cursor = usize::min(cap, self.cursor + s.as_bytes().len());
        Ok(())
    }
}
