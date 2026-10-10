$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$shell = New-Object -ComObject WScript.Shell
$seen = @{}
$applications = [System.Collections.Generic.List[object]]::new()
try {
	$folders = @(
		[Environment]::GetFolderPath('Programs'),
		[Environment]::GetFolderPath('CommonPrograms'),
		[Environment]::GetFolderPath('DesktopDirectory'),
		[Environment]::GetFolderPath('CommonDesktopDirectory')
	) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -Unique
	foreach ($folder in $folders) {
		Get-ChildItem -LiteralPath $folder -Filter '*.lnk' -File -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
			$shortcut = $null
			try {
				$shortcut = $shell.CreateShortcut($_.FullName)
				$target = [Environment]::ExpandEnvironmentVariables($shortcut.TargetPath)
				if (-not $target -or [IO.Path]::GetExtension($target) -ine '.exe') { return }
				if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { return }
				$target = [IO.Path]::GetFullPath($target)
				if ($seen.ContainsKey($target)) { return }
				$seen[$target] = $true
				$applications.Add([PSCustomObject]@{
					platform = 'windows'
					name = $_.BaseName
					executablePath = $target.Replace('\', '/')
				})
			} catch {
				# 跳过损坏或无权读取的快捷方式，不执行快捷方式目标。
			} finally {
				if ($null -ne $shortcut) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shortcut) }
			}
		}
	}
	ConvertTo-Json -InputObject @($applications | Sort-Object name, executablePath) -Depth 3 -Compress
} finally {
	[void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell)
}
