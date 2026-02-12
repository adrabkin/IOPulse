//! Directory layout generation and management
//!
//! This module provides functionality for generating and managing directory layouts
//! for filesystem metadata testing. It supports configurable directory structures,
//! file distribution, and metadata operation tracking.

use crate::Result;
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Directory layout configuration
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    /// Directory depth (number of nested directory levels)
    pub depth: usize,
    
    /// Directory width (number of subdirectories per level)
    pub width: usize,
    
    /// Number of files per directory (base count)
    pub files_per_dir: usize,
    
    /// File size for generated files
    pub file_size: u64,
    
    /// File naming pattern
    pub naming_pattern: NamingPattern,
    
    /// Number of workers (for per-worker distribution)
    /// When set, creates files with .workerN suffix
    pub num_workers: Option<usize>,
    
    /// Exact total number of files to generate (optional)
    /// When set, the generator will create exactly this many files
    /// by distributing remainder files across directories
    pub total_files: Option<usize>,
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

/// Directory layout generator
pub struct LayoutGenerator {
    /// Root directory path
    root: PathBuf,
    
    /// Layout configuration
    config: LayoutConfig,
    
    /// Metadata operation statistics
    stats: MetadataStats,
    
    /// List of generated file paths
    file_paths: Vec<PathBuf>,
}

impl LayoutGenerator {
    /// Create a new layout generator
    pub fn new(root: PathBuf, config: LayoutConfig) -> Self {
        Self {
            root,
            config,
            stats: MetadataStats::default(),
            file_paths: Vec::new(),
        }
    }
    
    /// Generate the directory layout
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
        
        // Generate layout recursively
        self.generate_level(&self.root.clone(), 0)?;
        
        // If total_files is specified, adjust to create exactly that many files
        if let Some(target_total) = self.config.total_files {
            let current_total = self.file_paths.len();
            
            if current_total < target_total {
                // Need to add more files to reach target
                let files_to_add = target_total - current_total;
                self.add_remainder_files(files_to_add)?;
            } else if current_total > target_total {
                // This shouldn't happen with correct calculation, but handle it
                eprintln!("Warning: Generated {} files but target was {}. Keeping all files.", 
                    current_total, target_total);
            }
        }
        
