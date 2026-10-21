use anyhow::{anyhow, Result};
use ironrdp_cliprdr::pdu::{
    ClipboardFileAttributes, FileContentsFlags, FileContentsRequest, FileContentsResponse,
    FileDescriptor,
};
use std::path::PathBuf;
use tracing::{debug, warn};

/// Convert local file paths into IronRDP FileDescriptor metadata structures.
pub fn create_file_descriptors_from_paths(paths: &[PathBuf]) -> Vec<FileDescriptor> {
    let mut descriptors = Vec::with_capacity(paths.len());

    for path in paths {
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            warn!(?path, "Skipping file path with invalid UTF-8 filename");
            continue;
        };

        let mut desc = FileDescriptor::new(file_name);

        match std::fs::metadata(path) {
            Ok(meta) => {
                desc = desc.with_file_size(meta.len());

                let mut attrs = ClipboardFileAttributes::empty();
                if meta.is_dir() {
                    attrs |= ClipboardFileAttributes::DIRECTORY;
                } else {
                    attrs |= ClipboardFileAttributes::ARCHIVE;
                    if meta.permissions().readonly() {
                        attrs |= ClipboardFileAttributes::READONLY;
                    }
                }
                desc = desc.with_attributes(attrs);

                if let Ok(modified) = meta.modified() {
                    desc = desc.with_last_write_time(system_time_to_windows_filetime(modified));
                }
            }
            Err(err) => {
                warn!(?err, ?path, "Unable to query metadata for file offer");
            }
        }

        descriptors.push(desc);
    }

    descriptors
}

/// Convert a `SystemTime` into 64-bit Windows FILETIME (100-nanosecond intervals since Jan 1, 1601).
pub fn system_time_to_windows_filetime(time: std::time::SystemTime) -> u64 {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(dur) => {
            let intervals = dur.as_secs() * 10_000_000 + (dur.subsec_nanos() as u64) / 100;
            // 11644473600 seconds between Jan 1 1601 and Jan 1 1970
            intervals + 116_444_736_000_000_000
        }
        Err(_) => 0,
    }
}

/// Serve a `FileContentsRequest` by reading the local file on disk.
pub fn serve_file_contents_request(
    request: &FileContentsRequest,
    paths: &[PathBuf],
) -> FileContentsResponse<'static> {
    let index = match usize::try_from(request.index) {
        Ok(idx) => idx,
        Err(_) => {
            warn!(index = request.index, "Negative file index in request");
            return FileContentsResponse::new_error(request.stream_id);
        }
    };

    let Some(path) = paths.get(index) else {
        warn!(
            index,
            total = paths.len(),
            "Requested file index out of range"
        );
        return FileContentsResponse::new_error(request.stream_id);
    };

    if request.flags.contains(FileContentsFlags::SIZE) {
        match std::fs::metadata(path) {
            Ok(meta) => {
                debug!(index, size = meta.len(), "Serving file size request");
                FileContentsResponse::new_size_response(request.stream_id, meta.len())
            }
            Err(err) => {
                warn!(?err, ?path, "Failed to stat file for SIZE request");
                FileContentsResponse::new_error(request.stream_id)
            }
        }
    } else {
        // RANGE request
        const MAX_CHUNK_BYTES: u32 = 4 * 1024 * 1024;
        let to_read = request.requested_size.min(MAX_CHUNK_BYTES);

        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(err) => {
                warn!(?err, ?path, "Failed to open file for RANGE request");
                return FileContentsResponse::new_error(request.stream_id);
            }
        };

        use std::io::{Read, Seek, SeekFrom};
        if let Err(err) = file.seek(SeekFrom::Start(request.position)) {
            warn!(
                ?err,
                position = request.position,
                "Failed to seek file for RANGE request"
            );
            return FileContentsResponse::new_error(request.stream_id);
        }

        let mut buffer = Vec::new();
        match file.take(u64::from(to_read)).read_to_end(&mut buffer) {
            Ok(bytes_read) => {
                debug!(
                    index,
                    position = request.position,
                    bytes_read,
                    "Serving file range chunk"
                );
                FileContentsResponse::new_data_response(request.stream_id, buffer)
            }
            Err(err) => {
                warn!(?err, ?path, "Failed to read file chunk for RANGE request");
                FileContentsResponse::new_error(request.stream_id)
            }
        }
    }
}

