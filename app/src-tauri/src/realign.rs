//! WebView2 can leave the window it draws into ("Intermediate D3D Window",
//! made by its GPU process) shifted inside ours, so the page shows moved up
//! and left with white beside and below it (decision #116; Joshua's tray
//! popover on 0.4.53 was 112 x 105 px off). Every other part of the webview
//! stays put, and Chromium only ever resizes that window, never moves it, so
//! putting it back in its parent's corner sticks.

#![allow(unsafe_code)]

use tauri::WebviewWindow;

/// Put a window's WebView2 drawing surface back where the page is.
#[cfg(windows)]
pub fn realign(window: &WebviewWindow) {
    use windows::Win32::Foundation::{LPARAM, TRUE};
    use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // SAFETY: `hwnd` is our live window and `fix` touches only the handle
    // Windows passes it; a window gone meanwhile just makes the calls fail.
    let _ = unsafe { EnumChildWindows(Some(hwnd), Some(fix), LPARAM(0)) };

    unsafe extern "system" fn fix(
        child: windows::Win32::Foundation::HWND,
        _: LPARAM,
    ) -> windows::core::BOOL {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetClassNameW, GetParent, GetWindowRect, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE,
            SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        };

        let mut class = [0u16; 64];
        // SAFETY: plain Win32 calls on a handle Windows just gave us.
        unsafe {
            let n = GetClassNameW(child, &mut class);
            if String::from_utf16_lossy(&class[..n.max(0) as usize]) != "Intermediate D3D Window" {
                return TRUE;
            }
            // The parent is a borderless child window, so its window rect's
            // corner is its client corner too.
            let (mut at, mut parent_at) = (RECT::default(), RECT::default());
            let Ok(parent) = GetParent(child) else {
                return TRUE;
            };
            if GetWindowRect(child, &mut at).is_ok()
                && GetWindowRect(parent, &mut parent_at).is_ok()
                && (at.left, at.top) != (parent_at.left, parent_at.top)
            {
                // Async: the window belongs to another process; never wait on it.
                let _ = SetWindowPos(
                    child,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                );
            }
        }
        TRUE
    }
}

/// Only WebView2 does this.
#[cfg(not(windows))]
pub fn realign(_window: &WebviewWindow) {}
