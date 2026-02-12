//! Layout manifest file handling
//!
//! This module provides functionality for reading and writing layout manifest files.
//! A layout manifest is a text file that defines the directory structure and file paths
//! for reproducible testing.

use crate::Result;
use anyhow::Context;
use chrono::{DateTime, Utc};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Layout manifest containing directory/file structure
#[derive(Debug, Clone)]
pub struct LayoutManifest {
    /// Manifest header with metadata
    pub header: ManifestHeader,
    /// List of file entries (path and size)
    pub file_entries: Vec<FileEntry>,
}

/// File entry with path and size
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// File path (relative to root)
    pub path: PathBuf,
    /// File size in bytes
    pub size: u64,
}

/// Manifest header with generation metadata
#[derive(Debug, Clone)]
pub struct ManifestHeader {
    /// When the manifest was generated
    pub generated_at: DateTime<Utc>,
    /// Directory tree depth
    pub depth: Option<usize>,
    /// Directory tree width
    pub width: Option<usize>,
    /// Total number of files
    pub total_files: usize,
    /// Total number of directories
    pub total_directories: Option<usize>,
    /// Files per directory (average)
    pub files_per_dir: Option<usize>,
    /// File size in bytes
    pub file_size: u64,
    /// Number of workers (for per-worker distribution)
    pub num_workers: Option<usize>,
}

impl LayoutManifest {
    /// Create a new layout manifest
    pub fn new(file_entries: Vec<FileEntry>, header: ManifestHeader) -> Self {
        Self {
            header,
            file_entries,
        }
    }
    
    /// Create from paths and uniform size
    pub fn from_paths_and_size(file_paths: Vec<PathBuf>, size: u64, header: ManifestHeader) -> Self {
        let file_entries = file_paths.into_iter()
            .map(|path| FileEntry { path, size })
            .collect();
        Self {
            header,
            file_entries,
        }
    }
    
    /// Parse layout manifest from file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read layout manifest: {}", path.display()))?;
        
        Self::from_string(&content)
    }
    
    /// Parse layout manifest from string
    pub fn from_string(content: &str) -> Result<Self> {
        let mut header = ManifestHeader {
            generated_at: Utc::now(),
            depth: None,
            width: None,
            total_files: 0,
            total_directories: None,
            files_per_dir: None,
            file_size: 0,
            num_workers: None,
        };
        
        let mut file_entries = Vec::new();
        
        for line in content.lines() {
            let line = line.trim();
            
            // Skip empty lines
            if line.is_empty() {
                continue;
            }
            
            // Parse header comments
            if line.starts_with('#') {
                // Try to parse metadata from comments
                if line.contains("Generated:") {
                    // Parse timestamp if needed
                } else if line.contains("depth=") {
                    if let Some(val) = extract_value(line, "depth=") {
                        header.depth = val.parse().ok();
                    }
                } else if line.contains("width=") {
                    if let Some(val) = extract_value(line, "width=") {
                        header.width = val.parse().ok();
                    }
                } else if line.contains("file_size=") {
                    if let Some(val) = extract_value(line, "file_size=") {
                        header.file_size = val.parse().unwrap_or(0);
                    }
                } else if line.contains("num_workers=") {
                    if let Some(val) = extract_value(line, "num_workers=") {
                        header.num_workers = val.parse().ok();
                    }
                } else if line.contains("total_files:") {
                    if let Some(val) = extract_value(line, "total_files:") {
                        header.total_files = val.parse().unwrap_or(0);
                    }
                } else if line.contains("Total files:") {
                    if let Some(val) = extract_value(line, "Total files:") {
                        header.total_files = val.parse().unwrap_or(0);
                    }
                } else if line.contains("File size:") {
                    if let Some(val) = extract_value(line, "File size:") {
                        // Parse "4096 bytes" or just "4096"
                        let size_str = val.split_whitespace().next().unwrap_or("0");
                        header.file_size = size_str.parse().unwrap_or(0);
                    }
                } else if line.contains("Total directories:") {
                    if let Some(val) = extract_value(line, "Total directories:") {
                        header.total_directories = val.parse().ok();
                    }
                } else if line.contains("Workers:") {
                    if let Some(val) = extract_value(line, "Workers:") {
                        header.num_workers = val.parse().ok();
                    }
                }
                continue;
            }
            
            // Parse file entry (path and size)
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            
            let path = PathBuf::from(parts[0]);
            let size = if parts.len() > 1 {
                parts[1].parse().unwrap_or(header.file_size)
            } else {
                header.file_size  // Fallback to header file_size if not specified per-line
            };
            
            file_entries.push(FileEntry { path, size });
        }
        
        // Update total_files from actual count if not set in header
        if header.total_files == 0 {
            header.total_files = file_entries.len();
        }
        
        Ok(Self {
            header,
            file_entries,
        })
    }
    
    /// Export layout manifest to file
    pub fn to_file(&self, path: &Path) -> Result<()> {
        let content = self.to_string();
        fs::write(path, content)
            .with_context(|| format!("Failed to write layout manifest: {}", path.display()))?;
        Ok(())
    }
    
    /// Convert layout manifest to string
    pub fn to_string(&self) -> String {
        let mut content = String::new();
        
        // Header
        content.push_str("# IOPulse Layout Manifest\n");
        content.push_str(&format!("# Generated: {}\n", self.header.generated_at.format("%Y-%m-%d %H:%M:%S UTC")));
        
        // Parameters
        if let (Some(depth), Some(width)) = (self.header.depth, self.header.width) {
            if let Some(num_workers) = self.header.num_workers {
                content.push_str(&format!("# Parameters: depth={}, width={}, total_files={}, file_size={}, num_workers={}\n", 
                    depth, width, self.header.total_files, self.header.file_size, num_workers));
            } else {
                content.push_str(&format!("# Parameters: depth={}, width={}, total_files={}, file_size={}\n", 
                    depth, width, self.header.total_files, self.header.file_size));
            }
        } else {
            if let Some(num_workers) = self.header.num_workers {
                content.push_str(&format!("# Parameters: total_files={}, file_size={}, num_workers={}\n", 
                    self.header.total_files, self.header.file_size, num_workers));
            } else {
                content.push_str(&format!("# Parameters: total_files={}, file_size={}\n", 
                    self.header.total_files, self.header.file_size));
            }
        }
        
        content.push_str(&format!("# Total files: {}\n", self.header.total_files));
        
        if let Some(dirs) = self.header.total_directories {
            content.push_str(&format!("# Total directories: {}\n", dirs));
        }
        
        if let Some(fpd) = self.header.files_per_dir {
            content.push_str(&format!("# Files per directory: {} (avg)\n", fpd));
        }
        
        content.push_str(&format!("# File size: {} bytes\n", self.header.file_size));
        
        if let Some(num_workers) = self.header.num_workers {
            content.push_str(&format!("# Workers: {} (per-worker distribution)\n", num_workers));
        }
        
        content.push_str("#\n");
        
        // File entries (path and size per line)
        for entry in &self.file_entries {
            content.push_str(&format!("{} {}\n", entry.path.display(), entry.size));
        }
        
        content
    }
    
    /// Calculate hash of manifest for marker validation
    pub fn hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        
        // Hash header metadata
        if let Some(depth) = self.header.depth {
            depth.hash(&mut hasher);
        }
        if let Some(width) = self.header.width {
            width.hash(&mut hasher);
        }
        self.header.total_files.hash(&mut hasher);
        self.header.file_size.hash(&mut hasher);
        if let Some(num_workers) = self.header.num_workers {
            num_workers.hash(&mut hasher);
        }
        
        // Hash file entries
        for entry in &self.file_entries {
            entry.path.hash(&mut hasher);
            entry.size.hash(&mut hasher);
        }
        
        hasher.finish()
    }
    
    /// Get total number of files
    pub fn file_count(&self) -> usize {
        self.file_entries.len()
    }
}

