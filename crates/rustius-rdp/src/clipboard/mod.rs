pub mod backend;
pub mod file_provider;

pub use backend::{RustiusCliprdrBackend, RustiusCliprdrBackendFactory};
pub use file_provider::{
    create_file_descriptors_from_paths, get_host_clipboard_files, serve_file_contents_request,
    system_time_to_windows_filetime,
};

#[cfg(windows)]
pub use file_provider::{extract_hdrop_paths, write_hdrop_to_clipboard};
