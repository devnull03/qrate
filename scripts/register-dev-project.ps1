# Called by run-dev.ps1. Keeps the previous per-user icon and removes only the
# Open With entry created for this launch.
function Register-DevProject {
    param([Parameter(Mandatory = $true)][string]$Executable)

    $classes = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Classes')
    $extensionExisted = $null -ne $classes.OpenSubKey('.qrate')
    $extension = $classes.CreateSubKey('.qrate')
    $iconExisted = $null -ne $extension.OpenSubKey('DefaultIcon')
    $icon = $extension.CreateSubKey('DefaultIcon')
    $hadIconValue = $icon.GetValueNames() -contains ''
    $oldIconValue = if ($hadIconValue) { $icon.GetValue('', $null, 'DoNotExpandEnvironmentNames') } else { $null }
    $oldIconKind = if ($hadIconValue) { $icon.GetValueKind('') } else { $null }
    $openWithExisted = $null -ne $extension.OpenSubKey('OpenWithProgids')
    $progId = 'qrate.DevProject.' + [guid]::NewGuid().ToString('N')
    $state = [pscustomobject]@{
        ProgId = $progId
        ExtensionExisted = $extensionExisted
        IconExisted = $iconExisted
        HadIconValue = $hadIconValue
        OldIconValue = $oldIconValue
        OldIconKind = $oldIconKind
        OpenWithExisted = $openWithExisted
    }
    try {
        $appType = $classes.CreateSubKey($progId)
        $appType.SetValue('', 'qrate (development)')
        $appType.CreateSubKey('DefaultIcon').SetValue('', '"' + $Executable + '",0')
        $appType.CreateSubKey('shell\open\command').SetValue('', '"' + $Executable + '" "%1"')
        $extension.CreateSubKey('OpenWithProgids').SetValue($progId, [byte[]]@(), [Microsoft.Win32.RegistryValueKind]::None)
        $icon.SetValue('', '"' + $Executable + '",0')
        Update-DevProjectShell
        return $state
    }
    catch {
        Unregister-DevProject $state
        throw
    }
}

function Unregister-DevProject {
    param([Parameter(Mandatory = $true)]$State)

    $classes = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Classes', $true)
    if ($null -eq $classes) { return }
    $extension = $classes.OpenSubKey('.qrate', $true)
    if ($null -ne $extension) {
        $openWith = $extension.OpenSubKey('OpenWithProgids', $true)
        if ($null -ne $openWith) {
            $openWith.DeleteValue($State.ProgId, $false)
            if (-not $State.OpenWithExisted -and $openWith.ValueCount -eq 0 -and $openWith.SubKeyCount -eq 0) {
                $openWith.Close()
                $extension.DeleteSubKey('OpenWithProgids', $false)
            }
        }
        $icon = $extension.OpenSubKey('DefaultIcon', $true)
        if ($null -ne $icon) {
            if ($State.HadIconValue) {
                $icon.SetValue('', $State.OldIconValue, $State.OldIconKind)
            } else {
                $icon.DeleteValue('', $false)
            }
            if (-not $State.IconExisted -and $icon.ValueCount -eq 0 -and $icon.SubKeyCount -eq 0) {
                $icon.Close()
                $extension.DeleteSubKey('DefaultIcon', $false)
            }
        }
        if (-not $State.ExtensionExisted -and $extension.ValueCount -eq 0 -and $extension.SubKeyCount -eq 0) {
            $extension.Close()
            $classes.DeleteSubKey('.qrate', $false)
        }
    }
    $classes.DeleteSubKeyTree($State.ProgId, $false)
    Update-DevProjectShell
}

function Update-DevProjectShell {
    if (-not ('QrateDevShell' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class QrateDevShell {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(uint eventId, uint flags, IntPtr item1, IntPtr item2);
}
'@
    }
    [QrateDevShell]::SHChangeNotify(0x08000000, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero)
}
