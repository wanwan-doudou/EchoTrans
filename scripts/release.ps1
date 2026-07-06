# EchoTrans 发版脚本：签名构建安装包并生成自动更新清单 latest.json
# 用法：在项目根目录执行 .\scripts\release.ps1
# 前置：更新签名私钥位于 %USERPROFILE%\.tauri\echotrans.key（丢失将无法向老用户推送更新，请备份）

$ErrorActionPreference = "Stop"

$keyPath = "$env:USERPROFILE\.tauri\echotrans.key"
if (-not (Test-Path $keyPath)) {
    throw "未找到更新签名私钥：$keyPath"
}
# 注意：tauri CLI 读取的是私钥内容本身，_PATH 变体不一定被识别
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $keyPath -Raw).Trim()
# 空密码需配合 --ci，避免 CLI 进入交互式等待
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""

pnpm tauri build --ci
if ($LASTEXITCODE -ne 0) {
    throw "构建失败"
}

$conf = Get-Content "src-tauri/tauri.conf.json" -Raw -Encoding UTF8 | ConvertFrom-Json
$version = $conf.version
$bundleDir = "src-tauri/target/release/bundle"
$setupName = "EchoTrans_${version}_x64-setup.exe"
$sigPath = "$bundleDir/nsis/$setupName.sig"

if (-not (Test-Path $sigPath)) {
    throw "未找到签名文件 $sigPath（确认 tauri.conf.json 中 createUpdaterArtifacts 已开启）"
}

$latest = [ordered]@{
    version  = $version
    notes    = "详见 GitHub Release 页面"
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = @{
        "windows-x86_64" = [ordered]@{
            signature = (Get-Content $sigPath -Raw -Encoding UTF8).Trim()
            url       = "https://github.com/wanwan-doudou/EchoTrans/releases/download/v$version/$setupName"
        }
    }
}

$latestPath = "$bundleDir/latest.json"
$latest | ConvertTo-Json -Depth 4 | Set-Content $latestPath -Encoding UTF8

Write-Host ""
Write-Host "构建完成 v$version，需上传到 GitHub Release（tag v$version）的文件："
Write-Host "  $bundleDir/nsis/$setupName"
Write-Host "  $bundleDir/msi/EchoTrans_${version}_x64_en-US.msi"
Write-Host "  $latestPath   <- 自动更新清单，必须上传"
