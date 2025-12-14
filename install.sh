#!/bin/bash
# ABOUTME: Universal installer script for Cool Rust Term
# ABOUTME: Downloads and installs the appropriate release for the current platform

set -euo pipefail

REPO="Aeolun/cool-rust-terminal"
APP_NAME="cool-rust-term"
DISPLAY_NAME="Cool Rust Term"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

info() { echo -e "${GREEN}==>${NC} $1"; }
warn() { echo -e "${YELLOW}warning:${NC} $1"; }
error() { echo -e "${RED}error:${NC} $1" >&2; exit 1; }

# Detect OS and architecture
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux*)  os="linux" ;;
        Darwin*) os="macos" ;;
        MINGW*|MSYS*|CYGWIN*) os="windows" ;;
        *) error "Unsupported operating system: $(uname -s)" ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *) error "Unsupported architecture: $(uname -m)" ;;
    esac

    # Linux only has x86_64 builds currently
    if [[ "$os" == "linux" && "$arch" == "aarch64" ]]; then
        error "Linux ARM64 builds are not yet available"
    fi

    echo "${os}-${arch}"
}

# Get the latest release version from GitHub
get_latest_version() {
    local version
    version=$(curl -sL "https://api.github.com/repos/${REPO}/releases/latest" |
              grep '"tag_name":' |
              sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')

    if [[ -z "$version" ]]; then
        error "Failed to fetch latest version from GitHub"
    fi

    echo "$version"
}

# Detect glibc version and choose appropriate binary
choose_linux_variant() {
    # Check if ldd is available (indicates glibc system)
    if ! command -v ldd &> /dev/null; then
        error "Could not detect glibc. This application requires glibc 2.35 or newer."
    fi

    # Get glibc version
    local glibc_version
    glibc_version=$(ldd --version 2>&1 | head -n1 | grep -oE '[0-9]+\.[0-9]+' | head -n1)

    if [[ -z "$glibc_version" ]]; then
        error "Could not detect glibc version. This application requires glibc 2.35 or newer."
    fi

    # Parse version (e.g., "2.39" -> 239)
    local version_num
    version_num=$(echo "$glibc_version" | tr -d '.')

    # Use modern build if glibc >= 2.39 (Ubuntu 24.04+)
    # Use compat build if glibc >= 2.35 (Ubuntu 22.04+)
    if [[ $version_num -ge 239 ]]; then
        echo "modern"
    elif [[ $version_num -ge 235 ]]; then
        echo "compat"
    else
        error "glibc ${glibc_version} is too old. This application requires glibc 2.35 or newer (Ubuntu 22.04+)."
    fi
}

# Install on Linux
install_linux() {
    local version="$1"
    local version_num="${version#v}"  # Remove 'v' prefix

    # Detect which variant to use
    local variant
    variant=$(choose_linux_variant)

    local suffix=""
    if [[ "$variant" == "compat" ]]; then
        suffix="-compat"
    fi

    info "Using ${variant} build (glibc)"

    local url="https://github.com/${REPO}/releases/download/${version}/${APP_NAME}-${version_num}-linux-x86_64${suffix}.tar.gz"
    local tmp_dir
    tmp_dir=$(mktemp -d)

    info "Downloading ${APP_NAME} ${version}..."
    curl -sL "$url" -o "${tmp_dir}/archive.tar.gz"

    info "Extracting..."
    tar -xzf "${tmp_dir}/archive.tar.gz" -C "${tmp_dir}"

    # Find the binary (might be at root or in a subdirectory)
    local binary_path
    binary_path=$(find "${tmp_dir}" -name "${APP_NAME}" -type f -perm -u+x 2>/dev/null | head -1)
    if [[ -z "$binary_path" ]]; then
        # Try without execute permission check (might not be preserved)
        binary_path=$(find "${tmp_dir}" -name "${APP_NAME}" -type f 2>/dev/null | head -1)
    fi
    if [[ -z "$binary_path" ]]; then
        error "Could not find ${APP_NAME} binary in archive"
    fi

    # Determine install location
    local bin_dir
    if [[ -w "/usr/local/bin" ]]; then
        bin_dir="/usr/local/bin"
    else
        bin_dir="${HOME}/.local/bin"
        mkdir -p "$bin_dir"
    fi

    info "Installing binary to ${bin_dir}..."
    cp "$binary_path" "${bin_dir}/${APP_NAME}"
    chmod +x "${bin_dir}/${APP_NAME}"

    # Install desktop entry
    local desktop_dir="${HOME}/.local/share/applications"
    local icon_dir="${HOME}/.local/share/icons/hicolor/256x256/apps"
    mkdir -p "$desktop_dir" "$icon_dir"

    # Download icon
    info "Installing desktop entry..."
    curl -sL "https://raw.githubusercontent.com/${REPO}/main/assets/icon.png" -o "${icon_dir}/${APP_NAME}.png"

    # Create desktop entry
    cat > "${desktop_dir}/${APP_NAME}.desktop" << EOF
[Desktop Entry]
Name=${DISPLAY_NAME}
Comment=CRT-style terminal emulator
Exec=${bin_dir}/${APP_NAME}
Icon=${APP_NAME}
Terminal=false
Type=Application
Categories=System;TerminalEmulator;
Keywords=terminal;console;command line;
EOF

    # Update desktop database if available
    if command -v update-desktop-database &> /dev/null; then
        update-desktop-database "$desktop_dir" 2>/dev/null || true
    fi

    rm -rf "$tmp_dir"

    info "Installation complete!"
    echo ""
    echo "  Binary installed to: ${bin_dir}/${APP_NAME}"
    echo "  Desktop entry: ${desktop_dir}/${APP_NAME}.desktop"
    echo ""

    # Check if bin_dir is in PATH
    if [[ ":$PATH:" != *":${bin_dir}:"* ]]; then
        warn "${bin_dir} is not in your PATH"
        echo "  Add it with: export PATH=\"\$PATH:${bin_dir}\""
    fi
}

# Install on macOS
install_macos() {
    local version="$1"
    local arch="$2"
    local version_num="${version#v}"
    local url="https://github.com/${REPO}/releases/download/${version}/${APP_NAME}-${version_num}-macos-${arch}.dmg"
    local tmp_dir
    tmp_dir=$(mktemp -d)
    local dmg_path="${tmp_dir}/${APP_NAME}.dmg"

    info "Downloading ${APP_NAME} ${version}..."
    curl -sL "$url" -o "$dmg_path"

    info "Mounting DMG..."
    local mount_point
    mount_point=$(hdiutil attach -nobrowse -readonly "$dmg_path" 2>/dev/null | grep "/Volumes" | cut -f3)

    if [[ -z "$mount_point" ]]; then
        error "Failed to mount DMG"
    fi

    info "Installing to /Applications..."
    local app_path="/Applications/${DISPLAY_NAME}.app"

    # Remove existing installation
    if [[ -d "$app_path" ]]; then
        rm -rf "$app_path"
    fi

    cp -R "${mount_point}/${DISPLAY_NAME}.app" "/Applications/"

    info "Unmounting DMG..."
    hdiutil detach "$mount_point" -quiet

    rm -rf "$tmp_dir"

    info "Installation complete!"
    echo ""
    echo "  App installed to: ${app_path}"
    echo "  Launch from Applications or Spotlight"
}

# Install on Windows (basic support)
install_windows() {
    local version="$1"
    local version_num="${version#v}"
    local url="https://github.com/${REPO}/releases/download/${version}/${APP_NAME}-${version_num}-windows-x86_64.zip"

    echo ""
    echo "Windows installation via this script is not fully supported."
    echo "Please download manually from:"
    echo "  $url"
    echo ""
    echo "Or use PowerShell:"
    echo "  Invoke-WebRequest -Uri '$url' -OutFile '${APP_NAME}.zip'"
    echo "  Expand-Archive -Path '${APP_NAME}.zip' -DestinationPath ."
}

main() {
    echo ""
    echo "  ╔═══════════════════════════════════════╗"
    echo "  ║     Cool Rust Term Installer          ║"
    echo "  ╚═══════════════════════════════════════╝"
    echo ""

    local platform
    platform=$(detect_platform)
    info "Detected platform: ${platform}"

    local version
    version=$(get_latest_version)
    info "Latest version: ${version}"

    case "$platform" in
        linux-x86_64)
            install_linux "$version"
            ;;
        macos-x86_64)
            install_macos "$version" "x86_64"
            ;;
        macos-aarch64)
            install_macos "$version" "aarch64"
            ;;
        windows-x86_64)
            install_windows "$version"
            ;;
        *)
            error "No installation method for platform: ${platform}"
            ;;
    esac
}

main "$@"
