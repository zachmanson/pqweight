<#
.SYNOPSIS
  Records Fixtures: real signed transactions plus the Oracle's (Bitcoin Core's) weight and vsize.

.DESCRIPTION
  Starts a throwaway regtest node, funds one address per spend type, builds and signs a
  spend of each, and asks Core to decode it. Writes <name>.hex and <name>.json to the
  fixtures directory. Not run in CI: fixtures are committed so CI never needs a node.

  Covers P2PKH, P2WPKH, P2SH-P2WPKH, P2WSH 2-of-3 multisig and P2TR key-path.
  P2TR script-path, annex and large mainnet transactions are added by hand.

.EXAMPLE
  ./scripts/record-fixtures.ps1
#>
param(
    [string]$BitcoinBin = 'C:\Program Files\Bitcoin\daemon',
    [string]$OutDir = (Join-Path (Split-Path $PSScriptRoot) 'crates\pqweight\tests\fixtures')
)

$ErrorActionPreference = 'Stop'

$bitcoind = Join-Path $BitcoinBin 'bitcoind.exe'
# Invoke-Cli builds a cmd.exe command line, where a path with spaces would need
# escaping, so bitcoin-cli is found via PATH and the temp paths must not contain spaces.
$env:Path = "$BitcoinBin;$env:Path"
$dataDir = Join-Path ([IO.Path]::GetTempPath()) ("pqweight-regtest-" + [Guid]::NewGuid().ToString('N'))

# Runs bitcoin-cli and returns its exit code and combined output. Goes through cmd.exe
# so stderr is merged into stdout (PowerShell 5.1 turns native stderr into terminating
# errors) and so stdin can be redirected from a BOM-free file. PowerShell's own pipe to a
# native command always prepends a byte-order mark, which corrupts the first key=value line.
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

# Runs an RPC. Parameters are passed as key=value lines on stdin, because PowerShell 5.1
# mangles embedded double quotes in native command arguments.
function Invoke-Rpc {
    param([string]$Method, [string[]]$Params = @())
    $cliArgs = @('-regtest', "-datadir=$dataDir", '-named')
    if ($Method -ne 'createwallet') { $cliArgs += '-rpcwallet=fixtures' }
    if ($Params.Count -gt 0) { $cliArgs += '-stdin' }
    $result = Invoke-Cli -CliArgs ($cliArgs + $Method) -Stdin $Params
    if ($result.ExitCode -ne 0) { throw "bitcoin-cli $Method failed: $($result.Output)" }
    return $result.Output
}

function Invoke-RpcJson {
    param([string]$Method, [string[]]$Params = @())
    return (Invoke-Rpc $Method $Params) | ConvertFrom-Json
}

function Write-Fixture {
    param([string]$Name, [string]$Description, [string]$Hex, $Decoded, [string]$CoreVersion)
    $meta = [ordered]@{
        description  = $Description
        oracle       = [ordered]@{
            weight = [int]$Decoded.weight
            vsize  = [int]$Decoded.vsize
            size   = [int]$Decoded.size
        }
        core_version = $CoreVersion
        recorded_by  = 'scripts/record-fixtures.ps1'
    }
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $OutDir "$Name.hex"), "$Hex`n", $utf8)
    [IO.File]::WriteAllText((Join-Path $OutDir "$Name.json"), (($meta | ConvertTo-Json -Depth 5) + "`n"), $utf8)
    Write-Host ("  {0,-14} weight={1} vsize={2} size={3}" -f $Name, $Decoded.weight, $Decoded.vsize, $Decoded.size)
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
New-Item -ItemType Directory -Force $dataDir | Out-Null

