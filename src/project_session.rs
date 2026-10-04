//! Editable sessions for directory projects and `.rowlyx` packages.

use std::{
    io,
    path::{Path, PathBuf},
};

use tempfile::TempDir;
use thiserror::Error;

use crate::{
    process::CsvDocument,
    project::{ProjectError, RowlyProject, SourceKind, resolve_project_reference},
    project_init::{self, ProjectInitError},
    rowlyx::{RowlyxArchive, RowlyxError, pack_project},
};

#[derive(Debug)]
enum ProjectBacking {
    Directory {
        manifest_path: PathBuf,
    },
    Package {
        archive_path: PathBuf,
        manifest_path: PathBuf,
        workspace: TempDir,
    },
}

/// The backing format used by a [`ProjectSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectBackingKind {
    Directory,
    Package,
}

/// Independent unsaved project state categories.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ProjectDirtyState {
    data: bool,
    structure_or_config: bool,
    package: bool,
}

impl ProjectDirtyState {
    pub fn data_is_dirty(self) -> bool {
        self.data
    }

    pub fn structure_or_config_is_dirty(self) -> bool {
        self.structure_or_config
    }

    pub fn package_is_dirty(self) -> bool {
        self.package
    }

    pub fn is_dirty(self) -> bool {
        self.data || self.structure_or_config || self.package
    }
}

/// An editable project model with a retained directory or package backing.
///
/// `.rowlyx` archives are extracted to a managed temporary workspace for the
/// lifetime of the session. A successful save packs that workspace to a
/// temporary archive, validates it, and atomically replaces the original.
#[derive(Debug)]
pub struct ProjectSession {
    project: RowlyProject,
    backing: ProjectBacking,
    dirty: ProjectDirtyState,
}