/// Extract value from comment line
fn extract_value(line: &str, prefix: &str) -> Option<String> {
    if let Some(pos) = line.find(prefix) {
        let after = &line[pos + prefix.len()..];
        // Take until comma or end of line
        let value = after.split(&[',', '\n'][..]).next()?;
        Some(value.trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_layout_manifest_parse() {
        let content = r#"# IOPulse Layout Manifest
# Generated: 2026-01-24 10:30:00 UTC
# Parameters: depth=3, width=10, total_files=1000, file_size=4096
# Total files: 1000
# File size: 4096 bytes
#
dir_0000/file_000000 4096
dir_0000/file_000001 4096
dir_0001/file_000000 4096
"#;
        
        let manifest = LayoutManifest::from_string(content).unwrap();
        assert_eq!(manifest.file_count(), 3);
        assert_eq!(manifest.header.total_files, 1000);
        assert_eq!(manifest.header.depth, Some(3));
        assert_eq!(manifest.header.width, Some(10));
        assert_eq!(manifest.header.file_size, 4096);
        assert_eq!(manifest.file_entries[0].size, 4096);
    }
    
    #[test]
    fn test_layout_manifest_export() {
        let header = ManifestHeader {
            generated_at: Utc::now(),
            depth: Some(2),
            width: Some(5),
            total_files: 100,
            total_directories: Some(25),
            files_per_dir: Some(4),
            file_size: 4096,
        };
        
        let file_entries = vec![
            FileEntry { path: PathBuf::from("dir_0000/file_000000"), size: 4096 },
            FileEntry { path: PathBuf::from("dir_0000/file_000001"), size: 4096 },
        ];
        
        let manifest = LayoutManifest::new(file_entries, header);
        let content = manifest.to_string();
        
        assert!(content.contains("# IOPulse Layout Manifest"));
        assert!(content.contains("depth=2"));
        assert!(content.contains("width=5"));
        assert!(content.contains("total_files=100"));
        assert!(content.contains("file_size=4096"));
        assert!(content.contains("File size: 4096 bytes"));
        assert!(content.contains("dir_0000/file_000000 4096"));
    }
    
    #[test]
    fn test_layout_manifest_hash() {
        let header = ManifestHeader {
            generated_at: Utc::now(),
            depth: Some(2),
            width: Some(5),
            total_files: 2,
            total_directories: None,
            files_per_dir: None,
            file_size: 4096,
        };
        
        let file_entries = vec![
            FileEntry { path: PathBuf::from("dir_0000/file_000000"), size: 4096 },
            FileEntry { path: PathBuf::from("dir_0000/file_000001"), size: 4096 },
        ];
        
        let manifest1 = LayoutManifest::new(file_entries.clone(), header.clone());
        let manifest2 = LayoutManifest::new(file_entries, header);
        
        // Same content should produce same hash
        assert_eq!(manifest1.hash(), manifest2.hash());
    }
}
