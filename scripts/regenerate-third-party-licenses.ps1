param(
  [string]$RepoRoot = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
  $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
} else {
  $RepoRoot = (Resolve-Path $RepoRoot).Path
}

function Write-Utf8File {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$Content
  )

  $parent = Split-Path -Parent $Path
  if ($parent -and -not (Test-Path -LiteralPath $parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
  }

  Set-Content -LiteralPath $Path -Value $Content -Encoding utf8
}

function Read-TextFileSafe {
  param(
    [Parameter(Mandatory = $true)][string]$Path
  )

  try {
    return Get-Content -LiteralPath $Path -Raw -ErrorAction Stop
  } catch {
    return "[unreadable text file: $Path]"
  }
}

function Get-RelativePath {
  param(
    [Parameter(Mandatory = $true)][string]$BasePath,
    [Parameter(Mandatory = $true)][string]$TargetPath
  )

  $baseFull = [System.IO.Path]::GetFullPath($BasePath)
  if (-not $baseFull.EndsWith([System.IO.Path]::DirectorySeparatorChar)) {
    $baseFull += [System.IO.Path]::DirectorySeparatorChar
  }

  $targetFull = [System.IO.Path]::GetFullPath($TargetPath)
  $baseUri = New-Object System.Uri($baseFull)
  $targetUri = New-Object System.Uri($targetFull)
  $relativeUri = $baseUri.MakeRelativeUri($targetUri)
  $relativePath = [System.Uri]::UnescapeDataString($relativeUri.ToString())
  return ($relativePath -replace "/", "\")
}

function Resolve-DeclaredLicensePath {
  param(
    [Parameter(Mandatory = $true)][string]$PackageRoot,
    [AllowNull()][string]$DeclaredLicenseFile
  )

  if ([string]::IsNullOrWhiteSpace($DeclaredLicenseFile)) {
    return $null
  }

  $candidate = if ([System.IO.Path]::IsPathRooted($DeclaredLicenseFile)) {
    $DeclaredLicenseFile
  } else {
    Join-Path $PackageRoot $DeclaredLicenseFile
  }

  if (Test-Path -LiteralPath $candidate -PathType Leaf) {
    return (Resolve-Path -LiteralPath $candidate).Path
  }

  return $null
}

function Get-LicenseCandidateRelativePaths {
  param(
    [Parameter(Mandatory = $true)][string]$PackageRoot,
    [AllowNull()][string]$DeclaredLicenseFile
  )

  $paths = New-Object System.Collections.Generic.List[string]
  $seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)

  $declaredPath = Resolve-DeclaredLicensePath -PackageRoot $PackageRoot -DeclaredLicenseFile $DeclaredLicenseFile
  if ($declaredPath) {
    $declaredRel = Get-RelativePath -BasePath $PackageRoot -TargetPath $declaredPath
    if ($seen.Add($declaredRel)) {
      $paths.Add($declaredRel) | Out-Null
    }
  }

  $nameRegex = '^(LICENSE(|[-_.].*)|LICENCE(|[-_.].*)|COPYING(|[-_.].*)|NOTICE(|[-_.].*)|UNLICENSE(|[-_.].*)|COPYRIGHT(|[-_.].*))$'
  $searchRoots = New-Object System.Collections.Generic.List[string]
  $searchRoots.Add($PackageRoot) | Out-Null

  foreach ($subdir in @("license", "licenses", "LICENCE", "LICENCES", "copying", "COPYING", "notice", "NOTICE")) {
    $candidate = Join-Path $PackageRoot $subdir
    if (Test-Path -LiteralPath $candidate -PathType Container) {
      $searchRoots.Add($candidate) | Out-Null
    }
  }

  foreach ($root in $searchRoots) {
    $files = Get-ChildItem -LiteralPath $root -File -Recurse -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -imatch $nameRegex }
    foreach ($file in $files) {
      $relativePath = Get-RelativePath -BasePath $PackageRoot -TargetPath $file.FullName
      if ($seen.Add($relativePath)) {
        $paths.Add($relativePath) | Out-Null
      }
    }
  }

  $paths.Sort([System.StringComparer]::OrdinalIgnoreCase)
  return ,$paths
}

Push-Location $RepoRoot
try {
  $metadataRaw = & cargo metadata --format-version 1 --locked --quiet
  if ($LASTEXITCODE -ne 0) {
    throw "cargo metadata failed with exit code $LASTEXITCODE"
  }
} finally {
  Pop-Location
}

$metadata = $metadataRaw | ConvertFrom-Json
if ($null -eq $metadata.resolve -or [string]::IsNullOrWhiteSpace([string]$metadata.resolve.root)) {
  throw "cargo metadata did not return a dependency resolve graph."
}

$rootId = [string]$metadata.resolve.root
$nodeById = @{}
foreach ($node in $metadata.resolve.nodes) {
  $nodeById[[string]$node.id] = $node
}

