//! 工作区窗口在 tauri.conf.json 里 `visible: false` 且 `focus: false`。
//! wry 只在创建 WebView 时走一遍 EnumChildWindows，并先关掉 WebView2 自己的拖放。
//! 那时接收拖放的子窗口经常还没出现，Windows 就在光标上画禁止符号。
//! 窗口真正显示之后再登记一次。悬浮岛不走这里。

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Serialize)]
struct DeskFileDrop {
    kind: &'static str,
    paths: Vec<String>,
}

pub fn arm(app: &AppHandle) {
    #[cfg(windows)]
    {
        attach(app);
        static ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if ARMED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        for delay in [250u64, 800, 2000] {
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(delay));
                let again = app.clone();
                let _ = app.run_on_main_thread(move || attach(&again));
            });
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

#[cfg(windows)]
fn attach(app: &AppHandle) {
    use std::cell::RefCell;

    use windows::Win32::Foundation::{DRAGDROP_E_INVALIDHWND, HWND, LPARAM};
    use windows::Win32::System::Ole::{IDropTarget, OleInitialize, RegisterDragDrop, RevokeDragDrop};
    use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    thread_local! {
        static HELD: RefCell<Vec<(isize, IDropTarget)>> = RefCell::new(Vec::new());
    }

    let Some(desk) = app.get_webview_window("desk") else {
        return;
    };
    let Ok(foreign) = desk.hwnd() else {
        return;
    };
    let root = HWND(foreign.0 as *mut _);
    let _ = unsafe { OleInitialize(None) };

    let mut children = Vec::new();
    {
        let mut collect = |hwnd: HWND| {
            children.push(hwnd);
            true
        };
        let mut trait_obj: &mut dyn FnMut(HWND) -> bool = &mut collect;
        let lparam = LPARAM(&mut trait_obj as *mut _ as isize);
        unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let closure = &mut *(lparam.0 as *mut &mut dyn FnMut(HWND) -> bool);
            closure(hwnd).into()
        }
        let _ = unsafe { EnumChildWindows(Some(root), Some(each), lparam) };
    }

    for hwnd in children {
        let key = hwnd.0 as isize;
        let already = HELD.with(|held| held.borrow().iter().any(|(seen, _)| *seen == key));
        if already {
            continue;
        }
        let target: IDropTarget = FileDrop {
            hwnd,
            app: app.clone(),
            hot: false.into(),
        }
        .into();
        let revoked = unsafe { RevokeDragDrop(hwnd) };
        if let Err(err) = &revoked {
            if err.code() == DRAGDROP_E_INVALIDHWND {
                continue;
            }
        }
        if unsafe { RegisterDragDrop(hwnd, &target) }.is_ok() {
            HELD.with(|held| held.borrow_mut().push((key, target)));
        }
    }
}

#[cfg(windows)]
#[windows::core::implement(windows::Win32::System::Ole::IDropTarget)]
struct FileDrop {
    hwnd: windows::Win32::Foundation::HWND,
    app: AppHandle,
    hot: std::cell::Cell<bool>,
}

#[cfg(windows)]
impl FileDrop {
    fn send(&self, kind: &'static str, paths: Vec<String>) {
        let _ = self.app.emit("desk-file-drop", DeskFileDrop { kind, paths });
    }
}

#[cfg(windows)]
fn file_paths(data: &windows::Win32::System::Com::IDataObject) -> Option<Vec<String>> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::{ReleaseStgMedium, CF_HDROP};
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let mut medium = unsafe { data.GetData(&format) }.ok()?;
    let hdrop = HDROP(unsafe { medium.u.hGlobal }.0);
    let mut paths = Vec::new();
    unsafe {
        let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
        for index in 0..count {
            let len = DragQueryFileW(hdrop, index, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, index, Some(&mut buf));
            let path: PathBuf = OsString::from_wide(&buf[..len]).into();
            paths.push(path.to_string_lossy().into_owned());
        }
        ReleaseStgMedium(&mut medium);
    }
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

#[cfg(windows)]
#[allow(non_snake_case)]
impl windows::Win32::System::Ole::IDropTarget_Impl for FileDrop_Impl {
    fn DragEnter(
        &self,
        pdataobj: windows::core::Ref<'_, windows::Win32::System::Com::IDataObject>,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        pt: &windows::Win32::Foundation::POINTL,
        pdweffect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ScreenToClient;
        use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE};

        let Some(obj) = pdataobj.as_ref() else {
            return Ok(());
        };
        let paths = file_paths(obj);
        let ok = paths.is_some();
        self.hot.set(ok);
        if let Some(paths) = paths {
            let mut point = POINT { x: pt.x, y: pt.y };
            let _ = unsafe { ScreenToClient(self.hwnd, &mut point) };
            self.send("enter", paths);
        }
        if !pdweffect.is_null() {
            unsafe {
                *pdweffect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
            }
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        _pt: &windows::Win32::Foundation::POINTL,
        pdweffect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE};

        if !pdweffect.is_null() {
            unsafe {
                *pdweffect = if self.hot.get() {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                };
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if self.hot.get() {
            self.send("leave", Vec::new());
        }
        self.hot.set(false);
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: windows::core::Ref<'_, windows::Win32::System::Com::IDataObject>,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        _pt: &windows::Win32::Foundation::POINTL,
        pdweffect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        use windows::Win32::System::Ole::DROPEFFECT_COPY;

        if self.hot.get() {
            if let Some(obj) = pdataobj.as_ref() {
                if let Some(paths) = file_paths(obj) {
                    self.send("drop", paths);
                }
            }
        }
        self.hot.set(false);
        if !pdweffect.is_null() {
            unsafe { *pdweffect = DROPEFFECT_COPY };
        }
        Ok(())
    }
}
