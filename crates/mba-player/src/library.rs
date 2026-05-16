use std::{
    cmp::Ordering,
    ffi::OsStr,
    fs, io,
    path::{Component, Path, PathBuf},
};

use mba_protocol::{LibraryDirectory, LibraryListing, LibraryTrack};

const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &["flac", "mp3", "ogg"];

#[derive(Debug, Clone)]
pub struct LibraryBrowser {
    root: PathBuf,
}

impl LibraryBrowser {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn list(&self, path: &str) -> Result<LibraryListing, LibraryError> {
        let relative = normalize_library_path(path)?;
        let directory = self.resolve_existing(&relative)?;
        if !directory.metadata.file_type().is_dir() {
            return Err(LibraryError::not_directory(relative_path_string(&relative)));
        }

        let mut directories = Vec::new();
        let mut tracks = Vec::new();
        for child in read_visible_children(&directory.path, &relative)? {
            match child.kind {
                LibraryChildKind::Directory => directories.push(LibraryDirectory {
                    path: relative_path_string(&child.relative),
                    name: child.name,
                }),
                LibraryChildKind::AudioFile => tracks.push(LibraryTrack {
                    uri: relative_path_string(&child.relative),
                    name: child.name,
                    title: None,
                    artist: None,
                    album: None,
                    duration_s: None,
                }),
            }
        }

        Ok(LibraryListing {
            path: relative_path_string(&relative),
            directories,
            tracks,
        })
    }

    pub fn validate_track_path(&self, path: &str) -> Result<String, LibraryError> {
        let relative = normalize_non_empty_library_path(path)?;
        if !is_supported_audio_path(&relative) {
            return Err(LibraryError::unsupported_track(relative_path_string(
                &relative,
            )));
        }

        let resolved = self.resolve_existing(&relative)?;
        if !resolved.metadata.file_type().is_file() {
            return Err(LibraryError::not_track(relative_path_string(&relative)));
        }

        Ok(relative_path_string(&relative))
    }

    pub fn audio_files_for_directory(&self, path: &str) -> Result<Vec<String>, LibraryError> {
        let relative = normalize_non_empty_library_path(path)?;
        let directory = self.resolve_existing(&relative)?;
        if !directory.metadata.file_type().is_dir() {
            return Err(LibraryError::not_directory(relative_path_string(&relative)));
        }

        let mut tracks = Vec::new();
        collect_audio_files_depth_first(&directory.path, &relative, &mut tracks)?;
        if tracks.is_empty() {
            return Err(LibraryError::no_supported_tracks(relative_path_string(
                &relative,
            )));
        }
        Ok(tracks)
    }

    fn resolve_existing(&self, relative: &Path) -> Result<ResolvedPath, LibraryError> {
        let root_metadata = fs::symlink_metadata(&self.root)
            .map_err(|source| map_lookup_error(&self.root, Path::new(""), source))?;
        if root_metadata.file_type().is_symlink() {
            return Err(LibraryError::bad_path("music root must not be a symlink"));
        }
        if !root_metadata.file_type().is_dir() {
            return Err(LibraryError::not_directory(String::new()));
        }

        let mut current = self.root.clone();
        let mut relative_so_far = PathBuf::new();
        let mut metadata = root_metadata;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(LibraryError::bad_path("path must be relative"));
            };
            current.push(name);
            relative_so_far.push(name);
            metadata = fs::symlink_metadata(&current)
                .map_err(|source| map_lookup_error(&current, &relative_so_far, source))?;
            if metadata.file_type().is_symlink() {
                return Err(LibraryError::bad_path("path must not traverse symlinks"));
            }
        }

        Ok(ResolvedPath {
            path: current,
            metadata,
        })
    }
}

#[derive(Debug)]
pub enum LibraryError {
    BadPath(String),
    NotFound(String),
    NotDirectory(String),
    NotTrack(String),
    NoSupportedTracks(String),
    UnsupportedTrack(String),
    Io { path: PathBuf, source: io::Error },
}

impl LibraryError {
    fn bad_path(message: impl Into<String>) -> Self {
        Self::BadPath(message.into())
    }

    fn not_found(path: impl Into<String>) -> Self {
        Self::NotFound(path.into())
    }

    fn not_directory(path: impl Into<String>) -> Self {
        Self::NotDirectory(path.into())
    }

    fn not_track(path: impl Into<String>) -> Self {
        Self::NotTrack(path.into())
    }

    fn no_supported_tracks(path: impl Into<String>) -> Self {
        Self::NoSupportedTracks(path.into())
    }

    fn unsupported_track(path: impl Into<String>) -> Self {
        Self::UnsupportedTrack(path.into())
    }

