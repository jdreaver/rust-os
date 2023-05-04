use genio::error::ReadExactError;
use zerocopy::{AsBytes, FromBytes};

pub fn zero_copy_read<R, T>(reader: &mut R) -> Result<T, ReadExactError<R::ReadError>>
where
    R: genio::Read,
    T: AsBytes + FromBytes,
    R::ReadError: core::fmt::Debug,
{
    let mut s = T::new_zeroed();
    let buf = s.as_bytes_mut();
    reader.read_exact(buf)?;
    Ok(s)
}
