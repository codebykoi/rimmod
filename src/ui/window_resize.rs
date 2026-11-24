use eframe::egui::{
    self, CursorIcon, Id, Order, Pos2, Rect, Sense, ViewportCommand, viewport::ResizeDirection,
};

const EDGE: f32 = 5.0;
const CORNER: f32 = 10.0;

/// Configures the native borderless window so the client area aligns 1:1 with
/// the window bounds instead of inheriting `winit`'s 1 px `WM_NCCALCSIZE`
/// shadow hack on Windows.
pub(crate) fn configure_borderless_window(cc: &eframe::CreationContext<'_>) {
    #[cfg(target_os = "windows")]
    windows_frame::configure(cc);

    #[cfg(not(target_os = "windows"))]
    let _ = cc;
}

/// Borderless windows lose their native resize borders, so the app draws its
/// own invisible edge handles in a foreground layer.
pub(crate) fn show_resize_handles(context: &egui::Context) {
    if context.input(|input| input.viewport().maximized.unwrap_or(false)) {
        return;
    }

    let screen = context.content_rect();
    let handles = [
        (
            Rect::from_min_max(
                Pos2::new(screen.left(), screen.top()),
                Pos2::new(screen.right(), screen.top() + EDGE),
            ),
            ResizeDirection::North,
            CursorIcon::ResizeVertical,
        ),
        (
            Rect::from_min_max(
                Pos2::new(screen.left(), screen.bottom() - EDGE),
                Pos2::new(screen.right(), screen.bottom()),
            ),
            ResizeDirection::South,
            CursorIcon::ResizeVertical,
        ),
        (
            Rect::from_min_max(
                Pos2::new(screen.left(), screen.top()),
                Pos2::new(screen.left() + EDGE, screen.bottom()),
            ),
            ResizeDirection::West,
            CursorIcon::ResizeHorizontal,
        ),
        (
            Rect::from_min_max(
                Pos2::new(screen.right() - EDGE, screen.top()),
                Pos2::new(screen.right(), screen.bottom()),
            ),
            ResizeDirection::East,
            CursorIcon::ResizeHorizontal,
        ),
        (
            Rect::from_min_max(
                Pos2::new(screen.left(), screen.top()),
                Pos2::new(screen.left() + CORNER, screen.top() + CORNER),
            ),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNwSe,
        ),
        (
            Rect::from_min_max(
                Pos2::new(screen.right() - CORNER, screen.top()),
                Pos2::new(screen.right(), screen.top() + CORNER),
            ),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNeSw,
        ),
        (
            Rect::from_min_max(
                Pos2::new(screen.left(), screen.bottom() - CORNER),
                Pos2::new(screen.left() + CORNER, screen.bottom()),
            ),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeNeSw,
        ),
        (
            Rect::from_min_max(
                Pos2::new(screen.right() - CORNER, screen.bottom() - CORNER),
                Pos2::new(screen.right(), screen.bottom()),
            ),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeNwSe,
        ),
    ];

    // A borderless window needs a visible edge so it does not melt into the
    // desktop. This layer is explicitly non-interactive.
    egui::Area::new(Id::new("window_border"))
        .order(Order::Foreground)
        .fixed_pos(Pos2::ZERO)
        .interactable(false)
        .show(context, |ui| {
            ui.set_min_size(screen.size());
            ui.painter().rect_stroke(
                screen,
                egui::CornerRadius::ZERO,
                egui::Stroke::new(
                    1.0,
                    crate::ui::theme::tint(crate::ui::theme::TEXT_FAINT, 40),
                ),
                egui::StrokeKind::Inside,
            );
        });

    // One small area per handle. A single full-window layer would capture
    // every click in the app, not just the window edges.
    for (index, (rect, direction, cursor)) in handles.into_iter().enumerate() {
        egui::Area::new(Id::new(("window_resize_handle", index)))
            .order(Order::Foreground)
            .fixed_pos(rect.min)
            .show(context, |ui| {
                let (_, response) = ui.allocate_exact_size(rect.size(), Sense::drag());

                if response.hovered() {
                    ui.ctx().set_cursor_icon(cursor);
                }

                if response.drag_started() {
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            });
    }
}

#[cfg(target_os = "windows")]
mod windows_frame {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
    use windows_sys::Win32::UI::Controls::MARGINS;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsZoomed, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
        SWP_NOZORDER, SetWindowPos, WM_DPICHANGED, WM_DWMCOMPOSITIONCHANGED, WM_NCCALCSIZE,
        WM_NCDESTROY,
    };

    const SUBCLASS_ID: usize = 0x5249_4d4d;

    /// A 1 px bottom DWM margin keeps the native drop shadow around a
    /// borderless window without shifting the client area or introducing a top
    /// accent line on Windows 10.
    const SHADOW_MARGINS: MARGINS = MARGINS {
        cxLeftWidth: 0,
        cxRightWidth: 0,
        cyTopHeight: 0,
        cyBottomHeight: 1,
    };

    /// `egui-winit` enables `winit`'s `undecorated_shadow` whenever
    /// `with_decorations(false)` is set on Windows. `winit` implements that
    /// shadow by shifting `WM_NCCALCSIZE`'s client rect down by 1 px
    /// (`top += 1; bottom += 1`), which leaves an `H`-pixel client surface
    /// inside `H - 1` visible rows. `wgpu`'s DX12 swap chain uses
    /// `DXGI_SCALING_STRETCH`, so Windows resamples the entire frame vertically
    /// and blurs all UI text and graphics whenever the window is restored.
    ///
    /// Subclass the HWND to keep `rgrc[0]` unshifted in restored mode so the
    /// client area matches the outer window 1:1, and request the DWM drop
    /// shadow via `DwmExtendFrameIntoClientArea` instead.
    pub(super) fn configure(cc: &eframe::CreationContext<'_>) {
        let Ok(handle) = cc.window_handle() else {
            return;
        };

        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };

        let hwnd = win32.hwnd.get() as *mut c_void as HWND;

        // SAFETY: `hwnd` is a valid live top-level Win32 window created by
        // `eframe` on the UI thread, and `borderless_frame_proc` removes itself
        // on `WM_NCDESTROY`.
        unsafe {
            if SetWindowSubclass(hwnd, Some(borderless_frame_proc), SUBCLASS_ID, 0) != 0 {
                let _ = DwmExtendFrameIntoClientArea(hwnd, &SHADOW_MARGINS);
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOZORDER
                        | SWP_NOOWNERZORDER
                        | SWP_NOACTIVATE
                        | SWP_FRAMECHANGED,
                );
            }
        }
    }

    unsafe extern "system" fn borderless_frame_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        subclass_id: usize,
        _ref_data: usize,
    ) -> LRESULT {
        // SAFETY: Invoked by Windows on the window's owning thread with valid
        // message arguments; unhandled messages are forwarded to
        // `DefSubclassProc`.
        unsafe {
            match msg {
                WM_NCCALCSIZE if wparam != 0 && IsZoomed(hwnd) == 0 => {
                    // Return 0 with `rgrc[0]` untouched so the client area
                    // matches the full window rectangle instead of `winit`'s
                    // 1 px downward shift.
                    0
                }
                WM_DPICHANGED | WM_DWMCOMPOSITIONCHANGED => {
                    let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                    let _ = DwmExtendFrameIntoClientArea(hwnd, &SHADOW_MARGINS);
                    result
                }
                WM_NCDESTROY => {
                    RemoveWindowSubclass(hwnd, Some(borderless_frame_proc), subclass_id);
                    DefSubclassProc(hwnd, msg, wparam, lparam)
                }
                _ => DefSubclassProc(hwnd, msg, wparam, lparam),
            }
        }
    }
}
