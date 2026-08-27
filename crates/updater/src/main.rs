#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;

#[cfg(target_os = "windows")]
mod progress_window {
    use std::{path::PathBuf, sync::mpsc, thread, time::Duration};

    use anyhow::{Context as _, Result};
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{COLOR_WINDOW, GetSysColorBrush},
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                Controls::{PBM_SETMARQUEE, PBS_MARQUEE, PROGRESS_CLASS},
                WindowsAndMessaging::{
                    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
                    DispatchMessageW, GetDesktopWindow, GetWindowRect, MSG, PM_REMOVE,
                    PeekMessageW, RegisterClassW, SW_SHOW, ShowWindow, TranslateMessage,
                    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WNDCLASSW, WS_CAPTION, WS_CHILD,
                    WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
                },
            },
        },
        core::w,
    };

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_CLOSE {
            LRESULT(0)
        } else {
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
    }

    fn create() -> Result<HWND> {
        unsafe {
            let class = w!("Qrate-Update-Progress");
            let instance = GetModuleHandleW(None).context("get update helper module")?;
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                lpszClassName: class,
                hInstance: instance.into(),
                hbrBackground: GetSysColorBrush(COLOR_WINDOW),
                style: CS_HREDRAW | CS_VREDRAW,
                ..Default::default()
            });
            let mut desktop = RECT::default();
            GetWindowRect(GetDesktopWindow(), &mut desktop).context("measure desktop")?;
            let width = 380;
            let height = 112;
            let window = CreateWindowExW(
                WS_EX_TOPMOST,
                class,
                w!("qrate"),
                WS_VISIBLE | WS_POPUP | WS_CAPTION,
                (desktop.right - width) / 2,
                (desktop.bottom - height) / 2,
                width,
                height,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .context("create update progress window")?;
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Installing the qrate update…"),
                WS_CHILD | WS_VISIBLE,
                20,
                18,
                340,
                22,
                Some(window),
                None,
                Some(instance.into()),
                None,
            )
            .context("create update status label")?;
            let progress = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PROGRESS_CLASS,
                None,
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(PBS_MARQUEE),
                20,
                50,
                340,
                18,
                Some(window),
                None,
                Some(instance.into()),
                None,
            )
            .context("create update progress bar")?;
            let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                progress,
                PBM_SETMARQUEE,
                Some(WPARAM(1)),
                Some(LPARAM(30)),
            );
            let _ = ShowWindow(window, SW_SHOW);
            Ok(window)
        }
    }

    pub fn run(job: PathBuf) -> Result<()> {
        let window = match create() {
            Ok(window) => window,
            Err(_) => return updater::run_job(&job),
        };

        let (tx, rx) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = tx.send(updater::run_job(&job));
        });
        let result = loop {
            match rx.try_recv() {
                Ok(result) => break result,
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Err(anyhow::anyhow!("update worker stopped unexpectedly"));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            unsafe {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            thread::sleep(Duration::from_millis(16));
        };
        unsafe {
            DestroyWindow(window).context("close update progress window")?;
        }
        result
    }
}

#[cfg(target_os = "windows")]
fn show_error(message: &str) {
    use windows::{Win32::UI::WindowsAndMessaging::*, core::PCWSTR};

    let title = "qrate update failed\0".encode_utf16().collect::<Vec<_>>();
    let message = format!("{message}\0").encode_utf16().collect::<Vec<_>>();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn show_error(message: &str) {
    log::error!("qrate update failed: {message}");
}

fn main() {
    let job = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| {
            updater::updates_dir()
                .ok()
                .map(|dir| dir.join(updater::JOB_NAME))
        })
        .unwrap_or_else(|| {
            show_error("Could not locate qrate's pending update job.");
            std::process::exit(2);
        });
    #[cfg(target_os = "windows")]
    let result = progress_window::run(job);
    #[cfg(not(target_os = "windows"))]
    let result = updater::run_job(&job);

    if let Err(error) = result {
        show_error(&format!("{error:#}"));
        std::process::exit(1);
    }
}
