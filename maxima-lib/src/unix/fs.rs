use std::path::PathBuf;

pub async fn case_insensitive_path(path: PathBuf) -> PathBuf {
    if path.exists() {
        return path;
    }
    let mut missing_parts: Vec<String> = Vec::new();

    let original_path = path.clone();
    let mut path = path;
    // Find first existing ancestor
    // And fill path file names array
    loop {
        if let Some(parent) = path.clone().parent() {
            missing_parts.push(path.file_name().unwrap().to_string_lossy().to_string());
            path = parent.to_owned();
            if parent.exists() {
                break;
            }
        } else {
            // If we run out of acnestors, return the original path
            return original_path;
        }
    }
    // Reverse the array so we have the proper order of path parts
    missing_parts.reverse();

    let mut not_existing_path = false;
    for part in missing_parts.into_iter() {
        if not_existing_path {
            path.push(part);
            continue;
        }
        let mut found = false;
        for entry in path.read_dir().unwrap() {
            if let Ok(entry) = entry {
                if entry.file_name().to_string_lossy().to_lowercase() == part.to_lowercase() {
                    found = true;
                    path.push(entry.file_name());
                    break;
                }
            }
        }
        // If the path part wasn't found, mark the path as not existing so we can push the rest
        // of the parts to the end, this will at least allow us to get as close to proper-cased path
        if !found {
            path.push(part);
            not_existing_path = true;
        }
    }

    path
}
