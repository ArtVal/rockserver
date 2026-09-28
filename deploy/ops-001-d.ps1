[CmdletBinding()]
param(
    [ValidateSet('bootstrap', 'deploy')][string]$Action = 'deploy',
    [string]$InventoryPath = '',
    [switch]$InstallDocker,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'ops-001-d.psm1') -Force
$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($InventoryPath)) { $InventoryPath = Join-Path $PSScriptRoot 'private.inventory.psd1' }

function Initialize-Ops001DLocalDocker {
    # Docker Desktop on Windows listens through a named pipe. A stale WSL/Linux
    # DOCKER_HOST makes the Windows client try unix:///var/run/docker.sock instead.
    if ($env:OS -eq 'Windows_NT' -and $env:DOCKER_HOST -match '^unix://') {
        Write-Host 'Ignoring incompatible Unix DOCKER_HOST for this Windows deployment process.'
        $env:DOCKER_HOST = 'npipe:////./pipe/docker_engine'
    }
    $null = & docker info --format '{{.ServerVersion}}' 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw 'Local Docker Engine is unavailable. Start Docker Desktop and wait for “Engine running”, then retry the deploy command.'
    }
}

$inventoryFile = Test-Ops001DPrivateInventory -Path $InventoryPath -RepositoryRoot $repoRoot
$inventory = Import-PowerShellDataFile -LiteralPath $inventoryFile
Test-Ops001DInventoryValues -Inventory $inventory | Out-Null
$catalog = Get-Ops001DFullCatalogMetadata -ManifestPath (Join-Path $repoRoot 'release/mobile-catalog/rockmobile-extended-2026.08.2-mobile.1.manifest.json') -CatalogPath (Join-Path $repoRoot 'release/mobile-catalog/rockmobile-extended-2026.08.2-mobile.1.sqlite')
$yandex = Get-Ops001DAllowedYandexEnvironment -Path (Join-Path $repoRoot '.env')
$onnxLock = Join-Path $PSScriptRoot 'onnx-assets.lock.json'
$onnx = Test-Ops001DOnnxManifest -Path $onnxLock
$commit = Get-Ops001DCurrentCommit -RepositoryRoot $repoRoot -AllowDirty:$DryRun
$image = "rockserver:sha-$commit"
$caddyImage = "rockserver-caddy:sha-$commit"

if ($DryRun) {
    $summary = Format-Ops001DSafeSummary -Environment $yandex -Commit $commit -ImageId 'not-built-in-dry-run' -Readiness 'not-contacted'
    Write-Host "Dry-run passed: action=$Action catalog=$($catalog.Version) count=$($catalog.Count) $summary"
    return
}

Initialize-Ops001DLocalDocker

$keyDir = Join-Path $PSScriptRoot '.keys'
$key = Join-Path $keyDir 'rockserver_ed25519'
$target = "$($inventory.SshUser)@$($inventory.SshHost)"
if (-not (Test-Path -LiteralPath $key)) {
    New-Item -ItemType Directory -Force -Path $keyDir | Out-Null
    # Windows PowerShell drops an empty native argument, turning `-N ''` into a
    # missing value. Run from the key directory through cmd.exe so `-N ""` is
    # passed to OpenSSH as an actual empty passphrase.
    Push-Location $keyDir
    try {
        if (Get-Command cmd.exe -ErrorAction SilentlyContinue) {
            & cmd.exe /d /c 'ssh-keygen -q -t ed25519 -f rockserver_ed25519 -N ""'
        } else {
            & ssh-keygen -q -t ed25519 -f $key -N ([string]::Empty)
        }
    } finally {
        Pop-Location
    }
    if ($LASTEXITCODE -ne 0) { throw 'Could not generate the ignored deployment SSH key.' }
    Write-Host 'OpenSSH will now prompt interactively for the VPS login password once. No password is read from config, passed in argv, stored, or logged.'
    Get-Content -LiteralPath "$key.pub" | & ssh $target 'umask 077; mkdir -p ~/.ssh; cat >> ~/.ssh/authorized_keys'
    if ($LASTEXITCODE -ne 0) { throw 'Could not install the SSH key through the interactive password prompt.' }
}

