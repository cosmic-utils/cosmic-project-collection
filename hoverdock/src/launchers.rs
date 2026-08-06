//! What sits in the dock: desktop entries, their icons, and how to start them.
//!
//! The `.desktop` format is a small INI dialect and we read it directly rather
//! than through a crate — the parts a dock needs are Name, Icon, Exec and
//! StartupWMClass, and hand-reading them means locale handling and the `%f`
//! placeholders behave exactly as we intend.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One of an application's own menu entries — "New Window", "New Incognito
/// Window". Every desktop file may declare as many as it likes.
#[derive(Debug, Clone)]
pub struct DesktopAction {
    pub name: String,
    pub exec: String,
}

/// One launcher in the dock.
#[derive(Debug, Clone)]
pub struct Launcher {
    /// Desktop entry id — the file name without `.desktop`.
    pub id: String,
    pub name: String,
    /// Icon name or absolute path, as written in the entry.
    pub icon: Option<String>,
    /// `Exec=` with the field codes still in it.
    pub exec: String,
    /// What the application calls its windows, when it says so. Used to tell
    /// whether it is running.
    pub startup_class: Option<String>,
    pub terminal: bool,
    pub path: PathBuf,
    /// The application's own actions, in the order it lists them.
    pub actions: Vec<DesktopAction>,
}

impl Launcher {
    /// Read one `.desktop` file. `None` for entries that should not be shown.
    pub fn from_file(path: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(path).ok()?;
        let fields = desktop_entry_section(&raw)?;

        if truthy(fields.get("NoDisplay")) || truthy(fields.get("Hidden")) {
            return None;
        }
        if fields.get("Type").map(String::as_str) != Some("Application") {
            return None;
        }

        let exec = fields.get("Exec")?.trim().to_string();
        if exec.is_empty() {
            return None;
        }

        let id = path.file_stem()?.to_string_lossy().to_string();
        let actions = parse_actions(&raw, fields.get("Actions").map(String::as_str));
        Some(Self {
            name: localised(&fields, "Name").unwrap_or_else(|| id.clone()),
            icon: fields.get("Icon").map(|i| i.trim().to_string()),
            exec,
            startup_class: fields.get("StartupWMClass").map(|c| c.trim().to_string()),
            terminal: truthy(fields.get("Terminal")),
            id,
            path: path.to_path_buf(),
            actions,
        })
    }

    /// Does this launcher own a window with this app id?
    pub fn matches_app_id(&self, app_id: &str) -> bool {
        let app_id = app_id.trim();
        if app_id.is_empty() {
            return false;
        }
        if let Some(class) = &self.startup_class {
            if class.eq_ignore_ascii_case(app_id) {
                return true;
            }
        }
        if self.id.eq_ignore_ascii_case(app_id) {
            return true;
        }
        // Toolkits report `org.example.App` where the entry is `App.desktop`,
        // and the other way round.
        let tail = |s: &str| s.rsplit('.').next().unwrap_or(s).to_string();
        tail(&self.id).eq_ignore_ascii_case(&tail(app_id))
    }

    /// The command line to run, with the field codes stripped.
    ///
    /// A dock never opens a file, so every `%f`/`%u`/`%F`/`%U` is dropped
    /// rather than substituted; `%%` is a literal percent.
    pub fn command(&self) -> Vec<String> {
        self.command_for(&self.exec)
    }

    /// The same, for one of the application's own actions.
    pub fn command_for(&self, exec: &str) -> Vec<String> {
        let mut out = Vec::new();
        for token in split_exec(exec) {
            match token.as_str() {
                "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
                "%i" => continue,
                "%c" => out.push(self.name.clone()),
                "%k" => out.push(self.path.to_string_lossy().to_string()),
                other => out.push(other.replace("%%", "%")),
            }
        }
        out
    }

