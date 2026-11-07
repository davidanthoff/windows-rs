use super::*;
use std::fs::{self, File};
use std::os::windows::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

static SERIAL: Mutex<()> = Mutex::new(());
static NEXT_LOG: AtomicU32 = AtomicU32::new(0);

/// A fixture process. `std::process::Command` cannot yet set the show command that a launcher
/// passes in `STARTUPINFO` (as `start /min` and shortcuts set to run minimized do), so fixtures
/// that need one are started through `CreateProcessW`.
enum Child {
    Std(std::process::Child),
    Shown(ShownChild),
}

impl Child {
    fn id(&self) -> u32 {
        match self {
            Self::Std(child) => child.id(),
            Self::Shown(child) => child.id,
        }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match self {
            Self::Std(child) => child.try_wait(),
            Self::Shown(child) => child.wait_for(0),
        }
    }

    fn kill(&mut self) -> std::io::Result<()> {
        match self {
            Self::Std(child) => child.kill(),
            Self::Shown(child) => unsafe {
                if !TerminateProcess(child.process, 1).as_bool() {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            },
        }
    }

    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        match self {
            Self::Std(child) => child.wait(),
            Self::Shown(child) => Ok(child.wait_for(INFINITE)?.unwrap()),
        }
    }
}

struct ShownChild {
    process: HANDLE,
    id: u32,
}

impl ShownChild {
    fn spawn(fixture: &str, show: i32) -> Result<Self> {
        let mut command: Vec<u16> = format!("\"{}\" {fixture}", env!("CARGO_BIN_EXE_test-reactor"))
            .encode_utf16()
            .chain([0])
            .collect();
        let mut startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            dwFlags: STARTF_USESHOWWINDOW as u32,
            wShowWindow: show as u16,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                windows_core::PCWSTR::null(),
                windows_core::PWSTR(command.as_mut_ptr()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                false.into(),
                0,
                std::ptr::null_mut(),
                windows_core::PCWSTR::null(),
                &mut startup,
                &mut process,
            )
            .ok()?;
            _ = CloseHandle(process.hThread);
        }
        Ok(Self {
            process: process.hProcess,
            id: process.dwProcessId,
        })
    }

    fn wait_for(&self, milliseconds: u32) -> std::io::Result<Option<ExitStatus>> {
        unsafe {
            if WaitForSingleObject(self.process, milliseconds) != WAIT_OBJECT_0.0 as u32 {
                return Ok(None);
            }
            let mut code = 0;
            if !GetExitCodeProcess(self.process, &mut code).as_bool() {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Some(ExitStatus::from_raw(code)))
        }
    }
}

impl Drop for ShownChild {
    fn drop(&mut self) {
        unsafe { _ = CloseHandle(self.process) };
    }
}

struct Host {
    child: Child,
    directory: PathBuf,
}

impl Host {
    fn spawn(fixture: &str) -> Result<Self> {
        let directory = Self::log_directory()?;
        let child = Command::new(env!("CARGO_BIN_EXE_test-reactor"))
            .arg(fixture)
            .stdin(Stdio::null())
            .stdout(File::create(directory.join("stdout.txt"))?)
            .stderr(File::create(directory.join("stderr.txt"))?)
            .spawn()?;
        eprintln!("fixture {fixture}: pid {}", child.id());
        Ok(Self {
            child: Child::Std(child),
            directory,
        })
    }

    /// Starts the fixture as a launcher that asks for `show` does. Its output is not captured.
    fn spawn_shown(fixture: &str, show: i32) -> Result<Self> {
        let directory = Self::log_directory()?;
        File::create(directory.join("stdout.txt"))?;
        File::create(directory.join("stderr.txt"))?;
        let child = ShownChild::spawn(fixture, show)?;
        eprintln!("fixture {fixture} (show {show}): pid {}", child.id);
        Ok(Self {
            child: Child::Shown(child),
            directory,
        })
    }

