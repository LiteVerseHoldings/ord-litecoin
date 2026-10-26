param(
  [Parameter(Mandatory = $true)][string]$Litecoind,
  [Parameter(Mandatory = $true)][string]$LitecoinCli,
  [string]$Ord = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target\debug\ord.exe')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

foreach ($executable in @($Litecoind, $LitecoinCli, $Ord)) {
  if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "Executable not found: $executable"
  }
}

if (@(Get-ChildItem Env:ORD_*).Count -gt 0) {
  throw 'Run in a shell without ORD_* environment overrides.'
}

function Get-FreePort {
  $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $listener.Start()
  try { return $listener.LocalEndpoint.Port } finally { $listener.Stop() }
}

function Start-TestProcess($File, $Arguments, $LogName) {
  $quoted = @($Arguments | ForEach-Object { '"{0}"' -f $_ })
  Start-Process -FilePath $File -ArgumentList $quoted -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput (Join-Path $root "$LogName.stdout.log") `
    -RedirectStandardError (Join-Path $root "$LogName.stderr.log")
}

function Invoke-Core {
  param([Parameter(ValueFromRemainingArguments = $true)][string[]]$RpcArguments)
  $output = & $LitecoinCli @coreOptions @RpcArguments
  if ($LASTEXITCODE -ne 0) { throw "Isolated regtest RPC failed: $($RpcArguments[0])" }
  $output -join "`n"
}

function Assert-Equal($Actual, $Expected, $Label) {
  if ($Actual -ne $Expected) { throw "$Label failed: expected $Expected, got $Actual" }
}

function Assert-HttpError($Path, $Method, $Body, $Headers, $ExpectedStatus) {
  Add-Type -AssemblyName System.Net.Http
  $client = [System.Net.Http.HttpClient]::new()
  $client.Timeout = [TimeSpan]::FromSeconds(10)
  $request = [System.Net.Http.HttpRequestMessage]::new(
    [System.Net.Http.HttpMethod]::new($Method), "$baseUrl$Path")
  $response = $null
  try {
    foreach ($header in $Headers.GetEnumerator()) {
      $request.Headers.TryAddWithoutValidation($header.Key, $header.Value) | Out-Null
    }
    if ($null -ne $Body) {
      $request.Content = [System.Net.Http.StringContent]::new(
        $Body, [System.Text.Encoding]::UTF8, 'application/json')
    }
    $response = $client.SendAsync($request).GetAwaiter().GetResult()
    Assert-Equal ([int]$response.StatusCode) $ExpectedStatus $Path
  } finally {
    if ($null -ne $response) { $response.Dispose() }
    $request.Dispose()
    $client.Dispose()
  }
}

$root = Join-Path ([System.IO.Path]::GetTempPath()) ("ord-litecoin-regtest-" + [guid]::NewGuid())
$nodeDir = Join-Path $root 'core'
$indexDir = Join-Path $root 'ord'
New-Item -ItemType Directory -Path $nodeDir, $indexDir | Out-Null
$rpcPort = Get-FreePort
$httpPort = Get-FreePort
while ($httpPort -eq $rpcPort) { $httpPort = Get-FreePort }
$coreOptions = @('-regtest', "-datadir=$nodeDir", "-rpcport=$rpcPort", '-rpcclienttimeout=5')
$ordOptions = @('--regtest', '--litecoin-data-dir', $nodeDir,
  '--litecoin-rpc-url', "http://127.0.0.1:$rpcPort", '--data-dir', $indexDir,
  '--index-cache-size', '67108864')
$core = $null
$server = $null
$baseUrl = "http://127.0.0.1:$httpPort"

