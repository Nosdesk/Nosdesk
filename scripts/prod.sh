#!/bin/bash

echo "🏭 Starting Nosdesk Production Environment..."
echo ""
echo "This will start:"
echo "  - PostgreSQL database"
echo "  - Redis cache"
echo "  - Rust backend serving both API and static Vue files"
echo ""
echo "Application will be available at: http://localhost:3000 or http://localhost:8080"
echo ""

# Start production environment
docker-compose --profile prod up --build -d

echo "✅ Production environment started in detached mode."
echo ""
echo "View logs with: docker-compose logs -f"
echo "Stop with: docker-compose down" 