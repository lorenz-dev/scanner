#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Building scanner for macOS...${NC}"

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo is not installed${NC}"
    echo "Please install Rust from https://rustup.rs/"
    exit 1
fi

# Get project information
PROJECT_NAME="scanner"
VERSION=$(grep -m 1 '^version' Cargo.toml | cut -d'"' -f2)
BUILD_DIR="target/release"
DIST_DIR="dist"

echo -e "${YELLOW}Project: ${PROJECT_NAME}${NC}"
echo -e "${YELLOW}Version: ${VERSION}${NC}"
echo ""

# Clean previous builds (optional, uncomment if desired)
# echo -e "${YELLOW}Cleaning previous builds...${NC}"
# cargo clean

# Build in release mode
echo -e "${YELLOW}Building release binary...${NC}"
cargo build --release

# Check if build was successful
if [ $? -ne 0 ]; then
    echo -e "${RED}Build failed!${NC}"
    exit 1
fi

# Create distribution directory
echo -e "${YELLOW}Creating distribution directory...${NC}"
mkdir -p "${DIST_DIR}"

# Copy binary to dist directory
cp "${BUILD_DIR}/${PROJECT_NAME}" "${DIST_DIR}/"

# Copy config.toml if it exists
if [ -f "config.toml" ]; then
    echo -e "${YELLOW}Copying config.toml...${NC}"
    cp "config.toml" "${DIST_DIR}/"
fi

# Get binary size
BINARY_SIZE=$(du -h "${DIST_DIR}/${PROJECT_NAME}" | cut -f1)

# Create zip file
ZIP_NAME="${PROJECT_NAME}-${VERSION}-macos.zip"
echo -e "${YELLOW}Creating zip archive...${NC}"
cd "${DIST_DIR}"
if [ -f "config.toml" ]; then
    zip -q "${ZIP_NAME}" "${PROJECT_NAME}" "config.toml"
else
    zip -q "${ZIP_NAME}" "${PROJECT_NAME}"
fi
cd ..

# Get zip size
ZIP_SIZE=$(du -h "${DIST_DIR}/${ZIP_NAME}" | cut -f1)

echo ""
echo -e "${GREEN}✓ Build successful!${NC}"
echo -e "${GREEN}Binary location: ${DIST_DIR}/${PROJECT_NAME}${NC}"
echo -e "${GREEN}Binary size: ${BINARY_SIZE}${NC}"
echo -e "${GREEN}Zip archive: ${DIST_DIR}/${ZIP_NAME}${NC}"
echo -e "${GREEN}Zip size: ${ZIP_SIZE}${NC}"
echo ""
echo -e "${YELLOW}To run the binary:${NC}"
echo -e "  ./${DIST_DIR}/${PROJECT_NAME}"
echo ""
echo -e "${YELLOW}To install system-wide:${NC}"
echo -e "  sudo cp ${DIST_DIR}/${PROJECT_NAME} /usr/local/bin/"
echo ""
echo -e "${YELLOW}To extract the zip:${NC}"
echo -e "  unzip ${DIST_DIR}/${ZIP_NAME}"
