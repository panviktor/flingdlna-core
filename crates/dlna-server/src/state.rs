//! Server state management

use dlna_core::{MediaFile, MediaType, ServerInfo};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Internal server state
pub struct ServerState {
    server_info: Arc<RwLock<ServerInfo>>,
    /// Files indexed by ID
    files: RwLock<HashMap<String, MediaFile>>,
    /// Content update counter (for ContentDirectory)
    content_update_id: std::sync::atomic::AtomicU32,
    /// Public folders (visible to all devices)
    public_folders: RwLock<HashSet<PathBuf>>,
    /// Per-device folder assignments (device IP -> allowed folders)
    device_folders: RwLock<HashMap<String, HashSet<PathBuf>>>,
}

impl ServerState {
    pub fn new(server_info: ServerInfo) -> Self {
        Self {
            server_info: Arc::new(RwLock::new(server_info)),
            files: RwLock::new(HashMap::new()),
            content_update_id: std::sync::atomic::AtomicU32::new(0),
            public_folders: RwLock::new(HashSet::new()),
            device_folders: RwLock::new(HashMap::new()),
        }
    }

    pub fn server_info(&self) -> ServerInfo {
        self.server_info.read().unwrap().clone()
    }

    /// Update the server's friendly name
    pub fn set_server_name(&self, new_name: String) {
        if let Ok(mut info) = self.server_info.write() {
            info.friendly_name = new_name;
        }
    }

    pub fn add_files(&self, files: Vec<MediaFile>) {
        let mut map = self.files.write().unwrap_or_else(|e| e.into_inner());
        for file in files {
            map.insert(file.id.clone(), file);
        }
        self.content_update_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn remove_files_from_directory(&self, path: &Path) {
        let mut map = self.files.write().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, f| !f.path.starts_with(path));
        self.content_update_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn files(&self) -> Vec<MediaFile> {
        let map = self.files.read().unwrap_or_else(|e| e.into_inner());
        map.values().cloned().collect()
    }

    /// Clear all files
    pub fn clear_files(&self) {
        let mut map = self.files.write().unwrap_or_else(|e| e.into_inner());
        map.clear();
        self.content_update_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get_file(&self, id: &str) -> Option<MediaFile> {
        let map = self.files.read().unwrap_or_else(|e| e.into_inner());
        map.get(id).cloned()
    }

    pub fn files_by_type(&self, media_type: MediaType) -> Vec<MediaFile> {
        let map = self.files.read().unwrap_or_else(|e| e.into_inner());
        map.values()
            .filter(|f| f.media_type == media_type)
            .cloned()
            .collect()
    }

    pub fn content_update_id(&self) -> u32 {
        self.content_update_id
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn file_count(&self) -> usize {
        self.files.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Add a single file
    pub fn add_file(&self, file: MediaFile) {
        let mut map = self.files.write().unwrap_or_else(|e| e.into_inner());
        map.insert(file.id.clone(), file);
        self.content_update_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Remove a single file by ID
    pub fn remove_file(&self, id: &str) -> Option<MediaFile> {
        let mut map = self.files.write().unwrap_or_else(|e| e.into_inner());
        let removed = map.remove(id);
        if removed.is_some() {
            self.content_update_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        removed
    }

    /// Remove a file by path
    pub fn remove_file_by_path(&self, path: &Path) -> Option<MediaFile> {
        let mut map = self.files.write().unwrap_or_else(|e| e.into_inner());
        let id = map
            .iter()
            .find(|(_, f)| f.path == path)
            .map(|(id, _)| id.clone());
        if let Some(id) = id {
            let removed = map.remove(&id);
            self.content_update_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            removed
        } else {
            None
        }
    }

    /// Get file ID by path
    pub fn get_id_by_path(&self, path: &Path) -> Option<String> {
        let map = self.files.read().unwrap_or_else(|e| e.into_inner());
        map.iter()
            .find(|(_, f)| f.path == path)
            .map(|(id, _)| id.clone())
    }

    // === Folder Visibility ===

    /// Set public folders (visible to all devices)
    pub fn set_public_folders(&self, folders: Vec<PathBuf>) {
        let mut set = self
            .public_folders
            .write()
            .unwrap_or_else(|e| e.into_inner());
        set.clear();
        set.extend(folders);
    }

    /// Set folders for a specific device (by IP address)
    pub fn set_device_folders(&self, device_ip: &str, folders: Vec<PathBuf>) {
        let mut map = self
            .device_folders
            .write()
            .unwrap_or_else(|e| e.into_inner());
        if folders.is_empty() {
            map.remove(device_ip);
        } else {
            map.insert(device_ip.to_string(), folders.into_iter().collect());
        }
    }

    /// Clear all device folder assignments
    pub fn clear_device_folders(&self) {
        let mut map = self
            .device_folders
            .write()
            .unwrap_or_else(|e| e.into_inner());
        map.clear();
    }

    /// Check if a file is visible to a device
    fn is_file_visible(&self, file: &MediaFile, client_ip: Option<&str>) -> bool {
        let public = self
            .public_folders
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let device = self
            .device_folders
            .read()
            .unwrap_or_else(|e| e.into_inner());

        // If no folders configured at all, everything is visible (no filtering)
        if public.is_empty() && device.is_empty() {
            return true;
        }

        // Check if file is in a public folder
        for folder in public.iter() {
            if file.path.starts_with(folder) {
                return true;
            }
        }

        // Check device-specific folders
        if let Some(ip) = client_ip {
            if let Some(folders) = device.get(ip) {
                for folder in folders.iter() {
                    if file.path.starts_with(folder) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Get files by type, filtered by client visibility
    pub fn files_by_type_for_client(
        &self,
        media_type: MediaType,
        client_ip: Option<&str>,
    ) -> Vec<MediaFile> {
        let map = self.files.read().unwrap_or_else(|e| e.into_inner());
        map.values()
            .filter(|f| f.media_type == media_type && self.is_file_visible(f, client_ip))
            .cloned()
            .collect()
    }

    /// Get a file by ID, checking visibility for client
    pub fn get_file_for_client(&self, id: &str, client_ip: Option<&str>) -> Option<MediaFile> {
        let map = self.files.read().unwrap_or_else(|e| e.into_inner());
        map.get(id)
            .filter(|f| self.is_file_visible(f, client_ip))
            .cloned()
    }
}
