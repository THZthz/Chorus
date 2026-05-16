# Debug endpoint examples — paste-n-run each line separately.
# Assumes server running on localhost:3000.

# Dump — full world state (markdown)
curl -s "http://localhost:3000/api/debug/dump" | head -80

# Search world — entities + messages
curl -s "http://localhost:3000/api/debug/search/world?query=murder+weapon&types=entities,messages&limit=5&threshold=0.5" | jq .

# Search world with reranker (requires RERANK_API_URL configured)
curl -s "http://localhost:3000/api/debug/search/world?query=murder+weapon&types=entities,messages&limit=5&rerank=true" | jq .

# Search plots
curl -s "http://localhost:3000/api/debug/search/plots?query=Crowne+murder&limit=5&threshold=0.4" | jq .

# Search notes
curl -s "http://localhost:3000/api/debug/search/notes?query=ley+line&limit=5&threshold=0.4" | jq .