    fn io(path: PathBuf, source: io::Error) -> Self {
        Self::Io { path, source }
    }
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadPath(message) => f.write_str(message),
            Self::NotFound(path) if path.is_empty() => f.write_str("music root was not found"),
            Self::NotFound(path) => write!(f, "library path was not found: {path}"),
            Self::NotDirectory(path) if path.is_empty() => {
                f.write_str("music root must be a directory")
            }
            Self::NotDirectory(path) => write!(f, "library path is not a directory: {path}"),
            Self::NotTrack(path) => write!(f, "library path is not a track: {path}"),
            Self::NoSupportedTracks(path) => {
                write!(
                    f,
                    "library directory contains no supported audio files: {path}"
                )
            }
            Self::UnsupportedTrack(path) => {
                write!(f, "library path is not a supported audio file: {path}")
            }
            Self::Io { path, source } => {
                write!(
                    f,
                    "library filesystem error at {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for LibraryError {}

struct ResolvedPath {
    path: PathBuf,
    metadata: fs::Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibraryChildKind {
    Directory,
    AudioFile,
}

impl LibraryChildKind {
    fn sort_rank(self) -> u8 {
        match self {
            Self::Directory => 0,
            Self::AudioFile => 1,
        }
    }
}

#[derive(Debug)]
struct LibraryChild {
    name: String,
    relative: PathBuf,
    path: PathBuf,
    kind: LibraryChildKind,
}

fn normalize_non_empty_library_path(path: &str) -> Result<PathBuf, LibraryError> {
    let relative = normalize_library_path(path)?;
    if relative.as_os_str().is_empty() {
        return Err(LibraryError::bad_path("path must not be empty"));
    }
    Ok(relative)
}

fn normalize_library_path(path: &str) -> Result<PathBuf, LibraryError> {
    let path = path.trim();
    if path.contains('\0') {
        return Err(LibraryError::bad_path("path must not contain NUL bytes"));
    }
    if path.contains("://") || path.starts_with("file:") {
        return Err(LibraryError::bad_path("path must be a library path"));
    }

    let library_path = Path::new(path);
    if library_path.is_absolute() {
        return Err(LibraryError::bad_path("path must be relative"));
    }

    let mut normalized = PathBuf::new();
    for component in library_path.components() {
        match component {
            Component::Normal(name) => {
                let Some(name) = name.to_str() else {
                    return Err(LibraryError::bad_path("path must be valid UTF-8"));
                };
                if is_hidden_name(name) {
                    return Err(LibraryError::bad_path(
                        "path must not include hidden components",
                    ));
                }
                normalized.push(name);
            }
            Component::CurDir => return Err(LibraryError::bad_path("path must not contain .")),
            Component::ParentDir => {
                return Err(LibraryError::bad_path("path must not contain .."));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(LibraryError::bad_path("path must be relative"));
            }
        }
    }

    Ok(normalized)
}

fn read_visible_children(
    directory: &Path,
    relative: &Path,
) -> Result<Vec<LibraryChild>, LibraryError> {
    let entries = fs::read_dir(directory)
        .map_err(|source| LibraryError::io(directory.to_path_buf(), source))?;
    let mut children = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|source| LibraryError::io(directory.to_path_buf(), source))?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if is_hidden_name(name) {
            continue;
        }

        let file_type = entry
            .file_type()
            .map_err(|source| LibraryError::io(entry.path(), source))?;
        if file_type.is_symlink() {
            continue;
        }

        let kind = if file_type.is_dir() {
            LibraryChildKind::Directory
        } else if file_type.is_file() && is_supported_audio_name(&file_name) {
            LibraryChildKind::AudioFile
        } else {
            continue;
        };

        children.push(LibraryChild {
            name: name.to_string(),
            relative: relative.join(name),
            path: entry.path(),
            kind,
        });
    }

    children.sort_by(compare_children);
    Ok(children)
}

fn collect_audio_files_depth_first(
    directory: &Path,
    relative: &Path,
    tracks: &mut Vec<String>,
) -> Result<(), LibraryError> {
    for child in read_visible_children(directory, relative)? {
        match child.kind {
            LibraryChildKind::Directory => {
                collect_audio_files_depth_first(&child.path, &child.relative, tracks)?;
            }
            LibraryChildKind::AudioFile => tracks.push(relative_path_string(&child.relative)),
        }
    }
    Ok(())
}

fn compare_children(left: &LibraryChild, right: &LibraryChild) -> Ordering {
    left.kind
        .sort_rank()
        .cmp(&right.kind.sort_rank())
        .then_with(|| compare_names(&left.name, &right.name))
}

fn compare_names(left: &str, right: &str) -> Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
}

fn is_supported_audio_path(path: &Path) -> bool {
    path.extension().is_some_and(is_supported_audio_extension)
}

fn is_supported_audio_name(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(is_supported_audio_extension)
}

fn is_supported_audio_extension(extension: &OsStr) -> bool {
    extension.to_str().is_some_and(|extension| {
        SUPPORTED_AUDIO_EXTENSIONS
            .iter()
            .any(|supported| extension.eq_ignore_ascii_case(supported))
    })
}

fn relative_path_string(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        if let Component::Normal(name) = component {
            if let Some(name) = name.to_str() {
                parts.push(name);
            }
        }
    }
    parts.join("/")
}

