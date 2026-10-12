//! Native workspace actions. Paths are arguments, never shell scripts.

use std::io;
use std::path::Path;
use std::process::Command;

pub const FILE_MANAGER: &str = if cfg!(target_os = "macos") {
    "Finder"
} else if cfg!(windows) {
    "File Explorer"
} else {
    "file manager"
};

pub fn open_directory(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if result <= 32 {
            return Err(io::Error::other(format!(
                "File Explorer could not open the folder ({result})."
            )));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let program = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        Command::new(program).arg(path).spawn().map(|_| ())
    }
}

pub fn open_terminal(path: &Path) -> io::Result<()> {
    terminal_command(path).spawn().map(|_| ())
}

fn terminal_command(path: &Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let home = crate::home_dir();
        let search_path = crate::search_path(home.as_deref());
        if let Some(wt) = crate::find_executable("wt.exe", &search_path) {
            let mut command = Command::new(wt);
            command.arg("-d").arg(path);
            return command;
        }
        let mut command = Command::new(crate::command_shell());
        command
            .arg("/D")
            .current_dir(path)
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE);
        command
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.args(["-a", "Terminal"]).arg(path);
        command
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let mut command = Command::new("x-terminal-emulator");
        command.current_dir(path);
        command
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn opening_terminal_keeps_workspace_as_a_literal_argument() {
        let path = Path::new("/workspace with spaces/$literal;name");
        let command = terminal_command(path);
        assert_eq!(command.get_program(), "open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["-a", "Terminal", path.to_str().unwrap()]
        );
    }
}