    /// Start the application, detached from the dock.
    ///
    /// The child must not die with us and must not inherit our streams — a
    /// dock that takes its applications down when it restarts is worse than no
    /// dock.
    pub fn launch(&self) -> Result<()> {
        self.run(&self.command())
    }

    /// Run one of the application's own actions.
    pub fn launch_action(&self, index: usize) -> Result<()> {
        let action = self
            .actions
            .get(index)
            .ok_or_else(|| anyhow!("{} has no action {index}", self.id))?;
        self.run(&self.command_for(&action.exec))
    }

    fn run(&self, command: &[String]) -> Result<()> {
        let mut command = command.to_vec();
        if command.is_empty() {
            return Err(anyhow!("{} has an empty Exec line", self.id));
        }

        let program = command.remove(0);
        std::process::Command::new(&program)
            .args(&command)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("could not start {program}"))?;
        Ok(())
    }
}

/// Find the entry a pinned id refers to.
///
/// COSMIC's favourites file holds ids like `Docker Desktop`, `Termius` and
/// `UniPersonal.Desktop`, none of which are the file name of the entry that
/// installs them. Matching only on the exact id silently drops those icons, so
/// the id is compared four ways before giving up.
pub fn find<'a>(installed: &'a HashMap<String, Launcher>, id: &str) -> Option<&'a Launcher> {
    if let Some(launcher) = installed.get(id) {
        return Some(launcher);
    }

    let wanted = squash(id);
    installed
        .values()
        .find(|l| l.id.eq_ignore_ascii_case(id))
        .or_else(|| installed.values().find(|l| squash(&l.id) == wanted))
        .or_else(|| installed.values().find(|l| squash(&l.name) == wanted))
        .or_else(|| {
            // `UniPersonal.Desktop` → `unipersonal`: some launchers write the
            // file name including a suffix that is not part of the id.
            let trimmed = squash(id.trim_end_matches(".Desktop").trim_end_matches(".desktop"));
            installed
                .values()
                .find(|l| squash(&l.id) == trimmed || squash(&l.name) == trimmed)
        })
        .or_else(|| {
            // Last resort, for ids like `VirtualBox Manager` whose entry is
            // simply `virtualbox`: one has to be the beginning of the other,
            // and the longest such match wins so `code` cannot swallow
            // `codeblocks`.
            installed
                .values()
                .filter_map(|launcher| {
                    let candidates = [squash(&launcher.id), squash(&launcher.name)];
                    let best = candidates
                        .iter()
                        .filter(|c| c.len() >= 4)
                        .filter(|c| wanted.starts_with(*c) || c.starts_with(&wanted))
                        .map(String::len)
                        .max()?;
                    Some((best, launcher))
                })
                .max_by_key(|(len, _)| *len)
                .map(|(_, launcher)| launcher)
        })
}

/// Lower-case letters and digits only, so `Docker Desktop`, `docker-desktop`
/// and `Docker_Desktop` all compare equal.
fn squash(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Every application on the system, by desktop entry id.
pub fn installed() -> HashMap<String, Launcher> {
    let mut found = HashMap::new();

    // Later directories must not override earlier ones: the user's own copy of
    // an entry wins over the system's.
    for dir in application_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            if let Some(launcher) = Launcher::from_file(&path) {
                found.entry(launcher.id.clone()).or_insert(launcher);
            }
        }
    }

    found
}

fn application_dirs() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut dirs = Vec::new();

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(data_home).join("applications"));
    } else {
        dirs.push(home.join(".local/share/applications"));
    }

    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    dirs.extend(data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("applications")));

    // Flatpak and snap put theirs outside XDG_DATA_DIRS on some systems.
    dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));

    dirs
}

