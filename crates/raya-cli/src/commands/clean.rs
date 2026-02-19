//! `raya clean` — Clear caches and build artifacts.

use std::path::Path;

pub fn execute(cache: bool, dist: bool, all: bool) -> anyhow::Result<()> {
    let clean_all = all || (!cache && !dist);

    if clean_all || cache {
        let cache_dir = Path::new(".raya-cache");
        if cache_dir.exists() {
            std::fs::remove_dir_all(cache_dir)?;
            println!("Removed .raya-cache/");
        } else {
            println!("No cache directory found.");
        }
    }

    if clean_all || dist {
        let dist_dir = Path::new("dist");
        if dist_dir.exists() {
            std::fs::remove_dir_all(dist_dir)?;
            println!("Removed dist/");
        } else {
            println!("No dist directory found.");
        }
    }

    if all {
        // Also clear global cache
        if let Some(home) = dirs::home_dir() {
            let global_cache = home.join(".raya").join("cache");
            if global_cache.exists() {
                std::fs::remove_dir_all(&global_cache)?;
                println!("Removed ~/.raya/cache/");
            }
        }
    }

    println!("Done.");
    Ok(())
}