    fn log_directory() -> Result<PathBuf> {
        Ok(loop {
            let directory = std::env::temp_dir().join(format!(
                "reactor-ui-{}-{}",
                std::process::id(),
                NEXT_LOG.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&directory) {
                Ok(()) => break directory,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        })
    }

    fn wait(&mut self, deadline: Instant) -> Result<ExitStatus> {
        loop {
            if let Some(status) = self.child.try_wait()? {
                if !status.success() {
                    return Err(format!("fixture exited with {status}").into());
                }
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err("fixture timed out".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            if let Err(error) = self.child.kill()
                && self.child.try_wait()?.is_none()
            {
                return Err(error.into());
            }
            self.child.wait()?;
        }
        Ok(())
    }

    fn output(&self) -> Result<String> {
        let stdout = fs::read(self.directory.join("stdout.txt"))?;
        let stderr = fs::read(self.directory.join("stderr.txt"))?;
        Ok(format!(
            "logs: {}\nstdout:\n{}\nstderr:\n{}",
            self.directory.display(),
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr),
        ))
    }

    fn remove_logs(&self) -> Result<()> {
        fs::remove_file(self.directory.join("stdout.txt"))?;
        fs::remove_file(self.directory.join("stderr.txt"))?;
        fs::remove_dir(&self.directory)?;
        Ok(())
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("failed to stop fixture {}: {error}", self.child.id());
        }
    }
}

pub fn run(fixture: &'static str, test: fn(&Automation) -> Result<()>) -> Result<()> {
    // Serialize this suite even when Cargo uses its default test-thread count.
    let _serial = SERIAL.lock().unwrap();
    run_host(Host::spawn(fixture)?, fixture, test)
}

/// Like [`run`], with the fixture started as a launcher that asks for the `SW_` show command
/// `show` starts it, for example `start /min`.
pub fn run_with_show(
    fixture: &'static str,
    show: i32,
    test: fn(&Automation) -> Result<()>,
) -> Result<()> {
    let _serial = SERIAL.lock().unwrap();
    run_host(Host::spawn_shown(fixture, show)?, fixture, test)
}

fn run_host(
    mut host: Host,
    fixture: &'static str,
    test: fn(&Automation) -> Result<()>,
) -> Result<()> {
    match drive(&mut host, fixture, test) {
        Ok(()) => host.remove_logs(),
        Err(error) => {
            if let Err(cleanup) = host.stop() {
                eprintln!("fixture cleanup also failed: {cleanup}");
            }
            Err(format!("{fixture}: {error}\n{}", host.output()?).into())
        }
    }
}

pub fn run_to_exit(fixture: &str) -> Result<()> {
    let _serial = SERIAL.lock().unwrap();
    let mut host = Host::spawn(fixture)?;
    match host.wait(Instant::now() + Duration::from_secs(30)) {
        Ok(_) => host.remove_logs(),
        Err(error) => {
            host.stop()?;
            Err(format!("{fixture}: {error}\n{}", host.output()?).into())
        }
    }
}

fn drive(
    host: &mut Host,
    fixture: &'static str,
    test: fn(&Automation) -> Result<()>,
) -> Result<()> {
    let process = host.child.id();
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver_cancelled = cancelled.clone();
    let pointer = PointerInput::default();
    let pressed = pointer.pressed.clone();
    let driver = std::thread::spawn(move || {
        let _com = Com::new()?;
        let ui = Automation::new(process, fixture, driver_cancelled, pressed)?;
        test(&ui)?;
        ui.window_handle()
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    if let Err(error) = wait_driver(host, &driver, &cancelled, deadline, Duration::from_secs(5)) {
        if driver.is_finished() {
            match driver.join() {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => eprintln!("UI worker also failed: {error}"),
                Err(_) => eprintln!("UI worker also panicked"),
            }
        }
        return Err(error);
    }
    let window = driver.join().map_err(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("non-string panic");
        format!("UI assertion panicked: {message}")
    })??;
    if *pointer.pressed.lock().unwrap() {
        return Err("UI test finished with an injected mouse button still pressed".into());
    }
    unsafe {
        let window = window as HWND;
        let mut owner = 0;
        GetWindowThreadProcessId(window, &mut owner);
        if owner != process {
            return Err("refusing to close another process's window".into());
        }
        PostMessageW(window, WM_CLOSE as u32, 0, 0).ok()?;
    }
    host.wait(deadline)?;
    Ok(())
}

#[test]
fn pointer_button_is_released_after_driver_failure() -> Result<()> {
    input_failure("pointer-capture-mounted", |ui| {
        ui.focus("baseline")?;
        ui.pointer_move("left", 0.5, 2.0)?;
        ui.name("pointer", "Left false; outside false")?;
        ui.pointer_button(true)?;
        ui.pointer_move("right", 0.5, 2.0)?;
        ui.name("pointer", "Left true; outside false")?;
        Err("input driver failure probe".into())
    })
}

#[test]
#[ignore = "requires a non-elevated interactive desktop; WinUI issue microsoft/microsoft-ui-xaml#7690"]
fn drag_source_is_stopped_after_driver_failure() -> Result<()> {
    drag::require_non_elevated()?;
    input_failure("drag-mounted", |ui| {
        drag::start(ui)?;
        ui.pointer_move("target", 0.5, 0.5)?;
        ui.name("feedback", "Copy")?;
        Err("input driver failure probe".into())
    })
}

fn input_failure(fixture: &'static str, test: fn(&Automation) -> Result<()>) -> Result<()> {
    let serial = SERIAL.lock().unwrap();
    let mut host = Host::spawn(fixture)?;
    let result = drive(&mut host, fixture, test);
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { GetAsyncKeyState(0x01) } & i16::MIN != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let released = unsafe { GetAsyncKeyState(0x01) } & i16::MIN == 0;
    host.stop()?;
    drop(serial);
    let error = result.unwrap_err();
    assert_eq!(error.to_string(), "input driver failure probe");
    assert!(
        released,
        "failed driver left the injected mouse button pressed"
    );
    host.remove_logs()
}

fn wait_driver(
    host: &mut Host,
    driver: &JoinHandle<Result<usize>>,
    cancelled: &AtomicBool,
    deadline: Instant,
    cleanup_timeout: Duration,
) -> Result<()> {
    let result: Result<()> = (|| {
        while !driver.is_finished() {
            if let Some(status) = host.child.try_wait()? {
                return Err(format!("fixture exited before the UI test finished: {status}").into());
            }
            if Instant::now() >= deadline {
                return Err("UI test timed out (including UI Automation calls)".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    if let Err(error) = result {
        cancelled.store(true, Ordering::Relaxed);
        host.stop()?;
        // Killing the provider normally releases an outstanding UIA call. Never block on join.
        let cleanup = Instant::now() + cleanup_timeout;
        while !driver.is_finished() && Instant::now() < cleanup {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !driver.is_finished() {
            eprintln!("UI Automation worker did not finish after its provider was terminated");
        }
        return Err(error);
    }
    Ok(())
}

#[test]
fn driver_timeout_cancels_worker() -> Result<()> {
    driver_timeout(true)
}

#[test]
fn driver_timeout_bounds_blocked_worker_cleanup() -> Result<()> {
    driver_timeout(false)
}

fn driver_timeout(cooperative: bool) -> Result<()> {
    use std::sync::mpsc::{RecvTimeoutError, channel};

    let mut host = Host::spawn("hang")?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let driver_cancelled = cancelled.clone();
    let (ready, started) = channel();
    let (release, released) = channel::<()>();
    let driver = std::thread::spawn(move || {
        ready.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if cooperative && driver_cancelled.load(Ordering::Relaxed) {
                return Ok(0);
            }
            match released.recv_timeout(Duration::from_millis(10)) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => return Ok(0),
                Err(RecvTimeoutError::Timeout) => {
                    if Instant::now() >= deadline {
                        return Err("synthetic UI worker was not released".into());
                    }
                }
            }
        }
    });
    let ready = started.recv_timeout(Duration::from_secs(10));
    let start = Instant::now();
    let timeout = Duration::from_millis(100);
    let cleanup = if cooperative {
        Duration::from_secs(10)
    } else {
        Duration::from_millis(100)
    };
    let result = wait_driver(&mut host, &driver, &cancelled, start + timeout, cleanup);
    let elapsed = start.elapsed();
    let finished = driver.is_finished();
    let status = host.child.try_wait();

    // Release and join even when the supervisor returned an unexpected result.
    drop(release);
    let worker = driver.join();
    host.stop()?;

    assert!(
        ready.is_ok(),
        "synthetic UI worker did not start: {ready:?}"
    );
    let error = result.unwrap_err();
    assert_eq!(
        error.to_string(),
        "UI test timed out (including UI Automation calls)"
    );
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(status?.is_some(), "supervisor did not reap the fixture");
    assert_eq!(finished, cooperative);
    assert!(worker.unwrap().is_ok(), "synthetic UI worker timed out");
    assert!(elapsed >= timeout, "supervisor returned early: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(5),
        "supervisor did not return promptly: {elapsed:?}"
    );
    if !cooperative {
        assert!(
            elapsed >= timeout + cleanup,
            "supervisor skipped worker cleanup: {elapsed:?}"
        );
    }
    host.remove_logs()
}

#[test]
fn child_panic_is_reported() -> Result<()> {
    let mut host = Host::spawn("panic")?;
    let error = host
        .wait(Instant::now() + Duration::from_secs(10))
        .unwrap_err();
    assert!(error.to_string().contains("fixture exited with"), "{error}");
    let output = host.output()?;
    assert!(output.contains("UI fixture panic probe stdout"));
    assert!(output.contains("UI fixture panic probe stderr"));
    host.remove_logs()
}

#[test]
fn hung_child_is_terminated() -> Result<()> {
    let mut host = Host::spawn("hang")?;
    let startup = Instant::now() + Duration::from_secs(10);
    while !fs::read_to_string(host.directory.join("stderr.txt"))?.contains("UI fixture hang probe")
    {
        assert!(
            host.child.try_wait()?.is_none() && Instant::now() < startup,
            "hang probe did not start: {}",
            host.output()?
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let error = host
        .wait(Instant::now() + Duration::from_millis(100))
        .unwrap_err();
    assert_eq!(error.to_string(), "fixture timed out");
    host.stop()?;
    assert!(host.child.try_wait()?.is_some());
    assert!(host.output()?.contains("UI fixture hang probe"));
    host.remove_logs()
}

#[test]
fn early_successful_exit_is_rejected() -> Result<()> {
    let serial = SERIAL.lock().unwrap();
    let mut host = Host::spawn("exit")?;
    let result = drive(&mut host, "exit", |ui| ui.value("input", ""));
    host.stop()?;
    drop(serial);
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("fixture exited before the UI test finished: exit code: 0"),
        "{error}"
    );
    host.remove_logs()
}
