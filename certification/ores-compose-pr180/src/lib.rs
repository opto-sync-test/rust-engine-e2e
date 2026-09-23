#![forbid(unsafe_code)]

use std::fmt::{Display, Formatter};
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, Metadata};
use std::path::{Component, Path, PathBuf};
#[cfg(unix)]
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct TrustedWorkingDirectory {
    configured_project_root: PathBuf,
    canonical_project_root: PathBuf,
    canonical_working_dir: PathBuf,
    project_identity: FileIdentity,
    working_dir_identity: FileIdentity,
    // Keep the admitted Unix directory objects alive for the lifetime of the
    // trusted plan. A removed directory's inode cannot be recycled while an
    // open handle still references it, so a remove/recreate at the same path
    // cannot masquerade as the originally admitted object by reusing dev+ino.
    #[cfg(unix)]
    project_handle: Arc<File>,
    #[cfg(unix)]
    working_dir_handle: Arc<File>,
}

impl PartialEq for TrustedWorkingDirectory {
    fn eq(&self, other: &Self) -> bool {
        self.configured_project_root == other.configured_project_root
            && self.canonical_project_root == other.canonical_project_root
            && self.canonical_working_dir == other.canonical_working_dir
            && self.project_identity == other.project_identity
            && self.working_dir_identity == other.working_dir_identity
    }
}

impl Eq for TrustedWorkingDirectory {}

impl TrustedWorkingDirectory {
    pub fn project_root(&self) -> &Path {
        &self.canonical_project_root
    }

    pub fn working_dir(&self) -> &Path {
        &self.canonical_working_dir
    }

    pub fn validate_current(&self) -> Result<(), TrustedWorkingDirectoryError> {
        #[cfg(unix)]
        {
            let pinned_root_metadata = self
                .project_handle
                .metadata()
                .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
            if FileIdentity::from_metadata(&pinned_root_metadata) != self.project_identity {
                return Err(TrustedWorkingDirectoryError::IdentityChanged);
            }
            let pinned_working_dir_metadata = self
                .working_dir_handle
                .metadata()
                .map_err(|_| TrustedWorkingDirectoryError::WorkingDirectoryUnavailable)?;
            if FileIdentity::from_metadata(&pinned_working_dir_metadata)
                != self.working_dir_identity
            {
                return Err(TrustedWorkingDirectoryError::IdentityChanged);
            }
        }

        let root_entry = fs::symlink_metadata(&self.configured_project_root)
            .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
        if root_entry.file_type().is_symlink() {
            return Err(TrustedWorkingDirectoryError::ProjectRootIsSymlink);
        }
        if !root_entry.is_dir() {
            return Err(TrustedWorkingDirectoryError::ProjectRootNotDirectory);
        }

        let current_root = fs::canonicalize(&self.configured_project_root)
            .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
        if current_root != self.canonical_project_root {
            return Err(TrustedWorkingDirectoryError::IdentityChanged);
        }
        let current_root_metadata = fs::metadata(&current_root)
            .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
        if FileIdentity::from_metadata(&current_root_metadata) != self.project_identity {
            return Err(TrustedWorkingDirectoryError::IdentityChanged);
        }

        let current_working_dir = fs::canonicalize(&self.canonical_working_dir)
            .map_err(|_| TrustedWorkingDirectoryError::WorkingDirectoryUnavailable)?;
        if current_working_dir != self.canonical_working_dir
            || !current_working_dir.starts_with(&current_root)
        {
            return Err(TrustedWorkingDirectoryError::IdentityChanged);
        }
        let current_metadata = fs::metadata(&current_working_dir)
            .map_err(|_| TrustedWorkingDirectoryError::WorkingDirectoryUnavailable)?;
        if !current_metadata.is_dir() {
            return Err(TrustedWorkingDirectoryError::WorkingDirectoryNotDirectory);
        }
        if FileIdentity::from_metadata(&current_metadata) != self.working_dir_identity {
            return Err(TrustedWorkingDirectoryError::IdentityChanged);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl FileIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = metadata;
            Self {}
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustedWorkingDirectoryError {
    ProjectRootNotAbsolute,
    ProjectRootUnavailable,
    ProjectRootIsSymlink,
    ProjectRootNotDirectory,
    InvalidRelativeWorkingDirectory,
    WorkingDirectoryUnavailable,
    WorkingDirectoryContainsSymlink,
    WorkingDirectoryNotDirectory,
    WorkingDirectoryEscapesRoot,
    IdentityChanged,
}

impl Display for TrustedWorkingDirectoryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ProjectRootNotAbsolute => "trusted project root must be absolute",
            Self::ProjectRootUnavailable => "trusted project root is unavailable",
            Self::ProjectRootIsSymlink => "trusted project root must not be a symlink",
            Self::ProjectRootNotDirectory => "trusted project root must be a directory",
            Self::InvalidRelativeWorkingDirectory => {
                "service working directory must be relative and traversal-free"
            }
            Self::WorkingDirectoryUnavailable => "service working directory is unavailable",
            Self::WorkingDirectoryContainsSymlink => {
                "service working directory path must not contain symlink components"
            }
            Self::WorkingDirectoryNotDirectory => "service working directory must be a directory",
            Self::WorkingDirectoryEscapesRoot => {
                "service working directory resolves outside the trusted project root"
            }
            Self::IdentityChanged => {
                "trusted service working-directory identity changed before execution"
            }
        };
        f.write_str(message)
    }
}

