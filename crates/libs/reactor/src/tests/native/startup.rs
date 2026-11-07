use super::*;

const USE_SHOW: u32 = native::STARTF_USESHOWWINDOW as u32;

#[test]
fn show_is_ignored_without_the_flag() {
    assert_eq!(requested(0, native::SW_SHOWMINNOACTIVE as u16), None);
}

#[test]
fn minimized_requests_activate_only_for_show_minimized() {
    assert_eq!(
        requested(USE_SHOW, native::SW_SHOWMINIMIZED as u16),
        Some(StartupShow::Minimized { activate: true })
    );
    for show in [
        native::SW_SHOWMINNOACTIVE,
        native::SW_MINIMIZE,
        native::SW_FORCEMINIMIZE,
    ] {
        assert_eq!(
            requested(USE_SHOW, show as u16),
            Some(StartupShow::Minimized { activate: false })
        );
    }
}

#[test]
fn maximized_and_normal_requests() {
    assert_eq!(
        requested(USE_SHOW, native::SW_SHOWMAXIMIZED as u16),
        Some(StartupShow::Maximized)
    );
    for show in [native::SW_SHOWNORMAL, native::SW_HIDE, native::SW_RESTORE] {
        assert_eq!(requested(USE_SHOW, show as u16), None);
    }
}
