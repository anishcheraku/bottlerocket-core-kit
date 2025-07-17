use std::collections::HashMap;
use std::fs::File;
use std::io::{ErrorKind, Seek, SeekFrom, Write};
use std::os::linux::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::{fs, io};
use tempfile::tempfile;

/// The SystemAccess trait allows for plumbing in host system access during execution of checks. It
/// exists solely to make is possible to unit tests checks.
pub trait SystemAccess {
    /// open opens the specified file
    fn open(&self, path: &str) -> io::Result<File>;
    /// metadata returns the file metadata (owner, group, mode)
    fn metadata(&self, path: &str) -> io::Result<FileMetadata>;
    /// exists returns true if the given path exists
    fn exists(&self, path: &str) -> bool;
    /// command_output returns the output of running the specified command with the suppplied arguments
    fn command_output(&self, cmd: &str, args: &[&str]) -> io::Result<Output>;
}

/// FileMetadata is the subset of file metadata used in checks. The standard fs::Metadata type can't
/// be mocked, so this type replaces it to allow for tests that relay on file metadata.
#[derive(Clone, Debug)]
pub struct FileMetadata {
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
}

/// NativeSystemAccess provides access to the host system.
pub struct NativeSystemAccess;
impl SystemAccess for NativeSystemAccess {
    fn open(&self, path: &str) -> io::Result<File> {
        File::open(path)
    }
    fn metadata(&self, path: &str) -> io::Result<FileMetadata> {
        let metadata = fs::metadata(path)?;
        Ok(FileMetadata {
            uid: metadata.st_uid(),
            gid: metadata.st_gid(),
            mode: metadata.permissions().mode(),
        })
    }
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
    fn command_output(&self, cmd: &str, args: &[&str]) -> io::Result<Output> {
        Command::new(cmd).args(args).output()
    }
}

/// UnitTestSystemAccess is only used for unit tests
#[doc(hidden)]
pub struct UnitTestSystemAccess {
    files: HashMap<String, String>,
    metadata: HashMap<String, FileMetadata>,
    commands: HashMap<(String, Vec<String>), Output>,
}

impl UnitTestSystemAccess {
    pub fn new() -> Self {
        UnitTestSystemAccess {
            files: HashMap::new(),
            metadata: HashMap::new(),
            commands: HashMap::new(),
        }
    }
    pub fn register_file(&mut self, path: &str, contents: &str) {
        self.files.insert(path.to_string(), contents.to_string());
    }

    pub fn register_file_with_metadata(
        &mut self,
        path: &str,
        contents: &str,
        mode: u32,
        uid: u32,
        gid: u32,
    ) {
        self.register_file(path, contents);
        self.metadata
            .insert(path.to_string(), FileMetadata { uid, gid, mode });
    }
    pub fn register_command(&mut self, command: &str, args: &[&str], output: Output) {
        let key = (
            command.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        );
        self.commands.insert(key, output);
    }
}
impl Default for UnitTestSystemAccess {
    fn default() -> Self {
        Self::new()
    }
}
impl SystemAccess for UnitTestSystemAccess {
    fn open(&self, path: &str) -> io::Result<File> {
        let contents = self.files.get(path).ok_or(io::Error::new(
            ErrorKind::NotFound,
            "No such file or directory",
        ))?;
        let mut file = tempfile()?;
        file.write_all(contents.as_bytes())?;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }

    fn metadata(&self, path: &str) -> io::Result<FileMetadata> {
        let metadata = self.metadata.get(path).ok_or(io::Error::new(
            ErrorKind::NotFound,
            "No such file or directory",
        ))?;
        Ok(metadata.clone())
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn command_output(&self, cmd: &str, args: &[&str]) -> io::Result<Output> {
        let output = self
            .commands
            .get(&(
                cmd.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ))
            .ok_or(io::Error::new(
                ErrorKind::NotFound,
                "No such file or directory",
            ))?;
        Ok(output.clone())
    }
}
