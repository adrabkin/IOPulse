//! Directory tree generation and management
//!
//! This module provides functionality for generating and managing directory trees
//! for filesystem metadata testing. It supports configurable tree structures,
//! file distribution, and metadata operation tracking.

use crate::Result;
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Directory tree configuration
#[derive(Debug, Clone)]
pub struct TreeConfig {
    /// Tree depth (number of nested directory levels)
    pub depth: usize,
    
    /// Tree width (number of subdirectories per level)
    pub width: usize,
    
    /// Number of files per directory
    pub files_per_dir: usize,
    
    /// File size for generated files
    pub file_size: u64,
    
    /// File naming pattern
    pub naming_pattern: NamingPattern,
}

/// File naming pattern
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingPattern {
    /// Sequential numbering (file_0001, file_0002, ...)
    Sequential,
    
    /// Random names
    Random,
    
    /// Prefixed names (prefix_0001, prefix_0002, ...)
    Prefixed,
}

/// Metadata operation statistics
#[derive(Debug, Default, Clone)]
pub struct MetadataStats {
    /// Number of mkdir operations
    pub mkdir_count: u64,
    
    /// Total mkdir latency (nanoseconds)
    pub mkdir_latency_ns: u64,
    
    /// Number of file create operations
    pub create_count: u64,
    
    /// Total create latency (nanoseconds)
    pub create_latency_ns: u64,
    
    /// Number of stat operations
    pub stat_count: u64,
    
    /// Total stat latency (nanoseconds)
    pub stat_latency_ns: u64,
}

impl MetadataStats {
    /// Get average mkdir latency in nanoseconds
    pub fn avg_mkdir_latency_ns(&self) -> u64 {
        if self.mkdir_count > 0 {
            self.mkdir_latency_ns / self.mkdir_count
        } else {
            0
        }
    }
    
    /// Get average create latency in nanoseconds
    pub fn avg_create_latency_ns(&self) -> u64 {
        if self.create_count > 0 {
            self.create_latency_ns / self.create_count
        } else {
            0
        }
    }
    
    /// Get average stat latency in nanoseconds
    pub fn avg_stat_latency_ns(&self) -> u64 {
        if self.stat_count > 0 {
            self.stat_latency_ns / self.stat_count
        } else {
            0
        }
    }
}

/// Directory tree generator
pub struct TreeGenerator {
    /// Root directory path
    root: PathBuf,
    
    /// Tree configuration
    config: TreeConfig,
    
    /// Metadata operation statistics
    stats: MetadataStats,
    
    /// List of generated file paths
    file_paths: Vec<PathBuf>,
}

impl TreeGenerator {
    /// Create a new tree generator
    pub fn new(root: PathBuf, config: TreeConfig) -> Self {
        Self {
            root,
            config,
            stats: MetadataStats::default(),
            file_paths: Vec::new(),
        }
    }
    
    /// Generate the directory tree
    ///
    /// Creates all directories and files according to the configuration.
    /// Tracks metadata operation statistics during generation.
    pub fn generate(&mut self) -> Result<()> {
        // Create root directory if it doesn't exist
        if !self.root.exists() {
            let start = Instant::now();
            fs::create_dir_all(&self.root)
                .with_context(|| format!("Failed to create root directory: {}", self.root.display()))?;
            self.stats.mkdir_latency_ns += start.elapsed().as_nanos() as u64;
            self.stats.mkdir_count += 1;
        }
        
        // Generate tree recursively
        self.generate_level(&self.root.clone(), 0)?;
        
        Ok(())
    }
    
    /// Generate a single level of the tree
    fn generate_level(&mut self, parent: &Path, depth: usize) -> Result<()> {
        if depth >= self.config.depth {
            // At max depth, create files
            self.create_files(parent)?;
            return Ok(());
        }
        
        // Create subdirectories
        for i in 0..self.config.width {
            let dir_name = format!("dir_{:04}", i);
            let dir_path = parent.join(dir_name);
            
            let start = Instant::now();
            fs::create_dir(&dir_path)
                .with_context(|| format!("Failed to create directory: {}", dir_path.display()))?;
            self.stats.mkdir_latency_ns += start.elapsed().as_nanos() as u64;
            self.stats.mkdir_count += 1;
            
            // Recurse into subdirectory
            self.generate_level(&dir_path, depth + 1)?;
        }
        
        // Also create files at this level
        self.create_files(parent)?;
        
        Ok(())
    }
    
    /// Create files in a directory
    fn create_files(&mut self, dir: &Path) -> Result<()> {
        for i in 0..self.config.files_per_dir {
            let file_name = match self.config.naming_pattern {
                NamingPattern::Sequential => format!("file_{:06}", i),
                NamingPattern::Random => format!("file_{:016x}", rand::random::<u64>()),
                NamingPattern::Prefixed => format!("test_file_{:06}", i),
            };
            
            let file_path = dir.join(file_name);
            
            let start = Instant::now();
            let file = fs::File::create(&file_path)
                .with_context(|| format!("Failed to create file: {}", file_path.display()))?;
            
            // Set file size if specified
            if self.config.file_size > 0 {
                file.set_len(self.config.file_size)
                    .with_context(|| format!("Failed to set file size: {}", file_path.display()))?;
            }
            
            self.stats.create_latency_ns += start.elapsed().as_nanos() as u64;
            self.stats.create_count += 1;
            
            self.file_paths.push(file_path);
        }
        
        Ok(())
    }
    
    /// Get metadata operation statistics
    pub fn stats(&self) -> &MetadataStats {
        &self.stats
    }
    
