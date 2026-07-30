//! XDG autostart

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use gtk4::glib;

const DESKTOP_FILE: &str = "dev.paloma.Paloma.desktop";

static AUTOSTART_DESKTOP_ENTRY: LazyLock<PathBuf> =
    LazyLock::new(|| glib::user_config_dir().join("autostart").join(DESKTOP_FILE));

pub(crate) fn enable() -> io::Result<()> {
    let entry = render_desktop_entry(&env::current_exe()?);
    let destination = AUTOSTART_DESKTOP_ENTRY.as_path();
    fs::create_dir_all(
        destination
            .parent()
            .expect("the autostart file always has a parent"),
    )?;
    fs::write(destination, entry)
}

pub(crate) fn disable() -> io::Result<()> {
    match fs::remove_file(AUTOSTART_DESKTOP_ENTRY.as_path()) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub(crate) fn is_enabled() -> io::Result<bool> {
    AUTOSTART_DESKTOP_ENTRY.try_exists()
}

fn render_desktop_entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Paloma\n\
         Exec={}\n\
         Icon=preferences-system\n\
         Terminal=false\n\
         StartupNotify=false\n",
        executable.display()
    )
}
