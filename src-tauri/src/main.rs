// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // P7.5 双模式（FR-26）：带子命令 = 无头 CLI（AI/脚本操控实例生命周期），
    // 无参数 = GUI（原路径零变化）。release 是 windows 子系统，CLI 模式在
    // cli::main 里自行挂接父进程控制台。
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    if !cli_args.is_empty() {
        std::process::exit(dsh_launcher_lib::cli::main(cli_args));
    }
    dsh_launcher_lib::run()
}
