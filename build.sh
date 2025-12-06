#!/bin/bash
set -e

# Cross-platform build script for scanner
# Uses 'cross' for cross-compilation (requires Docker)
# Install cross: cargo install cross --git https://github.com/cross-rs/cross
#
# Usage:
#   ./build.sh          # Build for all platforms
#   ./build.sh macos    # Build for macOS only
#   ./build.sh linux    # Build for Linux only

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Parse command line arguments
TARGET_PLATFORM="${1:-all}"

echo -e "${GREEN}Building scanner...${NC}"

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo is not installed${NC}"
    echo "Please install Rust from https://rustup.rs/"
    exit 1
fi

# Check if cross is installed
if ! command -v cross &> /dev/null; then
    echo -e "${YELLOW}Warning: cross is not installed${NC}"
    echo -e "${YELLOW}Installing cross for cross-compilation...${NC}"
    cargo install cross --git https://github.com/cross-rs/cross
    if [ $? -ne 0 ]; then
        echo -e "${RED}Failed to install cross${NC}"
        echo "Please install it manually: cargo install cross --git https://github.com/cross-rs/cross"
        exit 1
    fi
fi

# Get project information
PROJECT_NAME="scanner"
VERSION=$(grep -m 1 '^version' Cargo.toml | cut -d'"' -f2)
BUILD_DIR="target/release"
DIST_DIR="dist"

echo -e "${YELLOW}Project: ${PROJECT_NAME}${NC}"
echo -e "${YELLOW}Version: ${VERSION}${NC}"
echo ""

# Create distribution directory
mkdir -p "${DIST_DIR}"

# Clean up old build artifacts for the platforms we're building
echo -e "${YELLOW}Cleaning old build artifacts...${NC}"
if [[ "${TARGET_PLATFORM}" == "all" ]]; then
    rm -f "${DIST_DIR}"/scanner-*-macos.zip "${DIST_DIR}"/scanner-*-linux.zip
    rm -rf "${DIST_DIR}"/scanner-*-macos/ "${DIST_DIR}"/scanner-*-linux/
    rm -f "${DIST_DIR}"/scanner-macos "${DIST_DIR}"/scanner-linux
elif [[ "${TARGET_PLATFORM}" == "macos" ]]; then
    rm -f "${DIST_DIR}"/scanner-*-macos.zip
    rm -rf "${DIST_DIR}"/scanner-*-macos/
    rm -f "${DIST_DIR}"/scanner-macos
elif [[ "${TARGET_PLATFORM}" == "linux" ]]; then
    rm -f "${DIST_DIR}"/scanner-*-linux.zip
    rm -rf "${DIST_DIR}"/scanner-*-linux/
    rm -f "${DIST_DIR}"/scanner-linux
fi

# Detect current platform
CURRENT_OS=$(uname -s | tr '[:upper:]' '[:lower:]')