impl std::error::Error for TrustedWorkingDirectoryError {}

pub fn resolve_trusted_working_directory(
    project_root: &Path,
    relative_working_dir: &Path,
) -> Result<TrustedWorkingDirectory, TrustedWorkingDirectoryError> {
    if !project_root.is_absolute() {
        return Err(TrustedWorkingDirectoryError::ProjectRootNotAbsolute);
    }
    validate_relative_working_dir(relative_working_dir)?;

    let root_entry = fs::symlink_metadata(project_root)
        .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
    if root_entry.file_type().is_symlink() {
        return Err(TrustedWorkingDirectoryError::ProjectRootIsSymlink);
    }
    if !root_entry.is_dir() {
        return Err(TrustedWorkingDirectoryError::ProjectRootNotDirectory);
    }

    let canonical_project_root = fs::canonicalize(project_root)
        .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
    let project_metadata = fs::metadata(&canonical_project_root)
        .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
    let project_identity = FileIdentity::from_metadata(&project_metadata);

    let mut cursor = project_root.to_path_buf();
    for component in relative_working_dir.components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(part) => cursor.push(part),
            _ => return Err(TrustedWorkingDirectoryError::InvalidRelativeWorkingDirectory),
        }
        let metadata = fs::symlink_metadata(&cursor)
            .map_err(|_| TrustedWorkingDirectoryError::WorkingDirectoryUnavailable)?;
        if metadata.file_type().is_symlink() {
            return Err(TrustedWorkingDirectoryError::WorkingDirectoryContainsSymlink);
        }
        if !metadata.is_dir() {
            return Err(TrustedWorkingDirectoryError::WorkingDirectoryNotDirectory);
        }
    }

    let canonical_working_dir = fs::canonicalize(&cursor)
        .map_err(|_| TrustedWorkingDirectoryError::WorkingDirectoryUnavailable)?;
    if !canonical_working_dir.starts_with(&canonical_project_root) {
        return Err(TrustedWorkingDirectoryError::WorkingDirectoryEscapesRoot);
    }
    let working_dir_metadata = fs::metadata(&canonical_working_dir)
        .map_err(|_| TrustedWorkingDirectoryError::WorkingDirectoryUnavailable)?;
    if !working_dir_metadata.is_dir() {
        return Err(TrustedWorkingDirectoryError::WorkingDirectoryNotDirectory);
    }
    let working_dir_identity = FileIdentity::from_metadata(&working_dir_metadata);

    #[cfg(unix)]
    let project_handle = {
        let handle = File::open(&canonical_project_root)
            .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
        let metadata = handle
            .metadata()
            .map_err(|_| TrustedWorkingDirectoryError::ProjectRootUnavailable)?;
        if !metadata.is_dir() || FileIdentity::from_metadata(&metadata) != project_identity {
            return Err(TrustedWorkingDirectoryError::IdentityChanged);
        }
        Arc::new(handle)
    };

    #[cfg(unix)]
    let working_dir_handle = {
        let handle = File::open(&canonical_working_dir)
            .map_err(|_| TrustedWorkingDirectoryError::WorkingDirectoryUnavailable)?;
        let metadata = handle
            .metadata()
            .map_err(|_| TrustedWorkingDirectoryError::WorkingDirectoryUnavailable)?;
        if !metadata.is_dir() || FileIdentity::from_metadata(&metadata) != working_dir_identity {
            return Err(TrustedWorkingDirectoryError::IdentityChanged);
        }
        Arc::new(handle)
    };

    Ok(TrustedWorkingDirectory {
        configured_project_root: project_root.to_path_buf(),
        canonical_project_root,
        canonical_working_dir,
        project_identity,
        working_dir_identity,
        #[cfg(unix)]
        project_handle,
        #[cfg(unix)]
        working_dir_handle,
    })
}

