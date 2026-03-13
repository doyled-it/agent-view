#!/usr/bin/env bash
#
# Agent View Installer (MITRE)
# Usage: curl -kfsSL https://gitlab.mitre.org/mdoyle/agent-view/-/raw/main/install-mitre.sh | bash
#
# All GitLab API calls use -k to handle MITRE certificate issues.
#

set -euo pipefail

APP=agent-view
GITLAB_URL="https://gitlab.mitre.org"
GITLAB_PROJECT_PATH="mdoyle%2Fagent-view"

# Colors
MUTED='\033[0;2m'
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

INSTALL_DIR="${AGENT_VIEW_INSTALL_DIR:-$HOME/.agent-view/bin}"

usage() {
    cat <<EOF
Agent View Installer (MITRE)

Usage: install-mitre.sh [options]

Options:
    -h, --help              Display this help message
    -v, --version <version> Install a specific version (e.g., 1.0.0)
        --no-modify-path    Don't modify shell config files

Examples:
    curl -kfsSL https://gitlab.mitre.org/mdoyle/agent-view/-/raw/main/install-mitre.sh | bash
    curl -kfsSL https://gitlab.mitre.org/mdoyle/agent-view/-/raw/main/install-mitre.sh | bash -s -- --version 1.0.0
    curl -kfsSL https://gitlab.mitre.org/mdoyle/agent-view/-/raw/main/install-mitre.sh | bash -s -- --no-modify-path
EOF
}

requested_version=""
no_modify_path=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            usage
            exit 0
            ;;
        -v|--version)
            if [[ -n "${2:-}" ]]; then
                requested_version="$2"
                shift 2
            else
                echo -e "${RED}Error: --version requires a version argument${NC}"
                exit 1
            fi
            ;;
        --no-modify-path)
            no_modify_path=true
            shift
            ;;
        *)
            echo -e "${RED}Warning: Unknown option '$1'${NC}" >&2
            shift
            ;;
    esac
done

# Detect platform
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Darwin) os="darwin" ;;
        Linux) os="linux" ;;
        *) echo -e "${RED}Unsupported OS: $(uname -s)${NC}"; exit 1 ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x64" ;;
        arm64|aarch64) arch="arm64" ;;
        *) echo -e "${RED}Unsupported architecture: $(uname -m)${NC}"; exit 1 ;;
    esac

    echo "${os}-${arch}"
}

# Check for tmux (warn but don't block)
check_tmux() {
    if command -v tmux &> /dev/null; then
        return 0
    fi

    echo -e "${MUTED}tmux is not installed.${NC}"
    echo "Agent View requires tmux to function."
    echo ""

    local os_type
    os_type="$(uname -s)"

    if [[ "$os_type" == "Darwin" ]]; then
        echo "  Install with: brew install tmux"
    else
        echo "  Install with one of:"
        echo "    sudo apt install tmux      # Debian/Ubuntu"
        echo "    sudo dnf install tmux      # Fedora/RHEL"
        echo "    sudo pacman -S tmux        # Arch"
    fi

    echo ""
    echo -e "${MUTED}Continuing installation without tmux...${NC}"
    echo ""
}

# Resolve GitLab project ID dynamically
resolve_project_id() {
    echo -e "${MUTED}Resolving GitLab project...${NC}"

    local response
    response=$(curl -ksS "${GITLAB_URL}/api/v4/projects/${GITLAB_PROJECT_PATH}" 2>&1) || {
        echo -e "${RED}Failed to query GitLab API${NC}"
        echo -e "${MUTED}Response: ${response}${NC}"
        exit 1
    }

    local project_id
    project_id=$(echo "$response" | python3 -c "import sys, json; print(json.load(sys.stdin)['id'])" 2>/dev/null) || {
        echo -e "${RED}Failed to parse project ID from GitLab response${NC}"
        echo -e "${MUTED}Response: ${response}${NC}"
        exit 1
    }

    echo "$project_id"
}

# Get latest release version
get_latest_version() {
    local project_id=$1

    echo -e "${MUTED}Fetching latest release...${NC}" >&2

    local response
    response=$(curl -ksS "${GITLAB_URL}/api/v4/projects/${project_id}/releases" 2>&1) || {
        echo -e "${RED}Failed to fetch releases from GitLab${NC}"
        exit 1
    }

    local tag_name
    tag_name=$(echo "$response" | python3 -c "import sys, json; releases = json.load(sys.stdin); print(releases[0]['tag_name'] if releases else '')" 2>/dev/null) || {
        echo -e "${RED}Failed to parse release information${NC}"
        echo -e "${MUTED}Response: ${response}${NC}"
        exit 1
    }

    if [[ -z "$tag_name" ]]; then
        echo -e "${RED}No releases found${NC}" >&2
        exit 1
    fi

    # Strip v prefix if present
    echo "${tag_name#v}"
}

check_tmux

mkdir -p "$INSTALL_DIR"

platform=$(detect_platform)

# Resolve project ID
PROJECT_ID=$(resolve_project_id)

# Determine version
if [[ -z "$requested_version" ]]; then
    specific_version=$(get_latest_version "$PROJECT_ID")
else
    specific_version="${requested_version#v}"
fi

if [[ -z "$specific_version" ]]; then
    echo -e "${RED}Failed to determine version to install${NC}"
    exit 1
fi

