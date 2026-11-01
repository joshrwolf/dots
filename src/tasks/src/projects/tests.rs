use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use crate::{Error, Project, Store};

struct Fixture {
    root: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dots-projects-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self {
            root,
            store: Store::initialize(Connection::open_in_memory().unwrap()).unwrap(),
        }
    }
    fn directory(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn catalog_creation_refuses_to_replace_an_existing_file() {
    let f = Fixture::new();
    let path = f.root.join("workspace.sqlite3");
    fs::write(&path, b"untouched").unwrap();
    assert!(Store::create(&path).is_err());
    assert_eq!(fs::read(path).unwrap(), b"untouched");
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
}

#[test]
fn identities_are_explicit_and_names_do_not_merge_projects() {
    let mut f = Fixture::new();
    let common = f.directory("repo/.git");
    assert!(
        f.store
            .project_for_common_directory(&common)
            .unwrap()
            .is_none()
    );
    let one = f.store.register_project("same name").unwrap();
    let two = f.store.register_project("same name").unwrap();
    assert_ne!(one, two);
    assert!(one.as_str().starts_with("p-"));
    assert_eq!(one.as_str().len(), 34);
    f.store.bind_project(&one, &common).unwrap();
    f.store.bind_project(&one, &common).unwrap();
    assert_eq!(
        f.store.project_for_common_directory(&common).unwrap(),
        Some(one.clone())
    );
    assert!(matches!(
        f.store.bind_project(&two, &common),
        Err(Error::BindingConflict { .. })
    ));
    assert_eq!(
        f.store.registered_project(&one).unwrap().common_directories,
        [common]
    );
    assert_eq!(
        f.store.registered_project(&two).unwrap().common_directories,
        Vec::<PathBuf>::new()
    );
}

#[test]
fn common_directories_distinguish_nested_submodules_and_separate_clones() {
    let mut f = Fixture::new();
    let paths = [
        f.directory("repo/.git"),
        f.directory("repo/.git/modules/submodule"),
        f.directory("clone/.git"),
        f.directory("bare.git"),
    ];
    let mut ids = Vec::new();
    for path in &paths {
        let project = f.store.register_project("project").unwrap();
        f.store.bind_project(&project, path).unwrap();
        ids.push(project);
    }
    for (path, project) in paths.iter().zip(&ids) {
        assert_eq!(
            f.store.project_for_common_directory(path).unwrap().as_ref(),
            Some(project)
        );
    }
    assert!(ids.windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
fn rebinding_requires_the_exact_old_binding_and_does_not_steal() {
    let mut f = Fixture::new();
    let old = f.directory("old/.git");
    let moved = f.root.canonicalize().unwrap().join("moved/.git");
    let other = f.directory("other/.git");
    let one = f.store.register_project("one").unwrap();
    let two = f.store.register_project("two").unwrap();
    f.store.bind_project(&one, &old).unwrap();
    f.store.bind_project(&two, &other).unwrap();
    fs::rename(old.parent().unwrap(), moved.parent().unwrap()).unwrap();
    f.store.rebind_project(&one, &old, &moved).unwrap();
    assert_eq!(
        f.store.registered_project(&one).unwrap().common_directories,
        std::slice::from_ref(&moved)
    );
    assert!(f.store.rebind_project(&one, &old, &moved).is_err());
    assert!(matches!(
        f.store.rebind_project(&one, &moved, &other),
        Err(Error::BindingConflict { .. })
    ));
    assert_eq!(
        f.store.registered_project(&one).unwrap().common_directories,
        [moved]
    );
    assert_eq!(
        f.store.project_for_common_directory(&other).unwrap(),
        Some(two)
    );
}

#[test]
fn invalid_registration_and_unknown_binding_fail_without_changes() {
    let mut f = Fixture::new();
    for name in ["", "   ", "line\nbreak"] {
        assert!(f.store.register_project(name).is_err());
    }
    let path = f.directory("repo/.git");
    assert!(matches!(
        f.store.bind_project(
            &Project::try_new("p-00000000000000000000000000000003").unwrap(),
            &path
        ),
        Err(Error::UnregisteredProject(_))
    ));
    assert!(
        f.store
            .project_for_common_directory(&path)
            .unwrap()
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn aliases_resolve_to_the_same_common_directory() {
    let mut f = Fixture::new();
    let common = f.directory("repo/.git");
    let alias = f.root.join("alias");
    std::os::unix::fs::symlink(&common, &alias).unwrap();
    let project = f.store.register_project("one").unwrap();
    f.store.bind_project(&project, &alias).unwrap();
    assert_eq!(
        f.store.project_for_common_directory(&common).unwrap(),
        Some(project.clone())
    );
    assert_eq!(
        f.store
            .registered_project(&project)
            .unwrap()
            .common_directories,
        [common]
    );
}
