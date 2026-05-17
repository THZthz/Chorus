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

# ── Tool invocation (POST JSON body as tool args) ──
#
# About rawResult + instruction (phi-4-mini-instruct, 3.8B params):
#   CAN do:  column projection, rename, sort, convert to table/list — mechanical line-level transforms.
#   CANNOT:  aggregate (count/sum/group-by), semantic grouping, summarization, cross-row analysis.
#   Rule:    filtering and aggregation MUST be done in Cypher (WHERE, COUNT, DISTINCT, ORDER BY).
#            Use the local LLM only for cosmetic formatting of already-filtered Cypher results.

# queryWorld — READ (raw JSON, no formatting)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity) RETURN e.name, e.type LIMIT 5"}'

# queryWorld — READ: browse time history (raw)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (tp:TimePoint) RETURN tp.day, tp.segment, tp.label ORDER BY tp.day, tp.segment"}'

# queryWorld — FORMATTED: pick columns + sort + table (pure mechanical, safe)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity) RETURN e.name, e.type, e.brief LIMIT 20","rawResult":false,"reasoning":"hard","instruction":"Sort by type, then alphabetically by name. Format as a markdown table with columns Name, Type, Brief."}'

# queryWorld — FORMATTED: rename Cypher columns to readable headers
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity)-[r]->(t:Entity) RETURN e.name AS source, type(r) AS rel, t.name AS target LIMIT 30","rawResult":false,"instruction":"Format as a markdown table with columns Source, Relationship, Target. Sort alphabetically by Relationship."}'

# queryWorld — AGGREGATION IN CYPHER: count entities by type (do the math in Cypher, format only)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity) RETURN e.type AS type, count(e) AS count ORDER BY count DESC","rawResult":false,"instruction":"Format as a markdown table: Type, Count."}'

# queryWorld — AGGREGATION IN CYPHER: NPC dispositions toward the player
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (d:NPCDisposition) WHERE d.target_name = \"Player\" RETURN d.npc_name AS NPC, d.sentiment AS Sentiment, d.summary AS Summary","rawResult":false,"instruction":"Format as a markdown table: NPC, Sentiment, Summary. Sort by Sentiment."}'

# queryWorld — AGGREGATION IN CYPHER: active plots with their flags
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (p:Plot) WHERE p.status IN [\"ACTIVE\", \"IN_PROGRESS\"] RETURN p.name AS Plot, p.status AS Status, p.brief AS Brief","rawResult":false,"instruction":"Format as a markdown table: Plot, Status, Brief. Sort by Status then Plot."}'

# queryWorld — RAW: find entities by description keyword (Cypher does the search, no local LLM)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity) WHERE toLower(e.description) CONTAINS toLower(\"murder\") RETURN e.name, e.type, e.brief"}'

# queryWorld — FORMATTED: plot tree as indented ASCII (parent→child edges, formatter builds tree)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (p:Plot) OPTIONAL MATCH (p)-[:BRANCHES_TO]->(c:Plot) RETURN p.name AS parent, p.status AS status, collect(c.name) AS children ORDER BY parent","rawResult":false,"reasoning":"normal","instruction":"Build an ASCII tree from parent-child pairs. Include status in brackets after each name."}'

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
