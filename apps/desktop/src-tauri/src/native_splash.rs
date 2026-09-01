use std::ptr::{null, null_mut};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicIsize, AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DeleteDC, DeleteObject, DrawTextW,
    Ellipse, EndPaint, FW_BOLD, FW_NORMAL, FW_SEMIBOLD, FillRect, GetStockObject, InvalidateRect,
    LineTo, MoveToEx, NULL_PEN, PAINTSTRUCT, PS_SOLID, Polygon, SRCCOPY, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW, GetSystemMetrics,
    GetWindowLongPtrW, IDC_ARROW, LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNORMAL, SetTimer, SetWindowLongPtrW, ShowWindow,
    TranslateMessage, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_NCCREATE, WM_NCDESTROY, WM_PAINT,
    WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

const WIDTH: i32 = 880;
const HEIGHT: i32 = 550;
const TIMER_ID: usize = 1;
const TIMER_INTERVAL_MS: u32 = 250;

#[derive(Clone)]
pub(crate) struct NativeSplash {
    shared: Arc<Shared>,
}

#[derive(Clone)]
struct SplashStatus {
    progress: u8,
    label: String,
    current_file: String,
    phase: SplashPhase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplashPhase {
    Loading,
    Ready,
    Error,
}

struct Shared {
    status: Mutex<SplashStatus>,
    hwnd: AtomicIsize,
    frame: AtomicU64,
    started_at: Instant,
}

impl NativeSplash {
    pub(crate) fn start() -> Result<Self, String> {
        let shared = Arc::new(Shared {
            status: Mutex::new(SplashStatus {
                progress: 2,
                label: "正在启动 DRPA".to_owned(),
                current_file: "desktop://bootstrap".to_owned(),
                phase: SplashPhase::Loading,
            }),
            hwnd: AtomicIsize::new(0),
            frame: AtomicU64::new(0),
            started_at: Instant::now(),
        });
        let thread_shared = Arc::clone(&shared);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("drpa-native-splash".to_owned())
            .spawn(move || run_window(thread_shared, ready_tx))
            .map_err(|error| format!("无法启动原生启动图线程：{error}"))?;
        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self { shared }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err("原生启动图未能在 2 秒内创建".to_owned()),
        }
    }

    pub(crate) fn update(&self, progress: u8, label: &str, current_file: &str) {
        self.set_status(progress, label, current_file, SplashPhase::Loading);
    }

    pub(crate) fn ready(&self, label: &str, current_file: &str) {
        self.set_status(100, label, current_file, SplashPhase::Ready);
    }

    pub(crate) fn fail(&self, label: &str, detail: &str) {
        let progress = self
            .shared
            .status
            .lock()
            .map(|status| status.progress)
            .unwrap_or(0);
        self.set_status(progress, label, detail, SplashPhase::Error);
    }

    pub(crate) fn close(&self) {
        let hwnd = self.shared.hwnd.load(Ordering::Acquire);
        if hwnd != 0 {
            // SAFETY: hwnd is written by the splash UI thread and remains valid until WM_NCDESTROY.
            unsafe {
                PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
    }

    fn set_status(&self, progress: u8, label: &str, current_file: &str, phase: SplashPhase) {
        if let Ok(mut status) = self.shared.status.lock() {
            status.progress = progress.min(100);
            status.label = label.to_owned();
            status.current_file = current_file.to_owned();
            status.phase = phase;
        }
        let hwnd = self.shared.hwnd.load(Ordering::Acquire);
        if hwnd != 0 {
            // SAFETY: invalidating a live splash HWND is thread-safe and only schedules WM_PAINT.
            unsafe {
                InvalidateRect(hwnd as HWND, null(), 0);
            }
        }
    }
}

fn run_window(shared: Arc<Shared>, ready: mpsc::SyncSender<Result<(), String>>) {
    // SAFETY: all Win32 calls are confined to this dedicated UI thread. The shared Arc passed
    // through CREATESTRUCTW is retained until WM_NCDESTROY.
    unsafe {
        let instance = GetModuleHandleW(null());
        if instance.is_null() {
            let _ = ready.send(Err("无法获取 Windows 应用实例".to_owned()));
            return;
        }
        let class_name = wide("DRPA_NATIVE_SPLASH_V2");
        let window_title = wide("DRPA Next 正在启动");
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            let _ = ready.send(Err("无法注册 Windows 启动图窗口".to_owned()));
            return;
        }

        let x = (GetSystemMetrics(SM_CXSCREEN) - WIDTH).max(0) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - HEIGHT).max(0) / 2;
        let raw_shared = Arc::into_raw(Arc::clone(&shared));
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            window_title.as_ptr(),
            WS_POPUP,
            x,
            y,
            WIDTH,
            HEIGHT,
            null_mut(),
            null_mut(),
            instance,
            raw_shared.cast(),
        );
        if hwnd.is_null() {
            // Windows may already have delivered WM_NCDESTROY and released raw_shared.
            // Leaking this single startup Arc in the pre-create failure case is safer than
            // risking a double release while the process is about to abort startup.
            let _ = ready.send(Err("无法创建 Windows 原生启动图".to_owned()));
            return;
        }
        shared.hwnd.store(hwnd as isize, Ordering::Release);
        SetTimer(hwnd, TIMER_ID, TIMER_INTERVAL_MS, None);
        ShowWindow(hwnd, SW_SHOWNORMAL);
        UpdateWindow(hwnd);
        let _ = ready.send(Ok(()));

