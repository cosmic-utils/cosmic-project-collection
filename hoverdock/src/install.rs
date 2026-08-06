//! Putting the applet where cosmic-panel will find it, and taking it back out.
//!
//! Two things have to line up. The panel looks applets up by desktop-entry
//! *file stem*, so the file must be named exactly after the id; and the id must
//! appear in the dock's plugin list, which is a RON array in cosmic-config.
//! Neither is discoverable, so both are done here rather than in a README.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

/// The applet id. Also the desktop file's name, and what goes in the panel's
/// plugin list — all three have to agree.
pub const APPLET_ID: &str = "com.techy.HoverDock";

/// The applet this one stands in for. Replacing it in place keeps the dock's
/// order intact: launcher, workspaces, apps, us, minimised.
pub const REPLACES: &str = "com.system76.CosmicAppList";

const PANEL_CONFIG: &str = "com.system76.CosmicPanel.Dock";

pub fn desktop_file() -> PathBuf {
    data_home()
        .join("applications")
        .join(format!("{APPLET_ID}.desktop"))
}

fn plugins_path() -> PathBuf {
    config_home()
        .join("cosmic")
        .join(PANEL_CONFIG)
        .join("v1")
        .join("plugins_center")
}

/// Write the desktop entry. Idempotent: it is rewritten every time, so a moved
/// binary is picked up by running `--install` again.
pub fn write_desktop_entry() -> Result<PathBuf> {
    let exe = std::env::current_exe()
        .context("could not work out where this binary is")?
        .canonicalize()
        .context("could not resolve this binary's path")?;

    // X-CosmicApplet marks it as an applet; NoDisplay keeps it out of the
    // application menu, where a panel component has no business being.
    let entry = format!(
        "[Desktop Entry]\n\
         Name=Hover Dock\n\
         Comment=Pinned applications with hover magnification\n\
         Type=Application\n\
         Exec={exe}\n\
         Terminal=false\n\
         Categories=COSMIC;\n\
         Icon=com.system76.CosmicAppList\n\
         StartupNotify=true\n\
         NoDisplay=true\n\
         X-CosmicApplet=true\n\
         X-HostWaylandDisplay=true\n\
         X-OverflowPriority=100\n\
         X-OverflowMinSize=4\n",
        exe = exe.display()
    );

    let path = desktop_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, entry).with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}

/// Swap this applet in for COSMIC's own app list in the dock.
///
/// Returns false if it was already there, so running install twice is not an
/// error and does not report a change that did not happen.
pub fn enable() -> Result<bool> {
    let path = plugins_path();
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;

    if raw.contains(APPLET_ID) {
        return Ok(false);
    }
    if !raw.contains(REPLACES) {
        bail!(
            "{} does not list {REPLACES}, so there is nothing to replace. \
             Add \"{APPLET_ID}\" to it by hand.",
            path.display()
        );
    }

    backup(&path)?;
    let updated = raw.replace(REPLACES, APPLET_ID);
    std::fs::write(&path, updated)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(true)
}

/// Put COSMIC's own app list back.
pub fn disable() -> Result<bool> {
    let path = plugins_path();
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;

    if !raw.contains(APPLET_ID) {
        return Ok(false);
    }
    let updated = raw.replace(APPLET_ID, REPLACES);
    std::fs::write(&path, updated)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(true)
}

/// Keep one copy of the untouched plugin list, so there is a way back even if
/// the file is edited again afterwards.
fn backup(path: &std::path::Path) -> Result<()> {
    let backup = path.with_extension("hoverdock-backup");
    if !backup.exists() {
        std::fs::copy(path, &backup)
            .with_context(|| format!("could not back up {}", path.display()))?;
    }
    Ok(())
}

pub fn is_enabled() -> bool {
    std::fs::read_to_string(plugins_path())
        .map(|raw| raw.contains(APPLET_ID))
        .unwrap_or(false)
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local").join("share"))
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}