/// Extract file paths from a Windows HDROP handle.
///
/// # Safety
///
/// Caller must ensure `hdrop` is a valid `HDROP` handle provided by the Windows clipboard or shell.
#[cfg(windows)]
pub unsafe fn extract_hdrop_paths(hdrop: windows_sys::Win32::UI::Shell::HDROP) -> Vec<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Shell::DragQueryFileW;

    let count = DragQueryFileW(hdrop, u32::MAX, core::ptr::null_mut(), 0);
    let mut paths = Vec::with_capacity(count as usize);

    for i in 0..count {
        let len = DragQueryFileW(hdrop, i, core::ptr::null_mut(), 0);
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            DragQueryFileW(hdrop, i, buf.as_mut_ptr(), len + 1);
            let os_str = OsString::from_wide(&buf[..len as usize]);
            paths.push(PathBuf::from(os_str));
        }
    }

    paths
}

/// Write file paths to the Windows clipboard using CF_HDROP.
///
/// # Safety
///
/// Caller must ensure `hwnd` is either a valid window handle or null, and that clipboard access
/// adheres to the Win32 threading model.
#[cfg(windows)]
pub unsafe fn write_hdrop_to_clipboard(
    hwnd: windows_sys::Win32::Foundation::HWND,
    paths: &[PathBuf],
) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };

    if paths.is_empty() {
        return Ok(());
    }

    // Prepare DROPFILES buffer
    #[repr(C)]
    struct DropFiles {
        p_files: u32,
        pt_x: i32,
        pt_y: i32,
        f_nc: i32,
        f_wide: i32,
    }

    let header_size = std::mem::size_of::<DropFiles>();
    let mut utf16_bytes = Vec::new();

    for path in paths {
        let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        encoded.push(0); // Null terminator per path
        for u in encoded {
            utf16_bytes.extend_from_slice(&u.to_le_bytes());
        }
    }
    // Final double null terminator
    utf16_bytes.extend_from_slice(&0u16.to_le_bytes());

    let total_bytes = header_size + utf16_bytes.len();

    unsafe {
        let h_mem = GlobalAlloc(GMEM_MOVEABLE, total_bytes);
        if h_mem.is_null() {
            return Err(anyhow!("GlobalAlloc failed for DROPFILES"));
        }

        let ptr = GlobalLock(h_mem) as *mut u8;
        if ptr.is_null() {
            GlobalFree(h_mem);
            return Err(anyhow!("GlobalLock failed for DROPFILES"));
        }

        let header = DropFiles {
            p_files: header_size as u32,
            pt_x: 0,
            pt_y: 0,
            f_nc: 0,
            f_wide: 1, // Unicode paths
        };

        std::ptr::copy_nonoverlapping(&header as *const _ as *const u8, ptr, header_size);
        std::ptr::copy_nonoverlapping(
            utf16_bytes.as_ptr(),
            ptr.add(header_size),
            utf16_bytes.len(),
        );

        GlobalUnlock(h_mem);

        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(hwnd) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !opened {
            GlobalFree(h_mem);
            return Err(anyhow!("OpenClipboard failed after retries"));
        }

        EmptyClipboard();
        const CF_HDROP: u32 = 15;
        if SetClipboardData(CF_HDROP, h_mem as _).is_null() {
            GlobalFree(h_mem);
            CloseClipboard();
            return Err(anyhow!("SetClipboardData(CF_HDROP) failed"));
        }

        CloseClipboard();
    }

    Ok(())
}