fn validate_relative_working_dir(path: &Path) -> Result<(), TrustedWorkingDirectoryError> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(TrustedWorkingDirectoryError::InvalidRelativeWorkingDirectory);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ores-compose-working-dir-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("root");
        root
    }

    #[test]
    fn regular_nested_directory_is_admitted_and_revalidates() {
        let root = temp_root("regular");
        fs::create_dir_all(root.join("services/api")).expect("service dir");
        let trusted =
            resolve_trusted_working_directory(&root, Path::new("services/api")).expect("trusted");
        assert!(trusted.working_dir().ends_with("services/api"));
        trusted.validate_current().expect("stable");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn directory_content_changes_do_not_change_admitted_identity() {
        let root = temp_root("content-change");
        let dir = root.join("api");
        fs::create_dir(&dir).expect("api");
        let trusted = resolve_trusted_working_directory(&root, Path::new("api")).expect("trusted");
        fs::write(dir.join("generated.txt"), "runtime content").expect("content");
        trusted
            .validate_current()
            .expect("same directory object remains valid");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn non_directory_final_component_is_rejected() {
        let root = temp_root("file");
        fs::create_dir_all(root.join("services")).expect("services");
        fs::write(root.join("services/api"), "not a directory").expect("file");
        assert_eq!(
            resolve_trusted_working_directory(&root, Path::new("services/api")),
            Err(TrustedWorkingDirectoryError::WorkingDirectoryNotDirectory)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn final_component_symlink_is_rejected_even_when_in_tree() {
        use std::os::unix::fs::symlink;
        let root = temp_root("final-symlink");
        fs::create_dir_all(root.join("real-api")).expect("real");
        symlink(root.join("real-api"), root.join("api")).expect("symlink");
        assert_eq!(
            resolve_trusted_working_directory(&root, Path::new("api")),
            Err(TrustedWorkingDirectoryError::WorkingDirectoryContainsSymlink)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn intermediate_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let root = temp_root("intermediate-symlink");
        fs::create_dir_all(root.join("real/api")).expect("real");
        symlink(root.join("real"), root.join("services")).expect("symlink");
        assert_eq!(
            resolve_trusted_working_directory(&root, Path::new("services/api")),
            Err(TrustedWorkingDirectoryError::WorkingDirectoryContainsSymlink)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn replacement_after_admission_is_detected() {
        let root = temp_root("replacement");
        let dir = root.join("api");
        fs::create_dir(&dir).expect("api");
        let trusted = resolve_trusted_working_directory(&root, Path::new("api")).expect("trusted");
        fs::remove_dir(&dir).expect("remove");
        fs::create_dir(&dir).expect("replacement");
        assert_eq!(
            trusted.validate_current(),
            Err(TrustedWorkingDirectoryError::IdentityChanged)
        );
        let _ = fs::remove_dir_all(root);
    }
}
