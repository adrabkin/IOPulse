//! Dataset layout markers for skipping recreation
//!
//! This module implements dataset markers that track when files have been created
//! and filled, allowing IOPulse to skip expensive validation on subsequent runs.
//!
//! # Marker File Format
//!
//! ```text
//! # IOPulse Dataset Marker
//! # Created: 2026-01-25 10:30:00 UTC
//! # Config Hash: a3f5b2c8d1e9f4a7
//! #
//! # Parameters:
//! #   file_count: 1000000
//! #   file_size: 4096
//! #   layout_manifest: tree_1M.layout_manifest (hash: b4e6c3d9)
//! #
//! # Dataset:
//! #   Total files: 1000000
//! #   Total size: 3.8 GB
//! #   Files filled: true
//! ```

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Dataset marker file name
pub const MARKER_FILENAME: &str = ".iopulse-layout";

/// Dataset marker
///
/// Tracks the configuration and state of a dataset to enable fast validation
/// on subsequent test runs.
#[derive(Debug, Clone)]
pub struct DatasetMarker {
    /// When the marker was created
    pub created_at: DateTime<Utc>,
    
    /// Configuration hash (uniquely identifies the dataset layout)
    pub config_hash: u64,
    
    /// Number of files in the dataset
    pub file_count: usize,
    
    /// Size of each file (0 if variable sizes)
    pub file_size: u64,
    
    /// Total dataset size in bytes
    pub total_size: u64,
    
    /// Whether files have been filled with data
    pub files_filled: bool,
    
    /// Optional layout manifest path
    pub layout_manifest_path: Option<PathBuf>,
    
    /// Optional layout manifest hash
    pub layout_manifest_hash: Option<u64>,
    
    /// Optional layout parameters
    pub depth: Option<usize>,
    pub width: Option<usize>,
}

impl DatasetMarker {
    /// Create a new dataset marker
    pub fn new(
        file_count: usize,
        file_size: u64,
        total_size: u64,
        files_filled: bool,
    ) -> Self {
        let config_hash = Self::compute_config_hash(
            file_count,
            file_size,
            None,
            None,
            None,
            None,
        );
        
        Self {
            created_at: Utc::now(),
            config_hash,
            file_count,
            file_size,
            total_size,
            files_filled,
            layout_manifest_path: None,
            layout_manifest_hash: None,
            depth: None,
            width: None,
        }
    }
    
    /// Create a marker with layout manifest information
    pub fn with_manifest(
        file_count: usize,
        file_size: u64,
        total_size: u64,
        files_filled: bool,
        manifest_path: PathBuf,
        manifest_hash: u64,
    ) -> Self {
        let config_hash = Self::compute_config_hash(
            file_count,
            file_size,
            Some(&manifest_path),
            Some(manifest_hash),
            None,
            None,
        );
        
        Self {
            created_at: Utc::now(),
            config_hash,
            file_count,
            file_size,
            total_size,
            files_filled,
            layout_manifest_path: Some(manifest_path),
            layout_manifest_hash: Some(manifest_hash),
            depth: None,
            width: None,
        }
    }
    
    /// Create a marker with layout parameters
    pub fn with_layout_params(
        file_count: usize,
        file_size: u64,
        total_size: u64,
        files_filled: bool,
        depth: usize,
        width: usize,
    ) -> Self {
        let config_hash = Self::compute_config_hash(
            file_count,
            file_size,
            None,
            None,
            Some(depth),
            Some(width),
        );
        
        Self {
            created_at: Utc::now(),
            config_hash,
            file_count,
            file_size,
            total_size,
            files_filled,
            layout_manifest_path: None,
            layout_manifest_hash: None,
            depth: Some(depth),
            width: Some(width),
        }
    }
    