# Function to build for a specific target
build_for_target() {
    local target=$1
    local platform=$2

    echo -e "${YELLOW}Building for ${platform}...${NC}"

    # Determine if we need cross-compilation
    local build_cmd="cargo"
    if [[ "${CURRENT_OS}" == "darwin" && "${target}" == *"linux"* ]]; then
        build_cmd="cross"
        echo -e "${YELLOW}Using cross for cross-compilation (Docker required)${NC}"
    elif [[ "${CURRENT_OS}" == "linux" && "${target}" == *"apple"* ]]; then
        build_cmd="cross"
        echo -e "${YELLOW}Using cross for cross-compilation (Docker required)${NC}"
    else
        echo -e "${YELLOW}Using native cargo build${NC}"
        # For native builds, ensure target is installed
        if ! rustup target list | grep -q "${target} (installed)"; then
            echo -e "${YELLOW}Installing target ${target}...${NC}"
            rustup target add "${target}"
        fi
    fi

    # Build for target
    ${build_cmd} build --release --target="${target}"

    # Check if build was successful
    if [ $? -ne 0 ]; then
        echo -e "${RED}Build failed for ${platform}!${NC}"
        return 1
    fi

    # Copy binary to dist directory (keep platform suffix for reference)
    local binary_name="${PROJECT_NAME}"
    cp "target/${target}/release/${binary_name}" "${DIST_DIR}/${PROJECT_NAME}-${platform}"

    # Create temporary staging directory for clean zip contents
    local staging_dir="${DIST_DIR}/staging-${platform}"
    mkdir -p "${staging_dir}"

    # Copy files to staging with clean names (no platform suffix)
    cp "target/${target}/release/${binary_name}" "${staging_dir}/${PROJECT_NAME}"
    cp "INSTALL.md" "${staging_dir}/README.md"

    # Copy config.toml if it exists
    if [ -f "config.toml" ]; then
        cp "config.toml" "${staging_dir}/config.toml"
    fi

    # Create zip file from staging directory
    local zip_name="${PROJECT_NAME}-${VERSION}-${platform}.zip"
    echo -e "${YELLOW}Creating ${platform} zip archive...${NC}"
    cd "${staging_dir}"

    if [ -f "config.toml" ]; then
        zip -q "../${zip_name}" "${PROJECT_NAME}" "README.md" "config.toml"
    else
        zip -q "../${zip_name}" "${PROJECT_NAME}" "README.md"
    fi

    cd ../..

    # Clean up staging directory
    rm -rf "${staging_dir}"

    # Get sizes
    local binary_size=$(du -h "${DIST_DIR}/${PROJECT_NAME}-${platform}" | cut -f1)
    local zip_size=$(du -h "${DIST_DIR}/${zip_name}" | cut -f1)

    echo -e "${GREEN}✓ ${platform} build successful!${NC}"
    echo -e "${GREEN}  Binary: ${DIST_DIR}/${PROJECT_NAME}-${platform} (${binary_size})${NC}"
    echo -e "${GREEN}  Archive: ${DIST_DIR}/${zip_name} (${zip_size})${NC}"
    echo ""

    return 0
}

# Build based on target platform
case "${TARGET_PLATFORM}" in
    macos)
        build_for_target "x86_64-apple-darwin" "macos"
        ;;
    linux)
        build_for_target "x86_64-unknown-linux-gnu" "linux"
        ;;
    all)
        echo -e "${YELLOW}Building for all platforms...${NC}"
        echo ""
        build_for_target "x86_64-apple-darwin" "macos"
        build_for_target "x86_64-unknown-linux-gnu" "linux"
        ;;
    *)
        echo -e "${RED}Error: Unknown platform '${TARGET_PLATFORM}'${NC}"
        echo "Usage: $0 [macos|linux|all]"
        echo "  macos - Build for macOS only"
        echo "  linux - Build for Linux only"
        echo "  all   - Build for all platforms (default)"
        exit 1
        ;;
esac

echo -e "${GREEN}================================${NC}"
echo -e "${GREEN}All builds completed successfully!${NC}"
echo -e "${GREEN}================================${NC}"
echo ""
echo -e "${YELLOW}Available binaries in ${DIST_DIR}/:${NC}"
ls -lh "${DIST_DIR}/" | grep "${PROJECT_NAME}"
echo ""
echo -e "${YELLOW}To run a binary:${NC}"
echo -e "  ./${DIST_DIR}/${PROJECT_NAME}-macos    # macOS"
echo -e "  ./${DIST_DIR}/${PROJECT_NAME}-linux    # Linux"
echo ""
echo -e "${YELLOW}To install system-wide:${NC}"
echo -e "  # macOS:"
echo -e "  sudo cp ${DIST_DIR}/${PROJECT_NAME}-macos /usr/local/bin/${PROJECT_NAME}"
echo -e "  # Linux:"
echo -e "  sudo cp ${DIST_DIR}/${PROJECT_NAME}-linux /usr/local/bin/${PROJECT_NAME}"
echo ""
echo -e "${YELLOW}Each zip archive contains:${NC}"
echo -e "  - scanner binary"
echo -e "  - README.md (installation and usage guide)"
echo -e "  - config.toml (if present in project root)"
