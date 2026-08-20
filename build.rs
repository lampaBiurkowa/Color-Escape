#[cfg(windows)]
fn main() {
    // icon.rc is not in the repository, so a plain checkout - CI's, for one - builds
    // without the icon rather than failing on the missing file.
    if std::path::Path::new("icon.rc").exists() {
        embed_resource::compile("icon.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}

// Nothing to embed anywhere else, but a build script still needs a main.
#[cfg(not(windows))]
fn main() {}
