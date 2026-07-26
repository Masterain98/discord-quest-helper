use crate::LaunchError;

pub(crate) fn unsupported<T>() -> Result<T, LaunchError> {
    Err(LaunchError::UnsupportedPlatform)
}
