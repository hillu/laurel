use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// Read contents of file, return buffer.
fn slurp_file(path: impl AsRef<Path>) -> Result<Vec<u8>, Box<dyn Error>> {
    let f = File::open(path)?;
    let mut r = BufReader::with_capacity(1 << 16, f);
    r.fill_buf()?;
    let mut buf = Vec::with_capacity(8192);
    r.read_to_end(&mut buf)?;
    Ok(buf)
}

type Environment = Vec<(Vec<u8>, Vec<u8>)>;

/// Returns set of environment variables that match pred for a given process
pub fn get_environ<F>(pid: u32, pred: F) -> Result<Environment, Box<dyn Error>>
where
    F: Fn(&[u8]) -> bool,
{
    let buf = slurp_file(format!("/proc/{}/environ", pid))?;
    let mut res = Vec::new();

    for e in buf.split(|c| *c == 0) {
        let mut kv = e.splitn(2, |c| *c == b'=');
        let k = kv.next().unwrap_or_default();
        if pred(k) {
            let v = kv.next().unwrap_or_default();
            res.push((k.to_owned(), v.to_owned()));
        }
    }
    Ok(res)
}