# Check if already installed at this version
check_version() {
    if command -v agent-view >/dev/null 2>&1; then
        installed_version=$(agent-view --version 2>/dev/null || echo "")
        if [[ "$installed_version" == "$specific_version" ]]; then
            echo -e "${MUTED}Version ${NC}$specific_version${MUTED} already installed${NC}"
            exit 0
        else
            echo -e "${MUTED}Installed version: ${NC}$installed_version"
        fi
    fi
}

check_version

# Download and install
download_and_install() {
    local filename="$APP-$platform.tar.gz"
    local url="${GITLAB_URL}/api/v4/projects/${PROJECT_ID}/packages/generic/${APP}/${specific_version}/${filename}"

    echo -e "\n${MUTED}Installing ${NC}$APP ${MUTED}version: ${NC}$specific_version"
    echo -e "${MUTED}Platform: ${NC}$platform"

    local tmp_dir="${TMPDIR:-/tmp}/$APP-$$"
    mkdir -p "$tmp_dir"

    echo -e "${MUTED}Downloading...${NC}"
    if ! curl -k#fL -o "$tmp_dir/$filename" "$url"; then
        echo -e "${RED}Download failed.${NC}"
        echo -e "${MUTED}Pre-built binaries are only available for linux-x64.${NC}"
        echo -e "${MUTED}Your platform: ${NC}${platform}"
        echo ""
        echo -e "${MUTED}Install from source instead:${NC}"
        echo -e "  git clone ${GITLAB_URL}/mdoyle/agent-view.git"
        echo -e "  cd agent-view && bun install && bun run compile"
        echo -e "  cd bin/agent-view-${platform} && ./install.sh"
        rm -rf "$tmp_dir"
        exit 1
    fi

    # Extract tarball
    tar -xzf "$tmp_dir/$filename" -C "$tmp_dir"

    # Find the extracted content - check common locations
    local extract_dir="$tmp_dir"
    if [ -d "$tmp_dir/$APP-$platform" ]; then
        extract_dir="$tmp_dir/$APP-$platform"
    elif [ -d "$tmp_dir/$APP" ]; then
        extract_dir="$tmp_dir/$APP"
    fi

    # Find the binary
    local bin_path=""
    if [ -f "$extract_dir/$APP" ]; then
        bin_path="$extract_dir/$APP"
    elif [ -f "$tmp_dir/$APP" ]; then
        bin_path="$tmp_dir/$APP"
    else
        echo -e "${RED}Binary not found in archive${NC}"
        rm -rf "$tmp_dir"
        exit 1
    fi

    # Copy binary
    cp "$bin_path" "$INSTALL_DIR/$APP"
    chmod 755 "$INSTALL_DIR/$APP"

    # Ad-hoc codesign on macOS to prevent "killed" on launch
    if [[ "$(uname -s)" == "Darwin" ]] && command -v codesign &>/dev/null; then
        codesign -s - "$INSTALL_DIR/$APP" 2>/dev/null || true
    fi

    # Copy prebuilds/ if present (needed for native modules like node-pty)
    if [ -d "$extract_dir/prebuilds" ]; then
        rm -rf "$INSTALL_DIR/prebuilds"
        cp -r "$extract_dir/prebuilds" "$INSTALL_DIR/prebuilds"
        echo -e "${MUTED}Copied native module prebuilds${NC}"
    fi

    # If run.sh launcher exists, install it for native module support
    if [ -f "$extract_dir/run.sh" ]; then
        cp "$extract_dir/run.sh" "$INSTALL_DIR/run.sh"
        chmod 755 "$INSTALL_DIR/run.sh"
        # av symlink → run.sh (which resolves to SCRIPT_DIR and execs agent-view binary)
        ln -sf "$INSTALL_DIR/run.sh" "$INSTALL_DIR/av"
    else
        ln -sf "$INSTALL_DIR/$APP" "$INSTALL_DIR/av"
    fi

    rm -rf "$tmp_dir"
}

download_and_install

# Add to PATH
add_to_path() {
    local config_file=$1
    local command=$2

    if grep -Fxq "$command" "$config_file" 2>/dev/null; then
        return 0
    elif [[ -w $config_file ]]; then
        echo -e "\n# agent-view" >> "$config_file"
        echo "$command" >> "$config_file"
        echo -e "${MUTED}Added to PATH in ${NC}$config_file"
    fi
}

if [[ "$no_modify_path" != "true" ]]; then
    if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
        current_shell=$(basename "$SHELL")
        case $current_shell in
            fish)
                config_file="$HOME/.config/fish/config.fish"
                [[ -f "$config_file" ]] && add_to_path "$config_file" "fish_add_path $INSTALL_DIR"
                ;;
            zsh)
                config_file="${ZDOTDIR:-$HOME}/.zshrc"
                [[ -f "$config_file" ]] && add_to_path "$config_file" "export PATH=\"$INSTALL_DIR:\$PATH\""
                ;;
            *)
                config_file="$HOME/.bashrc"
                [[ -f "$config_file" ]] && add_to_path "$config_file" "export PATH=\"$INSTALL_DIR:\$PATH\""
                ;;
        esac
    fi
fi

echo ""
echo -e "${GREEN}Installation complete!${NC}"
echo ""
echo -e "  Run ${GREEN}agent-view${NC} or ${GREEN}av${NC} to open Agent View"
echo ""
echo -e "  ${MUTED}Version: ${NC}$specific_version"
echo -e "  ${MUTED}Binary:  ${NC}$INSTALL_DIR/$APP"
echo ""
echo -e "  ${MUTED}Restart your shell or run:${NC}"
echo -e "  export PATH=\"$INSTALL_DIR:\$PATH\""
echo ""
