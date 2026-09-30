//! Downloads go straight to your Downloads folder (from `xdg-user-dirs`),
//! under the name the site suggested, never overwriting a file already there.

use crate::browser::Browser;
use crate::store;
use gtk::gio;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use webkit6::Download;

pub struct Item {
    pub download: Download,
    pub name: String,
    pub path: Option<PathBuf>,
    pub done: bool,
    pub failed: bool,
}

/// `file.tar.gz` → `file (2).tar.gz` until the name is free.
pub fn free_name(dir: &Path, suggested: &str) -> PathBuf {
    let clean: String = suggested
        .chars()
        .map(|c| if c == '/' || c == '\0' { '_' } else { c })
        .collect::<String>()
        .trim_start_matches('.')
        .to_string();
    let name = if clean.trim().is_empty() { "download".to_string() } else { clean };
    let first = dir.join(&name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.find('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    (2..10_000).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

impl Browser {
    pub fn wire_downloads(self: &Rc<Self>) {
        let weak = self.weak();
        self.web.session.connect_download_started(move |_, download| {
            if let Some(b) = weak.upgrade() {
                b.track(download);
            }
        });
    }

    /// Private tabs have their own session; its downloads are tracked too.
    pub fn wire_private_downloads(self: &Rc<Self>, session: &webkit6::NetworkSession) {
        let weak = self.weak();
        session.connect_download_started(move |_, download| {
            if let Some(b) = weak.upgrade() {
                b.track(download);
            }
        });
    }

    fn track(self: &Rc<Self>, download: &Download) {
        let dir = store::downloads_dir();
        let _ = std::fs::create_dir_all(&dir);
        self.downloads.borrow_mut().push(Item {
            download: download.clone(),
            name: String::new(),
            path: None,
            done: false,
            failed: false,
        });

        let weak = self.weak();
        download.connect_decide_destination(move |d, suggested| {
            let path = free_name(&dir, suggested);
            if let Some(uri) = path.to_str() {
                d.set_destination(uri);
            }
            if let Some(b) = weak.upgrade() {
                if let Some(item) = b.downloads.borrow_mut().iter_mut().find(|i| &i.download == d) {
                    item.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    item.path = Some(path.clone());
                }
                b.toast(&format!("Downloading {}", path.file_name().unwrap_or_default().to_string_lossy()));
            }
            true
        });

        let weak = self.weak();
        download.connect_finished(move |d| {
            let Some(b) = weak.upgrade() else { return };
            let mut path = None;
            if let Some(item) = b.downloads.borrow_mut().iter_mut().find(|i| &i.download == d)
                && !item.failed
            {
                item.done = true;
                path = item.path.clone();
            }
            if let Some(path) = path {
                let toast =
                    adw::Toast::new(&format!("Downloaded {}", path.file_name().unwrap_or_default().to_string_lossy()));
                toast.set_button_label(Some("Open"));
                toast.connect_button_clicked(move |_| open_file(&path));
                b.toasts.add_toast(toast);
            }
        });

        let weak = self.weak();
        download.connect_failed(move |d, err| {
            let Some(b) = weak.upgrade() else { return };
            if let Some(item) = b.downloads.borrow_mut().iter_mut().find(|i| &i.download == d) {
                item.failed = true;
            }
            if !err.matches(webkit6::DownloadError::CancelledByUser) {
                b.toast(&format!("Download failed: {}", err.message()));
            }
        });
    }
}

/// Open a file with whatever the desktop uses for it (through the portal on
/// a sandboxed system, xdg-open otherwise).
pub fn open_file(path: &Path) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
    launcher.launch(None::<&gtk::Window>, None::<&gio::Cancellable>, |_| {});
}

pub fn show_in_folder(path: &Path) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
    launcher.open_containing_folder(None::<&gtk::Window>, None::<&gio::Cancellable>, |_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_never_clash() {
        let dir = std::env::temp_dir().join(format!("wisp-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_name(&dir, "a.tar.gz"), dir.join("a.tar.gz"));
        std::fs::write(dir.join("a.tar.gz"), b"").unwrap();
        assert_eq!(free_name(&dir, "a.tar.gz"), dir.join("a (2).tar.gz"));
        assert_eq!(free_name(&dir, "../../etc/passwd"), dir.join("_.._etc_passwd"));
        assert_eq!(free_name(&dir, ""), dir.join("download"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