$packageById = @{}
foreach ($pkg in $metadata.packages) {
  $packageById[[string]$pkg.id] = $pkg
}

if (-not $packageById.ContainsKey($rootId)) {
  throw "Unable to locate workspace root package in metadata: $rootId"
}

$visited = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
$stack = New-Object System.Collections.Generic.Stack[string]
$stack.Push($rootId)
while ($stack.Count -gt 0) {
  $current = $stack.Pop()
  if (-not $visited.Add($current)) {
    continue
  }
  if ($nodeById.ContainsKey($current)) {
    foreach ($dep in $nodeById[$current].dependencies) {
      $stack.Push([string]$dep)
    }
  }
}

$registryPrefix = "registry+https://github.com/rust-lang/crates.io-index"
$rows = @()
$manualReviewCrates = New-Object System.Collections.Generic.List[string]

foreach ($id in $visited) {
  if ($id -eq $rootId) {
    continue
  }
  if (-not $packageById.ContainsKey($id)) {
    continue
  }

  $pkg = $packageById[$id]
  if ($null -eq $pkg.source) {
    continue
  }
  if (-not ([string]$pkg.source).StartsWith($registryPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    continue
  }

  $pkgName = [string]$pkg.name
  $pkgVersion = [string]$pkg.version
  $manifestPath = [string]$pkg.manifest_path
  $pkgRoot = Split-Path -Parent $manifestPath
  $declaredLicenseExpression = if ($pkg.license) { [string]$pkg.license } else { "UNKNOWN" }
  $declaredLicenseFile = if ($pkg.license_file) { [string]$pkg.license_file } else { "" }

  $relativeLicensePaths = Get-LicenseCandidateRelativePaths -PackageRoot $pkgRoot -DeclaredLicenseFile $declaredLicenseFile
  if ($declaredLicenseExpression -eq "UNKNOWN") {
    $manualReviewCrates.Add(("{0}-{1}" -f $pkgName, $pkgVersion)) | Out-Null
  }

  $rows += [pscustomobject]@{
    Name = $pkgName
    Version = $pkgVersion
    LicenseExpression = $declaredLicenseExpression
    DeclaredLicenseFile = if ($declaredLicenseFile) { $declaredLicenseFile } else { "-" }
    Source = [string]$pkg.source
    Repository = if ($pkg.repository) { [string]$pkg.repository } else { "-" }
    ManifestPath = $manifestPath
    PackageRoot = $pkgRoot
    LicenseRelativePaths = @($relativeLicensePaths)
  }
}

$rows = $rows | Sort-Object Name, Version
$rootPackage = $packageById[$rootId]

$builder = New-Object System.Text.StringBuilder
[void]$builder.AppendLine("# Third-Party Licenses")
[void]$builder.AppendLine("")

foreach ($row in $rows) {
  [void]$builder.AppendLine(("## Crate: {0} {1}" -f $row.Name, $row.Version))
  [void]$builder.AppendLine("")
  [void]$builder.AppendLine(("- License expression: {0}" -f $row.LicenseExpression))
  [void]$builder.AppendLine(("- Declared license_file: {0}" -f $row.DeclaredLicenseFile))
  [void]$builder.AppendLine(("- Source: {0}" -f $row.Source))
  if (-not [string]::IsNullOrWhiteSpace($row.Repository) -and $row.Repository -ne "-") {
    [void]$builder.AppendLine(("- Repository: {0}" -f $row.Repository))
  }
  [void]$builder.AppendLine("")

  $licensePaths = @($row.LicenseRelativePaths)
  if ($licensePaths.Count -eq 0) {
    [void]$builder.AppendLine("_No standalone license file found in this crate package._")
    [void]$builder.AppendLine("")
    continue
  }

  foreach ($relativePath in $licensePaths) {
    $sourcePath = Join-Path $row.PackageRoot $relativePath
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
      continue
    }
    $title = ($relativePath -replace "\\", "/")
    $body = (Read-TextFileSafe -Path $sourcePath).TrimEnd()
    [void]$builder.AppendLine(("### {0}" -f $title))
    [void]$builder.AppendLine("")
    [void]$builder.AppendLine("~~~~text")
    [void]$builder.AppendLine($body)
    [void]$builder.AppendLine("~~~~")
    [void]$builder.AppendLine("")
  }
}

$licensesDir = Join-Path $RepoRoot "licenses"
if (-not (Test-Path -LiteralPath $licensesDir)) {
  New-Item -ItemType Directory -Force -Path $licensesDir | Out-Null
}

$outputPath = Join-Path $licensesDir "THIRD_PARTY_LICENSES_ALL.md"
Write-Utf8File -Path $outputPath -Content $builder.ToString()

Write-Host "Third-party license generation complete."
Write-Host "Output: $outputPath"
Write-Host "crates.io dependencies documented: $($rows.Count)"
Write-Host "Crates requiring manual review: $($manualReviewCrates.Count)"
