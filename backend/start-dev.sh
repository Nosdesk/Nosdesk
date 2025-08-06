#!/bin/bash
set -e

echo "🚀 Starting Nosdesk Backend (Development Mode)..."

# Wait for database to be ready
echo "⏳ Waiting for database to be ready..."
until diesel database setup; do
    echo "Database not ready, retrying in 5 seconds..."
    sleep 5
done

# Run database migrations
echo "📊 Running database migrations..."
if diesel migration run; then
    echo "✅ Migrations completed successfully"
else
    echo "❌ Migration failed"
    exit 1
fi

# Start the backend application with cargo watch
echo "🎯 Starting backend server with cargo watch..."
exec cargo watch -x run 