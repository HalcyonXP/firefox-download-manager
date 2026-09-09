//! Native Windows preview adapter. Only predefined messages and safe wrappers;
//! no raw FFI, message-filter changes, browser access or installed-state use.
use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use download_manager_native_host::HostConfig;
use winsafe::{self as w, co, gui, prelude::*};

use crate::lifecycle::{Lifecycle, Phase};
use crate::worker::Worker;

const TIMER: usize = 1;
const ICON_ID: u32 = 1;
const OPEN: u16 = 100;
const QUIT: u16 = 101;
const TOOLTIP: &str = "Download Manager - development preview";

struct Shell {
    window: gui::WindowMain,
    status: gui::Label,
    registration_status: gui::Label,
    registrations: RefCell<u64>,
    state: RefCell<Lifecycle>,
    tray: RefCell<Option<w::NOTIFYICONDATA>>,
    worker: RefCell<Option<Worker>>,
    ticks: RefCell<u8>,
}

impl Shell {
    fn register_events(self: &Rc<Self>, hide: &gui::Button, quit: &gui::Button) {
        let shell = Rc::clone(self);
        self.window.on().wm_create(move |_| {
            // GetAncestor(ROOT) returns the current top-level handle through a
            // safe API; equality proves it is our own live window, not a lookup
            // or a manufactured raw pointer.
            let hwnd = shell
                .window
                .hwnd()
                .GetAncestor(co::GA::ROOT)
                .ok_or("preview window ownership unavailable")?;
            if &hwnd != shell.window.hwnd() {
                return Err("preview window ownership changed".into());
            }
            let mut data = w::NOTIFYICONDATA::default();
            data.hWnd = hwnd;
            data.uID = ICON_ID;
            data.uFlags = co::NIF::ICON | co::NIF::MESSAGE | co::NIF::TIP;
            data.uCallbackMessage = co::WM::APP;
            data.uVersion = 3;
            // A shared stock icon is appropriate for a clearly marked preview;
            // no third-party artwork or raw handle duplication is needed.
            data.hIcon = gui::Icon::Idi(co::IDI::INFORMATION).as_hicon(&w::HINSTANCE::NULL)?;
            data.set_szTip(TOOLTIP);
            *shell.tray.borrow_mut() = Some(data);
            shell.confirm_tray();
            shell.window.hwnd().SetTimer(TIMER, 100, None)?;
            shell.refresh_text()?;
            Ok(0)
        });
        let shell = Rc::clone(self);
        self.window.on().wm_timer(TIMER, move || shell.tick());
        let shell = Rc::clone(self);
        self.window.on().wm_close(move || shell.hide_or_quit());
        let shell = Rc::clone(self);
        hide.on().bn_clicked(move || shell.hide_or_quit());
        let shell = Rc::clone(self);
        quit.on().bn_clicked(move || shell.request_quit());
        let shell = Rc::clone(self);
        self.window
            .on()
            .wm_command_acc_menu(OPEN, move || shell.show());
        let shell = Rc::clone(self);
        self.window
            .on()
            .wm_command_acc_menu(QUIT, move || shell.request_quit());
        let shell = Rc::clone(self);
        self.window.on().wm(co::WM::APP, move |message| {
            if message.wparam != ICON_ID as usize {
                return Ok(0);
            }
            let event = u32::try_from(message.lparam).ok();
            if event == Some(co::WM::LBUTTONUP.raw()) || matches!(event, Some(0x0400 | 0x0401)) {
                // NIN_SELECT / NIN_KEYSELECT with NOTIFYICON_VERSION3.
                shell.show()?;
            } else if event == Some(co::WM::RBUTTONUP.raw())
                || event == Some(co::WM::CONTEXTMENU.raw())
            {
                let menu = w::HMENU::CreatePopupMenu()?;
                menu.AppendMenu(
                    co::MF::STRING,
                    w::IdMenu::Id(OPEN),
                    w::BmpPtrStr::Str(w::WString::from_str("Open Manager status")),
                )?;
                menu.AppendMenu(
                    co::MF::STRING,
                    w::IdMenu::Id(QUIT),
                    w::BmpPtrStr::Str(w::WString::from_str("Quit")),
                )?;
                shell.window.hwnd().SetForegroundWindow();
                menu.TrackPopupMenu(
                    co::TPM::RIGHTBUTTON,
                    w::GetCursorPos()?,
                    shell.window.hwnd(),
                )?;
            }
            Ok(0)
        });
    }

