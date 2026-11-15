//! Loading the dictionary this process decodes with.

use bite::Dictionary;
use log::{info, warn};
use std::path::PathBuf;
use std::sync::Arc;

/// Load the dictionary at `path`, or the one named by `BITE_DICT_PATH` (or the
/// older `FAAD_DICT_PATH`).
///
/// With no path from either place the result is `None` and packets are
/// published undecoded. A path that cannot be loaded is an error: a
/// misconfigured decoder should not start.
pub fn load(path: Option<PathBuf>) -> Result<Option<Arc<Dictionary>>, bite::LoadError> {
    let Some(path) = path.or_else(bite::dict_path_from_env) else {
        warn!(
            "no dictionary: pass --dictionary or set {}; packets will be published undecoded",
            bite::DICT_PATH_ENV
        );
        return Ok(None);
    };
    let dictionary = Dictionary::load(&path)?;
    for issue in dictionary.issues() {
        warn!("dictionary {}: {issue}", path.display());
    }
    let messages: usize = dictionary
        .interfaces()
        .iter()
        .map(|interface| interface.messages().len())
        .sum();
    info!(
        "dictionary {}: {} interfaces, {} messages",
        path.display(),
        dictionary.interfaces().len(),
        messages
    );
    Ok(Some(Arc::new(dictionary)))
}