impl ProjectSession {
    /// Open a `.rwprj` directory project or an editable `.rowlyx` package.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProjectSessionError> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().and_then(|value| value.to_str());
        if extension.is_some_and(|value| value.eq_ignore_ascii_case("rowlyx")) {
            let archive = RowlyxArchive::open(&path)?;
            let workspace = tempfile::Builder::new()
                .prefix("rowly-session-")
                .tempdir()
                .map_err(ProjectSessionError::TemporaryWorkspace)?;
            let manifest_path = archive.extract_to(workspace.path())?;
            let project = RowlyProject::load(&manifest_path)?;
            Ok(Self {
                project,
                backing: ProjectBacking::Package {
                    archive_path: path,
                    manifest_path,
                    workspace,
                },
                dirty: ProjectDirtyState::default(),
            })
        } else if extension.is_some_and(|value| value.eq_ignore_ascii_case("rwprj")) {
            let project = RowlyProject::load(&path)?;
            Ok(Self {
                project,
                backing: ProjectBacking::Directory {
                    manifest_path: path,
                },
                dirty: ProjectDirtyState::default(),
            })
        } else {
            Err(ProjectSessionError::UnsupportedBacking(
                path.display().to_string(),
            ))
        }
    }

    pub fn project(&self) -> &RowlyProject {
        &self.project
    }

    /// Save a source's column type declarations to the app-owned generated
    /// init script. The project must be saved afterward to repack `.rowlyx`.
    pub fn save_generated_column_types(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        if !self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(ProjectSessionError::SourceNotFound(source_id.to_owned()));
        }
        project_init::save_generated_column_types(
            &self.project,
            self.manifest_path(),
            source_id,
            document,
        )?;
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// Save one source's validation rules to the app-owned generated init DSL.
    pub fn save_generated_validation_rules(
        &mut self,
        source_id: &str,
        document: &CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        if !self
            .project
            .sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(ProjectSessionError::SourceNotFound(source_id.to_owned()));
        }
        project_init::save_generated_validation_rules(
            &self.project,
            self.manifest_path(),
            source_id,
            document,
        )?;
        self.mark_structure_or_config_dirty();
        Ok(())
    }

    /// Open one declared file source and apply its config-only project init.
    pub fn open_source_document(
        &self,
        source_id: &str,
    ) -> Result<CsvDocument, ProjectSessionError> {
        let source = self
            .project
            .sources
            .iter()
            .find(|source| source.id == source_id)
            .ok_or_else(|| ProjectSessionError::SourceNotFound(source_id.to_owned()))?;
        if source.kind != SourceKind::File {
            return Err(ProjectSessionError::DirectorySource(source_id.to_owned()));
        }
        let path = resolve_project_reference(self.manifest_path(), &source.path);
        let mut document = CsvDocument::open_without_metadata(path)?;
        self.apply_safe_init(source_id, &mut document)?;
        Ok(document)
    }

    /// Rebuild supported project configuration using the config-only init
    /// interpreter. It never runs user macros or general Rowly DSL statements.
    pub fn apply_safe_init(
        &self,
        source_id: &str,
        document: &mut CsvDocument,
    ) -> Result<(), ProjectSessionError> {
        project_init::apply_safe_init(&self.project, self.manifest_path(), source_id, document)?;
        Ok(())
    }

    /// Mutably access the manifest model and mark the session dirty.
    pub fn project_mut(&mut self) -> &mut RowlyProject {
        self.mark_structure_or_config_dirty();
        &mut self.project
    }

    /// Mark data changed through a document or data command owned by this session.
    pub fn mark_data_dirty(&mut self) {
        self.dirty.data = true;
        self.mark_package_dirty_if_needed();
    }

    /// Clear the in-memory data dirty flag after its owning document has saved.
    /// A package still needs a project save so the updated working file is
    /// included in the archive.
    pub fn mark_data_saved(&mut self) {
        self.dirty.data = false;
    }

    /// Mark project structure or persisted configuration changed.
    pub fn mark_structure_or_config_dirty(&mut self) {
        self.dirty.structure_or_config = true;
        self.mark_package_dirty_if_needed();
    }

    fn mark_package_dirty_if_needed(&mut self) {
        if matches!(self.backing, ProjectBacking::Package { .. }) {
            self.dirty.package = true;
        }
    }

    /// Root of the directory or extracted package working tree.
    pub fn working_directory(&self) -> &Path {
        match &self.backing {
            ProjectBacking::Directory { manifest_path } => {
                manifest_path.parent().unwrap_or_else(|| Path::new("."))
            }
            ProjectBacking::Package { workspace, .. } => workspace.path(),
        }
    }

    /// Manifest path used by this session. For package sessions this points
    /// into the managed working tree, not into the archive.
    pub fn manifest_path(&self) -> &Path {
        match &self.backing {
            ProjectBacking::Directory { manifest_path }
            | ProjectBacking::Package { manifest_path, .. } => manifest_path,
        }
    }

    /// Original project path associated with this session.
    pub fn backing_path(&self) -> &Path {
        match &self.backing {
            ProjectBacking::Directory { manifest_path } => manifest_path,
            ProjectBacking::Package { archive_path, .. } => archive_path,
        }
    }

    pub fn backing_kind(&self) -> ProjectBackingKind {
        match self.backing {
            ProjectBacking::Directory { .. } => ProjectBackingKind::Directory,
            ProjectBacking::Package { .. } => ProjectBackingKind::Package,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.is_dirty()
    }

    pub fn dirty_state(&self) -> ProjectDirtyState {
        self.dirty
    }

    /// Persist the current manifest and, for packages, atomically repack the
    /// original `.rowlyx` target. Failed package saves leave the original
    /// archive untouched and keep this session dirty for retry.
    pub fn save(&mut self) -> Result<(), ProjectSessionError> {
        self.project.save(self.manifest_path())?;
        if let ProjectBacking::Package {
            archive_path,
            manifest_path,
            ..
        } = &self.backing
        {
            pack_project(manifest_path, archive_path)?;
        }
        let data_dirty = self.dirty.data;
        self.dirty = ProjectDirtyState {
            data: data_dirty,
            structure_or_config: false,
            package: data_dirty && matches!(self.backing, ProjectBacking::Package { .. }),
        };
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ProjectSessionError {
    #[error("unsupported project backing `{0}`; expected `.rwprj` or `.rowlyx`")]
    UnsupportedBacking(String),
    #[error("failed to create a managed project workspace: {0}")]
    TemporaryWorkspace(io::Error),
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error(transparent)]
    ProjectInit(#[from] ProjectInitError),
    #[error(transparent)]
    Rowlyx(#[from] RowlyxError),
    #[error(transparent)]
    Document(#[from] crate::process::DocumentError),
    #[error("project source `{0}` does not exist")]
    SourceNotFound(String),
    #[error("project source `{0}` is a directory; open a file source instead")]
    DirectorySource(String),
}
