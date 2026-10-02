<#
.SYNOPSIS
  Records the block Fixture: a regtest block plus Bitcoin Core's view of it.

.DESCRIPTION
  Starts a throwaway regtest node and mines one block holding a coinbase, a legacy
  (non-witness) transaction and a segwit transaction. Three transactions make the txid
  merkle tree's first level odd, so the last hash is duplicated, and the segwit spend makes
  Core add a witness commitment to the coinbase.

  The Oracle is Bitcoin Core: `getblock <hash> 0` for the bytes, and `getblock <hash> 1`
  for the hash, merkle root, weight, sizes, transaction count and txids. Writes
  regtest-block.hex and regtest-block.json to the block fixtures directory, which is kept
  apart from the transaction Fixtures so their loops don't pick it up. Not run in CI.

  Every run uses fresh keys and timestamps, so the block changes on every run.

.EXAMPLE
  ./scripts/record-block-fixture.ps1
#>
param(
    [string]$BitcoinBin = 'C:\Program Files\Bitcoin\daemon',
    [string]$OutDir = (Join-Path (Split-Path $PSScriptRoot) 'crates\pqweight\tests\fixtures\block')
)

$ErrorActionPreference = 'Stop'

$bitcoind = Join-Path $BitcoinBin 'bitcoind.exe'
# As in record-fixtures.ps1: bitcoin-cli is found via PATH and temp paths must not contain spaces.
$env:Path = "$BitcoinBin;$env:Path"
$dataDir = Join-Path ([IO.Path]::GetTempPath()) ("pqweight-regtest-" + [Guid]::NewGuid().ToString('N'))

