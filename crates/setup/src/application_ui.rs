//! Paired development setup UI. No browser/profile launch or silent startup.
//! Launch requested is explicitly distinct from companion/tray readiness.
use crate::{
    SetupError,
    installed_image::InstalledImage,
    package::VerifiedPackage,
    process::{ApplicationProbe, probe_application, require_apps_closed},
    registry::CurrentUserRegistration,
    transaction::SetupSession,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    process::{Child, Command, Stdio},
    rc::Rc,
    thread::JoinHandle,
};
use winsafe::{self as w, co, gui, prelude::*};

#[derive(Clone, Copy)]
enum Action {
    Install,
    Open,
    Repair,
    Recover,
    Uninstall,
    Cleanup,
}
struct Configuration {
    local: PathBuf,
    root: PathBuf,
    source: PathBuf,
}
impl Configuration {
    fn current() -> Result<Self, SetupError> {
        let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(SetupError::Path)?);
        let root = local
            .join("HalcyonXP")
            .join("FirefoxDownloadManager")
            .join("host");
        let source = std::env::current_exe()
            .map_err(|_| SetupError::Package)?
            .parent()
            .ok_or(SetupError::Package)?
            .to_owned();
        Ok(Self {
            local,
            root,
            source,
        })
    }
}
enum Outcome {
    Launched(Child),
    Done(&'static str),
}
fn perform(action: Action, config: &Configuration) -> Result<Outcome, SetupError> {
    if !matches!(action, Action::Open) {
        require_apps_closed()?;
        let package = if matches!(action, Action::Install) {
            Some(VerifiedPackage::open(&config.source)?)
        } else {
            None
        };
        let session = SetupSession::open(&config.root, &config.local)?;
        let mut registry = CurrentUserRegistration;
        match action {
            Action::Install => {
                session.install(
                    package.as_ref().ok_or(SetupError::Package)?,
                    &mut registry,
                    &mut ApplicationProbe,
                )?;
            }
            Action::Repair => {
                session.repair(&mut registry, &mut ApplicationProbe)?;
                return Ok(Outcome::Done(
                    "Verified application registration repaired. Task state and downloads were preserved.",
                ));
            }
            Action::Recover => {
                session.recover(&mut registry)?;
                return Ok(Outcome::Done(
                    "Journal recovery completed. Unknown content and task state were preserved.",
                ));
            }
            Action::Uninstall => {
                session.uninstall(&mut registry)?;
                return Ok(Outcome::Done(
                    "Verified program files and matching registration removed. Downloads and task state were preserved.",
                ));
            }
            Action::Cleanup => {
                session.cleanup(&mut registry)?;
                return Ok(Outcome::Done(
                    "Verified retired generations removed. Unknown files and downloads were preserved.",
                ));
            }
            Action::Open => return Err(SetupError::Arguments),
        }
        // The child must independently acquire existing setup coordination during
        // its image verification; never hold our setup lock across that startup.
        drop(session);
    }
    let image = InstalledImage::open_installed(&config.root, &config.local)?;
    // Receipt1 also describes legacy helpers. Content agreement alone cannot
    // authorize --companion against an old EOF host with different entry modes.
    probe_application(&image.executable(), &config.local)?;
    let child = Command::new(image.executable())
        .arg("--companion")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SetupError::Launch)?;
    // Retain verified immutable files until CreateProcess has loaded the image.
    // The child independently revalidates receipt/registration before state use.
    drop(image);
    Ok(Outcome::Launched(child))
}
struct Window {
    main: gui::WindowMain,
    status: gui::Label,
    worker: RefCell<Option<JoinHandle<Result<Outcome, SetupError>>>>,
    launched: RefCell<Vec<Child>>,
}
impl Window {
    fn start(&self, action: Action) -> w::AnyResult<()> {
        if self.worker.borrow().is_some() {
            return Ok(());
        }
        if matches!(action, Action::Open) && !self.launched.borrow().is_empty() {
            self.status.hwnd().SetWindowText("This setup is still observing a launched Manager. Use its status window or Quit before launching another.")?;
            return Ok(());
        }
        let config = match Configuration::current() {
            Ok(config) => config,
            Err(error) => {
                self.status.hwnd().SetWindowText(&error.to_string())?;
                return Ok(());
            }
        };
        self.status.hwnd().SetWindowText(
            "Checking package and ownership. Please wait; no browser is opened or closed by setup.",
        )?;
        match std::thread::Builder::new()
            .name("paired-setup-operation".into())
            .spawn(move || perform(action, &config))
        {
            Ok(handle) => *self.worker.borrow_mut() = Some(handle),
            Err(_) => self
                .status
                .hwnd()
                .SetWindowText("Setup could not start its worker. No operation was started.")?,
        }
        Ok(())
    }
    fn accept(&self, result: Result<Outcome, SetupError>) -> w::AnyResult<()> {
        let text = match result {
            Ok(Outcome::Launched(child)) => {
                self.launched.borrow_mut().push(child);
                "Installation verified; Manager launch requested. Check its visible status window. This is not a tray-readiness or persistent-XPI receipt.".to_owned()
            }
            Ok(Outcome::Done(text)) => text.to_owned(),
            Err(error) => error.to_string(),
        };
        self.status.hwnd().SetWindowText(&text)?;
        Ok(())
    }
    fn tick(&self) -> w::AnyResult<()> {
        if self
            .worker
            .borrow()
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            let handle = self
                .worker
                .borrow_mut()
                .take()
                .ok_or("missing retained setup worker")?;
            self.accept(handle.join().unwrap_or(Err(SetupError::Io)))?;
        }
        let mut failed = false;
        self.launched
            .borrow_mut()
            .retain_mut(|child| match child.try_wait() {
                Ok(Some(status)) => {
                    let joined = child.wait();
                    failed |= !status.success() || joined.is_err();
                    false
                }
                Ok(None) => true,
                Err(_) => {
                    failed = true;
                    true
                }
            });
        if failed {
            self.status.hwnd().SetWindowText("A launched Manager process failed or could not be observed. No successful startup is asserted; state was preserved.")?;
        }
        Ok(())
    }
    fn close(&self) -> w::AnyResult<()> {
        if self.worker.borrow().is_some() {
            self.status.hwnd().SetWindowText("An installation operation is still running. Its worker must finish before this window closes.")?;
        } else {
            self.main.hwnd().DestroyWindow()?;
        }
        Ok(())
    }
}

