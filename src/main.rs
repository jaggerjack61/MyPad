#![windows_subsystem = "windows"]

fn main() -> iced::Result {
    let initial_path = std::env::args().nth(1).map(std::path::PathBuf::from);
    mypad::ui::run(initial_path)
}