    /// Compute configuration hash
    ///
    /// The hash uniquely identifies a dataset configuration based on:
    /// - File count
    /// - File size
    /// - Layout manifest path and hash (if used)
    /// - Layout parameters (if used)
    fn compute_config_hash(
        file_count: usize,
        file_size: u64,
        manifest_path: Option<&Path>,
        manifest_hash: Option<u64>,
        depth: Option<usize>,
        width: Option<usize>,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();
        
        file_count.hash(&mut hasher);
        file_size.hash(&mut hasher);
        
        if let Some(path) = manifest_path {
            path.to_string_lossy().hash(&mut hasher);
        }
        
        if let Some(hash) = manifest_hash {
            hash.hash(&mut hasher);
        }
        
        if let Some(d) = depth {
            d.hash(&mut hasher);
        }
        
        if let Some(w) = width {
            w.hash(&mut hasher);
        }
        
        hasher.finish()
    }
    
    /// Write marker to file
    pub fn write_to_file(&self, target_dir: &Path) -> Result<()> {
        use std::io::Write;
        
        let marker_path = target_dir.join(MARKER_FILENAME);
        let mut file = std::fs::File::create(&marker_path)
            .context("Failed to create marker file")?;
        
        writeln!(file, "# IOPulse Dataset Marker")?;
        writeln!(file, "# Created: {}", self.created_at.format("%Y-%m-%d %H:%M:%S UTC"))?;
        writeln!(file, "# Config Hash: {:016x}", self.config_hash)?;
        writeln!(file, "#")?;
        writeln!(file, "# Parameters:")?;
        writeln!(file, "#   file_count: {}", self.file_count)?;
        writeln!(file, "#   file_size: {}", self.file_size)?;
        
        if let Some(ref path) = self.layout_manifest_path {
            writeln!(file, "#   layout_manifest: {} (hash: {:016x})", 
                path.display(), 
                self.layout_manifest_hash.unwrap_or(0))?;
        }
        
        if let (Some(d), Some(w)) = (self.depth, self.width) {
            writeln!(file, "#   depth: {}", d)?;
            writeln!(file, "#   width: {}", w)?;
        }
        
        writeln!(file, "#")?;
        writeln!(file, "# Dataset:")?;
        writeln!(file, "#   Total files: {}", self.file_count)?;
        writeln!(file, "#   Total size: {}", format_bytes(self.total_size))?;
        writeln!(file, "#   Files filled: {}", self.files_filled)?;
        
        Ok(())
    }
    
    /// Read marker from file
    pub fn read_from_file(target_dir: &Path) -> Result<Option<Self>> {
        let marker_path = target_dir.join(MARKER_FILENAME);
        
        if !marker_path.exists() {
            return Ok(None);
        }
        
        let content = std::fs::read_to_string(&marker_path)
            .context("Failed to read marker file")?;
        
        Self::parse(&content).map(Some)
    }
    