        let mut message = MSG::default();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            // SAFETY: lparam is a valid CREATESTRUCTW for WM_NCCREATE.
            let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
            1
        }
        WM_TIMER => {
            if wparam == TIMER_ID {
                if let Some(shared) = unsafe { shared_for(hwnd) } {
                    shared.frame.fetch_add(1, Ordering::Relaxed);
                }
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            0
        }
        WM_PAINT => {
            if let Some(shared) = unsafe { shared_for(hwnd) } {
                unsafe {
                    paint(hwnd, shared);
                }
            } else {
                unsafe {
                    DefWindowProcW(hwnd, message, wparam, lparam);
                }
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_CLOSE => {
            unsafe {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        WM_NCDESTROY => {
            let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            if pointer != 0 {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    (*(pointer as *const Shared))
                        .hwnd
                        .store(0, Ordering::Release);
                    drop(Arc::from_raw(pointer as *const Shared));
                }
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn shared_for(hwnd: HWND) -> Option<&'static Shared> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if pointer == 0 {
        None
    } else {
        // SAFETY: the Arc raw pointer is installed during WM_NCCREATE and released at WM_NCDESTROY.
        Some(unsafe { &*(pointer as *const Shared) })
    }
}

unsafe fn paint(hwnd: HWND, shared: &Shared) {
    let mut paint = PAINTSTRUCT::default();
    let target_dc = unsafe { BeginPaint(hwnd, &mut paint) };
    if target_dc.is_null() {
        return;
    }
    let mut client = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut client);
    }
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    let memory_dc = unsafe { CreateCompatibleDC(target_dc) };
    let bitmap = unsafe { CreateCompatibleBitmap(target_dc, width, height) };
    if memory_dc.is_null() || bitmap.is_null() {
        unsafe {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !memory_dc.is_null() {
                DeleteDC(memory_dc);
            }
            EndPaint(hwnd, &paint);
        }
        return;
    }
    let old_bitmap = unsafe { SelectObject(memory_dc, bitmap) };

    let status = shared
        .status
        .lock()
        .map(|value| value.clone())
        .unwrap_or(SplashStatus {
            progress: 2,
            label: "正在启动 DRPA".to_owned(),
            current_file: "desktop://bootstrap".to_owned(),
            phase: SplashPhase::Loading,
        });
    let frame = shared.frame.load(Ordering::Relaxed);
    let elapsed = shared.started_at.elapsed();

    unsafe {
        fill(
            memory_dc,
            RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
            rgb(248, 249, 253),
        );
        draw_geometry(memory_dc, width, height, frame);
        draw_brand(memory_dc);
        draw_status(memory_dc, width, height, &status, elapsed, frame);
        BitBlt(target_dc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY);
        SelectObject(memory_dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(memory_dc);
        EndPaint(hwnd, &paint);
    }
}

unsafe fn draw_geometry(dc: *mut core::ffi::c_void, width: i32, height: i32, frame: u64) {
    let panel = RECT {
        left: width * 53 / 100,
        top: 0,
        right: width,
        bottom: height,
    };
    unsafe {
        fill(dc, panel, rgb(241, 246, 255));
    }

    let cyan = unsafe { CreateSolidBrush(rgb(74, 205, 220)) };
    let blue = unsafe { CreateSolidBrush(rgb(49, 87, 232)) };
    let pale = unsafe { CreateSolidBrush(rgb(219, 235, 255)) };
    let old_pen = unsafe { SelectObject(dc, GetStockObject(NULL_PEN)) };
    let shape_a = [
        point(width - 340, 85),
        point(width - 64, 12),
        point(width - 64, 112),
        point(width - 286, 170),
    ];
    let shape_b = [
        point(width - 300, 270),
        point(width - 130, 205),
        point(width - 45, 315),
        point(width - 220, 382),
    ];
    let shape_c = [
        point(width - 410, 330),
        point(width - 245, 270),
        point(width - 155, 388),
        point(width - 320, 450),
    ];
    let previous = unsafe { SelectObject(dc, pale) };
    unsafe {
        Polygon(dc, shape_a.as_ptr(), shape_a.len() as i32);
        SelectObject(dc, blue);
        Polygon(dc, shape_b.as_ptr(), shape_b.len() as i32);
        SelectObject(dc, cyan);
        Polygon(dc, shape_c.as_ptr(), shape_c.len() as i32);
        SelectObject(dc, previous);
        SelectObject(dc, old_pen);
        DeleteObject(cyan);
        DeleteObject(blue);
        DeleteObject(pale);
    }

    let pen = unsafe { CreatePen(PS_SOLID, 1, rgb(146, 178, 245)) };
    let old = unsafe { SelectObject(dc, pen) };
    for offset in [0, 72, 144, 216] {
        unsafe {
            MoveToEx(dc, width - 430 + offset, 68, null_mut());
            LineTo(dc, width - 80 + offset / 3, height - 72);
        }
    }
    unsafe {
        SelectObject(dc, old);
        DeleteObject(pen);
    }

    let pulse = 7 + (frame % 8) as i32;
    let pulse_brush = unsafe { CreateSolidBrush(rgb(255, 130, 112)) };
    let old_brush = unsafe { SelectObject(dc, pulse_brush) };
    let old_pulse_pen = unsafe { SelectObject(dc, GetStockObject(NULL_PEN)) };
    unsafe {
        Ellipse(
            dc,
            width - 278 - pulse,
            height - 184 - pulse,
            width - 278 + pulse,
            height - 184 + pulse,
        );
        SelectObject(dc, old_pulse_pen);
        SelectObject(dc, old_brush);
        DeleteObject(pulse_brush);
    }
}

unsafe fn draw_brand(dc: *mut core::ffi::c_void) {
    let colors = [
        rgb(49, 87, 232),
        rgb(66, 201, 217),
        rgb(255, 122, 107),
        rgb(137, 161, 255),
    ];
    for (index, color) in colors.into_iter().enumerate() {
        let column = (index % 2) as i32;
        let row = (index / 2) as i32;
        unsafe {
            fill(
                dc,
                RECT {
                    left: 68 + column * 16,
                    top: 56 + row * 16,
                    right: 80 + column * 16,
                    bottom: 68 + row * 16,
                },
                color,
            );
        }
    }
    unsafe {
        text(
            dc,
            "D R P A    NEXT",
            rect(68, 106, 380, 132),
            15,
            FW_BOLD as i32,
            rgb(24, 48, 109),
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            dc,
            "让数据、运行与流程",
            rect(68, 160, 455, 205),
            31,
            FW_SEMIBOLD as i32,
            rgb(21, 33, 61),
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            dc,
            "自然汇入 AI。",
            rect(68, 205, 455, 250),
            31,
            FW_SEMIBOLD as i32,
            rgb(21, 33, 61),
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            dc,
            "Data · Runtime · Process · AI",
            rect(68, 278, 420, 306),
            14,
            FW_SEMIBOLD as i32,
            rgb(49, 87, 232),
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            dc,
            "数据 · 运行 · 流程 · AI",
            rect(68, 310, 420, 336),
            12,
            FW_NORMAL as i32,
            rgb(104, 118, 145),
            DT_LEFT | DT_SINGLELINE,
        );
    }
}

unsafe fn draw_status(
    dc: *mut core::ffi::c_void,
    width: i32,
    height: i32,
    status: &SplashStatus,
    elapsed: Duration,
    frame: u64,
) {
    let phase_color = match status.phase {
        SplashPhase::Loading => rgb(49, 87, 232),
        SplashPhase::Ready => rgb(35, 164, 109),
        SplashPhase::Error => rgb(211, 67, 67),
    };
    let label = if status.phase == SplashPhase::Error {
        format!("启动失败：{}", status.label)
    } else {
        status.label.clone()
    };
    unsafe {
        text(
            dc,
            &label,
            rect(68, height - 137, 405, height - 111),
            12,
            FW_NORMAL as i32,
            rgb(82, 97, 127),
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        text(
            dc,
            &format!("{}%", status.progress),
            rect(410, height - 137, 454, height - 111),
            11,
            FW_BOLD as i32,
            phase_color,
            DT_LEFT | DT_SINGLELINE,
        );
        fill(
            dc,
            rect(68, height - 102, 455, height - 98),
            rgb(222, 228, 244),
        );
        let progress_width = 387 * i32::from(status.progress.max(2)) / 100;
        fill(
            dc,
            rect(68, height - 102, 68 + progress_width, height - 98),
            phase_color,
        );
    }

    let pulse = if status.phase == SplashPhase::Error {
        "!"
    } else {
        ["●", "•", "·", "•"][(frame as usize) % 4]
    };
    let elapsed_seconds = elapsed.as_secs();
    let heartbeat = match status.phase {
        SplashPhase::Loading => format!(
            "应用仍在响应 · 已用时 {:02}:{:02}",
            elapsed_seconds / 60,
            elapsed_seconds % 60
        ),
        SplashPhase::Ready => "界面已经就绪".to_owned(),
        SplashPhase::Error => "请记录上方信息后重新启动应用".to_owned(),
    };
    unsafe {
        text(
            dc,
            pulse,
            rect(68, height - 84, 82, height - 60),
            10,
            FW_BOLD as i32,
            phase_color,
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            dc,
            &status.current_file,
            rect(84, height - 84, 455, height - 60),
            10,
            FW_NORMAL as i32,
            rgb(127, 139, 164),
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        text(
            dc,
            &heartbeat,
            rect(68, height - 55, 455, height - 32),
            9,
            FW_NORMAL as i32,
            rgb(145, 154, 176),
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            dc,
            "LOCAL-FIRST AUTOMATION WORKSPACE",
            rect(width - 240, height - 35, width - 24, height - 15),
            8,
            FW_SEMIBOLD as i32,
            rgb(142, 153, 178),
            DT_LEFT | DT_SINGLELINE,
        );
    }
}

unsafe fn text(
    dc: *mut core::ffi::c_void,
    value: &str,
    mut bounds: RECT,
    size: i32,
    weight: i32,
    color: COLORREF,
    format: u32,
) {
    let face = wide("Microsoft YaHei UI");
    let font = unsafe {
        CreateFontW(
            -size,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            face.as_ptr(),
        )
    };
    if font.is_null() {
        return;
    }
    let previous = unsafe { SelectObject(dc, font) };
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
    }
    let value = wide(value);
    unsafe {
        DrawTextW(
            dc,
            value.as_ptr(),
            value.len().saturating_sub(1) as i32,
            &mut bounds,
            format,
        );
        SelectObject(dc, previous);
        DeleteObject(font);
    }
}

unsafe fn fill(dc: *mut core::ffi::c_void, bounds: RECT, color: COLORREF) {
    let brush = unsafe { CreateSolidBrush(color) };
    if !brush.is_null() {
        unsafe {
            FillRect(dc, &bounds, brush);
            DeleteObject(brush);
        }
    }
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

const fn point(x: i32, y: i32) -> windows_sys::Win32::Foundation::POINT {
    windows_sys::Win32::Foundation::POINT { x, y }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
