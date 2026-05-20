# Debug endpoint examples — paste-n-run each line separately.
# Assumes server running on localhost:3000.

# queryWorld — READ (raw JSON)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity) RETURN e.name, e.type LIMIT 5"}'

# queryWorld — READ: browse time history (raw)
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (tp:TimePoint) RETURN tp.day, tp.segment, tp.label ORDER BY tp.day, tp.segment"}'

# queryWorld — RAW: find entities by description keyword
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"READ","query":"MATCH (e:Entity) WHERE toLower(e.description) CONTAINS toLower(\"murder\") RETURN e.name, e.type, e.brief"}'

# queryWorld — WRITE
curl -s -X POST "http://localhost:3000/api/debug/tools/queryWorld" -H "Content-Type: application/json" -d '{"action":"WRITE","query":"MERGE (e:Entity {name: \"Test_NPC\"}) SET e.type = \"CHARACTER\", e.brief = \"A debug test entity\" RETURN e"}'

# searchWorld
curl -s -X POST "http://localhost:3000/api/debug/tools/searchWorld" -H "Content-Type: application/json" -d '{"query":"weapon","target":"node","domains":["Entity","Plot"],"limit":3}' | jq .

# editNode — CREATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNode" -H "Content-Type: application/json" -d '{"nodeLabel":"Entity","action":"CREATE","properties":{"name":"debug_entity","type":"CHARACTER","brief":"A debug test entity"}}'

# editNode — UPDATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNode" -H "Content-Type: application/json" -d '{"nodeLabel":"Entity","action":"UPDATE","match":{"name":"debug_entity"},"properties":{"brief":"Updated via debug endpoint"}}'

# editNode — DELETE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNode" -H "Content-Type: application/json" -d '{"nodeLabel":"Entity","action":"DELETE","match":{"name":"Test_NPC"}}'

# editRelationship — CREATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editRelationship" -H "Content-Type: application/json" -d '{"action":"CREATE","relationshipType":"LOCATED_AT","sourceLabel":"Entity","sourceMatch":{"name":"Player"},"targetLabel":"Location","targetMatch":{"name":"Engine Room"}}'

# manageSchema — register node type
curl -s -X POST "http://localhost:3000/api/debug/tools/manageSchema" -H "Content-Type: application/json" -d '{"target":"node","action":"register","name":"Artifact","description":"A magical or mechanical artifact","properties":[{"name":"power_level","description":"Numeric power rating","tags":["number"]},{"name":"origin","description":"Where it came from","tags":["string"]}]}'

# editNote — CREATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNote" -H "Content-Type: application/json" -d '{"noteName":"debug_note","action":"CREATE","content":"A GM scratchpad note from the debug endpoint"}'

# editNote — UPDATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNote" -H "Content-Type: application/json" -d '{"noteName":"debug_note","action":"UPDATE","content":"Updated note content"}'

# editNote — DELETE
curl -s -X POST "http://localhost:3000/api/debug/tools/editNote" -H "Content-Type: application/json" -d '{"noteName":"debug_note","action":"DELETE"}'

# editPlot — CREATE
curl -s -X POST "http://localhost:3000/api/debug/tools/editPlot" -H "Content-Type: application/json" -d '{"plotName":"debug_plot","action":"CREATE","description":"A test plot from the debug endpoint"}'

# editPlot — UPDATE with status change
curl -s -X POST "http://localhost:3000/api/debug/tools/editPlot" -H "Content-Type: application/json" -d '{"plotName":"debug_plot","action":"UPDATE","status":"COMPLETED"}'

# editPlot — DELETE
curl -s -X POST "http://localhost:3000/api/debug/tools/editPlot" -H "Content-Type: application/json" -d '{"plotName":"debug_plot","action":"DELETE"}'

# getContext
curl -s -X POST "http://localhost:3000/api/debug/tools/getContext" -H "Content-Type: application/json" -d '{"types":["SCENE_CONTEXT","CHARACTERS_BRIEF","LOCATIONS_BRIEF","OBJECTS_BRIEF","PLOTS_BRIEF","SCHEMA_DUMP","RELATIONSHIP_DUMP"]}'
