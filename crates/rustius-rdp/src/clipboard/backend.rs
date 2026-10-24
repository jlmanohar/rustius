use parking_lot::RwLock;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use ironrdp::core::impl_as_any;
use ironrdp_cliprdr::backend::{
    ClipboardMessage, ClipboardMessageProxy, CliprdrBackend, CliprdrBackendFactory,
};
use ironrdp_cliprdr::chunked_fetch::{ChunkedFetch, ChunkedFetchProgress};
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardGeneralCapabilityFlags, FileContentsRequest, FileContentsResponse,
    FileDescriptor, FormatDataRequest, FormatDataResponse, LockDataId,
};

use crate::clipboard::file_provider::{
    create_file_descriptors_from_paths, get_host_clipboard_files, serve_file_contents_request,
    write_hdrop_to_clipboard,
};

/// Factory producing wrapped IronRDP clipboard backends with file-copy capabilities.
pub struct RustiusCliprdrBackendFactory {
    inner_factory: Box<dyn CliprdrBackendFactory + Send>,
    proxy: Arc<dyn ClipboardMessageProxy + Send + Sync>,
    local_files: Arc<RwLock<Vec<PathBuf>>>,
    remote_staging_dir: PathBuf,
    is_ready: Arc<std::sync::atomic::AtomicBool>,
}

impl RustiusCliprdrBackendFactory {
    pub fn new(
        inner_factory: Box<dyn CliprdrBackendFactory + Send>,
        proxy: Arc<dyn ClipboardMessageProxy + Send + Sync>,
        local_files: Arc<RwLock<Vec<PathBuf>>>,
        remote_staging_dir: PathBuf,
        is_ready: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            inner_factory,
            proxy,
            local_files,
            remote_staging_dir,
            is_ready,
        }
    }
}

impl CliprdrBackendFactory for RustiusCliprdrBackendFactory {
    fn build_cliprdr_backend(&self) -> Box<dyn CliprdrBackend> {
        Box::new(RustiusCliprdrBackend {
            inner: self.inner_factory.build_cliprdr_backend(),
            proxy: Arc::clone(&self.proxy),
            local_files: Arc::clone(&self.local_files),
            remote_staging_dir: self.remote_staging_dir.clone(),
            is_ready: Arc::clone(&self.is_ready),
            active_fetch: None,
            current_remote_file: None,
            remote_file_queue: VecDeque::new(),
            downloaded_paths: Vec::new(),
            current_file_index: 0,
            current_clip_data_id: None,
        })
    }
}

/// Custom clipboard backend that wraps the platform text clipboard backend
/// and adds bidirectional file streaming support via MS-RDPECLIP.
#[derive(Debug)]
pub struct RustiusCliprdrBackend {
    inner: Box<dyn CliprdrBackend>,
    proxy: Arc<dyn ClipboardMessageProxy + Send + Sync>,
    local_files: Arc<RwLock<Vec<PathBuf>>>,
    remote_staging_dir: PathBuf,
    is_ready: Arc<std::sync::atomic::AtomicBool>,
    active_fetch: Option<ChunkedFetch>,
    current_remote_file: Option<FileDescriptor>,
    remote_file_queue: VecDeque<FileDescriptor>,
    downloaded_paths: Vec<PathBuf>,
    current_file_index: i32,
    current_clip_data_id: Option<u32>,
}

