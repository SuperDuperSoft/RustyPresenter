// Common Controls v6 is required (the window layer imports TaskDialogIndirect). Rust's std on windows-gnu
// embeds its own manifest too; the linker keeps this one (and warns ".rsrc merge failure: multiple non-default
// manifests"), so this one repeats std's settings.
const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <asmv3:application>
    <asmv3:windowsSettings xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">
      <longPathAware>true</longPathAware>
    </asmv3:windowsSettings>
  </asmv3:application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v2">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#;

fn main() {
    // Element ids stay in the generated code so the testing backend can find elements by id (check C4).
    let slint = slint_build::CompilerConfiguration::new().with_debug_info(true);
    slint_build::compile_with_config("ui/app.slint", slint).expect("slint build failed");
    let windows = tauri_build::WindowsAttributes::new().app_manifest(MANIFEST);
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("tauri build setup failed");
}
