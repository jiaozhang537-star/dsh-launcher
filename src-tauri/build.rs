fn main() {
    // tauri-build 默认经 winres 只给「bin」嵌 Common-Controls v6 manifest（.res 资源），
    // lib 测试二进制链接不到那份 .res——而 tauri/tao/muda 的静态导入里有 v6 独有的
    // TaskDialogIndirect，测试 exe 没有 manifest 时加载器在 System32 comctl32 5.82
    // 上解析失败（STATUS_ENTRYPOINT_NOT_FOUND）。
    //
    // 统一 manifest 来源：关闭 tauri-build 的 .res manifest，改由 /MANIFESTINPUT
    // 给本 crate 的所有可执行目标（bin 与 test）提供同一份 manifest，避免重复资源。
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let manifest_path = std::path::Path::new(&out_dir).join("app-common-controls.manifest");
    std::fs::write(
        &manifest_path,
        r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#,
    )
    .unwrap();

    #[cfg(windows)]
    {
        use tauri_build::{Attributes, WindowsAttributes};
        tauri_build::try_build(
            Attributes::new().windows_attributes(WindowsAttributes::new_without_app_manifest()),
        )
        .expect("failed to run tauri-build");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            manifest_path.display()
        );
    }

    #[cfg(not(windows))]
    {
        tauri_build::build();
    }
}
