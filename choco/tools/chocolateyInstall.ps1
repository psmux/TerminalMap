$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = "$env:CHOCO_PKG_URL"
  checksum64     = "$env:CHOCO_PKG_CHECKSUM"
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
