use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
};

use objc2_foundation::{NSFileManager, NSString, NSURL};

use super::DeleteError;

pub(super) async fn trash(paths: &[String]) -> Result<Vec<(String, String)>, DeleteError> {
    todo!()
}
