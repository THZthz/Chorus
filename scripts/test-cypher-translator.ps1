# Test suites for POST /api/query/intent and /api/query/intents
param(
  [switch]$All,
  [switch]$Basic,
  [switch]$Complex,
  [switch]$Batch,
  [switch]$Edge
)
$base = "http://localhost:3000"
$jq = '.results[] | "\n============================================================\nINTENT: \(.intent)\nQUERY: \(.result.query // "N/A")\nROWS: \(.result.rowCount // 0)\n============================================================\n\n\(.result.markdown // "ERROR: \(.error)")\n"'

function Run-Intents {
  param($Label, [string[]]$Intents)
  Write-Host "`n$Label" -ForegroundColor Cyan
  Write-Host ('=' * 72)
  $start = Get-Date
  $json = ConvertTo-Json @{ intents = $Intents } -Compress
  $result = curl -s -X POST "$base/api/query/intents" -H 'Content-Type: application/json' -d $json
  $end = Get-Date
  $result | jq -r $jq 2>$null
  Write-Host "`n$(($end - $start).TotalSeconds)s" -ForegroundColor Yellow
}

# ── Basic: single-label lookups ──
$basicIntents = @(
  "Find ALL Character entities with their name, type, and brief",
  "Find all Location entities with their name and description",
  "Find all Object entities with their name, type, and brief",
  "List all NPCDisposition entries toward the Player",
  "Show all active and in-progress plots with their status and trigger condition"
)

# ── Complex: multi-hop / aggregations ──
$complexIntents = @(
  "Find all characters at the player location along with their disposition toward the player",
  "Count how many entities of each type exist",
  "Show the plot tree hierarchy with each plot status and child plots",
  "Find all locations and list who and what is at each one, including objects carried by characters there",
  "List all notes linked to the player character via ABOUT_ENTITY"
)

# ── Batch: parallel diverse queries ──
$batchIntents = @(
  "Find all characters that are hostile toward the player",
  "Show all notes created in the world with their content summary",
  "Find locations that have no characters present at them",
  "List the most recent 10 messages with their role and speaker",
  "Show all completed plots and when they were completed"
)

# ── Edge: tricky for the small model ──
$edgeIntents = @(
  "Find pairs of characters that are mutually hostile to each other",
  "Show entities that have no relationships at all (isolated)",
  "Count how many entities are at each location",
  "Find the time point when each plot became active",
  "List all relationship types in use with example connections",
  "Show the conversation history between Player and Veyla, ordered by time"
)

$runAll = $All -or (-not ($Basic -or $Complex -or $Batch -or $Edge))

if ($runAll -or $Basic)   { Run-Intents "BASIC"   $basicIntents }
if ($runAll -or $Complex) { Run-Intents "COMPLEX" $complexIntents }
if ($runAll -or $Batch)   { Run-Intents "BATCH"   $batchIntents }
if ($runAll -or $Edge)    { Run-Intents "EDGE"    $edgeIntents }