fn buttons(main: &gui::WindowMain) -> w::AnyResult<Vec<gui::Button>> {
    let mut controls = Vec::new();
    for (index, text) in [
        "Install / upgrade",
        "Open Manager",
        "Repair registration",
        "Recover journal",
        "Uninstall",
        "Close setup",
        "Clean retired versions",
    ]
    .iter()
    .enumerate()
    {
        let column = i32::try_from(index % 3).map_err(|_| "button column")?;
        let row = i32::try_from(index / 3).map_err(|_| "button row")?;
        controls.push(gui::Button::new(
            main,
            gui::ButtonOpts {
                text,
                position: gui::dpi(20 + 220 * column, 177 + 50 * row),
                width: gui::dpi_x(200),
                ctrl_id: 300 + u16::try_from(index).map_err(|_| "button identifier")?,
                ..Default::default()
            },
        ));
    }
    Ok(controls)
}

/// Open the ordinary no-arguments setup interface for paired development builds.
/// # Errors
/// Refuses UI/worker failure; never treats process creation as tray readiness.
pub fn run() -> w::AnyResult<()> {
    let main = gui::WindowMain::new(gui::WindowMainOpts {
        class_name: "DownloadManagerPairedSetup",
        title: "Download Manager setup — development candidate",
        size: gui::dpi(680, 340),
        class_icon: gui::Icon::Idi(co::IDI::INFORMATION),
        ..Default::default()
    });
    let _heading = gui::Label::new(
        &main,
        gui::LabelOpts {
            text: "Paired application setup — not an install-ready release",
            position: gui::dpi(20, 18),
            size: gui::dpi(640, 28),
            ..Default::default()
        },
    );
    let status = gui::Label::new(
        &main,
        gui::LabelOpts {
            text: "Install/upgrade requires Firefox and Manager closed normally. Open Manager does not change registration. Signed XPI, shortcut and ordinary-click qualification remain pending.",
            position: gui::dpi(20, 62),
            size: gui::dpi(640, 90),
            ctrl_id: 310,
            ..Default::default()
        },
    );
    let controls = buttons(&main)?;
    let window = Rc::new(Window {
        main,
        status,
        worker: RefCell::new(None),
        launched: RefCell::new(Vec::new()),
    });
    for (button, action) in controls.iter().zip([
        Action::Install,
        Action::Open,
        Action::Repair,
        Action::Recover,
        Action::Uninstall,
    ]) {
        let window = Rc::clone(&window);
        button.on().bn_clicked(move || window.start(action));
    }
    {
        let window = Rc::clone(&window);
        controls[5].on().bn_clicked(move || window.close());
    }
    {
        let window = Rc::clone(&window);
        let target = Rc::clone(&window);
        window.main.on().wm_create(move |_| {
            target.main.hwnd().SetTimer(1, 100, None)?;
            Ok(0)
        });
    }
    {
        let target = Rc::clone(&window);
        window.main.on().wm_timer(1, move || target.tick());
    }
    {
        let target = Rc::clone(&window);
        window.main.on().wm_close(move || target.close());
    }
    {
        let target = Rc::clone(&window);
        controls[6]
            .on()
            .bn_clicked(move || target.start(Action::Cleanup));
    }
    let result = window.main.run_main(None);
    // An exceptional message-loop exit still joins the retained operation.
    if let Some(handle) = window.worker.borrow_mut().take() {
        let outcome = handle.join().map_err(|_| "setup worker failed")?;
        if let Outcome::Launched(child) = outcome? {
            window.launched.borrow_mut().push(child);
        }
    }
    // Deliberate lifecycle transfer: a launched visible companion is meant to
    // outlive setup. This releases observation handles, not a shutdown receipt.
    window.launched.borrow_mut().clear();
    result?;
    Ok(())
}