/// What COSMIC's own dock is pinned to, so a fresh install of TechyDock shows
/// the applications the user already put there.
///
/// The file is RON, but every version of it so far is a list of quoted strings,
/// and reading it that way means a format change cannot crash the dock.
/// Where COSMIC keeps the dock's pinned applications.
///
/// Writing back to this file rather than to a list of our own is deliberate: it
/// is the same list `cosmic-app-list` reads, so unpinning something here is
/// still unpinned after switching back to the stock applet. Two docks with two
/// opinions about what is pinned would be worse than no menu at all.
pub fn favourites_path() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".config/cosmic/com.system76.CosmicAppList/v1/favorites")
}

/// Write the pinned list back in the RON shape cosmic-config wrote it in.
pub fn write_favourites(ids: &[String]) -> std::io::Result<()> {
    let path = favourites_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut out = String::from("[\n");
    for id in ids {
        // The ids come from this same file or from desktop entry names; a quote
        // in one would be a malformed entry, but escaping costs nothing.
        out.push_str("    \"");
        out.push_str(&id.replace('\\', "\\\\").replace('"', "\\\""));
        out.push_str("\",\n");
    }
    out.push(']');
    std::fs::write(&path, out)
}

pub fn cosmic_favourites() -> Vec<String> {
    let candidates = [
        favourites_path(),
        PathBuf::from("/usr/share/cosmic/com.system76.CosmicAppList/v1/favorites"),
    ];

    for path in candidates {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let ids = quoted_strings(&raw);
        if !ids.is_empty() {
            log::info!("pinned {} applications from {}", ids.len(), path.display());
            return ids;
        }
    }

    Vec::new()
}

/// Pull every double-quoted string out of a blob of text.
fn quoted_strings(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut value = String::new();
        for c in chars.by_ref() {
            if c == '"' {
                break;
            }
            value.push(c);
        }
        if !value.is_empty() {
            out.push(value);
        }
    }
    out
}

/// The `[Desktop Action <id>]` groups, in the order `Actions=` lists them.
///
/// The order matters: it is the order the application intends its menu to be
/// read in, and reordering someone else's menu is not a dock's job.
fn parse_actions(raw: &str, declared: Option<&str>) -> Vec<DesktopAction> {
    let mut groups: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current: Option<String> = None;

    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = header
                .strip_prefix("Desktop Action ")
                .map(|id| id.trim().to_string());
            if let Some(id) = &current {
                groups.entry(id.clone()).or_default();
            }
            continue;
        }
        let Some(id) = &current else { continue };
        if let Some((key, value)) = line.split_once('=') {
            groups
                .entry(id.clone())
                .or_default()
                .insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    // `Actions=` is the declared order; anything not declared is ignored, as
    // the specification says.
    declared
        .unwrap_or_default()
        .split(';')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .filter_map(|id| {
            let fields = groups.get(id)?;
            let exec = fields.get("Exec")?.trim().to_string();
            if exec.is_empty() {
                return None;
            }
            Some(DesktopAction {
                name: localised(fields, "Name").unwrap_or_else(|| id.to_string()),
                exec,
            })
        })
        .collect()
}

/// The `[Desktop Entry]` group as key/value pairs. Other groups — the actions —
/// are deliberately ignored.
fn desktop_entry_section(raw: &str) -> Option<HashMap<String, String>> {
    let mut fields = HashMap::new();
    let mut inside = false;

    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            inside = line.eq_ignore_ascii_case("[Desktop Entry]");
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    inside_or_found(fields)
}

fn inside_or_found(fields: HashMap<String, String>) -> Option<HashMap<String, String>> {
    (!fields.is_empty()).then_some(fields)
}

/// `Name[sv]` when the session is Swedish, `Name` otherwise.
fn localised(fields: &HashMap<String, String>, key: &str) -> Option<String> {
    for locale in locales() {
        if let Some(value) = fields.get(&format!("{key}[{locale}]")) {
            return Some(value.clone());
        }
    }
    fields.get(key).cloned()
}

