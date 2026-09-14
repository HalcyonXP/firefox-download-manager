//! Fileless, registration-free stdio fixture; never an installed Manager host.
//!
//! A retained browser SDK process test may copy this image into its own domain.
//! This validates fixed arguments and bounded framing only: no engine, network,
//! download files, permissions, policy verdict or native peer authentication.

use std::{
    ffi::{OsStr, OsString},
    io::{self, Read, Write},
    path::Path,
    process::ExitCode,
};

const HOST: &str = "com.halcyonxp.firefox_download_manager";
const EXTENSION: &str = "download-manager@halcyonxp.local";
const IMAGE: &str = "download-manager-native-host.exe";
const MAX_FRAME: usize = 1024 * 1024;
const PING: &str = r#"{"fixture":"owned-parent-stdio-v1","op":"ping","value":"π"}"#;
const PONG: &str = r#"{"fixture":"owned-parent-stdio-v1","op":"pong","value":"π"}"#;

fn arguments_match(executable: &Path, arguments: &[OsString]) -> bool {
    let Some(parent) = executable.parent() else {
        return false;
    };
    executable.file_name() == Some(OsStr::new(IMAGE))
        && arguments
            == [
                OsString::from("--browser-parent"),
                parent.join(format!("{HOST}.json")).into_os_string(),
                OsString::from(EXTENSION),
            ]
}

fn frame(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    let length =
        u32::try_from(body.len()).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

fn receive(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    // EOF is natural retirement only at a frame boundary. Incomplete requests
    // are not treated as delivered or replayed, even in this diagnostic fixture.
    let mut header = [0; 4];
    loop {
        match reader.read(&mut header[..1]) {
            Ok(0) => return Ok(None),
            Ok(1) => break,
            Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    reader.read_exact(&mut header[1..])?;
    let length = usize::try_from(u32::from_le_bytes(header))
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    if length == 0 || length > MAX_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

fn serve(reader: &mut impl Read, writer: &mut impl Write, pid: u32) -> io::Result<()> {
    if pid == 0 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    frame(
        writer,
        format!(r#"{{"fixture":"owned-parent-stdio-v1","op":"ready","pid":{pid}}}"#).as_bytes(),
    )?;
    let mut answered = false;
    while let Some(body) = receive(reader)? {
        if answered || body != PING.as_bytes() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        answered = true;
        frame(writer, PONG.as_bytes())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let Ok(executable) = std::env::current_exe() else {
        return ExitCode::from(2);
    };
    // At most one excess argument is retained. Never log paths or input bytes.
    let arguments: Vec<_> = std::env::args_os().skip(1).take(4).collect();
    if !arguments_match(&executable, &arguments) {
        return ExitCode::from(2);
    }
    let result = (|| {
        // Fixed non-sensitive data exercises the separate retained stderr pipe.
        io::stderr().lock().write_all(b"owned stdio fixture\n")?;
        serve(
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
            std::process::id(),
        )
    })();
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn request(body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        frame(&mut bytes, body).unwrap();
        bytes
    }

    #[test]
    fn only_fixed_private_arguments_of_the_owned_renamed_fixture_match() {
        let image = Path::new("owned").join(IMAGE);
        let arguments = [
            OsString::from("--browser-parent"),
            Path::new("owned")
                .join(format!("{HOST}.json"))
                .into_os_string(),
            OsString::from(EXTENSION),
        ];
        assert!(arguments_match(&image, &arguments));
        assert!(!arguments_match(
            &Path::new("owned").join("other.exe"),
            &arguments
        ));
        assert!(!arguments_match(&image, &arguments[1..]));
        for index in 0..3 {
            let mut wrong = arguments.clone();
            wrong[index] = OsString::from("refused");
            assert!(!arguments_match(&image, &wrong));
        }
        let mut excess = arguments.to_vec();
        excess.push(OsString::from("excess"));
        assert!(!arguments_match(&image, &excess));
    }

    #[test]
    fn one_fixed_utf8_roundtrip_and_boundary_eof() {
        let mut output = Vec::new();
        serve(&mut Cursor::new(request(PING.as_bytes())), &mut output, 4).unwrap();
        let mut expected = Vec::new();
        frame(
            &mut expected,
            br#"{"fixture":"owned-parent-stdio-v1","op":"ready","pid":4}"#,
        )
        .unwrap();
        frame(&mut expected, PONG.as_bytes()).unwrap();
        assert_eq!(output, expected);
        let mut output = Vec::new();
        serve(&mut Cursor::new([]), &mut output, 4).unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn failed_output_or_flush_refuses_before_reading_a_request() {
        struct RefusedOutput(bool);
        impl Write for RefusedOutput {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0 {
                    Err(io::ErrorKind::BrokenPipe.into())
                } else {
                    Ok(bytes.len())
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }
        for write_fails in [false, true] {
            let mut reader = Cursor::new(request(PING.as_bytes()));
            assert_eq!(
                serve(&mut reader, &mut RefusedOutput(write_fails), 4)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::BrokenPipe,
            );
            assert_eq!(reader.position(), 0);
        }
    }

    #[test]
    fn truncated_oversized_unknown_and_repeated_frames_refuse() {
        let valid = request(PING.as_bytes());
        for cut in 1..valid.len() {
            assert!(serve(&mut Cursor::new(&valid[..cut]), &mut Vec::new(), 4).is_err());
        }
        // Keep even an omitted-bound mutation resource-bounded (just over 1 MiB).
        for length in [0, u32::try_from(MAX_FRAME + 1).unwrap()] {
            assert_eq!(
                receive(&mut Cursor::new(length.to_le_bytes()))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData,
            );
        }
        assert!(serve(&mut Cursor::new(request(b"{}")), &mut Vec::new(), 4).is_err());
        let mut repeated = valid.clone();
        repeated.extend_from_slice(&valid);
        assert!(serve(&mut Cursor::new(repeated), &mut Vec::new(), 4).is_err());
        assert!(serve(&mut Cursor::new([]), &mut Vec::new(), 0).is_err());
    }
}
