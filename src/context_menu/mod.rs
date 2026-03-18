use std::path::Path;
use windows_registry::CURRENT_USER;

const FILE_SHELL_KEY: &str = r"Software\Classes\*\shell\MyPad";
const FILE_COMMAND_KEY: &str = r"Software\Classes\*\shell\MyPad\command";
const DIR_SHELL_KEY: &str = r"Software\Classes\Directory\shell\MyPad";
const DIR_COMMAND_KEY: &str = r"Software\Classes\Directory\shell\MyPad\command";

pub fn is_registered() -> bool {
    CURRENT_USER.open(FILE_SHELL_KEY).is_ok()
}

pub fn register(exe_path: &Path) -> Result<(), String> {
    let exe = exe_path.to_string_lossy();
    let icon_value = format!("\"{exe}\",0");

    let file_shell = CURRENT_USER
        .create(FILE_SHELL_KEY)
        .map_err(|e| format!("Failed to create file shell key: {e}"))?;
    file_shell
        .set_string("", "Open with MyPad")
        .map_err(|e| format!("Failed to set file shell value: {e}"))?;
    file_shell
        .set_string("Icon", &icon_value)
        .map_err(|e| format!("Failed to set file shell icon: {e}"))?;

    let file_cmd = CURRENT_USER
        .create(FILE_COMMAND_KEY)
        .map_err(|e| format!("Failed to create file command key: {e}"))?;
    file_cmd
        .set_string("", &format!("\"{exe}\" \"%1\""))
        .map_err(|e| format!("Failed to set file command value: {e}"))?;

    let dir_shell = CURRENT_USER
        .create(DIR_SHELL_KEY)
        .map_err(|e| format!("Failed to create directory shell key: {e}"))?;
    dir_shell
        .set_string("", "Open with MyPad")
        .map_err(|e| format!("Failed to set directory shell value: {e}"))?;
    dir_shell
        .set_string("Icon", &icon_value)
        .map_err(|e| format!("Failed to set directory shell icon: {e}"))?;

    let dir_cmd = CURRENT_USER
        .create(DIR_COMMAND_KEY)
        .map_err(|e| format!("Failed to create directory command key: {e}"))?;
    dir_cmd
        .set_string("", &format!("\"{exe}\" \"%V\""))
        .map_err(|e| format!("Failed to set directory command value: {e}"))?;

    Ok(())
}

pub fn unregister() -> Result<(), String> {
    remove_key_tree(FILE_SHELL_KEY)?;
    remove_key_tree(DIR_SHELL_KEY)?;
    Ok(())
}

fn remove_key_tree(parent: &str) -> Result<(), String> {
    CURRENT_USER
        .remove_tree(parent)
        .map_err(|e| format!("Failed to remove {parent}: {e}"))
}

pub fn current_exe_path() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("Failed to determine executable path: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_keys_use_current_user_hive() {
        assert!(FILE_SHELL_KEY.starts_with("Software\\Classes"));
        assert!(DIR_SHELL_KEY.starts_with("Software\\Classes"));
    }

    #[test]
    fn command_keys_are_children_of_shell_keys() {
        assert!(FILE_COMMAND_KEY.starts_with(FILE_SHELL_KEY));
        assert!(DIR_COMMAND_KEY.starts_with(DIR_SHELL_KEY));
    }

    #[test]
    fn current_exe_path_succeeds() {
        assert!(current_exe_path().is_ok());
    }

    #[test]
    fn register_and_unregister_round_trip() {
        let exe = current_exe_path().unwrap();
        register(&exe).expect("register should succeed");
        assert!(is_registered(), "should be registered after register()");

        unregister().expect("unregister should succeed");
        assert!(
            !is_registered(),
            "should not be registered after unregister()"
        );
    }
}
