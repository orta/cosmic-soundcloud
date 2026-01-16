#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Get the script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Check for uncommitted changes
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo -e "${RED}Error: You have uncommitted changes. Please commit or stash them first.${NC}"
    exit 1
fi

# Get current version from Cargo.toml
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo -e "${BLUE}Current version:${NC} $CURRENT_VERSION"

# Ask for new version
echo -e "${YELLOW}Enter new version (without 'v' prefix):${NC}"
read -r NEW_VERSION

if [ -z "$NEW_VERSION" ]; then
    echo -e "${RED}Error: Version cannot be empty${NC}"
    exit 1
fi

# Validate version format (basic semver check)
if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    echo -e "${RED}Error: Invalid version format. Use semver (e.g., 1.0.0, 1.0.0-beta.1)${NC}"
    exit 1
fi

# Check if tag already exists
if git rev-parse "v$NEW_VERSION" >/dev/null 2>&1; then
    echo -e "${RED}Error: Tag v$NEW_VERSION already exists${NC}"
    exit 1
fi

# Ask for release notes
echo -e "${YELLOW}Enter release notes (one item per line, empty line to finish):${NC}"
RELEASE_NOTES=()
while IFS= read -r line; do
    [ -z "$line" ] && break
    RELEASE_NOTES+=("$line")
done

if [ ${#RELEASE_NOTES[@]} -eq 0 ]; then
    echo -e "${RED}Error: Release notes cannot be empty${NC}"
    exit 1
fi

# Show summary
echo ""
echo -e "${BLUE}=== Release Summary ===${NC}"
echo -e "Version: ${GREEN}$NEW_VERSION${NC}"
echo -e "Release notes:"
for note in "${RELEASE_NOTES[@]}"; do
    echo -e "  - $note"
done
echo ""

# Confirm
echo -e "${YELLOW}Proceed with release? (y/N)${NC}"
read -r CONFIRM
if [[ ! "$CONFIRM" =~ ^[Yy]$ ]]; then
    echo -e "${RED}Release cancelled${NC}"
    exit 1
fi

echo -e "${BLUE}Updating Cargo.toml...${NC}"
sed -i "0,/^version = /s/^version = .*/version = \"$NEW_VERSION\"/" Cargo.toml

echo -e "${BLUE}Updating metainfo.xml...${NC}"
METAINFO_FILE="resources/com.github.orta.cosmic-soundcloud.metainfo.xml"
TODAY=$(date +%Y-%m-%d)

# Build the release notes XML
NOTES_XML="    <release version=\"$NEW_VERSION\" date=\"$TODAY\">\n      <description>\n"
for note in "${RELEASE_NOTES[@]}"; do
    # Escape XML special characters
    escaped_note=$(echo "$note" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g')
    NOTES_XML+="        <p>$escaped_note</p>\n"
done
NOTES_XML+="      </description>\n    </release>"

# Check if releases section exists
if grep -q "<releases>" "$METAINFO_FILE"; then
    # Add new release after <releases> tag
    sed -i "s|<releases>|<releases>\n$NOTES_XML|" "$METAINFO_FILE"
else
    # Add releases section before </component>
    sed -i "s|</component>|  <releases>\n$NOTES_XML\n  </releases>\n</component>|" "$METAINFO_FILE"
fi

echo -e "${BLUE}Running cargo check to update Cargo.lock...${NC}"
cargo check --quiet

echo -e "${BLUE}Creating commit...${NC}"
git add Cargo.toml Cargo.lock "$METAINFO_FILE"
git commit -m "release: v$NEW_VERSION"

echo -e "${BLUE}Creating tag v$NEW_VERSION...${NC}"
git tag -a "v$NEW_VERSION" -m "Release v$NEW_VERSION"

echo ""
echo -e "${GREEN}=== Release prepared successfully! ===${NC}"
echo ""
echo -e "To publish the release, run:"
echo -e "  ${YELLOW}git push && git push origin v$NEW_VERSION${NC}"
echo ""
echo -e "This will trigger the GitHub Actions workflow to build and publish:"
echo -e "  - Debian package (.deb)"
echo -e "  - Flatpak bundle (.flatpak)"
echo -e "  - GitHub Release with artifacts"
