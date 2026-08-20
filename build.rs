#[cfg(windows)]
fn main() {
    embed_resource::compile("icon.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}

// Nothing to embed anywhere else, but a build script still needs a main.
#[cfg(not(windows))]
fn main() {}
