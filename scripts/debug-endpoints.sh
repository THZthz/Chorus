# Debug endpoint examples — paste-n-run each line separately.
# Assumes server running on localhost:3000.

# Dump — full world state (markdown)
curl -s "http://localhost:3000/api/debug/dump" | head -80

# Search world — entities + messages
curl -s "http://localhost:3000/api/debug/search/world?query=murder+weapon&types=entities,messages&limit=5&threshold=0.5" | jq .

# Search world with reranker (requires LLAMA_RERANK_URL configured)
curl -s "http://localhost:3000/api/debug/search/world?query=murder+weapon&types=entities,messages&limit=5&rerank=true" | jq .

# Search plots
curl -s "http://localhost:3000/api/debug/search/plots?query=Crowne+murder&limit=5&threshold=0.4" | jq .

# Search notes
curl -s "http://localhost:3000/api/debug/search/notes?query=ley+line&limit=5&threshold=0.4" | jq .

# queryWorld — READ (raw JSON)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity) RETURN e.name, e.type LIMIT 5"}'

# queryWorld — READ: browse time history (raw)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (tp:TimePoint) RETURN tp.day, tp.segment, tp.label ORDER BY tp.day, tp.segment"}'

# queryWorld — RAW: find entities by description keyword
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity) WHERE toLower(e.description) CONTAINS toLower(\"murder\") RETURN e.name, e.type, e.brief"}'

# queryWorld — WRITE
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"WRITE","query":"MERGE (e:Entity {name: \"Test_NPC\"}) SET e.type = \"CHARACTER\", e.brief = \"A debug test entity\" RETURN e"}'

# searchWorld
curl -s -X POST "http://localhost:3000/api/debug/tools/searchWorld" -H "Content-Type: application/json" -d '{"query":"weapon","types":["entities","plots"],"limit":3}' | jq .

# editNode — CREATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNode" -H "Content-Type: application/json" -d '{"nodeLabel":"Note","action":"CREATE","properties":{"name":"debug_note","content":"A test note from the debug endpoint"}}'

# editNode — UPDATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNode" -H "Content-Type: application/json" -d '{"nodeLabel":"Note","action":"UPDATE","match":{"name":"debug_note"},"properties":{"content":"Updated via debug endpoint"}}'

# editNode — DELETE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNode" -H "Content-Type: application/json" -d '{"nodeLabel":"Entity","action":"DELETE","match":{"name":"Test_NPC"}}'

# editRelationship — CREATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editRelationship" -H "Content-Type: application/json" -d '{"action":"CREATE","relationshipType":"LOCATED_AT","sourceLabel":"Entity","sourceMatch":{"name":"Player"},"targetLabel":"Location","targetMatch":{"name":"Engine Room"}}'

# manageSchema — register node type
curl -s -X POST "http://localhost:3000/api/debug/tools/manageSchema" -H "Content-Type: application/json" -d '{"target":"node","action":"register","name":"Artifact","description":"A magical or mechanical artifact","properties":[{"name":"power_level","description":"Numeric power rating","type":"number"},{"name":"origin","description":"Where it came from","type":"string"}]}'

# resetSceneContext
curl -s -X POST "http://localhost:3000/api/debug/tools/resetSceneContext" -H "Content-Type: application/json" -d '{}'
