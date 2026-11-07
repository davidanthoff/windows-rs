use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// The show state that the program which started this process asked for, through
/// `STARTUPINFO`: for example `start /min`, or a shortcut set to run minimized or maximized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StartupShow {
    /// Minimized, and activated only for `SW_SHOWMINIMIZED`. `SW_SHOWMINNOACTIVE`, `SW_MINIMIZE`
    /// and `SW_FORCEMINIMIZE` leave the foreground window alone.
    Minimized {
        activate: bool,
    },
    Maximized,
}

static TAKEN: AtomicBool = AtomicBool::new(false);

/// Returns the launcher's request the first time it is called in this process and `None` after
/// that, so that only the application's first window follows it, as only a Win32 application's
/// first `ShowWindow` call does.
pub(super) fn take() -> Option<StartupShow> {
    if TAKEN.swap(true, Ordering::Relaxed) {
        return None;
    }
    let mut info = native::STARTUPINFOW {
        cb: size_of::<native::STARTUPINFOW>() as u32,
        ..Default::default()
    };
    unsafe { native::GetStartupInfoW(&mut info) };
    requested(info.dwFlags, info.wShowWindow)
}

fn requested(flags: u32, show: u16) -> Option<StartupShow> {
    if flags & native::STARTF_USESHOWWINDOW as u32 == 0 {
        return None;
    }
    match i32::from(show) {
        native::SW_SHOWMINIMIZED => Some(StartupShow::Minimized { activate: true }),
        native::SW_SHOWMINNOACTIVE | native::SW_MINIMIZE | native::SW_FORCEMINIMIZE => {
            Some(StartupShow::Minimized { activate: false })
        }
        native::SW_SHOWMAXIMIZED => Some(StartupShow::Maximized),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../../tests/native/startup.rs"]
mod tests;