/// Query the host Windows clipboard for any currently available CF_HDROP file paths.
#[cfg(windows)]
pub fn get_host_clipboard_files() -> Vec<PathBuf> {
    const CF_HDROP: u32 = 15;
    unsafe {
        if windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable(CF_HDROP) == 0 {
            return Vec::new();
        }
        let mut opened = false;
        for _ in 0..10 {
            if windows_sys::Win32::System::DataExchange::OpenClipboard(core::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if !opened {
            return Vec::new();
        }
        let hdrop = windows_sys::Win32::System::DataExchange::GetClipboardData(CF_HDROP);
        let paths = if !hdrop.is_null() {
            extract_hdrop_paths(hdrop as _)
        } else {
            Vec::new()
        };
        windows_sys::Win32::System::DataExchange::CloseClipboard();
        paths
    }
}

#[cfg(not(windows))]
pub fn get_host_clipboard_files() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(not(windows))]
pub unsafe fn extract_hdrop_paths(_hdrop: *mut std::ffi::c_void) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(not(windows))]
pub unsafe fn write_hdrop_to_clipboard(
    _hwnd: *mut std::ffi::c_void,
    _paths: &[PathBuf],
) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_file_descriptors_and_serve_contents() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("rustius_test_fp_{}", timestamp));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let test_file = temp_dir.join("test_sample.bin");

        let test_data =
            b"Rustius clipboard file transfer test payload with multiple bytes of data.";
        {
            let mut f = std::fs::File::create(&test_file).unwrap();
            f.write_all(test_data).unwrap();
        }

        let paths = vec![test_file.clone()];
        let descriptors = create_file_descriptors_from_paths(&paths);
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].name, "test_sample.bin");
        assert_eq!(descriptors[0].file_size, Some(test_data.len() as u64));

        // Test SIZE request
        let size_req = FileContentsRequest {
            stream_id: 1,
            index: 0,
            flags: FileContentsFlags::SIZE,
            position: 0,
            requested_size: 8,
            data_id: None,
        };
        let size_resp = serve_file_contents_request(&size_req, &paths);
        assert_eq!(size_resp.stream_id(), 1);
        assert!(!size_resp.is_error());
        eprintln!(
            "size_resp data len: {}, data: {:?}",
            size_resp.data().len(),
            size_resp.data()
        );
        assert_eq!(size_resp.data().len(), 8);
        let reported_size = u64::from_le_bytes(size_resp.data().try_into().unwrap());
        assert_eq!(reported_size, test_data.len() as u64);

        let size_bytes = ironrdp::core::encode_vec(&size_resp).unwrap();
        eprintln!("encoded size_resp: {:?}", size_bytes);

        // Test DATA request for first 10 bytes
        let data_req = FileContentsRequest {
            stream_id: 2,
            index: 0,
            flags: FileContentsFlags::empty(),
            position: 0,
            requested_size: 10,
            data_id: None,
        };
        let data_resp = serve_file_contents_request(&data_req, &paths);
        assert_eq!(data_resp.stream_id(), 2);
        assert!(!data_resp.is_error());
        eprintln!(
            "data_resp data len: {}, data: {:?}",
            data_resp.data().len(),
            data_resp.data()
        );
        assert_eq!(data_resp.data(), &test_data[0..10]);
        let data_bytes = ironrdp::core::encode_vec(&data_resp).unwrap();
        eprintln!("encoded data_resp: {:?}", data_bytes);

        // Test invalid index returns error response
        let invalid_req = FileContentsRequest {
            stream_id: 3,
            index: 99,
            flags: FileContentsFlags::SIZE,
            position: 0,
            requested_size: 8,
            data_id: None,
        };
        let invalid_resp = serve_file_contents_request(&invalid_req, &paths);
        assert_eq!(invalid_resp.stream_id(), 3);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_clipboard_format_methods() {
        use ironrdp_cliprdr::pdu::{ClipboardFormat, ClipboardFormatId, ClipboardFormatName};
        let fmt = ClipboardFormat::new(ClipboardFormatId::new(49161))
            .with_name(ClipboardFormatName::new("FileGroupDescriptorW".to_string()));
        let name_str = fmt.name().map(|n| n.value());
        assert_eq!(name_str, Some("FileGroupDescriptorW"));
    }
}
