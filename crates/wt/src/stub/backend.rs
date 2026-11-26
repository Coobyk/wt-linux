//! wt-backend contract stub: `std::process` with the child's stdout and stderr on one pipe.
//! Not a real pty (no job control, `isatty` is false, resize is a no-op), but enough to run
//! a command and pump its output through the terminal. Output gets the `\n` -> `\r\n`
//! translation a pty's line discipline (ONLCR) would apply.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

pub struct PtySpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

pub struct Pty {
    child: Mutex<Child>,
    output: Mutex<Option<std::io::PipeReader>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
}

impl Pty {
    pub fn spawn(spec: PtySpec) -> anyhow::Result<Pty> {
        let (reader, writer) = std::io::pipe()?;
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .env("COLUMNS", spec.cols.to_string())
            .env("LINES", spec.rows.to_string())
            .stdin(Stdio::piped())
            .stdout(writer.try_clone()?)
            .stderr(writer);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("spawning {:?}: {e}", spec.program))?;
        // `cmd` holds the write end too; drop it so the reader sees EOF when the child exits.
        drop(cmd);
        let stdin = child.stdin.take();
        Ok(Pty {
            child: Mutex::new(child),
            output: Mutex::new(Some(reader)),
            stdin: Arc::new(Mutex::new(stdin)),
        })
    }

    /// The child's output. The first call takes the pipe; later calls read nothing.
    pub fn reader(&self) -> Box<dyn Read + Send> {
        match self.output.lock().unwrap().take() {
            Some(r) => Box::new(Onlcr {
                inner: r,
                pending_lf: false,
            }),
            None => Box::new(std::io::empty()),
        }
    }

    pub fn writer(&self) -> Box<dyn Write + Send> {
        Box::new(SharedStdin(self.stdin.clone()))
    }

    pub fn resize(&self, _cols: u16, _rows: u16) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn child_exited(&self) -> Option<ExitStatus> {
        self.child.lock().unwrap().try_wait().ok().flatten()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap();
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct SharedStdin(Arc<Mutex<Option<ChildStdin>>>);

impl Write for SharedStdin {
    /// `\r` -> `\n`, as a tty's ICRNL input flag does, so Enter submits a line, and
    /// `^D` closes the child's stdin, as VEOF does.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let (data, eof) = match buf.iter().position(|&b| b == 0x04) {
            Some(i) => (&buf[..i], true),
            None => (buf, false),
        };
        let translated: Vec<u8> = data
            .iter()
            .map(|&b| if b == b'\r' { b'\n' } else { b })
            .collect();
        let mut stdin = self.0.lock().unwrap();
        match stdin.as_mut() {
            Some(s) => s.write_all(&translated)?,
            None => return Err(std::io::ErrorKind::BrokenPipe.into()),
        }
        if eof {
            stdin.take();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.0.lock().unwrap().as_mut() {
            Some(s) => s.flush(),
            None => Ok(()),
        }
    }
}

/// `\n` -> `\r\n`, as a tty's ONLCR output flag does.
struct Onlcr<R> {
    inner: R,
    pending_lf: bool,
}

impl<R: Read> Read for Onlcr<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.pending_lf {
            self.pending_lf = false;
            out[0] = b'\n';
            return Ok(1);
        }
        let mut raw = vec![0u8; out.len().div_ceil(2)];
        let n = self.inner.read(&mut raw)?;
        let mut w = 0;
        for &b in &raw[..n] {
            if b == b'\n' {
                out[w] = b'\r';
                w += 1;
                if w == out.len() {
                    self.pending_lf = true;
                    break;
                }
            }
            out[w] = b;
            w += 1;
        }
        Ok(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onlcr_translates_across_tiny_buffers() {
        let mut r = Onlcr {
            inner: &b"a\nb\n\n"[..],
            pending_lf: false,
        };
        let mut got = Vec::new();
        let mut buf = [0u8; 1];
        while let Ok(n @ 1..) = r.read(&mut buf) {
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(got, b"a\r\nb\r\n\r\n");
        let mut r = Onlcr {
            inner: &b"x\ny"[..],
            pending_lf: false,
        };
        let mut big = Vec::new();
        r.read_to_end(&mut big).unwrap();
        assert_eq!(big, b"x\r\ny");
    }
}