$stage = Join-Path ([IO.Path]::GetTempPath()) ("rockserver-ops001d-" + [Guid]::NewGuid().ToString('N'))
$remoteId = 'rockserver-ops001d-' + [Guid]::NewGuid().ToString('N')
$remoteStage = "/tmp/$remoteId"
$nonInteractiveSshOptions = @('-n', '-o', 'BatchMode=yes', '-o', 'StdinNull=yes', '-o', 'ConnectTimeout=15', '-o', 'ServerAliveInterval=10', '-o', 'ServerAliveCountMax=3')
$nonInteractiveScpOptions = @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', '-o', 'ServerAliveInterval=10', '-o', 'ServerAliveCountMax=3')
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    Copy-Item (Join-Path $PSScriptRoot 'compose.yaml'), (Join-Path $PSScriptRoot 'compose.production.yaml'), (Join-Path $PSScriptRoot 'Caddyfile.production.template'), (Join-Path $PSScriptRoot 'remote-ops-001-d.sh') -Destination $stage
    Write-Ops001DOwnerEnvironmentFile -Path (Join-Path $stage 'owner.env') -Domain $inventory.Domain -Catalog $catalog -Yandex $yandex -OnnxEnabled ([bool]$onnx.enabled)
    Copy-Item $onnxLock (Join-Path $stage 'onnx-assets.json')

    $imageId = ''
    $archiveHash = ''
    $caddyArchiveHash = ''
    if ($Action -eq 'deploy') {
        & docker build --label "org.opencontainers.image.revision=$commit" --tag $image $repoRoot
        if ($LASTEXITCODE -ne 0) { throw 'Local Docker build failed.' }
        # PowerShell's native argument handling removes the inner quotes from a
        # Docker Go-template map lookup on Windows. Inspect the whole labels
        # map as JSON instead, so the revision label is read identically on
        # Windows and Linux.
        $imageId = (& docker image inspect --format '{{.Id}}' $image).Trim()
        $labelsJson = (& docker image inspect --format '{{json .Config.Labels}}' $image).Trim()
        if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the local deployment image.' }
        $labelCommit = [string](ConvertFrom-Json -InputObject $labelsJson).'org.opencontainers.image.revision'
        Test-Ops001DArtifactIdentity -Image $image -Commit $commit -ImageId $imageId -LabelCommit $labelCommit | Out-Null
        $archive = Join-Path $stage 'rockserver-image.tar'
        & docker image save --output $archive $image
        if ($LASTEXITCODE -ne 0) { throw 'Could not create the local image artifact.' }
        $archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
        # Docker Desktop may save an OCI manifest list while a Linux Docker
        # Engine loads the platform image's config ID. Read the config digest
        # from this exact tarball so the release summary uses the portable
        # identity that the remote engine will see after docker load.
        # MSYS/GNU tar resolves `C:\...` as a remote `host:file` argument and
        # fails with "Cannot connect to C:", so prefer the Windows system
        # bsdtar, which understands drive-letter paths.
        $tarTool = if ($env:SystemRoot -and (Test-Path "$env:SystemRoot\System32\tar.exe")) { "$env:SystemRoot\System32\tar.exe" } else { 'tar.exe' }
        $archiveManifestJson = (& $tarTool -xOf $archive 'manifest.json' | Out-String)
        if ($LASTEXITCODE -ne 0) { throw 'Could not read manifest.json from the local image artifact.' }
        $archiveManifest = @(ConvertFrom-Json -InputObject $archiveManifestJson)
        if ($archiveManifest.Count -ne 1 -or [string]$archiveManifest[0].Config -notmatch '^(?:blobs/sha256/)?(?<id>[0-9a-f]{64})(?:\.json)?$') { throw 'Local image artifact has an invalid manifest config reference.' }
        $portableImageId = "sha256:$($Matches.id)"
        Test-Ops001DArtifactIdentity -Image $image -Commit $commit -ImageId $portableImageId -LabelCommit $labelCommit | Out-Null
        $imageId = $portableImageId
        & docker build --label "org.opencontainers.image.revision=$commit" --file (Join-Path $PSScriptRoot 'Dockerfile.caddy') --tag $caddyImage $repoRoot
        if ($LASTEXITCODE -ne 0) { throw 'Local Caddy image build failed.' }
        $caddyLabelsJson = (& docker image inspect --format '{{json .Config.Labels}}' $caddyImage).Trim()
        if ($LASTEXITCODE -ne 0 -or [string](ConvertFrom-Json -InputObject $caddyLabelsJson).'org.opencontainers.image.revision' -ne $commit) { throw 'Caddy image revision label does not match the current commit.' }
        $caddyArchive = Join-Path $stage 'rockserver-caddy-image.tar'
        & docker image save --output $caddyArchive $caddyImage
        if ($LASTEXITCODE -ne 0) { throw 'Could not create the local Caddy image artifact.' }
        $caddyArchiveHash = (Get-FileHash -LiteralPath $caddyArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    }

    & ssh -i $key @nonInteractiveSshOptions $target "umask 077; mkdir '$remoteStage'"
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the protected remote staging directory.' }
    $uploadFiles = @(
        (Join-Path $stage 'compose.yaml'),
        (Join-Path $stage 'compose.production.yaml'),
        (Join-Path $stage 'Caddyfile.production.template'),
        (Join-Path $stage 'remote-ops-001-d.sh'),
        (Join-Path $stage 'owner.env'),
        (Join-Path $stage 'onnx-assets.json')
    )
    if ($Action -eq 'deploy') { $uploadFiles += (Join-Path $stage 'rockserver-image.tar'), (Join-Path $stage 'rockserver-caddy-image.tar') }
    & scp -i $key @nonInteractiveScpOptions @uploadFiles "${target}:$remoteStage/"
    if ($LASTEXITCODE -ne 0) { throw 'Secure copy of the deployment bundle to the VPS failed.' }

    if ($Action -eq 'bootstrap') {
        Write-Host 'The VPS may now prompt interactively for sudo. The password is entered only into the remote TTY and is never stored or logged.'
        $remoteCommand = "sudo env OPS001D_INSTALL_DOCKER=$([int][bool]$InstallDocker) bash '$remoteStage/remote-ops-001-d.sh' bootstrap '$remoteStage' '$($inventory.SshUser)'"
        $sshArgs = Get-Ops001DSshCommand -Action bootstrap -KeyPath $key -Target $target -RemoteCommand $remoteCommand
        & ssh @sshArgs
        if ($LASTEXITCODE -ne 0) {
            throw 'Remote bootstrap failed.'
        }
    } else {
        # The VPS immediately acknowledges a detached worker.  The worker is
        # independent of this SSH connection, so losing Wi-Fi or closing this
        # PowerShell window cannot interrupt a long first-run ONNX backfill.
        $remoteCommand = "sudo -n /opt/rockserver/remote-ops-001-d.sh deploy '$remoteStage' '$image' '$caddyImage' '$commit' '$archiveHash' '$caddyArchiveHash'"
        $sshArgs = Get-Ops001DSshCommand -Action deploy -KeyPath $key -Target $target -RemoteCommand $remoteCommand
        $submitOutput = @(& ssh @sshArgs 2>&1)
        $submitExitCode = $LASTEXITCODE
        if ($submitOutput.Count -gt 0) { $submitOutput | ForEach-Object { Write-Host $_ } }

        $deadline = [DateTime]::UtcNow.AddMinutes(90)
        $lastStatus = ''
        $deploymentFinished = $false
        while ([DateTime]::UtcNow -lt $deadline) {
            Start-Sleep -Seconds 5
            $statusCommand = "sudo -n /opt/rockserver/remote-ops-001-d.sh status '$commit'"
            $statusOutput = @(& ssh -i $key @nonInteractiveSshOptions $target $statusCommand 2>&1)
            $statusExitCode = $LASTEXITCODE
            if ($statusExitCode -ne 0) {
                # The worker is already detached.  Retry status checks rather
                # than treating a transient SSH outage as a deployment failure.
                if ($lastStatus -ne 'connection-retrying') {
                    Write-Host 'SSH status check was interrupted; the VPS worker continues and this script will retry.'
                    $lastStatus = 'connection-retrying'
                }
                continue
            }
            $statusText = (($statusOutput | ForEach-Object { [string]$_ }) -join "`n").Trim()
            if ($statusText -ne $lastStatus) {
                if (-not [string]::IsNullOrWhiteSpace($statusText)) { Write-Host $statusText }
                $lastStatus = $statusText
            }
            if ($statusText -match '(^|\n)status=succeeded\b') {
                $deploymentFinished = $true
                break
            }
            if ($statusText -match '(^|\n)status=failed\b') {
                throw 'Remote deploy worker failed. Inspect its protected VPS deploy log, then rerun the same deploy command after correcting the reported cause.'
            }
            if ($statusText -match '(^|\n)status=unknown\b') {
                if ($submitExitCode -ne 0) {
                    throw 'Could not submit the remote deploy worker. If the bootstrap sudo rule was changed, rerun bootstrap once interactively to refresh it.'
                }
                throw 'Remote deploy worker acknowledgement was missing; rerun the deploy command to submit or reattach safely.'
            }
        }
        if (-not $deploymentFinished) {
            throw 'Timed out waiting for the remote deploy worker. The VPS worker continues safely; rerun the same deploy command later to reattach and read its status.'
        }
    }
    Write-Host (Format-Ops001DSafeSummary -Environment $yandex -Commit $commit -ImageId $imageId -Readiness $(if ($Action -eq 'deploy') { 'passed' } else { 'bootstrap-only' }))
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
}