    /// Parse marker from string content
    fn parse(content: &str) -> Result<Self> {
        let mut created_at = None;
        let mut config_hash = None;
        let mut file_count = None;
        let mut file_size = None;
        let mut total_size = None;
        let mut files_filled = None;
        let mut layout_manifest_path = None;
        let mut layout_manifest_hash = None;
        let mut depth = None;
        let mut width = None;
        
        for line in content.lines() {
            let line = line.trim();
            
            if line.starts_with("# Created:") {
                if let Some(date_str) = line.strip_prefix("# Created:").map(|s| s.trim()) {
                    created_at = DateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S %Z")
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc));
                }
            } else if line.starts_with("# Config Hash:") {
                if let Some(hash_str) = line.strip_prefix("# Config Hash:").map(|s| s.trim()) {
                    config_hash = u64::from_str_radix(hash_str, 16).ok();
                }
            } else if line.contains("file_count:") {
                if let Some(val) = extract_value(line, "file_count:") {
                    file_count = val.parse().ok();
                }
            } else if line.contains("file_size:") && !line.contains("layout_manifest") {
                if let Some(val) = extract_value(line, "file_size:") {
                    file_size = val.parse().ok();
                }
            } else if line.contains("Total size:") {
                if let Some(val) = extract_value(line, "Total size:") {
                    total_size = parse_size_string(&val);
                }
            } else if line.contains("Files filled:") {
                if let Some(val) = extract_value(line, "Files filled:") {
                    files_filled = val.parse().ok();
                }
            } else if line.contains("layout_manifest:") {
                if let Some(val) = extract_value(line, "layout_manifest:") {
                    // Format: "path (hash: 0x...)"
                    if let Some(path_part) = val.split(" (hash:").next() {
                        layout_manifest_path = Some(PathBuf::from(path_part.trim()));
                    }
                    if let Some(hash_part) = val.split("hash: ").nth(1) {
                        if let Some(hash_str) = hash_part.trim_end_matches(')').strip_prefix("0x") {
                            layout_manifest_hash = u64::from_str_radix(hash_str, 16).ok();
                        } else {
                            layout_manifest_hash = u64::from_str_radix(hash_part.trim_end_matches(')'), 16).ok();
                        }
                    }
                }
            } else if line.contains("depth:") {
                if let Some(val) = extract_value(line, "depth:") {
                    depth = val.parse().ok();
                }
            } else if line.contains("width:") {
                if let Some(val) = extract_value(line, "width:") {
                    width = val.parse().ok();
                }
            }
        }
        
        Ok(Self {
            created_at: created_at.unwrap_or_else(Utc::now),
            config_hash: config_hash.ok_or_else(|| anyhow::anyhow!("Missing config hash"))?,
            file_count: file_count.ok_or_else(|| anyhow::anyhow!("Missing file count"))?,
            file_size: file_size.ok_or_else(|| anyhow::anyhow!("Missing file size"))?,
            total_size: total_size.unwrap_or(0),
            files_filled: files_filled.unwrap_or(false),
            layout_manifest_path,
            layout_manifest_hash,
            depth,
            width,
        })
    }
    
    /// Check if this marker matches the given configuration
    pub fn matches_config(
        &self,
        file_count: usize,
        file_size: u64,
        manifest_path: Option<&Path>,
        manifest_hash: Option<u64>,
        depth: Option<usize>,
        width: Option<usize>,
    ) -> bool {
        let expected_hash = Self::compute_config_hash(
            file_count,
            file_size,
            manifest_path,
            manifest_hash,
            depth,
            width,
        );
        
        self.config_hash == expected_hash
    }
}

/// Extract value from a line like "#   key: value"
fn extract_value(line: &str, key: &str) -> Option<String> {
    line.split(key)
        .nth(1)
        .map(|s| s.trim().to_string())
}

/// Parse size string like "3.8 GB" to bytes
fn parse_size_string(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }
    
    let num: f64 = parts[0].parse().ok()?;
    let multiplier = match parts[1].to_uppercase().as_str() {
        "B" => 1_u64,
        "KB" => 1024_u64,
        "MB" => 1024_u64 * 1024,
        "GB" => 1024_u64 * 1024 * 1024,
        "TB" => 1024_u64 * 1024 * 1024 * 1024,
        _ => return None,
    };
    
    Some((num * multiplier as f64) as u64)
}

/// Format bytes as human-readable string
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_marker_creation() {
        let marker = DatasetMarker::new(1000, 4096, 4096000, true);
        assert_eq!(marker.file_count, 1000);
        assert_eq!(marker.file_size, 4096);
        assert_eq!(marker.total_size, 4096000);
        assert!(marker.files_filled);
    }
    
    #[test]
    fn test_marker_write_read() {
        let temp_dir = TempDir::new().unwrap();
        let marker = DatasetMarker::new(1000, 4096, 4096000, true);
        
        marker.write_to_file(temp_dir.path()).unwrap();
        
        let read_marker = DatasetMarker::read_from_file(temp_dir.path())
            .unwrap()
            .expect("Marker should exist");
        
        assert_eq!(read_marker.file_count, marker.file_count);
        assert_eq!(read_marker.file_size, marker.file_size);
        assert_eq!(read_marker.config_hash, marker.config_hash);
    }
    
    #[test]
    fn test_marker_matching() {
        let marker = DatasetMarker::new(1000, 4096, 4096000, true);
        
        // Should match same config
        assert!(marker.matches_config(1000, 4096, None, None, None, None));
        
        // Should not match different config
        assert!(!marker.matches_config(2000, 4096, None, None, None, None));
        assert!(!marker.matches_config(1000, 8192, None, None, None, None));
    }
    
    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }
}
