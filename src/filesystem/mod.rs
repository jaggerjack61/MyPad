use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

pub const SUPPORTED_FILE_EXTENSIONS: &[&str] = &[
    "rs", "md", "txt", "json", "toml", "yaml", "yml", "js", "ts", "html", "css",
    "php",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Directory {
        expanded: bool,
        loaded: bool,
        children: Vec<FileNode>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    pub name: String,
    pub path: PathBuf,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleNode {
    pub name: String,
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

pub struct WorkspaceWatcher {
    receiver: Receiver<Result<notify::Event, notify::Error>>,
    _watcher: RecommendedWatcher,
}

pub fn build_tree(root: &Path) -> io::Result<FileNode> {
    build_root_node(root)
}

pub fn expand_directory(tree: &mut FileNode, target: &Path) -> io::Result<bool> {
    if tree.path == target {
        if let NodeKind::Directory {
            expanded,
            loaded,
            children,
        } = &mut tree.kind
        {
            if !*loaded {
                *children = load_children(&tree.path)?;
                *loaded = true;
                *expanded = true;
            } else {
                *expanded = !*expanded;
            }

            return Ok(true);
        }

        return Ok(false);
    }

    if let NodeKind::Directory { children, .. } = &mut tree.kind {
        for child in children {
            if expand_directory(child, target)? {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

pub fn visible_nodes(tree: &FileNode) -> Vec<VisibleNode> {
    let mut nodes = Vec::new();
    collect_visible(tree, 0, &mut nodes);
    nodes
}

pub fn read_text_file(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

pub fn save_text_file(path: &Path, contents: &str) -> io::Result<()> {
    fs::write(path, contents)
}

pub fn supported_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|extension| {
            SUPPORTED_FILE_EXTENSIONS
                .iter()
                .any(|supported| supported.eq_ignore_ascii_case(extension))
        })
}

pub fn refresh_tree(tree: &mut FileNode) -> io::Result<()> {
    if let NodeKind::Directory {
        expanded,
        loaded,
        children,
    } = &mut tree.kind
    {
        if !*loaded {
            return Ok(());
        }

        let refreshed_children = load_children(&tree.path)?;
        let old_children = std::mem::take(children);
        let mut merged = Vec::with_capacity(refreshed_children.len());

        for mut refreshed in refreshed_children {
            if let Some(existing) = old_children.iter().find(|node| node.path == refreshed.path) {
                if let (
                    NodeKind::Directory {
                        expanded: old_expanded,
                        loaded: old_loaded,
                        children: old_children,
                    },
                    NodeKind::Directory {
                        expanded: new_expanded,
                        loaded: new_loaded,
                        children: new_children,
                    },
                ) = (&existing.kind, &mut refreshed.kind)
                {
                    *new_expanded = *old_expanded;
                    *new_loaded = *old_loaded;
                    *new_children = old_children.clone();

                    if *new_loaded {
                        refresh_tree(&mut refreshed)?;
                    }
                }
            }

            merged.push(refreshed);
        }

        *children = merged;

        if *expanded {
            for child in children {
                refresh_tree(child)?;
            }
        }
    }

    Ok(())
}

impl WorkspaceWatcher {
    pub fn watch(path: &Path) -> notify::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;

        let mode = if path.is_dir() {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        watcher.watch(path, mode)?;

        Ok(Self {
            receiver,
            _watcher: watcher,
        })
    }

    pub fn drain(&self) -> Vec<Result<notify::Event, notify::Error>> {
        self.receiver.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{build_tree, expand_directory, visible_nodes};
    use std::fs;

    #[test]
    fn build_tree_sorts_directories_before_files() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let root = temp_dir.path();

        fs::create_dir(root.join("src")).expect("src dir");
        fs::write(root.join("notes.md"), "# hello").expect("notes");
        fs::write(root.join("image.png"), "bin").expect("image");

        let tree = build_tree(root).expect("tree");
        let visible = visible_nodes(&tree);
        let names: Vec<_> = visible.into_iter().map(|node| node.name).collect();

        assert_eq!(
            names,
            vec![
                root.file_name().unwrap().to_string_lossy().to_string(),
                "src".to_string(),
                "image.png".to_string(),
                "notes.md".to_string(),
            ]
        );
    }

    #[test]
    fn build_tree_keeps_nested_folders_closed_by_default() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let root = temp_dir.path();
        let docs = root.join("docs");

        fs::create_dir(&docs).expect("docs dir");
        fs::write(docs.join("guide.md"), "# guide").expect("guide");

        let tree = build_tree(root).expect("tree");
        let visible = visible_nodes(&tree);
        let names: Vec<_> = visible.into_iter().map(|node| node.name).collect();

        assert!(names.contains(&"docs".to_string()));
        assert!(!names.contains(&"guide.md".to_string()));
    }

    #[test]
    fn expand_directory_toggles_nested_folder_visibility() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let root = temp_dir.path();
        let docs = root.join("docs");

        fs::create_dir(&docs).expect("docs dir");
        fs::write(docs.join("guide.md"), "# guide").expect("guide");

        let mut tree = build_tree(root).expect("tree");
        let expanded = expand_directory(&mut tree, &docs).expect("expand");

        assert!(expanded);

        let visible = visible_nodes(&tree);
        let names: Vec<_> = visible.into_iter().map(|node| node.name).collect();
        assert!(names.contains(&"guide.md".to_string()));

        let collapsed = expand_directory(&mut tree, &docs).expect("collapse");

        assert!(collapsed);

        let collapsed_visible = visible_nodes(&tree);
        let collapsed_names: Vec<_> = collapsed_visible
            .into_iter()
            .map(|node| node.name)
            .collect();
        assert!(!collapsed_names.contains(&"guide.md".to_string()));
    }

    #[test]
    fn build_tree_shows_files_inside_nested_folders_after_expanding_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let root = temp_dir.path();
        let src = root.join("src");
        let editor = src.join("editor");

        fs::create_dir(&src).expect("src dir");
        fs::create_dir(&editor).expect("editor dir");
        fs::write(editor.join("mod.rs"), "pub fn render() {}\n").expect("mod file");

        let mut tree = build_tree(root).expect("tree");

        expand_directory(&mut tree, &src).expect("expand src");
        expand_directory(&mut tree, &editor).expect("expand editor");

        let visible = visible_nodes(&tree);
        let names: Vec<_> = visible.into_iter().map(|node| node.name).collect();

        assert!(names.contains(&"mod.rs".to_string()));
    }

    #[test]
    fn build_tree_shows_php_files_after_expanding_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let root = temp_dir.path();
        let app = root.join("app");
        let api = app.join("Api");

        fs::create_dir(&app).expect("app dir");
        fs::create_dir(&api).expect("api dir");
        fs::write(api.join("FinanceController.php"), "<?php\n").expect("php file");

        let mut tree = build_tree(root).expect("tree");

        expand_directory(&mut tree, &app).expect("expand app");
        expand_directory(&mut tree, &api).expect("expand api");

        let visible = visible_nodes(&tree);
        let names: Vec<_> = visible.into_iter().map(|node| node.name).collect();

        assert!(names.contains(&"FinanceController.php".to_string()));
    }
}

fn collect_visible(node: &FileNode, depth: usize, output: &mut Vec<VisibleNode>) {
    let (is_dir, expanded) = match &node.kind {
        NodeKind::File => (false, false),
        NodeKind::Directory { expanded, .. } => (true, *expanded),
    };

    output.push(VisibleNode {
        name: node.name.clone(),
        path: node.path.clone(),
        depth,
        is_dir,
        expanded,
    });

    if let NodeKind::Directory {
        expanded: true,
        children,
        ..
    } = &node.kind
    {
        for child in children {
            collect_visible(child, depth + 1, output);
        }
    }
}

fn build_root_node(path: &Path) -> io::Result<FileNode> {
    Ok(FileNode {
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .to_string(),
        path: path.to_path_buf(),
        kind: NodeKind::Directory {
            expanded: true,
            loaded: true,
            children: load_children(path)?,
        },
    })
}

fn build_directory_node(path: &Path) -> io::Result<FileNode> {
    Ok(FileNode {
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .to_string(),
        path: path.to_path_buf(),
        kind: NodeKind::Directory {
            expanded: false,
            loaded: true,
            children: load_children(path)?,
        },
    })
}

fn load_children(path: &Path) -> io::Result<Vec<FileNode>> {
    let mut children = fs::read_dir(path)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let metadata = entry.metadata().ok()?;

            if metadata.is_dir() {
                build_directory_node(&path).ok()
            } else {
                Some(FileNode {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path,
                    kind: NodeKind::File,
                })
            }
        })
        .collect::<Vec<_>>();

    children.sort_by(|left, right| match (&left.kind, &right.kind) {
        (NodeKind::Directory { .. }, NodeKind::File) => std::cmp::Ordering::Less,
        (NodeKind::File, NodeKind::Directory { .. }) => std::cmp::Ordering::Greater,
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    });

    Ok(children)
}