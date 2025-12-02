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

# macOS app bundle (release)
bundle: release
    cargo bundle --release --format osx -p crt-app

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

# Regenerate icon assets from icon.png
icons:
    cd assets && \
    rm -rf CoolRustTerm.iconset && \
    mkdir CoolRustTerm.iconset && \
    sips -z 16 16 icon.png --out CoolRustTerm.iconset/icon_16x16.png && \
    sips -z 32 32 icon.png --out CoolRustTerm.iconset/icon_16x16@2x.png && \
    sips -z 32 32 icon.png --out CoolRustTerm.iconset/icon_32x32.png && \
    sips -z 64 64 icon.png --out CoolRustTerm.iconset/icon_32x32@2x.png && \
    sips -z 128 128 icon.png --out CoolRustTerm.iconset/icon_128x128.png && \
    sips -z 256 256 icon.png --out CoolRustTerm.iconset/icon_128x128@2x.png && \
    sips -z 256 256 icon.png --out CoolRustTerm.iconset/icon_256x256.png && \
    sips -z 512 512 icon.png --out CoolRustTerm.iconset/icon_256x256@2x.png && \
    sips -z 512 512 icon.png --out CoolRustTerm.iconset/icon_512x512.png && \
    sips -z 1024 1024 icon.png --out CoolRustTerm.iconset/icon_512x512@2x.png && \
    iconutil -c icns CoolRustTerm.iconset -o icon.icns

# Clean build artifacts
clean:
    cargo clean
    rm -f CoolRustTerm.dmg

# Set up git hooks for pre-commit checks
setup:
    git config core.hooksPath .githooks
    @echo "Git hooks installed!"
