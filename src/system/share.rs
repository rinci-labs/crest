//! Share files through the Windows Share sheet (Nearby Sharing to other PCs, email, apps), the
//! Windows counterpart of AirDrop. Used when files are dropped on the "Share" zone. Crest sends
//! files nowhere itself; the user picks the destination in that Windows sheet.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
use windows::Foundation::TypedEventHandler;
use windows::Storage::{IStorageItem, StorageFile, StorageFolder};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows::core::{HSTRING, Interface, Ref, Result, factory};
use windows_collections::IIterable;

/// Share sheet of the Crest window; registered once, its contents replaced on every share.
pub struct Share {
    hwnd: HWND,
    interop: IDataTransferManagerInterop,
    manager: DataTransferManager,
    token: i64,
    /// Files being shared; read when Windows asks for the contents.
    paths: Arc<Mutex<Vec<PathBuf>>>,
}

impl Share {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let interop = factory::<DataTransferManager, IDataTransferManagerInterop>()?;
        let manager: DataTransferManager = unsafe { interop.GetForWindow(hwnd)? };
        let paths: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let shared = paths.clone();
        // Windows asks for the contents when the Share sheet opens.
        let handler = TypedEventHandler::new(move |_, args: Ref<DataRequestedEventArgs>| {
            let Some(args) = args.as_ref() else { return Ok(()) };
            let paths = shared.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let items: Vec<IStorageItem> = paths.iter().filter_map(|path| storage_item(path).ok()).collect();
            let title = match items.as_slice() {
                [one] => one.Name()?.to_string(),
                many => format!("{} files", many.len()),
            };
            let data = args.Request()?.Data()?;
            data.Properties()?.SetTitle(&HSTRING::from(title))?;
            let items: Vec<Option<IStorageItem>> = items.into_iter().map(Some).collect();
            data.SetStorageItemsReadOnly(&IIterable::from(items))?;
            Ok(())
        });
        let token = manager.DataRequested(&handler)?;
        Ok(Self { hwnd, interop, manager, token, paths })
    }

    /// Open the Share sheet for `paths` (files or folders).
    pub fn show(&self, paths: &[PathBuf]) -> Result<()> {
        let paths: Vec<PathBuf> = paths.iter().filter(|path| path.exists()).cloned().collect();
        if paths.is_empty() {
            return Ok(());
        }
        *self.paths.lock().unwrap_or_else(|e| e.into_inner()) = paths;
        unsafe { self.interop.ShowShareUIForWindow(self.hwnd) }
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        let _ = self.manager.RemoveDataRequested(self.token);
    }
}

fn storage_item(path: &std::path::Path) -> Result<IStorageItem> {
    let name = HSTRING::from(path.as_os_str());
    if path.is_dir() {
        StorageFolder::GetFolderFromPathAsync(&name)?.join()?.cast()
    } else {
        StorageFile::GetFileFromPathAsync(&name)?.join()?.cast()
    }
}
