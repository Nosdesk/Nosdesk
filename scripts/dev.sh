#!/bin/bash

# Development setup script for Nosdesk
# This script provides different development modes to simplify the development experience

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}🚀 Nosdesk Development Setup${NC}"
echo ""

# Function to display usage
usage() {
    echo "Usage: $0 [MODE]"
    echo ""
    echo "Available modes:"
    echo "  unified     - Backend serves frontend (production-like, recommended)"
    echo "  watch       - Backend + frontend with hot reloading"
    echo "  frontend    - Frontend development server only (requires backend running)"
    echo "  build       - Build frontend for production"
    echo "  clean       - Stop all containers and clean up"
    echo ""
    echo "If no mode is specified, 'unified' mode will be used."
}

# Function to check if Docker is running
check_docker() {
    if ! docker info > /dev/null 2>&1; then
        echo -e "${RED}❌ Docker is not running. Please start Docker first.${NC}"
        exit 1
    fi
}

# Function to build frontend
build_frontend() {
    echo -e "${BLUE}📦 Building frontend...${NC}"
    cd frontend
    npm install
    npm run build:production
    cd ..
    echo -e "${GREEN}✅ Frontend built to backend/public${NC}"
}

# Function to start unified development (backend serves frontend)
start_unified() {
    echo -e "${BLUE}🏗️  Starting unified development mode...${NC}"
    
    # Build frontend first
    build_frontend
    
    # Start backend with database
    echo -e "${BLUE}🚀 Starting backend services...${NC}"
    docker compose --profile dev up -d
    
    echo ""
    echo -e "${GREEN}✅ Development environment ready!${NC}"
    echo -e "${YELLOW}🌐 Application: http://localhost:8080${NC}"
    echo -e "${YELLOW}🌐 Alternative: http://localhost:3000${NC}"
    echo -e "${YELLOW}📊 Database: localhost:5432${NC}"
    echo -e "${YELLOW}🚀 Redis: localhost:6379${NC}"
    echo ""
    echo "To view logs: docker compose logs -f backend-dev"
    echo "To rebuild frontend: cd frontend && npm run build:production"
}

# Function to start with frontend hot reloading
start_watch() {
    echo -e "${BLUE}🔥 Starting development with frontend hot reloading...${NC}"
    
    # Start backend and frontend watch
    docker compose --profile dev --profile dev-watch up -d
    
    echo ""
    echo -e "${GREEN}✅ Development environment with hot reloading ready!${NC}"
    echo -e "${YELLOW}🌐 Application: http://localhost:8080${NC}"
    echo -e "${YELLOW}🌐 Alternative: http://localhost:3000${NC}"
    echo ""
    echo "Frontend will rebuild automatically when you make changes."
    echo "To view logs: docker compose logs -f backend-dev frontend-watch"
}

# Function to start frontend development server
start_frontend() {
    echo -e "${BLUE}🎨 Starting frontend development server...${NC}"
    echo -e "${YELLOW}⚠️  Make sure backend is running first!${NC}"
    
    cd frontend
    npm install
    npm run dev
}

# Function to clean up
clean() {
    echo -e "${BLUE}🧹 Cleaning up development environment...${NC}"
    docker compose --profile dev --profile dev-watch down -v --remove-orphans
    docker system prune -f
    echo -e "${GREEN}✅ Cleanup complete!${NC}"
}

# Main script logic
MODE=${1:-unified}

check_docker

case $MODE in
    unified)
        start_unified
        ;;
    watch)
        start_watch
        ;;
    frontend)
        start_frontend
        ;;
    build)
        build_frontend
        ;;
    clean)
        clean
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        echo -e "${RED}❌ Unknown mode: $MODE${NC}"
        echo ""
        usage
        exit 1
        ;;
esac 