    /// Get list of generated file paths
    pub fn file_paths(&self) -> &[PathBuf] {
        &self.file_paths
    }
    
    /// Get total number of files generated
    pub fn file_count(&self) -> usize {
        self.file_paths.len()
    }
    
    /// Export tree structure to a definition file
    ///
    /// Creates a text file describing the directory structure that can be
    /// used to recreate the tree later.
    pub fn export_to_file(&self, output_path: &Path) -> Result<()> {
        let mut content = String::new();
        content.push_str("# Directory Tree Definition\n");
        content.push_str(&format!("# Generated from: {}\n\n", self.root.display()));
        
        // Export directory structure
        for path in &self.file_paths {
            let relative = path.strip_prefix(&self.root)
                .unwrap_or(path);
            content.push_str(&format!("{}\n", relative.display()));
        }
        
        fs::write(output_path, content)
            .with_context(|| format!("Failed to write tree definition: {}", output_path.display()))?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_tree_generator_simple() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("tree");
        
        let config = TreeConfig {
            depth: 2,
            width: 2,
            files_per_dir: 3,
            file_size: 1024,
            naming_pattern: NamingPattern::Sequential,
        };
        
        let mut generator = TreeGenerator::new(root.clone(), config);
        assert!(generator.generate().is_ok());
        
        // Verify root exists
        assert!(root.exists());
        
        // Verify files were created
        assert!(generator.file_count() > 0);
        
        // Verify stats were tracked
        let stats = generator.stats();
        assert!(stats.mkdir_count > 0);
        assert!(stats.create_count > 0);
    }
    
    #[test]
    fn test_tree_generator_depth() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("tree_depth");
        
        let config = TreeConfig {
            depth: 3,
            width: 2,
            files_per_dir: 1,
            file_size: 0,
            naming_pattern: NamingPattern::Sequential,
        };
        
        let mut generator = TreeGenerator::new(root.clone(), config);
        generator.generate().unwrap();
        
        // With depth=3, width=2, files_per_dir=1:
        // Level 0: 1 file
        // Level 1: 2 dirs, 2 files
        // Level 2: 4 dirs, 4 files  
        // Level 3: 8 files (at max depth)
        // Total: 1 + 2 + 4 + 8 = 15 files
        assert_eq!(generator.file_count(), 15);
    }
    
    #[test]
    fn test_tree_generator_file_size() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("tree_size");
        
        let config = TreeConfig {
            depth: 1,
            width: 1,
            files_per_dir: 2,
            file_size: 4096,
            naming_pattern: NamingPattern::Sequential,
        };
        
        let mut generator = TreeGenerator::new(root.clone(), config);
        generator.generate().unwrap();
        
        // Verify file sizes
        for path in generator.file_paths() {
            let metadata = fs::metadata(path).unwrap();
            assert_eq!(metadata.len(), 4096);
        }
    }
    
    #[test]
    fn test_tree_generator_naming_patterns() {
        let temp_dir = TempDir::new().unwrap();
        
        // Test sequential
        let root_seq = temp_dir.path().join("tree_seq");
        let config_seq = TreeConfig {
            depth: 1,
            width: 1,
            files_per_dir: 3,
            file_size: 0,
            naming_pattern: NamingPattern::Sequential,
        };
        let mut gen_seq = TreeGenerator::new(root_seq, config_seq);
        gen_seq.generate().unwrap();
        
        let paths = gen_seq.file_paths();
        assert!(paths[0].to_string_lossy().contains("file_000000"));
        
        // Test prefixed
        let root_pre = temp_dir.path().join("tree_pre");
        let config_pre = TreeConfig {
            depth: 1,
            width: 1,
            files_per_dir: 2,
            file_size: 0,
            naming_pattern: NamingPattern::Prefixed,
        };
        let mut gen_pre = TreeGenerator::new(root_pre, config_pre);
        gen_pre.generate().unwrap();
        
        let paths = gen_pre.file_paths();
        assert!(paths[0].to_string_lossy().contains("test_file_"));
    }
    
    #[test]
    fn test_tree_generator_export() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("tree_export");
        
        let config = TreeConfig {
            depth: 2,
            width: 2,
            files_per_dir: 2,
            file_size: 0,
            naming_pattern: NamingPattern::Sequential,
        };
        
        let mut generator = TreeGenerator::new(root, config);
        generator.generate().unwrap();
        
        // Export tree definition
        let export_path = temp_dir.path().join("tree_def.txt");
        assert!(generator.export_to_file(&export_path).is_ok());
        
        // Verify export file exists and has content
        assert!(export_path.exists());
        let content = fs::read_to_string(&export_path).unwrap();
        assert!(content.contains("# Directory Tree Definition"));
        assert!(content.contains("file_"));
    }
    
    #[test]
    fn test_metadata_stats() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("tree_stats");
        
        let config = TreeConfig {
            depth: 2,
            width: 2,
            files_per_dir: 3,
            file_size: 0,
            naming_pattern: NamingPattern::Sequential,
        };
        
        let mut generator = TreeGenerator::new(root, config);
        generator.generate().unwrap();
        
        let stats = generator.stats();
        
        // Should have created directories
        assert!(stats.mkdir_count > 0);
        assert!(stats.mkdir_latency_ns > 0);
        
        // Should have created files
        assert!(stats.create_count > 0);
        assert!(stats.create_latency_ns > 0);
        
        // Average latencies should be reasonable
        assert!(stats.avg_mkdir_latency_ns() > 0);
        assert!(stats.avg_create_latency_ns() > 0);
    }
}
