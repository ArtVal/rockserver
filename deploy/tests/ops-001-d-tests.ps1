$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Import-Module (Join-Path $root 'deploy/ops-001-d.psm1') -Force
$temp = Join-Path ([IO.Path]::GetTempPath()) ('ops001d-tests-' + [Guid]::NewGuid().ToString('N'))

function Assert-Throws([scriptblock]$Operation, [string]$Failure) {
    try { & $Operation; throw $Failure } catch { if ($_.Exception.Message -eq $Failure) { throw } }
}

New-Item -ItemType Directory -Path $temp | Out-Null
try {
    $git = Join-Path $temp 'repo'
    New-Item -ItemType Directory -Path $git | Out-Null
    & git -C $git init -q
    'deploy/private.inventory.psd1' | Set-Content "$git/.gitignore"
    New-Item -ItemType Directory "$git/deploy" | Out-Null
    '@{}' | Set-Content "$git/deploy/private.inventory.psd1"
    Test-Ops001DPrivateInventory "$git/deploy/private.inventory.psd1" $git | Out-Null
    & git -C $git add -f deploy/private.inventory.psd1
    & git -C $git -c user.email=test@example.invalid -c user.name=test commit -qm test
    Assert-Throws { Test-Ops001DPrivateInventory "$git/deploy/private.inventory.psd1" $git | Out-Null } 'tracked inventory was accepted'

    $inventory = [ordered]@{ SshUser = 'deploy'; SshHost = 'staging.example.test'; Domain = 'api.example.test' }
    Test-Ops001DInventoryValues $inventory | Out-Null
    Assert-Throws { Test-Ops001DInventoryValues ([ordered]@{ SshUser = 'deploy'; SshHost = 'host'; Domain = 'domain'; SshPassword = 'must-not-exist' }) | Out-Null } 'SshPassword was accepted'

    @('YANDEX_AI_API_KEY=secret', 'UNRELATED_SECRET=nope', 'YANDEX_FOLDER_ID=folder', 'YANDEX_SPEECHKIT_API_KEY=speech', 'YANDEX_HOME_CLIENTID=home-client', 'YANDEX_HOME_SECRET=home-secret') | Set-Content "$temp/.env"
    $env = Get-Ops001DAllowedYandexEnvironment "$temp/.env"
    if ($env.Keys.Count -ne 5 -or $env.Contains('UNRELATED_SECRET') -or -not $env.Contains('YANDEX_HOME_CLIENTID') -or -not $env.Contains('YANDEX_HOME_SECRET')) { throw 'allowed-env filter failed' }
    $safe = Format-Ops001DSafeSummary -Environment $env -Commit 'safe' -ImageId 'safe' -Readiness 'passed'
    if ($safe -match 'secret|folder|speech|home-client|nope') { throw 'safe summary exposed a secret value' }

    $catalog = [pscustomobject]@{ Version = 'v1-test'; Count = 41; Sha256 = ('a' * 64) }
    $ownerPath = Join-Path $temp 'owner.env'
    Write-Ops001DOwnerEnvironmentFile -Path $ownerPath -Domain 'api.example.test' -Catalog $catalog -Yandex $env -OnnxEnabled $false
    $ownerLines = [IO.File]::ReadAllLines($ownerPath)
    if ($ownerLines.Count -ne 9 -or $ownerLines[0] -ne 'ROCKSERVER_DOMAIN=api.example.test' -or $ownerLines[1] -ne 'OPS001D_CATALOG_VERSION=v1-test' -or $ownerLines[2] -ne 'OPS001D_CATALOG_COUNT=41' -or $ownerLines[3] -ne ('OPS001D_CATALOG_SHA256=' + ('a' * 64)) -or $ownerLines -notcontains 'YANDEX_HOME_CLIENTID=home-client' -or $ownerLines -notcontains 'YANDEX_HOME_SECRET=home-secret') { throw 'owner.env entries were not serialized as distinct correct lines' }
    if (($ownerLines -join "`n") -match 'UNRELATED_SECRET|nope') { throw 'owner.env included a non-allowlisted value' }
    $ownerBytes = [IO.File]::ReadAllBytes($ownerPath)
    if ($ownerBytes -contains [byte]13) { throw 'owner.env must use Linux LF line endings for remote shell parsing' }
    Write-Ops001DOwnerEnvironmentFile -Path $ownerPath -Domain 'api.example.test' -Catalog $catalog -Yandex $env -OnnxEnabled $true
    $ownerText = [IO.File]::ReadAllText($ownerPath)
    if ($ownerText -notmatch 'ROCKSERVER_ONNX_ASSET_DIR=/opt/rockserver/assets/onnx' -or $ownerText -match "[`r`0]") { throw 'ONNX owner.env entries are not safe for remote parsing' }

    $commit = '0123456789012345678901234567890123456789'
    $imageId = 'sha256:' + ('a' * 64)
    Test-Ops001DArtifactIdentity "rockserver:sha-$commit" $commit $imageId $commit | Out-Null
    Assert-Throws { Test-Ops001DArtifactIdentity 'ghcr.io/example/rockserver:latest' $commit $imageId $commit | Out-Null } 'registry/latest artifact was accepted'
    Assert-Throws { Test-Ops001DArtifactIdentity "rockserver:sha-$commit" $commit $imageId ('f' * 40) | Out-Null } 'mismatched revision label was accepted'

    $bootstrapSsh = Get-Ops001DSshCommand bootstrap 'key path' 'deploy@host' "sudo env OPS001D_INSTALL_DOCKER=1 '/tmp/stage/remote.sh' bootstrap '/tmp/stage' 'deploy'"
    $deploySsh = Get-Ops001DSshCommand deploy 'key path' 'deploy@host' "sudo -n /opt/rockserver/remote-ops-001-d.sh deploy '/tmp/stage' 'rockserver:sha-$commit' '$commit' '$imageId' $('b' * 64)"
    if ($bootstrapSsh -notcontains '-tt' -or ($bootstrapSsh -join ' ') -notmatch 'sudo env' -or ($bootstrapSsh -join ' ') -match 'password') { throw 'bootstrap SSH/sudo construction is not interactive and secret-safe' }
    if ($deploySsh -contains '-tt' -or $deploySsh -notcontains '-n' -or $deploySsh -notcontains 'BatchMode=yes' -or $deploySsh -notcontains 'ConnectTimeout=15' -or $deploySsh -notcontains 'ServerAliveInterval=10' -or ($deploySsh -join ' ') -notmatch 'sudo -n') { throw 'normal deploy SSH/sudo construction is not non-interactive and bounded' }
    $launcherScript = Get-Content -Raw (Join-Path $root 'deploy/ops-001-d.ps1')
    if ($launcherScript -notmatch 'cmd\.exe /d /c.*ssh-keygen.*-N') { throw 'Windows-safe empty-passphrase SSH key generation is missing' }
    if ($launcherScript -notmatch "DOCKER_HOST -match '\^unix://'" -or $launcherScript -notmatch 'npipe:////\./pipe/docker_engine' -or $launcherScript -notmatch 'docker info --format') { throw 'Windows Docker Engine preflight is missing' }
    if ($launcherScript -notmatch "docker image inspect --format '\{\{json \.Config\.Labels\}\}'" -or $launcherScript -match 'index \.Config\.Labels') { throw 'Windows-safe Docker image revision inspection is missing' }
    if ($launcherScript -notmatch 'System32\\tar\.exe' -or $launcherScript -notmatch '\$tarTool -xOf' -or $launcherScript -notmatch 'blobs/sha256' -or $launcherScript -notmatch 'portableImageId') { throw 'portable image config ID extraction is missing' }
    if ($launcherScript -notmatch 'StdinNull=yes' -or $launcherScript -notmatch 'ServerAliveCountMax=3') { throw 'staging SSH is not protected from an unbounded stdin wait' }
    if ($launcherScript -notmatch '\$nonInteractiveScpOptions' -or $launcherScript -match '& scp -i \$key @nonInteractiveSshOptions' -or $launcherScript -match '\$nonInteractiveScpOptions = @\([^\r\n]*StdinNull=yes') { throw 'SCP has incompatible SSH stdin options' }

    '{"enabled":true,"assetDirectory":"/opt/rockserver/assets/onnx","assets":[{"name":"model.onnx","url":"REQUIRED_OFFICIAL_HTTPS_URL","sha256":"REQUIRED_64_HEX_SHA256"}]}' | Set-Content "$temp/onnx.json"
    Assert-Throws { Test-Ops001DOnnxManifest "$temp/onnx.json" | Out-Null } 'incomplete ONNX lock was accepted'
    $onnxLock = Test-Ops001DOnnxManifest "$root/deploy/onnx-assets.lock.json"
    if (-not $onnxLock.enabled -or @($onnxLock.assets).Count -ne 3) { throw 'committed automatic ONNX lock is invalid' }
    $catalogActual = Get-Ops001DFullCatalogMetadata "$root/release/mobile-catalog/rockmobile-extended-2026.08.2-mobile.1.manifest.json" "$root/release/mobile-catalog/rockmobile-extended-2026.08.2-mobile.1.sqlite"
    $catalogAgain = Get-Ops001DFullCatalogMetadata "$root/release/mobile-catalog/rockmobile-extended-2026.08.2-mobile.1.manifest.json" "$root/release/mobile-catalog/rockmobile-extended-2026.08.2-mobile.1.sqlite"
    if ($catalogActual.Count -lt 16000 -or $catalogActual.Version -ne $catalogAgain.Version -or $catalogActual.Sha256 -ne $catalogAgain.Sha256) { throw 'pinned complete catalog seed input is not stable' }

    $launcher = Get-Content -Raw (Join-Path $root 'deploy/ops-001-d.ps1')
    $remote = Get-Content -Raw (Join-Path $root 'deploy/remote-ops-001-d.sh')
    $compose = Get-Content -Raw (Join-Path $root 'deploy/compose.yaml')
    $productionCompose = Get-Content -Raw (Join-Path $root 'deploy/compose.production.yaml')
    if ($launcher -match 'ghcr|docker push|docker pull|SshPassword') { throw 'launcher still has a registry or password dependency' }
    if ($launcher -notmatch 'docker image save' -or $remote -notmatch 'docker image load') { throw 'registry-free artifact transfer is missing' }
    if ($remote -notmatch 'transferred image artifact checksum mismatch' -or $remote -notmatch 'revision label binds that verified artifact to commit' -or $remote -match 'loaded image ID does not match') { throw 'cross-engine image artifact verification is not portable' }
    if ($remote -notmatch 'Accept the former five-argument form' -or $remote -notmatch 'deploy requires stage, image, commit, and artifact hash') { throw 'operator-script upgrade compatibility is missing' }
    if ($remote -notmatch 'ROCKSERVER_IMAGE="\$image".*\$compose config' -or $remote -notmatch 'ROCKSERVER_IMAGE="\$image".*\$compose up') { throw 'Compose interpolation environment is not preserved for deployment commands' }
    if ($remote -notmatch 'os\.chmod\(path, 0o755 if name == .libonnxruntime\.so. else 0o644\)') { throw 'ONNX runtime executable permissions are not repaired after download' }
    if ($remote -notmatch 'os\.chmod\(root, 0o755\)') { throw 'ONNX asset directory is not traversable by the container user' }
    if (($remote -notmatch 'tarfile\.open') -or ($remote -notmatch 'for _ in range\(8\)') -or ($remote -notmatch 'member\.issym\(\)') -or ($remote -notmatch 'member\.islnk\(\)')) { throw 'ONNX runtime symlink archive extraction is not safe' }
    if ($remote -notmatch 'ensure_low_memory_swap' -or $remote -notmatch 'fallocate -l 2G /swapfile' -or $remote -notmatch '/swapfile none swap sw 0 0') { throw 'low-memory ONNX swap provisioning is missing' }
    if ($remote -notmatch 'nohup.*deploy-worker' -or $remote -notmatch 'write_deploy_status.*running' -or $remote -notmatch 'write_deploy_status.*succeeded' -or $remote -notmatch 'write_deploy_status.*failed' -or $remote -notmatch 'reattached=true') { throw 'remote deploy worker is not resilient to a lost SSH session' }
    if ($remote -notmatch 'Activate the validated operator script before starting the worker' -or $remote -notmatch 'install -m 0750.*remote-ops-001-d\.sh.*release_root/remote-ops-001-d\.sh') { throw 'normal deploy does not activate its operator script before starting the worker' }
    if ($remote -notmatch 'status \*' -or $launcher -notmatch 'statusCommand' -or $launcher -notmatch 'connection-retrying' -or $launcher -notmatch 'AddMinutes\(90\)') { throw 'launcher cannot reconnect to a detached remote deploy worker' }
    if ($remote -notmatch 'catalog_seed_is_current' -or $remote -notmatch 'catalog seed skipped' -or $remote -notmatch 'OPS001D_CATALOG_SHA256' -or $remote -notmatch 'station_embeddings') { throw 'unchanged full catalog cannot skip redundant ONNX backfill' }
    if ($remote -match 'pg_dump --format=custom' -or $remote -match 'prune_backups') { throw 'deploy-time pg_dump must stay disabled by operator decision' }
    if ($remote -notmatch "backup_hash='-'") { throw 'deploy summary backup continuity marker is missing' }
    $seedAt = $remote.IndexOf('run --rm catalog_seed')
    $readyAt = $remote.IndexOf('/health/ready')
    if ($seedAt -lt 0 -or $readyAt -le $seedAt -or $remote -match 'fixture') { throw 'seed/readiness fail-closed ordering changed' }
    if ($remote -notmatch 'applies embedded migrations' -or $remote -notmatch 'exact HTTPS URLs and SHA-256' -or $launcher -notmatch 'onnx-assets.lock.json') { throw 'migration or automatic ONNX safeguards are missing' }
    if ($remote -notmatch '\[\^\[:cntrl:\]\]\*\$') { throw 'owner.env control-character validation is missing' }
    if ($compose -notmatch 'import_full_catalog.*backfill_embeddings' -or $compose -notmatch 'ROCKSERVER_LOG_DIR: /var/log/rockserver' -or $compose -notmatch 'ROCKSERVER_SEMANTIC_PROVIDER: onnx-e5-local' -or $compose -notmatch 'ORT_DYLIB_PATH') { throw 'first-deploy ONNX backfill or seed logging is not wired' }
    if ($compose -notmatch 'ROCKSERVER_LOG_RETENTION_DAYS: \$\{ROCKSERVER_LOG_RETENTION_DAYS:-14\}') { throw 'RockServer log retention is not configured for production' }
    $ownerValidation = (($remote -split "`n" | Where-Object { $_ -match 'grep -Ev' }) -join "`n")
    if ($compose -notmatch 'YANDEX_HOME_CLIENTID: \$\{YANDEX_HOME_CLIENTID:-\}' -or $compose -notmatch 'YANDEX_HOME_SECRET: \$\{YANDEX_HOME_SECRET:-\}' -or $ownerValidation -notmatch 'YANDEX_HOME_CLIENTID' -or $ownerValidation -notmatch 'YANDEX_HOME_SECRET') { throw 'Yandex Home OAuth environment is not deployed safely' }
    if ($compose -notmatch 'ROCKSERVER_ONNX_INTRA_THREADS: \$\{ROCKSERVER_ONNX_INTRA_THREADS:-2\}') { throw 'ONNX CPU thread default must be a positive integer' }
    if ($productionCompose -notmatch '/home/rockserver/logs:/var/log/rockserver' -or $remote -notmatch 'host_log_dir="/home/rockserver/logs"' -or $remote -notmatch 'ensure_host_log_dir') { throw 'persistent host log directory is not wired for production' }
    if ($compose -notmatch 'ROCKSERVER_STATION_ICON_DIR: /var/lib/rockserver/station-icons' -or $productionCompose -notmatch '/home/rockserver/station-icons:/var/lib/rockserver/station-icons' -or $remote -notmatch 'host_station_icon_dir="/home/rockserver/station-icons"' -or $remote -notmatch 'ensure_host_station_icon_dir') { throw 'persistent station-icon directory is not wired for production' }

    Write-Host 'OPS-001-D local tests passed: registry-free artifact identity, password-free inventory, secret-safe env/summary, TTY/sudo construction, seed, and automatic pinned ONNX safeguards.'
} finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