$node = Start-Process $bitcoind -ArgumentList @('-regtest', "-datadir=$dataDir", '-server', '-listen=0', '-fallbackfee=0.0001') -WindowStyle Hidden -PassThru
try {
    # Wait for the node to accept RPC.
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

    # Build a 2-of-3 P2WSH multisig from keys the wallet already owns, so it can sign.
    $descriptors = (Invoke-RpcJson 'listdescriptors' @('private=true')).descriptors
    $wpkh = $descriptors | Where-Object { $_.desc -like 'wpkh(*' -and -not $_.internal } | Select-Object -First 1
    if ($wpkh.desc -notmatch '^wpkh\((.+)/0/\*\)#') { throw "unexpected descriptor shape: $($wpkh.desc)" }
    $base = $Matches[1]
    # Imports a descriptor that carries private keys and returns its address. The wallet can
    # then sign for it. deriveaddresses needs the private form, so the checksum (which
    # getdescriptorinfo computes for the input as given) is appended to that form.
    function Import-PrivateDescriptor([string]$Descriptor) {
        $info = Invoke-RpcJson 'getdescriptorinfo' @("descriptor=$Descriptor")
        $checked = "$Descriptor#$($info.checksum)"
        $request = '[{"desc":"' + $checked + '","timestamp":"now"}]'
        $imported = Invoke-RpcJson 'importdescriptors' @("requests=$request")
        if (-not $imported[0].success) { throw "importdescriptors failed: $($imported[0].error.message)" }
        return (Invoke-RpcJson 'deriveaddresses' @("descriptor=$checked"))[0]
    }

    $multisigAddress = Import-PrivateDescriptor "wsh(multi(2,$base/10/0,$base/11/0,$base/12/0))"
    # BIP-341's provably unspendable internal key, so only the script path can spend.
    $nums = '50929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0'
    $scriptPathAddress = Import-PrivateDescriptor "tr($nums,pk($base/13/0))"

    $types = @(
        @{ Name = 'p2pkh';       Description = 'Legacy P2PKH spend, 1 input, 1 output';                   Address = (Invoke-Rpc 'getnewaddress' @('address_type=legacy')).Trim() },
        @{ Name = 'p2wpkh';      Description = 'Native segwit P2WPKH spend, 1 input, 1 output';           Address = (Invoke-Rpc 'getnewaddress' @('address_type=bech32')).Trim() },
        @{ Name = 'p2sh-p2wpkh'; Description = 'Wrapped segwit P2SH-P2WPKH spend, 1 input, 1 output';    Address = (Invoke-Rpc 'getnewaddress' @('address_type=p2sh-segwit')).Trim() },
        @{ Name = 'p2wsh-multisig'; Description = 'P2WSH 2-of-3 multisig spend, 1 input, 1 output';      Address = $multisigAddress },
        @{ Name = 'p2tr-keypath'; Description = 'Taproot key-path spend, 1 input, 1 output';             Address = (Invoke-Rpc 'getnewaddress' @('address_type=bech32m')).Trim() },
        @{ Name = 'p2tr-scriptpath'; Description = 'Taproot script-path spend (single pk leaf, unspendable internal key), 1 input, 1 output'; Address = $scriptPathAddress }
    )

    # One funding transaction pays every address. Separate sends could spend each other's
    # outputs as inputs, consuming an address's coins before we get to spend them.
    $amounts = '{' + (($types | ForEach-Object { '"' + $_.Address + '":1' }) -join ',') + '}'
    Invoke-Rpc 'sendmany' @("amounts=$amounts") | Out-Null
    Invoke-Rpc 'generatetoaddress' @('nblocks=1', "address=$minerAddress") | Out-Null

    Write-Host "Writing fixtures to $OutDir"
    $signedByName = @{}
    foreach ($t in $types) {
        $unspentRaw = Invoke-Rpc 'listunspent' @('minconf=1', ('addresses=["' + $t.Address + '"]'))
        $utxo = @($unspentRaw | ConvertFrom-Json)[0]
        if (-not $utxo.txid) { throw "no spendable output for $($t.Name) at $($t.Address); listunspent said: $unspentRaw" }
        $destination = (Invoke-Rpc 'getnewaddress' @('address_type=bech32')).Trim()
        $inputs = '[{"txid":"' + $utxo.txid + '","vout":' + $utxo.vout + '}]'
        $outputs = '[{"' + $destination + '":0.999}]'
        $raw = (Invoke-Rpc 'createrawtransaction' @("inputs=$inputs", "outputs=$outputs")).Trim()
        $signed = Invoke-RpcJson 'signrawtransactionwithwallet' @("hexstring=$raw")
        if (-not $signed.complete) { throw "signing incomplete for $($t.Name)" }
        $decoded = Invoke-RpcJson 'decoderawtransaction' @("hexstring=$($signed.hex)")
        $signedByName[$t.Name] = $signed.hex
        Write-Fixture -Name $t.Name -Description $t.Description -Hex $signed.hex -Decoded $decoded -CoreVersion $coreVersion
    }

    # A transaction with 260 outputs: the output count no longer fits in one byte, so it is
    # serialized with the 3-byte compact size (0xFD prefix). Paid from the wallet's own coins.
    $recipients = 1..260 | ForEach-Object { '"' + (Invoke-Rpc 'getnewaddress' @('address_type=bech32')).Trim() + '":0.01' }
    $manyTxid = (Invoke-Rpc 'sendmany' @('amounts={' + ($recipients -join ',') + '}')).Trim()
    $manyHex = (Invoke-Rpc 'getrawtransaction' @("txid=$manyTxid")).Trim()
    $manyDecoded = Invoke-RpcJson 'decoderawtransaction' @("hexstring=$manyHex")
    Write-Fixture -Name 'many-outputs' -Description 'Funding transaction with 260 outputs, so the output count needs a 3-byte compact size' -Hex $manyHex -Decoded $manyDecoded -CoreVersion $coreVersion

    # Regtest cannot produce an annex, so append one to the witness of the key-path spend.
    # Core still reports the weight, but the signature no longer verifies. The key-path
    # witness is [item count 01][sig length 40][64-byte signature], followed by locktime.
    $keyHex = $signedByName['p2tr-keypath']
    $witnessStart = $keyHex.Length - 8 - (2 + 2 + 128)
    if ($keyHex.Substring($witnessStart, 4) -ne '0140') { throw 'unexpected key-path witness layout' }
    $annexHex = $keyHex.Substring(0, $witnessStart) + '02' + $keyHex.Substring($witnessStart + 2, 130) + '03501234' + $keyHex.Substring($keyHex.Length - 8)
    $annexDecoded = Invoke-RpcJson 'decoderawtransaction' @("hexstring=$annexHex")
    Write-Fixture -Name 'p2tr-keypath-annex' -Description 'Taproot key-path spend with a 3-byte annex appended to the witness. Modified after signing: the signature no longer verifies, only the weight is meaningful' -Hex $annexHex -Decoded $annexDecoded -CoreVersion $coreVersion
}
finally {
    Invoke-Cli -CliArgs @('-regtest', "-datadir=$dataDir", 'stop') | Out-Null
    Start-Sleep 3
    if ($node -and -not $node.HasExited) { Stop-Process $node -Force }
    Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue
}