try {
  $core = Start-TestProcess $Litecoind (@('-regtest', "-datadir=$nodeDir", "-rpcport=$rpcPort") + @('-server=1', '-listen=0',
    '-connect=0', '-dnsseed=0', '-discover=0', '-upnp=0', '-txindex=1',
    '-rpcbind=127.0.0.1', '-rpcallowip=127.0.0.1')) 'core'

  $chain = $null
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  while ([DateTime]::UtcNow -lt $deadline) {
    if ($core.HasExited) { throw 'Isolated Litecoin node exited before readiness' }
    try {
      $json = & $LitecoinCli @coreOptions 'getblockchaininfo' 2>$null
      if ($LASTEXITCODE -eq 0) {
        $chain = ($json -join "`n") | ConvertFrom-Json
        break
      }
    } catch {}
    Start-Sleep -Milliseconds 250
  }
  if ($null -eq $chain) { throw 'Isolated Litecoin node did not become ready' }
  Assert-Equal $chain.chain 'regtest' 'isolated chain'
  Assert-Equal $chain.blocks 0 'fresh chain'
  Assert-Equal (Invoke-Core 'getconnectioncount') '0' 'peer count'

  $address = 'rltc1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqnmxrc5'
  $blocks = @(Invoke-Core 'generatetoaddress' '3' $address | ConvertFrom-Json)
  Assert-Equal $blocks.Count 3 'generated synthetic blocks'

  & $Ord @ordOptions 'index' 'update'
  if ($LASTEXITCODE -ne 0) { throw 'Ord could not index real Litecoin regtest blocks' }

  $server = Start-TestProcess $Ord ($ordOptions + @('server', '--address', '127.0.0.1',
    '--http-port', "$httpPort", '--polling-interval', '250ms')) 'ord'

  $headers = @{ Accept = 'application/json' }
  $status = $null
  for ($attempt = 0; $attempt -lt 60; $attempt++) {
    if ($server.HasExited) { throw 'Isolated Ord server exited before readiness' }
    try {
      $status = Invoke-RestMethod -Uri "$baseUrl/status" -Headers $headers -TimeoutSec 1
      break
    } catch { Start-Sleep -Milliseconds 250 }
  }
  if ($null -eq $status) { throw 'Isolated Ord server did not become ready' }
  Assert-Equal $status.chain 'regtest' 'Ord chain'
  Assert-Equal $status.height 3 'Ord indexed height'
  Assert-Equal $status.unrecoverably_reorged $false 'Ord reorganization state'

  $genesis = Invoke-RestMethod -Uri "$baseUrl/block/0" -Headers $headers -TimeoutSec 10
  Assert-Equal $genesis.hash (Invoke-Core 'getblockhash' '0') 'Core/Ord genesis identity'

  $block = Invoke-Core 'getblock' $blocks[0] | ConvertFrom-Json
  $outpoint = "$($block.tx[0]):0"
  $output = Invoke-RestMethod -Uri "$baseUrl/output/$outpoint" -Headers $headers -TimeoutSec 10
  Assert-Equal $output.value 5000000000 'Litecoin output value in litoshis'
  Assert-Equal $output.address $address 'Litecoin output address'

  $tip = @(Invoke-Core 'generatetoaddress' '1' $address | ConvertFrom-Json)[0]
  for ($attempt = 0; $attempt -lt 60; $attempt++) {
    $status = Invoke-RestMethod -Uri "$baseUrl/status" -Headers $headers -TimeoutSec 1
    if ($status.height -eq 4) { break }
    Start-Sleep -Milliseconds 250
  }
  Assert-Equal $status.height 4 'live block progression'
  $latest = Invoke-RestMethod -Uri "$baseUrl/block/4" -Headers $headers -TimeoutSec 10
  Assert-Equal $latest.hash $tip 'Core/Ord live tip identity'

  $batch = ConvertTo-Json -InputObject @((1..101) | ForEach-Object { $outpoint }) -Compress
  Assert-HttpError '/outputs' 'POST' $batch $headers 400
  Assert-HttpError '/content/0000000000000000000000000000000000000000000000000000000000000000i0' `
    'GET' $null @{ 'Service-Worker' = 'script' } 403

  [ordered]@{
    result = 'passed'
    chain = 'regtest'
    peers = 0
    height = $status.height
    live_block_progression = $true
    genesis_matches_core = $true
    litecoin_output_matches_core = $true
    batch_limit_enforced = $true
    service_workers_blocked = $true
    evidence_directory = $root
  } | ConvertTo-Json
} finally {
  if ($null -ne $server -and -not $server.HasExited) {
    $server.Kill()
    $server.WaitForExit()
  }
  if ($null -ne $core -and -not $core.HasExited) {
    try { Invoke-Core 'stop' | Out-Null } catch { $core.Kill() }
    if (-not $core.WaitForExit(10000)) {
      $core.Kill()
      $core.WaitForExit()
    }
  }
}