    fn confirm_tray(&self) {
        let confirmed = self.tray.borrow().as_ref().is_some_and(|data| {
            let known = *self.registrations.borrow() > 0;
            if known && w::Shell_NotifyIcon(co::NIM::MODIFY, data).is_ok() {
                return w::Shell_NotifyIcon(co::NIM::SETVERSION, data).is_ok();
            }
            // Never modify/delete an entry before this window has received an
            // ADD acknowledgement for it. A successful builder is not proof.
            if w::Shell_NotifyIcon(co::NIM::ADD, data).is_err() {
                return false;
            }
            let mut count = self.registrations.borrow_mut();
            *count = count.saturating_add(1);
            drop(count);
            w::Shell_NotifyIcon(co::NIM::MODIFY, data).is_ok()
                && w::Shell_NotifyIcon(co::NIM::SETVERSION, data).is_ok()
        });
        let start = self.state.borrow_mut().tray_observed(confirmed);
        if start {
            match preview_worker() {
                Ok(worker) => *self.worker.borrow_mut() = Some(worker),
                Err(()) => self.state.borrow_mut().joined(false),
            }
        }
        if !confirmed {
            self.window.hwnd().ShowWindow(co::SW::SHOW);
        }
    }

    fn show(&self) -> w::AnyResult<()> {
        self.window.hwnd().ShowWindow(co::SW::SHOW);
        self.window.hwnd().SetForegroundWindow();
        self.refresh_text()
    }

    fn refresh_text(&self) -> w::AnyResult<()> {
        self.status
            .hwnd()
            .SetWindowText(self.state.borrow().status())?;
        self.registration_status.hwnd().SetWindowText(&format!(
            "Tray registrations: {}",
            *self.registrations.borrow()
        ))?;
        Ok(())
    }

    fn hide_or_quit(&self) -> w::AnyResult<()> {
        if self.state.borrow().phase() == Phase::Running {
            self.confirm_tray(); // Refresh the proof before deliberately hiding.
        }
        if self.state.borrow().can_hide() {
            self.window.hwnd().ShowWindow(co::SW::HIDE);
            Ok(())
        } else {
            self.request_quit()
        }
    }

    fn request_quit(&self) -> w::AnyResult<()> {
        self.state.borrow_mut().quit();
        if let Some(worker) = self.worker.borrow_mut().as_mut() {
            worker.request_stop();
        } else {
            self.state.borrow_mut().joined(true);
        }
        self.show()
    }

    fn tick(&self) -> w::AnyResult<()> {
        // Joining is polled at100ms; shell health at1s. These are scheduling
        // intervals, not a promise of latency when Windows is stalled.
        let check_tray = {
            let mut ticks = self.ticks.borrow_mut();
            *ticks = (*ticks + 1) % 10;
            *ticks == 0
        };
        if check_tray && self.state.borrow().phase() != Phase::Stopped {
            self.confirm_tray();
        }
        let joined = {
            let mut slot = self.worker.borrow_mut();
            if let Some(worker) = slot.as_mut() {
                if worker.take_ready() {
                    self.state.borrow_mut().started();
                }
                worker.try_join()
            } else {
                None
            }
        };
        if let Some(result) = joined {
            self.worker.borrow_mut().take();
            self.state.borrow_mut().joined(result.is_ok());
        }
        if self.state.borrow().phase() == Phase::Failed {
            self.show()?;
        }
        self.refresh_text()?;
        if self.state.borrow().phase() == Phase::Stopped {
            if self.remove_tray().is_err() {
                if !self.state.borrow().failed() {
                    self.state.borrow_mut().joined(false);
                    return self.show();
                }
                // Acknowledged failure may exit after engine joining; it can
                // never yield a successful preview report/exit code.
                self.tray.borrow_mut().take();
            }
            self.window.hwnd().KillTimer(TIMER)?;
            self.window.hwnd().DestroyWindow()?;
        }
        Ok(())
    }