        Ok(())
    }
    
    /// Generate a single level of the directory structure
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
        
        // Only create files at intermediate levels if depth > 1
        // For depth=1 (flat structure), files should only be in subdirectories
        if depth > 0 && depth < self.config.depth {
            self.create_files(parent)?;
        }
        
        Ok(())
    }
    
    /// Create files in a directory
    fn create_files(&mut self, dir: &Path) -> Result<()> {
        let num_workers = self.config.num_workers.unwrap_or(1);
        
        for i in 0..self.config.files_per_dir {
            // Generate base file name
            let base_name = match self.config.naming_pattern {
                NamingPattern::Sequential => format!("file_{:06}", i),
                NamingPattern::Random => format!("file_{:016x}", rand::random::<u64>()),
                NamingPattern::Prefixed => format!("test_file_{:06}", i),
            };
            
            // Create files for each worker if per-worker mode
            for worker_id in 0..num_workers {
                let file_name = if num_workers > 1 {
                    // Per-worker mode: add .workerN suffix
                    format!("{}.worker{}", base_name, worker_id)
                } else {
                    // Normal mode: no suffix
                    base_name.clone()
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
        }
        
        Ok(())
    }
    
    /// Add remainder files to reach exact total_files count
    /// Distributes remainder files across existing directories
    fn add_remainder_files(&mut self, count: usize) -> Result<()> {
        // Collect all directories that have files
        let mut dirs_with_files = Vec::new();
        
        // Walk the tree to find all directories with files
        self.collect_dirs_with_files(&self.root.clone(), 0, &mut dirs_with_files)?;
        
        if dirs_with_files.is_empty() {
            anyhow::bail!("No directories found to add remainder files");
        }
        
        let num_workers = self.config.num_workers.unwrap_or(1);
        
        // Distribute remainder files across directories
        for i in 0..count {
            let dir_idx = i % dirs_with_files.len();
            let dir = &dirs_with_files[dir_idx];
            
            let file_idx = self.config.files_per_dir + (i / dirs_with_files.len());
            
            // Generate base file name
            let base_name = match self.config.naming_pattern {
                NamingPattern::Sequential => format!("file_{:06}", file_idx),
                NamingPattern::Random => format!("file_{:016x}", rand::random::<u64>()),
                NamingPattern::Prefixed => format!("test_file_{:06}", file_idx),
            };
            
            // Create files for each worker if per-worker mode
            for worker_id in 0..num_workers {
                let file_name = if num_workers > 1 {
                    format!("{}.worker{}", base_name, worker_id)
                } else {
                    base_name.clone()
                };
                
                let file_path = dir.join(file_name);
                
                let start = Instant::now();
                let file = fs::File::create(&file_path)
                    .with_context(|| format!("Failed to create file: {}", file_path.display()))?;
                
                if self.config.file_size > 0 {
                    file.set_len(self.config.file_size)
                        .with_context(|| format!("Failed to set file size: {}", file_path.display()))?;
                }
                
                self.stats.create_latency_ns += start.elapsed().as_nanos() as u64;
                self.stats.create_count += 1;
                
                self.file_paths.push(file_path);
            }
        }
        
        Ok(())
    }
    
    /// Collect all directories that have files
    fn collect_dirs_with_files(&self, dir: &Path, depth: usize, result: &mut Vec<PathBuf>) -> Result<()> {
        // Check if this directory should have files based on layout rules
        let should_have_files = if depth >= self.config.depth {
            // At max depth
            true
        } else if depth > 0 && depth < self.config.depth {
            // Intermediate level
            true
        } else {
            // Root level (depth == 0)
            false
        };
        
        if should_have_files {
            result.push(dir.to_path_buf());
        }
        
        // Recurse into subdirectories if not at max depth
        if depth < self.config.depth {
            for i in 0..self.config.width {
                let dir_name = format!("dir_{:04}", i);
                let dir_path = dir.join(dir_name);
                if dir_path.exists() {
                    self.collect_dirs_with_files(&dir_path, depth + 1, result)?;
                }
            }
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
    
    /// Export layout structure to a definition file
    ///
    /// Creates a text file describing the directory structure that can be
    /// used to recreate the layout later.
    pub fn export_to_file(&self, output_path: &Path) -> Result<()> {
        let mut content = String::new();
        content.push_str("# IOPulse Layout Definition\n");
        content.push_str(&format!("# Generated from: {}\n\n", self.root.display()));
        
        // Export directory structure
        for path in &self.file_paths {
            let relative = path.strip_prefix(&self.root)
                .unwrap_or(path);
            content.push_str(&format!("{}\n", relative.display()));
        }
        
        fs::write(output_path, content)
            .with_context(|| format!("Failed to write layout definition: {}", output_path.display()))?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_layout_generator_simple() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("layout");
        
        let config = LayoutConfig {
            depth: 2,
            width: 2,
            files_per_dir: 3,
            file_size: 1024,
            naming_pattern: NamingPattern::Sequential,
            num_workers: None,
        };
        
        let mut generator = LayoutGenerator::new(root.clone(), config);
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
    fn test_layout_generator_depth() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("layout_depth");
        
        let config = LayoutConfig {
            depth: 3,
            width: 2,
            files_per_dir: 1,
            file_size: 0,
            naming_pattern: NamingPattern::Sequential,
            num_workers: None,
        };
        
        let mut generator = LayoutGenerator::new(root.clone(), config);
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
    fn test_layout_generator_file_size() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("layout_size");
        
        let config = LayoutConfig {
            depth: 1,
            width: 1,
            files_per_dir: 2,
            file_size: 4096,
            naming_pattern: NamingPattern::Sequential,
            num_workers: None,
        };
        
        let mut generator = LayoutGenerator::new(root.clone(), config);
        generator.generate().unwrap();
        
        // Verify file sizes
        for path in generator.file_paths() {
            let metadata = fs::metadata(path).unwrap();
            assert_eq!(metadata.len(), 4096);
        }
    }
    
    #[test]
    fn test_layout_generator_naming_patterns() {
        let temp_dir = TempDir::new().unwrap();
        
        // Test sequential
        let root_seq = temp_dir.path().join("layout_seq");
        let config_seq = LayoutConfig {
            depth: 1,
            width: 1,
            files_per_dir: 3,
            file_size: 0,
            naming_pattern: NamingPattern::Sequential,
            num_workers: None,
        };
        let mut gen_seq = LayoutGenerator::new(root_seq, config_seq);
        gen_seq.generate().unwrap();
        
        let paths = gen_seq.file_paths();
        assert!(paths[0].to_string_lossy().contains("file_000000"));
        
        // Test prefixed
        let root_pre = temp_dir.path().join("layout_pre");
        let config_pre = LayoutConfig {
            depth: 1,
            width: 1,
            files_per_dir: 2,
            file_size: 0,
            naming_pattern: NamingPattern::Prefixed,
            num_workers: None,
        };
        let mut gen_pre = LayoutGenerator::new(root_pre, config_pre);
        gen_pre.generate().unwrap();
        
        let paths = gen_pre.file_paths();
        assert!(paths[0].to_string_lossy().contains("test_file_"));
    }
    
    #[test]
    fn test_layout_generator_export() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("layout_export");
        
        let config = LayoutConfig {
            depth: 2,
            width: 2,
            files_per_dir: 2,
            file_size: 0,
            naming_pattern: NamingPattern::Sequential,
            num_workers: None,
        };
        
        let mut generator = LayoutGenerator::new(root, config);
        generator.generate().unwrap();
        
        // Export layout definition
        let export_path = temp_dir.path().join("layout_def.txt");
        assert!(generator.export_to_file(&export_path).is_ok());
        
        // Verify export file exists and has content
        assert!(export_path.exists());
        let content = fs::read_to_string(&export_path).unwrap();
        assert!(content.contains("# IOPulse Layout Definition"));
        assert!(content.contains("file_"));
    }
    
    #[test]
    fn test_metadata_stats() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("layout_stats");
        
        let config = LayoutConfig {
            depth: 2,
            width: 2,
            files_per_dir: 3,
            file_size: 0,
            naming_pattern: NamingPattern::Sequential,
            num_workers: None,
        };
        
        let mut generator = LayoutGenerator::new(root, config);
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
    
    #[test]
    fn test_layout_generator_per_worker() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("layout_per_worker");
        
        let config = LayoutConfig {
            depth: 1,
            width: 2,
            files_per_dir: 3,
            file_size: 1024,
            naming_pattern: NamingPattern::Sequential,
            num_workers: Some(4),
        };
        
        let mut generator = LayoutGenerator::new(root.clone(), config);
        generator.generate().unwrap();
        
        // Should create 24 files (3 files × 2 dirs × 4 workers)
        assert_eq!(generator.file_count(), 24);
        
        // Verify worker suffixes exist
        let paths = generator.file_paths();
        assert!(paths.iter().any(|p| p.to_string_lossy().contains(".worker0")));
        assert!(paths.iter().any(|p| p.to_string_lossy().contains(".worker3")));
        
        // Verify all files have worker suffixes
        for path in paths {
            let path_str = path.to_string_lossy();
            let has_worker_suffix = (0..4).any(|i| path_str.contains(&format!(".worker{}", i)));
            assert!(has_worker_suffix, "File {} missing worker suffix", path_str);
        }
    }
}
