# ABOUTME: Build automation for Cool Rust Term
# ABOUTME: Recipes for building, packaging, and cleaning the project

# Default recipe
default: build

# Debug build
build:
    cargo build

# Release build
release:
    cargo build --release

# Run debug build
run:
    cargo run

# macOS app bundle (release). cargo-bundle only handles the legacy .icns,
# so the Icon Composer asset catalog is added afterwards.
bundle: release
    cargo bundle --release --format osx -p crt-app
    cp assets/macos/Assets.car "target/release/bundle/osx/Cool Rust Term.app/Contents/Resources/"
    plutil -replace CFBundleIconName -string AppIcon "target/release/bundle/osx/Cool Rust Term.app/Contents/Info.plist"

# macOS DMG installer
dmg: bundle
    rm -f CoolRustTerm.dmg
    create-dmg \
        --volname "Cool Rust Term" \
        --window-pos 200 120 \
        --window-size 600 400 \
        --icon-size 100 \
        --icon "Cool Rust Term.app" 150 190 \
        --app-drop-link 450 190 \
        "CoolRustTerm.dmg" \
        "target/release/bundle/osx/Cool Rust Term.app"

# Regenerate the macOS icon (Assets.car + fallback AppIcon.icns) from the layer SVGs
icons:
    rsvg-convert assets/macos/screen.svg -o assets/macos/AppIcon.icon/Assets/screen.png
    rsvg-convert assets/macos/glyphs.svg -o assets/macos/AppIcon.icon/Assets/glyphs.png
    xcrun actool "{{justfile_directory()}}/assets/macos/AppIcon.icon" \
        --compile "{{justfile_directory()}}/assets/macos" --app-icon AppIcon \
        --enable-on-demand-resources NO --development-region en \
        --target-device mac --platform macosx --minimum-deployment-target 10.13 \
        --output-partial-info-plist /dev/null --output-format human-readable-text

# Clean build artifacts
clean:
    cargo clean
    rm -f CoolRustTerm.dmg

# Set up git hooks for pre-commit checks
setup:
    git config core.hooksPath .githooks
    @echo "Git hooks installed!"
