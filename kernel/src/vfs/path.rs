use core::str::FromStr;

use alloc::fmt;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A path in the file system.
#[derive(Debug, Clone)]
pub(crate) struct FilePath {
    /// An absolute path starts from the root directory.
    pub(crate) absolute: bool,

    /// Components of a path not including separators (the `/` character).
    pub(crate) components: Vec<FilePathComponent>,
}

impl FilePath {
    pub(crate) fn split_dirname_filename(&self) -> Option<(Self, FilePathComponent)> {
        let (filename, parent) = self.components.split_last()?;
        let parent_path = Self {
            absolute: self.absolute,
            components: parent.to_vec(),
        };
        Some((parent_path, filename.clone()))
    }

    pub(crate) fn as_string(&self) -> String {
        let mut s = String::new();
        if self.absolute {
            s.push('/');
        }
        s.push_str(
            &self
                .components
                .iter()
                .map(FilePathComponent::as_str)
                .collect::<Vec<_>>()
                .join("/"),
        );
        s
    }
}

/// A component of a file path. Notably, this cannot include the `/` character,
/// and is non-empty.
#[derive(Debug, Clone)]
pub(crate) struct FilePathComponent(String);

impl FilePathComponent {
    fn new(s: &str) -> Option<Self> {
        assert!(
            !s.contains('/'),
            "constructed FilePathComponent with '/': {s}"
        );
        if s.is_empty() {
            None
        } else {
            Some(Self(s.to_string()))
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FilePathComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        Ok(())
    }
}

impl FilePath {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let absolute = s.starts_with('/');
        let components: Vec<FilePathComponent> = s
            .split('/')
            .filter(|s| !s.is_empty())
            .filter_map(FilePathComponent::new)
            .collect();
        if !absolute && components.is_empty() {
            None
        } else {
            Some(Self {
                absolute,
                components,
            })
        }
    }
}

impl FromStr for FilePath {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or("file path is empty")
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.absolute {
            write!(f, "/")?;
        }
        for component in &self.components {
            write!(f, "{}", component.0)?;
            write!(f, "/")?;
        }
        Ok(())
    }
}
