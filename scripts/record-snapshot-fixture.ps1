<#
.SYNOPSIS
  Records the snapshot Fixture: a regtest UTXO snapshot plus the Oracle's view of every coin in it.

.DESCRIPTION
  Starts a throwaway regtest node, pays one output of each script type the snapshot parser
  and Move cost must handle (P2PK compressed and uncompressed, bare multisig 1-of-1 to
  3-of-3, P2TR, P2PKH, P2SH, P2WPKH, P2WSH), mines it, then runs `dumptxoutset`.

  The Oracle is Bitcoin Core: `gettxoutsetinfo` for the coin count and total amount, and
  `gettxout` for every output of every block, which lists the whole UTXO set coin by coin
  (value, height, coinbase flag and full scriptPubKey). Writes regtest-utxo.dat and
  regtest-utxo.json to the snapshot fixtures directory. Not run in CI.

  Every run uses fresh keys and timestamps, so the snapshot bytes change on every run.

.EXAMPLE
  ./scripts/record-snapshot-fixture.ps1
#>
param(
    [string]$BitcoinBin = 'C:\Program Files\Bitcoin\daemon',
    [string]$OutDir = (Join-Path (Split-Path $PSScriptRoot) 'crates\pqweight\tests\fixtures\snapshot')
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

# A BTC amount as Core prints it (8 decimal places) in satoshis, without going through a double.
function ConvertTo-Sats([string]$Btc) {
    return [long]([decimal]::Parse($Btc, [Globalization.CultureInfo]::InvariantCulture) * 100000000)
}

function ConvertTo-Hex([byte[]]$Bytes) { return (($Bytes | ForEach-Object { $_.ToString('x2') }) -join '') }

# Compact size, for script lengths below 253 (every script here).
function Get-LengthByte([string]$Hex) { return ([byte]($Hex.Length / 2)).ToString('x2') }

function Get-Push([string]$Hex) { return (Get-LengthByte $Hex) + $Hex }

# OP_m <keys> OP_n OP_CHECKMULTISIG, m and n from 1 to 16.
function Get-MultisigScript([int]$M, [string[]]$Keys) {
    return (0x50 + $M).ToString('x2') + (($Keys | ForEach-Object { Get-Push $_ }) -join '') + (0x50 + $Keys.Count).ToString('x2') + 'ae'
}

# The generator point G, uncompressed (even y), and -G (odd y). Valid keys, so Core's
# script compressor stores them as compressed-script codes 4 and 5. Nobody can spend them.
$uncompressedEven = '0479be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798483ada7726a3c4655da4fbfc0e1108a8fd17b448a68554199c47d08ffb10d4b8'
$uncompressedOdd = '0479be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798b7c52588d95c3b9aa25b0403f1eef75702e84bb7597aabe663b82f6f04ef2777'
# Right length and prefix but not a point on the curve, so Core stores the script raw.
$uncompressedInvalid = '04' + ('00' * 64)

New-Item -ItemType Directory -Force $OutDir | Out-Null
New-Item -ItemType Directory -Force $dataDir | Out-Null

# Bare multisig has not been relayed by default since Core v28.
$nodeArgs = @('-regtest', "-datadir=$dataDir", '-server', '-listen=0', '-fallbackfee=0.0001', '-permitbaremultisig=1', '-acceptnonstdtxn=1', '-changetype=bech32')
$node = Start-Process $bitcoind -ArgumentList $nodeArgs -WindowStyle Hidden -PassThru
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
    $minerAddress = (Invoke-Rpc 'getnewaddress').Trim()
    Invoke-Rpc 'generatetoaddress' @('nblocks=101', "address=$minerAddress") | Out-Null

    $keys = 1..3 | ForEach-Object {
        $address = (Invoke-Rpc 'getnewaddress' @('address_type=bech32')).Trim()
        (Invoke-RpcJson 'getaddressinfo' @("address=$address")).pubkey
    }
    function Get-AddressScript([string]$Type) {
        $address = (Invoke-Rpc 'getnewaddress' @("address_type=$Type")).Trim()
        return (Invoke-RpcJson 'getaddressinfo' @("address=$address")).scriptPubKey
    }
    $p2wsh = '0020' + (ConvertTo-Hex ([Security.Cryptography.SHA256]::Create().ComputeHash([byte[]](0x51))))
    $p2sh = 'a914' + ('11' * 20) + '87'

    # Script, amount in satoshis. P2TR at 545 and 546 straddles the dust cut Move cost reports.
    $outputs = @(
        @{ Script = (Get-Push $keys[0]) + 'ac'; Sats = 100000000 },
        @{ Script = (Get-Push $uncompressedEven) + 'ac'; Sats = 12345678 },
        @{ Script = (Get-Push $uncompressedOdd) + 'ac'; Sats = 5000 },
        @{ Script = (Get-Push $uncompressedInvalid) + 'ac'; Sats = 6000 },
        @{ Script = (Get-MultisigScript 1 @($keys[0])); Sats = 7000 },
        @{ Script = (Get-MultisigScript 1 @($keys[0], $uncompressedEven)); Sats = 8000 },
        @{ Script = (Get-MultisigScript 2 @($keys[0], $keys[1])); Sats = 9000 },
        @{ Script = (Get-MultisigScript 2 @($keys[0], $keys[1], $keys[2])); Sats = 10000 },
        @{ Script = (Get-MultisigScript 3 @($keys[0], $keys[1], $keys[2])); Sats = 11000 },
        @{ Script = (Get-AddressScript 'bech32m'); Sats = 2100000000 },
        @{ Script = (Get-AddressScript 'bech32m'); Sats = 546 },
        @{ Script = (Get-AddressScript 'bech32m'); Sats = 545 },
        @{ Script = (Get-AddressScript 'legacy'); Sats = 300000 },
        @{ Script = (Get-AddressScript 'bech32'); Sats = 400000 },
        @{ Script = $p2sh; Sats = 500000 },
        @{ Script = $p2wsh; Sats = 600000 }
    )

    # An unsigned transaction with no inputs: version, input count 0, the outputs, locktime.
    # fundrawtransaction adds inputs and change from the wallet's coinbase coins.
    $raw = '02000000' + '00' + ([byte]$outputs.Count).ToString('x2')
    foreach ($output in $outputs) {
        $raw += (ConvertTo-Hex ([BitConverter]::GetBytes([long]$output.Sats))) + (Get-Push $output.Script)
    }
    $raw += '00000000'
    $funded = Invoke-RpcJson 'fundrawtransaction' @("hexstring=$raw", 'iswitness=false')
    $signed = Invoke-RpcJson 'signrawtransactionwithwallet' @("hexstring=$($funded.hex)")
    if (-not $signed.complete) { throw 'signing the funding transaction failed' }
    Invoke-Rpc 'sendrawtransaction' @("hexstring=$($signed.hex)", 'maxfeerate=0') | Out-Null
    Invoke-Rpc 'generatetoaddress' @('nblocks=1', "address=$minerAddress") | Out-Null

    $snapshotPath = Join-Path $OutDir 'regtest-utxo.dat'
    if (Test-Path $snapshotPath) { Remove-Item $snapshotPath }
    $dump = Invoke-RpcJson 'dumptxoutset' @("path=$snapshotPath", 'type=latest') -NoWallet
    $infoText = Invoke-Rpc 'gettxoutsetinfo' @('hash_type=none') -NoWallet
    $info = $infoText | ConvertFrom-Json
    if ($infoText -notmatch '"total_amount":\s*([0-9.]+)') { throw 'no total_amount in gettxoutsetinfo' }
    $totalAmount = ConvertTo-Sats $Matches[1]
    if ($info.txouts -ne $dump.coins_written) { throw "gettxoutsetinfo says $($info.txouts) coins, dumptxoutset wrote $($dump.coins_written)" }

    # Every unspent output, coin by coin: walk every block and ask gettxout about each output.
    # Height 0 is skipped: the genesis coinbase is never added to the UTXO set.
    $coins = New-Object System.Collections.ArrayList
    for ($height = 1; $height -le [int]$info.height; $height++) {
        $hash = (Invoke-Rpc 'getblockhash' @("height=$height") -NoWallet).Trim()
        $block = Invoke-RpcJson 'getblock' @("blockhash=$hash", 'verbosity=2') -NoWallet
        foreach ($tx in $block.tx) {
            $txid = $tx.txid
            foreach ($vout in $tx.vout) {
                $coinText = Invoke-Rpc 'gettxout' @("txid=$txid", "n=$($vout.n)", 'include_mempool=false') -NoWallet
                if (-not $coinText.Trim()) { continue }
                $coin = $coinText | ConvertFrom-Json
                # Amounts are re-read from the raw text so no double rounding is involved.
                if ($coinText -notmatch '"value":\s*([0-9.]+)') { throw "no value in gettxout for ${txid}:$($vout.n)" }
                [void]$coins.Add([ordered]@{
                    txid     = $txid
                    vout     = [int]$vout.n
                    height   = $height
                    coinbase = [bool]$coin.coinbase
                    value    = (ConvertTo-Sats $Matches[1])
                    script   = $coin.scriptPubKey.hex
                    type     = $coin.scriptPubKey.type
                })
            }
        }
    }
    if ($coins.Count -ne $info.txouts) { throw "walked $($coins.Count) coins, gettxoutsetinfo says $($info.txouts)" }

    $meta = [ordered]@{
        description  = 'Regtest UTXO snapshot (dumptxoutset latest) holding one coin of each script type Move cost handles, plus coinbase and change coins'
        oracle       = [ordered]@{
            base_hash    = $dump.base_hash
            base_height  = [int]$dump.base_height
            coins_count  = [int]$info.txouts
            total_amount = $totalAmount
            coins        = $coins
        }
        core_version = $coreVersion
        recorded_by  = 'scripts/record-snapshot-fixture.ps1'
    }
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $OutDir 'regtest-utxo.json'), (($meta | ConvertTo-Json -Depth 6) + "`n"), $utf8)
    Write-Host ("  regtest-utxo  coins={0} height={1} bytes={2}" -f $info.txouts, $dump.base_height, (Get-Item $snapshotPath).Length)
}
finally {
    Invoke-Cli -CliArgs @('-regtest', "-datadir=$dataDir", 'stop') | Out-Null
    Start-Sleep 3
    if ($node -and -not $node.HasExited) { Stop-Process $node -Force }
    Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue
}
