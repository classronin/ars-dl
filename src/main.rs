mod batch;
mod single;
mod utils;

use std::env;

#[tokio::main]
async fn main() {
    // 在 Windows 上启用 ANSI 转义序列支持
    #[cfg(windows)]
    enable_ansi_support();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: ars-dl <URL> [save_name]");
        std::process::exit(1);
    }
    let url = &args[1];
    let save_name = args.get(2).map(|s| s.as_str());

    if url.contains('[') && url.contains(']') {
        batch::run(url, save_name).await;
    } else {
        single::run(url, save_name).await;
    }
}

#[cfg(windows)]
fn enable_ansi_support() {
    use winapi::um::consoleapi::{GetConsoleMode, SetConsoleMode};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::processenv::GetStdHandle;
    use winapi::um::winbase::STD_OUTPUT_HANDLE;
    use winapi::um::wincon::ENABLE_VIRTUAL_TERMINAL_PROCESSING;

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle != INVALID_HANDLE_VALUE {
            let mut mode = 0;
            if GetConsoleMode(handle, &mut mode) != 0 {
                SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}