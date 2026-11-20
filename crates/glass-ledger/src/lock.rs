use std::fs::{File, TryLockError};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// Acquire an advisory ledger lock without indefinitely blocking a worker.
pub fn lock_ledger_file(file: &File, path: &Path) -> io::Result<()> {
    let timeout = std::env::var("OMNIS_GLASS_LEDGER_LOCK_TIMEOUT_MS").ok()
        .and_then(|value| value.parse::<u64>().ok()).unwrap_or(2_000);
    let limit = Duration::from_millis(timeout);
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) => {
                let elapsed = start.elapsed();
                if elapsed >= limit {
                    return Err(io::Error::new(io::ErrorKind::WouldBlock,
                        format!("{} is locked by another process", path.display())));
                }
                std::thread::sleep((limit - elapsed).min(Duration::from_millis(10)));
            }
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
}
