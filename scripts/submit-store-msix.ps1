param(
  [Parameter(Mandatory)] [string] $PackagePath,
  [Parameter(Mandatory)] [string] $QrateVersion
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Require-Environment([string] $Name) {
  $value = [Environment]::GetEnvironmentVariable($Name)
  if ([string]::IsNullOrWhiteSpace($value)) { throw "Required release-signing environment value '$Name' is missing." }
  return $value
}

function Invoke-StoreApi([string] $Method, [string] $Uri, [string] $Token, [object] $Body = $null) {
  $parameters = @{
    Method = $Method
    Uri = $Uri
    Headers = @{ Authorization = "Bearer $Token" }
    ErrorAction = 'Stop'
  }
  if ($null -ne $Body) {
    $parameters.ContentType = 'application/json'
    $parameters.Body = ConvertTo-Json -InputObject $Body -Depth 100 -Compress
  }
  try {
    return Invoke-RestMethod @parameters
  } catch {
    $status = $null
    if ($_.Exception.Response) { $status = [int]$_.Exception.Response.StatusCode }
    $detail = if ($status) { "HTTP $status" } else { 'transport failure' }
    $response = $_.ErrorDetails.Message
    if ([string]::IsNullOrWhiteSpace($response)) { $response = 'no response details' }
    throw "Microsoft Store API request failed ($detail): $response"
  }
}

$applicationId = Require-Environment 'QRATE_STORE_APPLICATION_ID'
$tenantId = Require-Environment 'QRATE_STORE_TENANT_ID'
$clientId = Require-Environment 'QRATE_STORE_CLIENT_ID'
$clientSecret = Require-Environment 'QRATE_STORE_CLIENT_SECRET'
if (-not (Test-Path -LiteralPath $PackagePath -PathType Leaf)) { throw "MSIX package not found: $PackagePath" }
if ($QrateVersion -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
  throw "Store submission only accepts a stable three-part qrate version; got '$QrateVersion'."
}

$tokenEndpoint = "https://login.microsoftonline.com/$tenantId/oauth2/token"
try {
  $tokenResponse = Invoke-RestMethod -Method Post -Uri $tokenEndpoint -ContentType 'application/x-www-form-urlencoded' `
    -Body @{ grant_type = 'client_credentials'; client_id = $clientId; client_secret = $clientSecret; resource = 'https://manage.devcenter.microsoft.com' } `
    -ErrorAction Stop
} catch {
  throw 'Could not obtain a Microsoft Store submission token. Check the Entra tenant, application, secret, and Partner Center Manager role.'
}
$token = $tokenResponse.access_token
if ([string]::IsNullOrWhiteSpace($token)) { throw 'Microsoft Entra returned no access token.' }

$baseUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$applicationId/submissions"
$submission = Invoke-StoreApi -Method Post -Uri $baseUri -Token $token
$submissionId = [string]$submission.id
if ([string]::IsNullOrWhiteSpace($submissionId)) { throw 'The Store API created no submission ID.' }
if ([string]::IsNullOrWhiteSpace([string]$submission.fileUploadUrl)) {
  throw "Submission $submissionId has no package upload URL. It may need attention in Partner Center."
}

$packages = @()
foreach ($oldPackage in @($submission.applicationPackages)) {
  if ([string]$oldPackage.fileName -ne [IO.Path]::GetFileName($PackagePath)) {
    $packages += [ordered]@{
      fileName = [string]$oldPackage.fileName
      fileStatus = 'PendingDelete'
      minimumDirectXVersion = 'None'
      minimumSystemRam = 'None'
    }
  }
}
$packages += [ordered]@{
  fileName = [IO.Path]::GetFileName($PackagePath)
  fileStatus = 'PendingUpload'
  minimumDirectXVersion = 'None'
  minimumSystemRam = 'None'
}
$submission.applicationPackages = $packages
$submission.targetPublishMode = 'Manual'

$body = [ordered]@{}
foreach ($property in $submission.PSObject.Properties) {
  if ($property.Name -notin @('id', 'status', 'statusDetails', 'fileUploadUrl', 'friendlyName')) {
    $body[$property.Name] = $property.Value
  }
}
$submissionUri = "$baseUri/$submissionId"
$null = Invoke-StoreApi -Method Put -Uri $submissionUri -Token $token -Body $body

$zipPath = Join-Path ([IO.Path]::GetTempPath()) ("qrate-store-upload-" + [guid]::NewGuid().ToString('N') + '.zip')
try {
  Compress-Archive -LiteralPath (Resolve-Path -LiteralPath $PackagePath).Path -DestinationPath $zipPath -CompressionLevel Optimal
  try {
    Invoke-WebRequest -Method Put -Uri ([string]$submission.fileUploadUrl) -InFile $zipPath `
      -ContentType 'application/zip' -Headers @{
        'x-ms-blob-type' = 'BlockBlob'
        'x-ms-date' = [DateTime]::UtcNow.ToString('R')
        'x-ms-version' = '2019-12-12'
      } -ErrorAction Stop | Out-Null
  } catch {
    throw "Could not upload the package archive for Store submission $submissionId. The SAS URL was omitted from logs."
  }
} finally {
  Remove-Item -LiteralPath $zipPath -Force -ErrorAction SilentlyContinue
}

$null = Invoke-StoreApi -Method Post -Uri "$submissionUri/commit" -Token $token
$statusUri = "$submissionUri/status"
$deadline = (Get-Date).AddMinutes(8)
do {
  Start-Sleep -Seconds 15
  $status = Invoke-StoreApi -Method Get -Uri $statusUri -Token $token
  switch ([string]$status.status) {
    'PreProcessing' {
      Write-Host "Submitted qrate $QrateVersion to Microsoft Store (submission $submissionId). Certification continues in Partner Center."
      exit 0
    }
    { $_ -in @('PendingPublication', 'Publishing', 'Published', 'Certification', 'Release') } {
      Write-Host "Submitted qrate $QrateVersion to Microsoft Store (submission $submissionId; status $($status.status)). Certification or publication continues in Partner Center."
      exit 0
    }
    'CommitFailed' {
      $messages = @($status.statusDetails.errors | ForEach-Object { "[$($_.code)] $($_.details)" }) -join "`n"
      throw "Microsoft Store rejected the submission commit ($submissionId):`n$messages"
    }
    { $_ -in @('CommitStarted', 'PendingCommit') } { continue }
    default { throw "Store submission $submissionId entered unexpected commit status '$($status.status)'." }
  }
} while ((Get-Date) -lt $deadline)
throw "Store submission $submissionId is still committing. Check its status in Partner Center before retrying."