/// The session's languages, most specific first: `sv_SE.UTF-8` also matches
/// `Name[sv_SE]` and `Name[sv]`.
fn locales() -> Vec<String> {
    let raw = std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();

    let base = raw.split('.').next().unwrap_or("").trim().to_string();
    if base.is_empty() || base == "C" || base == "POSIX" {
        return Vec::new();
    }

    match base.split_once('_') {
        Some((language, _)) => vec![base.clone(), language.to_string()],
        None => vec![base],
    }
}

fn truthy(value: Option<&String>) -> bool {
    value.is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}

/// Split an `Exec` line the way the spec says: quoted arguments stay together.
fn split_exec(exec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;

    for c in exec.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn reads_a_normal_entry() {
        let dir = tempdir();
        let path = write(
            dir.path(),
            "org.example.Editor.desktop",
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Editor\n\
             Name[sv]=Redigerare\n\
             Icon=accessories-text-editor\n\
             Exec=editor %U\n\
             StartupWMClass=Editor\n",
        );

        let launcher = Launcher::from_file(&path).expect("should parse");
        assert_eq!(launcher.id, "org.example.Editor");
        assert_eq!(launcher.icon.as_deref(), Some("accessories-text-editor"));
        assert_eq!(launcher.startup_class.as_deref(), Some("Editor"));
        // The file placeholder must not survive into the command line.
        assert_eq!(launcher.command(), vec!["editor"]);
    }

    #[test]
    fn entries_that_ask_to_be_hidden_are_hidden() {
        let dir = tempdir();
        let hidden = write(
            dir.path(),
            "hidden.desktop",
            "[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n",
        );
        assert!(Launcher::from_file(&hidden).is_none());

        let link = write(
            dir.path(),
            "link.desktop",
            "[Desktop Entry]\nType=Link\nName=X\nURL=https://example.com\n",
        );
        assert!(Launcher::from_file(&link).is_none());
    }

    #[test]
    fn only_the_desktop_entry_group_is_read() {
        let dir = tempdir();
        let path = write(
            dir.path(),
            "app.desktop",
            "[Desktop Entry]\nType=Application\nName=App\nExec=app\n\
             [Desktop Action New]\nName=New Window\nExec=app --new\n",
        );
        let launcher = Launcher::from_file(&path).unwrap();
        assert_eq!(launcher.command(), vec!["app"]);
        assert_eq!(launcher.name, "App");
    }

    #[test]
    fn an_applications_own_actions_are_read_in_its_own_order() {
        let dir = tempdir();
        // Trimmed from a real browser entry — this is where "New Incognito
        // Window" comes from.
        let path = write(
            dir.path(),
            "browser.desktop",
            "[Desktop Entry]\n\
             Type=Application\nName=Browser\nExec=browser %U\n\
             Actions=new-window;new-private-window;undeclared;\n\n\
             [Desktop Action new-private-window]\n\
             Name=New Incognito Window\n\
             Name[sv]=Nytt inkognitofönster\n\
             Exec=browser --incognito %U\n\n\
             [Desktop Action new-window]\n\
             Name=New Window\n\
             Exec=browser --new-window %U\n",
        );

        let launcher = Launcher::from_file(&path).expect("should parse");
        let names: Vec<&str> = launcher.actions.iter().map(|a| a.name.as_str()).collect();
        // Declared order, not file order, and the undeclared group is ignored.
        assert_eq!(names.len(), 2, "got {names:?}");
        assert_eq!(names[0], "New Window");
        assert!(names[1].contains("ncognito") || names[1].contains("nkognito"));

        // And the action's command is stripped of field codes like any other.
        assert_eq!(
            launcher.command_for(&launcher.actions[1].exec),
            vec!["browser", "--incognito"]
        );
    }

    #[test]
    fn an_entry_without_actions_simply_has_none() {
        let dir = tempdir();
        let path = write(
            dir.path(),
            "plain.desktop",
            "[Desktop Entry]\nType=Application\nName=Plain\nExec=plain\n",
        );
        assert!(Launcher::from_file(&path).unwrap().actions.is_empty());
    }

    #[test]
    fn quoted_arguments_survive_the_split() {
        assert_eq!(
            split_exec(r#"/usr/bin/app --flag "two words" %U"#),
            vec!["/usr/bin/app", "--flag", "two words", "%U"]
        );
    }

    #[test]
    fn a_literal_percent_is_not_a_field_code() {
        let launcher = Launcher {
            id: "x".into(),
            name: "X".into(),
            icon: None,
            exec: "app --format=100%% %f".into(),
            startup_class: None,
            terminal: false,
            path: PathBuf::from("/tmp/x.desktop"),
            actions: Vec::new(),
        };
        assert_eq!(launcher.command(), vec!["app", "--format=100%"]);
    }

    #[test]
    fn windows_are_matched_to_their_launcher() {
        let launcher = Launcher {
            id: "org.example.Editor".into(),
            name: "Editor".into(),
            icon: None,
            exec: "editor".into(),
            startup_class: Some("Editor".into()),
            terminal: false,
            path: PathBuf::from("/tmp/e.desktop"),
            actions: Vec::new(),
        };

        assert!(launcher.matches_app_id("Editor"));
        assert!(launcher.matches_app_id("editor"), "case must not matter");
        assert!(launcher.matches_app_id("org.example.Editor"));
        assert!(!launcher.matches_app_id("org.example.Other"));
        assert!(!launcher.matches_app_id(""));
    }

    #[test]
    fn a_pinned_id_is_matched_even_when_it_is_not_the_file_name() {
        let mut installed = HashMap::new();
        for (id, name) in [
            ("docker-desktop", "Docker Desktop"),
            ("com.termius.Termius", "Termius"),
            ("md.obsidian.Obsidian", "Obsidian"),
            ("unipersonal", "Uni Personal"),
        ] {
            installed.insert(
                id.to_string(),
                Launcher {
                    id: id.into(),
                    name: name.into(),
                    icon: None,
                    exec: "x".into(),
                    startup_class: None,
                    terminal: false,
                    path: PathBuf::from("/tmp/x.desktop"),
                    actions: Vec::new(),
                },
            );
        }

        // Exactly the ids COSMIC's own favourites file uses.
        assert_eq!(find(&installed, "Docker Desktop").unwrap().id, "docker-desktop");
        assert_eq!(find(&installed, "Termius").unwrap().id, "com.termius.Termius");
        assert_eq!(find(&installed, "obsidian").unwrap().id, "md.obsidian.Obsidian");
        assert_eq!(find(&installed, "UniPersonal.Desktop").unwrap().id, "unipersonal");
        assert!(find(&installed, "not-installed-at-all").is_none());

        // `VirtualBox Manager` is pinned, but the entry is just `virtualbox`.
        installed.insert(
            "virtualbox".into(),
            Launcher {
                id: "virtualbox".into(),
                name: "Oracle VirtualBox".into(),
                icon: None,
                exec: "x".into(),
                startup_class: None,
                terminal: false,
                path: PathBuf::from("/tmp/x.desktop"),
                actions: Vec::new(),
            },
        );
        assert_eq!(find(&installed, "VirtualBox Manager").unwrap().id, "virtualbox");
        // But a prefix match must not be greedy enough to grab the wrong app.
        assert!(find(&installed, "vim").is_none(), "three letters is not a match");
    }

    #[test]
    fn cosmics_favourites_are_read_out_of_their_ron() {
        // Trailing comma and all — this is not valid JSON, and must still work.
        let raw = "[\n    \"com.system76.CosmicFiles\",\n    \"vivaldi-stable\",\n]";
        assert_eq!(
            quoted_strings(raw),
            vec!["com.system76.CosmicFiles", "vivaldi-stable"]
        );
        assert!(quoted_strings("").is_empty());
    }

    /// A tiny stand-in so the tests do not need a dev-dependency.
    fn tempdir() -> TempDir {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "techydock-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