impl Drop for RustiusCliprdrBackend {
    fn drop(&mut self) {
        self.is_ready
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl_as_any!(RustiusCliprdrBackend);

impl RustiusCliprdrBackend {
    fn fetch_next_remote_file(&mut self) {
        if let Some(file_info) = self.remote_file_queue.pop_front() {
            let index = self.current_file_index;
            self.current_file_index += 1;
            let stream_id = (index + 1) as u32;
            let chunk_size = 256 * 1024;
            let max_ceiling = 100 * 1024 * 1024 * 1024;

            let mut fetch = if let Some(size) = file_info.file_size {
                ChunkedFetch::new(
                    stream_id,
                    index,
                    size,
                    chunk_size,
                    self.current_clip_data_id,
                    max_ceiling,
                )
            } else {
                ChunkedFetch::new_with_size_query(
                    stream_id,
                    index,
                    chunk_size,
                    self.current_clip_data_id,
                    max_ceiling,
                )
            };

            if let Some(req) = fetch.next_request() {
                debug!(?req, index, name = %file_info.name, "Requesting chunks of remote file");
                self.active_fetch = Some(fetch);
                self.current_remote_file = Some(file_info);
                self.proxy
                    .send_clipboard_message(ClipboardMessage::SendFileContentsRequest(req));
            }
        } else if !self.downloaded_paths.is_empty() {
            info!(
                count = self.downloaded_paths.len(),
                "All remote files downloaded to local staging directory; updating host clipboard"
            );
            #[cfg(windows)]
            {
                let res = unsafe {
                    write_hdrop_to_clipboard(core::ptr::null_mut(), &self.downloaded_paths)
                };
                if let Err(e) = res {
                    warn!(
                        ?e,
                        "Failed to update host clipboard with downloaded remote files"
                    );
                }
            }
        }
    }
}

impl CliprdrBackend for RustiusCliprdrBackend {
    fn temporary_directory(&self) -> &str {
        self.inner.temporary_directory()
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        self.inner.client_capabilities()
            | ClipboardGeneralCapabilityFlags::STREAM_FILECLIP_ENABLED
            | ClipboardGeneralCapabilityFlags::CAN_LOCK_CLIPDATA
            | ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
            | ClipboardGeneralCapabilityFlags::HUGE_FILE_SUPPORT_ENABLED
    }

    fn on_ready(&mut self) {
        self.inner.on_ready();
        self.is_ready
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let mut paths = self.local_files.read().clone();
        if paths.is_empty() {
            paths = get_host_clipboard_files();
            if !paths.is_empty() {
                *self.local_files.write() = paths.clone();
            }
        }

        if !paths.is_empty() {
            let descriptors = create_file_descriptors_from_paths(&paths);
            if !descriptors.is_empty() {
                info!(
                    count = descriptors.len(),
                    "Clipboard channel ready; announcing local files to remote session"
                );
                self.proxy
                    .send_clipboard_message(ClipboardMessage::SendInitiateFileCopy(descriptors));
            }
        }
    }

    fn on_request_format_list(&mut self) {
        let mut paths = self.local_files.read().clone();
        if paths.is_empty() {
            paths = get_host_clipboard_files();
            if !paths.is_empty() {
                *self.local_files.write() = paths;
            }
        }
        self.inner.on_request_format_list();
    }

    fn on_process_negotiated_capabilities(
        &mut self,
        capabilities: ClipboardGeneralCapabilityFlags,
    ) {
        debug!(
            ?capabilities,
            "Processing negotiated clipboard capabilities in Rustius backend"
        );
        self.inner.on_process_negotiated_capabilities(capabilities);
    }

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        debug!(
            formats_count = available_formats.len(),
            "Processing on_remote_copy from server"
        );

        for format in available_formats {
            if let Some(name) = format.name() {
                if name.value().eq_ignore_ascii_case("FileGroupDescriptorW") {
                    info!(
                        format_id = ?format.id(),
                        "Server offered FileGroupDescriptorW; requesting format data to initiate file fetch"
                    );
                    self.proxy
                        .send_clipboard_message(ClipboardMessage::SendInitiatePaste(format.id()));
                    return;
                }
            }
        }

        self.inner.on_remote_copy(available_formats);
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        self.inner.on_format_data_request(request);
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        self.inner.on_format_data_response(response);
    }

    fn on_file_contents_request(&mut self, request: FileContentsRequest) {
        debug!(
            stream_id = request.stream_id,
            index = request.index,
            flags = ?request.flags,
            position = request.position,
            size = request.requested_size,
            "Handling FileContentsRequest from remote desktop"
        );

        let paths = self.local_files.read().clone();
        let response = serve_file_contents_request(&request, &paths);
        self.proxy
            .send_clipboard_message(ClipboardMessage::SendFileContentsResponse(response));
    }

    fn on_remote_file_list(&mut self, files: &[FileDescriptor], clip_data_id: Option<u32>) {
        info!(
            file_count = files.len(),
            ?clip_data_id,
            "Remote desktop offered files via clipboard"
        );

        self.remote_file_queue = files.iter().cloned().collect();
        self.downloaded_paths.clear();
        self.current_file_index = 0;
        self.current_clip_data_id = clip_data_id;
        self.fetch_next_remote_file();
    }

    fn on_file_contents_response(&mut self, response: FileContentsResponse<'_>) {
        debug!(
            stream_id = response.stream_id(),
            is_error = response.is_error(),
            data_len = response.data().len(),
            "Received FileContentsResponse from remote desktop"
        );

        if let Some(mut fetch) = self.active_fetch.take() {
            match fetch.on_response(&response) {
                ChunkedFetchProgress::InProgress => {
                    if let Some(next_req) = fetch.next_request() {
                        self.proxy.send_clipboard_message(
                            ClipboardMessage::SendFileContentsRequest(next_req),
                        );
                        self.active_fetch = Some(fetch);
                    }
                }
                ChunkedFetchProgress::Complete => {
                    let data = fetch.into_data();
                    if let Some(file_info) = self.current_remote_file.take() {
                        let raw_name = &file_info.name;
                        let safe_name = std::path::Path::new(raw_name)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .filter(|n| !n.is_empty() && *n != "." && *n != ".." && !raw_name.contains(':'));

                        if let Some(safe_name) = safe_name {
                            let dest_path = self.remote_staging_dir.join(safe_name);
                            let _ = std::fs::create_dir_all(&self.remote_staging_dir);
                            match std::fs::write(&dest_path, &data) {
                                Ok(_) => {
                                    info!(
                                        ?dest_path,
                                        "Remote file downloaded to local staging directory"
                                    );
                                    self.downloaded_paths.push(dest_path);
                                }
                                Err(e) => {
                                    warn!(
                                        ?e,
                                        ?dest_path,
                                        "Failed to write downloaded remote file to staging directory"
                                    );
                                }
                            }
                        } else {
                            warn!(
                                ?raw_name,
                                "Rejected untrusted or invalid clipboard file name from remote desktop"
                            );
                        }
                    }
                    self.fetch_next_remote_file();
                }
                ChunkedFetchProgress::Failed => {
                    warn!("Remote file transfer failed or cancelled");
                    self.current_remote_file = None;
                    self.fetch_next_remote_file();
                }
            }
        }
    }

    fn on_lock(&mut self, data_id: LockDataId) {
        self.inner.on_lock(data_id);
    }

    fn on_unlock(&mut self, data_id: LockDataId) {
        self.inner.on_unlock(data_id);
    }

    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }

    fn elapsed_ms(&self, since: u64) -> u64 {
        self.inner.elapsed_ms(since)
    }
}