    fn remove_tray(&self) -> w::AnyResult<()> {
        if *self.registrations.borrow() > 0
            && let Some(data) = self.tray.borrow().as_ref()
        {
            w::Shell_NotifyIcon(co::NIM::DELETE, data)?;
        }
        self.tray.borrow_mut().take();
        Ok(())
    }
}

fn preview_worker() -> Result<Worker, ()> {
    // Exclusive fresh domain, never adoption of an installed root. Preserve it
    // on exit rather than recursively deleting potentially changed contents.
    let root = std::env::temp_dir().join(format!(
        "DownloadManagerCompanionPreview-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).map_err(|_| ())?;
    let destination = root.join("downloads");
    fs::create_dir(&destination).map_err(|_| ())?;
    let endpoint = download_manager_local_ipc::Endpoint::generate().map_err(|_| ())?;
    let key =
        std::sync::Arc::new(download_manager_local_ipc::Capability::generate().map_err(|_| ())?);
    // Memory-only explicit preview binding; no installed descriptor/registration.
    Worker::start_local(
        HostConfig::new(root.join("state"), Some(destination)),
        endpoint,
        key,
    )
    .map_err(|_| ())
}

/// Runs a visibly labelled isolated preview, not an installed companion.
///
/// # Errors
/// Refuses unavailable UI/tray/engine resources; never prints raw error text.
pub fn run_preview() -> w::AnyResult<()> {
    let window = gui::WindowMain::new(gui::WindowMainOpts {
        class_name: "DownloadManagerCompanionPreview",
        title: "Download Manager - development preview",
        size: gui::dpi(600, 240),
        class_icon: gui::Icon::Idi(co::IDI::INFORMATION),
        ..Default::default()
    });
    let _heading = gui::Label::new(
        &window,
        gui::LabelOpts {
            text: "Visible companion preview - not an installable release",
            position: gui::dpi(22, 18),
            size: gui::dpi(556, 28),
            ..Default::default()
        },
    );
    let status = gui::Label::new(
        &window,
        gui::LabelOpts {
            text: "Confirming the tray icon before starting the engine...",
            position: gui::dpi(22, 57),
            size: gui::dpi(556, 40),
            ctrl_id: 200,
            ..Default::default()
        },
    );
    let _detail = gui::Label::new(
        &window,
        gui::LabelOpts {
            text: "Isolated temporary state. No browser connection or installation changes.",
            position: gui::dpi(22, 108),
            size: gui::dpi(556, 30),
            ..Default::default()
        },
    );
    let registration_status = gui::Label::new(
        &window,
        gui::LabelOpts {
            text: "Tray registrations: 0",
            position: gui::dpi(22, 140),
            size: gui::dpi(556, 25),
            ctrl_id: 203,
            ..Default::default()
        },
    );
    let hide = gui::Button::new(
        &window,
        gui::ButtonOpts {
            text: "Hide to tray",
            position: gui::dpi(22, 185),
            width: gui::dpi_x(140),
            ctrl_id: 201,
            ..Default::default()
        },
    );
    let quit = gui::Button::new(
        &window,
        gui::ButtonOpts {
            text: "Quit",
            position: gui::dpi(438, 185),
            width: gui::dpi_x(140),
            ctrl_id: 202,
            ..Default::default()
        },
    );
    let shell = Rc::new(Shell {
        window,
        status,
        registration_status,
        registrations: RefCell::new(0),
        state: RefCell::new(Lifecycle::default()),
        tray: RefCell::new(None),
        worker: RefCell::new(None),
        ticks: RefCell::new(0),
    });
    shell.register_events(&hide, &quit);
    let result = shell.window.run_main(None);
    // Exceptional window-loop exit still joins the exact retained worker via
    // Drop; it must not be reported as an orderly successful Quit.
    let clean = shell.state.borrow().phase() == Phase::Stopped && !shell.state.borrow().failed();
    shell.worker.borrow_mut().take();
    let cleanup = shell.remove_tray();
    result?;
    cleanup?;
    if !clean {
        return Err("preview stopped without a successful joined Quit".into());
    }
    Ok(())
}