# Runs bitcoin-cli through cmd.exe; see record-fixtures.ps1 for why.
function Invoke-Cli {
    param([string[]]$CliArgs, [string[]]$Stdin = @())
    $argLine = $CliArgs -join ' '
    if ($Stdin.Count -eq 0) {
        $lines = & cmd.exe /c "bitcoin-cli $argLine 2>&1"
    }
    else {
        $inputFile = [IO.Path]::GetTempFileName()
        try {
            [IO.File]::WriteAllText($inputFile, (($Stdin -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
            $lines = & cmd.exe /c "bitcoin-cli $argLine < $inputFile 2>&1"
        }
        finally { [IO.File]::Delete($inputFile) }
    }
    return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = (($lines | ForEach-Object { "$_" }) -join "`n") }
}

function Invoke-Rpc {
    param([string]$Method, [string[]]$Params = @(), [switch]$NoWallet)
    $cliArgs = @('-regtest', "-datadir=$dataDir", '-named')
    if (-not $NoWallet -and $Method -ne 'createwallet') { $cliArgs += '-rpcwallet=fixtures' }
    if ($Params.Count -gt 0) { $cliArgs += '-stdin' }
    $result = Invoke-Cli -CliArgs ($cliArgs + $Method) -Stdin $Params
    if ($result.ExitCode -ne 0) { throw "bitcoin-cli $Method failed: $($result.Output)" }
    return $result.Output
}

function Invoke-RpcJson {
    param([string]$Method, [string[]]$Params = @(), [switch]$NoWallet)
    return (Invoke-Rpc $Method $Params -NoWallet:$NoWallet) | ConvertFrom-Json
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
New-Item -ItemType Directory -Force $dataDir | Out-Null

# Without -changetype, the funding transaction's change would match its legacy payment, and
# the segwit spend below could pick that change and end up legacy too.
$node = Start-Process $bitcoind -ArgumentList @('-regtest', "-datadir=$dataDir", '-server', '-listen=0', '-fallbackfee=0.0001', '-changetype=bech32') -WindowStyle Hidden -PassThru
try {
    $ready = $false
    for ($i = 0; $i -lt 60 -and -not $ready; $i++) {
        $probe = Invoke-Cli -CliArgs @('-regtest', "-datadir=$dataDir", 'getblockchaininfo')
        if ($probe.ExitCode -eq 0) { $ready = $true } else { Start-Sleep 1 }
    }
    if (-not $ready) { throw 'bitcoind did not become ready' }

    $network = (Invoke-Cli -CliArgs @('-regtest', "-datadir=$dataDir", 'getnetworkinfo')).Output | ConvertFrom-Json
    $coreVersion = $network.subversion.Trim('/')
    Write-Host "Recording against $coreVersion (regtest)"

    Invoke-Rpc 'createwallet' @('wallet_name=fixtures') | Out-Null
    $minerAddress = (Invoke-Rpc 'getnewaddress' @('address_type=bech32')).Trim()
    Invoke-Rpc 'generatetoaddress' @('nblocks=101', "address=$minerAddress") | Out-Null

    # A legacy coin to spend, confirmed one block before the Fixture block.
    $legacyAddress = (Invoke-Rpc 'getnewaddress' @('address_type=legacy')).Trim()
    $fundingTxid = (Invoke-Rpc 'sendtoaddress' @("address=$legacyAddress", 'amount=1')).Trim()
    Invoke-Rpc 'generatetoaddress' @('nblocks=1', "address=$minerAddress") | Out-Null
    $funding = Invoke-RpcJson 'gettransaction' @("txid=$fundingTxid", 'verbose=true')
    $vout = ($funding.decoded.vout | Where-Object { $_.scriptPubKey.address -eq $legacyAddress }).n

    # Spends only that coin to another legacy address, so the transaction has no witness.
    $legacyOut = (Invoke-Rpc 'getnewaddress' @('address_type=legacy')).Trim()
    $raw = (Invoke-Rpc 'createrawtransaction' @("inputs=[{`"txid`":`"$fundingTxid`",`"vout`":$vout}]", "outputs=[{`"$legacyOut`":0.999}]")).Trim()
    $signed = Invoke-RpcJson 'signrawtransactionwithwallet' @("hexstring=$raw")
    if (-not $signed.complete) { throw 'signing the legacy spend failed' }
    $legacyTxid = (Invoke-Rpc 'sendrawtransaction' @("hexstring=$($signed.hex)")).Trim()

    # A segwit spend: the wallet's coins are all P2WPKH coinbase outputs.
    $segwitOut = (Invoke-Rpc 'getnewaddress' @('address_type=bech32')).Trim()
    $segwitTxid = (Invoke-Rpc 'sendtoaddress' @("address=$segwitOut", 'amount=2')).Trim()
    # A transaction has witness data exactly when its wtxid differs from its txid.
    $segwitDecoded = (Invoke-RpcJson 'gettransaction' @("txid=$segwitTxid", 'verbose=true')).decoded
    if ($segwitDecoded.hash -eq $segwitDecoded.txid) { throw 'the segwit spend has no witness' }
    $legacyDecoded = (Invoke-RpcJson 'gettransaction' @("txid=$legacyTxid", 'verbose=true')).decoded
    if ($legacyDecoded.hash -ne $legacyDecoded.txid) { throw 'the legacy spend has a witness' }

    $blockHash = (Invoke-RpcJson 'generatetoaddress' @('nblocks=1', "address=$minerAddress"))[0]
    $hex = (Invoke-Rpc 'getblock' @("blockhash=$blockHash", 'verbosity=0') -NoWallet).Trim()
    $block = Invoke-RpcJson 'getblock' @("blockhash=$blockHash", 'verbosity=1') -NoWallet
    if ($block.nTx -ne 3) { throw "expected 3 transactions, Core mined $($block.nTx)" }
    if (($legacyTxid -notin $block.tx) -or ($segwitTxid -notin $block.tx)) { throw 'the block is missing a spend' }

    $meta = [ordered]@{
        description  = 'Regtest block: coinbase with witness commitment, one legacy P2PKH spend and one P2WPKH spend (3 transactions, so the merkle tree duplicates a hash)'
        oracle       = [ordered]@{
            hash         = $block.hash
            merkleroot   = $block.merkleroot
            weight       = [int]$block.weight
            size         = [int]$block.size
            strippedsize = [int]$block.strippedsize
            nTx          = [int]$block.nTx
            tx           = @($block.tx)
        }
        core_version = $coreVersion
        recorded_by  = 'scripts/record-block-fixture.ps1'
    }
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $OutDir 'regtest-block.hex'), "$hex`n", $utf8)
    [IO.File]::WriteAllText((Join-Path $OutDir 'regtest-block.json'), (($meta | ConvertTo-Json -Depth 5) + "`n"), $utf8)
    Write-Host ("  regtest-block  hash={0} weight={1} nTx={2}" -f $block.hash, $block.weight, $block.nTx)
}
finally {
    Invoke-Cli -CliArgs @('-regtest', "-datadir=$dataDir", 'stop') | Out-Null
    Start-Sleep 3
    if ($node -and -not $node.HasExited) { Stop-Process $node -Force }
    Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue
}
