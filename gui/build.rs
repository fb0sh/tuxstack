fn main() {
    // Normal build: main.slint
    // Prototype UI build (--features prototype-ui): prototype-ui.slint
    // Never both — they define overlapping types
    if std::env::var("CARGO_FEATURE_PROTOTYPE_UI").is_ok() {
        slint_build::compile("ui/prototype-ui.slint").unwrap();
    } else {
        slint_build::compile("ui/main.slint").unwrap();
    }
}
