use std::path::PathBuf;
use flexi_logger::{Logger, Criterion, Naming, Cleanup, Duplicate, FileSpec};

/// Returns the appropriate log directory for the current OS, using the app name.
pub fn get_log_directory() -> PathBuf {
    let app_name = "preft";
    #[cfg(target_os = "windows")]
    {
        if let Some(dir) = dirs::data_dir() {
            return dir.join(app_name);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(dir) = dirs::data_dir() {
            return dir.join(app_name);
        }
        // Fallback to home directory if data_dir is not available
        if let Some(home) = dirs::home_dir() {
            return home.join(format!(".{}", app_name));
        }
    }
    // Fallback: current directory
    PathBuf::from(".")
}

/// Initializes the logger to write to a rotating file in the app data directory.
/// Keeps at most 5 log files, each up to 1 MB.
///
/// USAGE:
/// Call `logging::init_logging();` early in main() before any log macros are used.
pub fn init_logging() {
    let log_dir = get_log_directory();
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("Failed to create log directory {:?}: {}", log_dir, e);
        // Fallback: use current directory
    }
    
    Logger::try_with_str("info")
        .unwrap()
        .log_to_file(FileSpec::default()
            .directory(log_dir)
            .basename("preft")
            .suffix("log"))
        .rotate(
            Criterion::Size(1_000_000), // 1 MB per file
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .duplicate_to_stderr(Duplicate::Error)
        .duplicate_to_stderr(Duplicate::Warn)
        .duplicate_to_stdout(Duplicate::Info)
        .duplicate_to_stdout(Duplicate::Debug)
        .start()
        .unwrap_or_else(|e| {
            eprintln!("Logger initialization failed: {}", e);
            // Return a dummy logger handle or panic
            panic!("Logger initialization failed: {}", e);
        });
}