fn map_lookup_error(path: &Path, relative: &Path, source: io::Error) -> LibraryError {
    if source.kind() == io::ErrorKind::NotFound {
        LibraryError::not_found(relative_path_string(relative))
    } else {
        LibraryError::io(path.to_path_buf(), source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLibrary {
        root: PathBuf,
    }

    impl TestLibrary {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mba-library-test-{}-{}",
                std::process::id(),
                unique_suffix()
            ));
            fs::create_dir_all(&root).expect("create test library");
            Self { root }
        }

        fn browser(&self) -> LibraryBrowser {
            LibraryBrowser::new(self.root.clone())
        }

        fn mkdir(&self, path: &str) {
            fs::create_dir_all(self.root.join(path)).expect("create directory");
        }

        fn write(&self, path: &str) {
            let path = self.root.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(path, b"audio").expect("write test file");
        }
    }

    impl Drop for TestLibrary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn unique_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos()
    }

    #[test]
    fn list_filters_hidden_unsupported_and_sorts_entries() {
        let library = TestLibrary::new();
        library.mkdir("beta");
        library.mkdir("Alpha");
        library.mkdir(".hidden");
        library.write("z-notes.txt");
        library.write("b.MP3");
        library.write("A.flac");
        library.write(".secret.ogg");

        let listing = library.browser().list("").expect("list root");

        let directories: Vec<_> = listing
            .directories
            .iter()
            .map(|dir| dir.name.as_str())
            .collect();
        let tracks: Vec<_> = listing
            .tracks
            .iter()
            .map(|track| track.name.as_str())
            .collect();
        assert_eq!(directories, ["Alpha", "beta"]);
        assert_eq!(tracks, ["A.flac", "b.MP3"]);
    }

    #[test]
    fn directory_queueing_is_depth_first_with_directories_before_files() {
        let library = TestLibrary::new();
        library.write("Album/root.flac");
        library.write("Album/Disc 2/02.mp3");
        library.write("Album/Disc 1/01.OGG");
        library.write("Album/.hidden/hidden.flac");
        library.write("Album/cover.jpg");

        let tracks = library
            .browser()
            .audio_files_for_directory("Album")
            .expect("collect tracks");

        assert_eq!(
            tracks,
            [
                "Album/Disc 1/01.OGG",
                "Album/Disc 2/02.mp3",
                "Album/root.flac"
            ]
        );
    }

    #[test]
    fn validates_supported_track_case_insensitively() {
        let library = TestLibrary::new();
        library.write("Album/TRACK.FLAC");

        let track = library
            .browser()
            .validate_track_path(" Album/TRACK.FLAC ")
            .expect("valid track");

        assert_eq!(track, "Album/TRACK.FLAC");
    }

    #[test]
    fn rejects_paths_outside_the_library() {
        let library = TestLibrary::new();
        let browser = library.browser();

        for path in [
            "/data/music/song.flac",
            "../song.flac",
            "Album/../song.flac",
            "./song.flac",
            "http://example.test/song.mp3",
            "file:///data/music/song.flac",
            ".hidden/song.flac",
        ] {
            assert!(
                browser.list(path).is_err(),
                "expected {path:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_non_audio_tracks_for_playback() {
        let library = TestLibrary::new();
        library.write("Album/cover.jpg");

        assert!(library
            .browser()
            .validate_track_path("Album/cover.jpg")
            .is_err());
    }

    #[test]
    fn rejects_directory_queueing_when_no_supported_tracks_are_found() {
        let library = TestLibrary::new();
        library.write("Album/cover.jpg");
        library.write("Album/.hidden/secret.mp3");

        let error = library
            .browser()
            .audio_files_for_directory("Album")
            .expect_err("directory has no visible supported tracks");

        assert!(matches!(error, LibraryError::NoSupportedTracks(path) if path == "Album"));
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks_in_listing_and_recursive_queueing() {
        use std::os::unix::fs::symlink;

        let library = TestLibrary::new();
        library.write("real.flac");
        library.write("Album/first.flac");
        library.write("Album/real.flac");
        symlink(
            library.root.join("real.flac"),
            library.root.join("linked.flac"),
        )
        .expect("create symlink");
        symlink(
            library.root.join("Album/real.flac"),
            library.root.join("Album/linked.flac"),
        )
        .expect("create nested symlink");

        let listing = library.browser().list("").expect("list root");
        let tracks: Vec<_> = listing
            .tracks
            .iter()
            .map(|track| track.name.as_str())
            .collect();
        assert_eq!(tracks, ["real.flac"]);

        let queued = library
            .browser()
            .audio_files_for_directory("Album")
            .expect("collect tracks");
        assert_eq!(queued, ["Album/first.flac", "Album/real.flac"]);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_explicit_symlink_paths() {
        use std::os::unix::fs::symlink;

        let library = TestLibrary::new();
        library.write("real.flac");
        symlink(
            library.root.join("real.flac"),
            library.root.join("linked.flac"),
        )
        .expect("create symlink");

        assert!(library
            .browser()
            .validate_track_path("linked.flac")
            .is_err());
    }
}
