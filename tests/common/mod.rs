use std::path::PathBuf;

pub fn pxh_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove 'deps'
    path.push("pxh");
    assert!(path.exists(), "pxh binary not found at {:?}", path);
    path
}
