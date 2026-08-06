//! Library/folder navigation methods for App

use dlna_combo::MediaFileInfo;
use std::collections::HashSet;
use std::path::PathBuf;

use super::App;
use crate::tui::types::LibraryEntry;

impl App {
    /// Get the currently selected library entry
    pub fn selected_library_entry(&self) -> Option<&LibraryEntry> {
        self.selected_library
            .selected()
            .and_then(|i| self.library_entries.get(i))
    }

    /// Rebuild library_entries based on current_folder and media_files
    pub fn build_library_entries(&mut self) {
        self.library_entries.clear();

        match &self.current_folder {
            None => {
                // Root view: show unique parent directories of all files
                let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
                let mut folders: Vec<(String, PathBuf, usize)> = Vec::new();

                for file in &self.media_files {
                    if let Some(_parent) = file.path.parent() {
                        // Find the root directory (first component after common prefix)
                        // We want to show immediate children of root
                        let root = self.find_root_folder(&file.path);
                        if let Some(root_path) = root {
                            if seen_dirs.insert(root_path.clone()) {
                                let name = root_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| root_path.to_string_lossy().to_string());
                                // Count files in this root
                                let count = self
                                    .media_files
                                    .iter()
                                    .filter(|f| f.path.starts_with(&root_path))
                                    .count();
                                folders.push((name, root_path, count));
                            }
                        } else {
                            // File is directly in a root - add to a "(root)" pseudo-folder
                            // or just show the parent
                            let parent_path = _parent.to_path_buf();
                            if seen_dirs.insert(parent_path.clone()) {
                                let name = parent_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| parent_path.to_string_lossy().to_string());
                                let count = self
                                    .media_files
                                    .iter()
                                    .filter(|f| f.path.parent() == Some(&parent_path))
                                    .count();
                                folders.push((name, parent_path, count));
                            }
                        }
                    }
                }

                // Sort folders alphabetically
                folders.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

                for (name, path, count) in folders {
                    self.library_entries.push(LibraryEntry::Folder {
                        name,
                        path,
                        file_count: count,
                    });
                }
            }
            Some(current) => {
                // Inside a folder: show subfolders + files in this folder
                let mut seen_subdirs: HashSet<PathBuf> = HashSet::new();
                let mut subdirs: Vec<(String, PathBuf, usize)> = Vec::new();
                let mut files: Vec<MediaFileInfo> = Vec::new();

                for file in &self.media_files {
                    if !file.path.starts_with(current) {
                        continue;
                    }

                    // Check if file is directly in current folder
                    if file.path.parent() == Some(current.as_path()) {
                        files.push(file.clone());
                    } else {
                        // File is in a subdirectory - find immediate child folder
                        let relative = file.path.strip_prefix(current).ok();
                        if let Some(rel) = relative {
                            if let Some(first_component) = rel.components().next() {
                                let subdir_path = current.join(first_component);
                                if seen_subdirs.insert(subdir_path.clone()) {
                                    let name =
                                        first_component.as_os_str().to_string_lossy().to_string();
                                    let count = self
                                        .media_files
                                        .iter()
                                        .filter(|f| f.path.starts_with(&subdir_path))
                                        .count();
                                    subdirs.push((name, subdir_path, count));
                                }
                            }
                        }
                    }
                }

                // Sort subdirs alphabetically
                subdirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

                // Add subfolders first
                for (name, path, count) in subdirs {
                    self.library_entries.push(LibraryEntry::Folder {
                        name,
                        path,
                        file_count: count,
                    });
                }

                // Sort files alphabetically
                files.sort_by(|a, b| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()));

                // Add files
                for file in files {
                    self.library_entries.push(LibraryEntry::File(file));
                }
            }
        }

        // Reset selection if out of bounds
        if let Some(selected) = self.selected_library.selected() {
            if selected >= self.library_entries.len() {
                self.selected_library
                    .select(if self.library_entries.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
            }
        } else if !self.library_entries.is_empty() {
            self.selected_library.select(Some(0));
        }
    }

    /// Find the root folder for a file path (the top-level directory that contains it)
    fn find_root_folder(&self, file_path: &std::path::Path) -> Option<PathBuf> {
        // Find the topmost unique parent by checking where files diverge
        // This is a simple heuristic: take the first directory component
        let mut components = file_path.components();
        let mut root = PathBuf::new();

        for component in &mut components {
            root.push(component);
            if root.is_dir() {
                // Check if there's more than one level below this
                if let Some(next) = components.next() {
                    let potential_root = root.join(next);
                    if potential_root.is_dir() {
                        return Some(potential_root);
                    }
                }
                // Return the deepest parent that's a directory containing the file
                if file_path.parent() == Some(&root) {
                    return None; // File is directly in this root
                }
                return Some(root);
            }
        }
        None
    }

    /// Navigate into the selected folder
    pub fn enter_selected_folder(&mut self) -> bool {
        if let Some(LibraryEntry::Folder { path, .. }) = self.selected_library_entry().cloned() {
            self.current_folder = Some(path);
            self.selected_library.select(Some(0));
            self.build_library_entries();
            return true;
        }
        false
    }

    /// Go back to parent folder (or root if already at top level)
    pub fn go_parent_folder(&mut self) -> bool {
        if let Some(current) = &self.current_folder.clone() {
            // Check if we can go up
            if let Some(parent) = current.parent() {
                // Check if parent is still within our media files tree
                let parent_has_files = self.media_files.iter().any(|f| f.path.starts_with(parent));

                if parent_has_files {
                    self.current_folder = Some(parent.to_path_buf());
                } else {
                    self.current_folder = None; // Go to root
                }
            } else {
                self.current_folder = None;
            }
            self.selected_library.select(Some(0));
            self.build_library_entries();
            return true;
        }
        false
    }

    /// Get breadcrumb path for display
    pub fn library_breadcrumb(&self) -> String {
        match &self.current_folder {
            None => "/".to_string(),
            Some(path) => {
                // Show just the last 2-3 components for brevity
                let components: Vec<_> = path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect();

                if components.len() <= 2 {
                    format!("/{}", components.join("/"))
                } else {
                    format!("/.../{}", components[components.len() - 2..].join("/"))
                }
            }
        }
    }

    /// Check if currently selected entry is a folder
    pub fn is_selected_folder(&self) -> bool {
        matches!(
            self.selected_library_entry(),
            Some(LibraryEntry::Folder { .. })
        )
    }

    /// Get selected file (if a file is selected, not a folder)
    pub fn selected_file(&self) -> Option<&MediaFileInfo> {
        match self.selected_library_entry() {
            Some(LibraryEntry::File(file)) => Some(file),
            _ => None,
        }
    }
}
