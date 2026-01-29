#!/bin/bash
#
# Build RAUTA in Docker
#
# This script builds the RAUTA Docker image with the control plane.

set -e

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}[RAUTA]${NC} Building Docker image..."

# Build the image
docker build -t rauta:latest -f Dockerfile .

echo ""
echo -e "${GREEN}[RAUTA]${NC} Build complete!"
echo ""
echo "Image: rauta:latest"
echo ""
echo "Built components:"
echo "  - Control plane:  /home/rauta/bin/rauta-control"
echo ""
echo "Next steps:"
echo "  1. Run tests:     ./docker/test.sh"
echo "  2. Start stack:   docker-compose up"
echo "  3. Interactive:   docker run -it rauta:latest"
