#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod runtime;
use std::{path::PathBuf, sync::Mutex};
use tauri::Manager;

fn main() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let dev_root = if cfg!(debug_assertions) {
                std::env::var_os("STUDY_ROOT")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::current_exe().ok().and_then(|exe| {
                            exe.ancestors()
                                .find(|p| {
                                    p.join("web/dist/index.html").is_file()
                                        && p.join("server/target/debug/study-core").is_file()
                                })
                                .map(|p| p.to_path_buf())
                        })
                    })
            } else {
                None
            };
            let (root, binary) = if let Some(root) = dev_root {
                let binary = root.join("server/target/debug").join(if cfg!(windows) {
                    "study-core.exe"
                } else {
                    "study-core"
                });
                (root, binary)
            } else {
                let root = app.path().resource_dir()?.join("runtime");
                let binary = root.join("bin").join(if cfg!(windows) {
                    "study-core.exe"
                } else {
                    "study-core"
                });
                (root, binary)
            };
            let data = std::env::var_os("STUDY_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or(if cfg!(debug_assertions) {
                    root.join("data")
                } else {
                    app.path().app_data_dir()?
                });
            let (core, address) = runtime::Core::start(&binary, &root, &data)?;
            app.manage(Mutex::new(Some(core)));
            let url = url::Url::parse(&address)?;
            let expected_origin = url.origin();
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
                .title("真题学习")
                .inner_size(1280.0, 860.0)
                .min_inner_size(900.0, 600.0)
                // This web UI has no Tauri IPC grants and cannot navigate to another origin.
                .on_navigation(move |url| url.origin() == expected_origin)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Could not start Study desktop; inspect core.log in the app data directory");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(core) = handle.try_state::<Mutex<Option<runtime::Core>>>() {
                if let Ok(mut core) = core.lock() {
                    core.take();
                }
            }
        }
    });
}
