# Fixed bootstrap supplies validated opcode/path. This script never receives secrets.
$ErrorActionPreference = 'Stop'
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try { $sid = $identity.User.Value } finally { $identity.Dispose() }
    if ($sid -notmatch '^S-1-(5-21|12-1)-[0-9]+-[0-9]+-[0-9]+-[0-9]+$') { throw 'refused' }
    $sections = [Security.AccessControl.AccessControlSections]::Owner -bor [Security.AccessControl.AccessControlSections]::Access
    # The fixed Windows Modules Installer service can own drive ancestors. It
    # runs in the privileged SYSTEM boundary; no arbitrary service SID is trusted.
    $trusted = @($sid, 'S-1-5-18', 'S-1-5-32-544', 'S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464')
    $parent = [IO.Path]::GetDirectoryName($dmPath)
    $cursor = $parent
    $protected = $false
    while ($null -ne $cursor) {
        $actual = [IO.Directory]::GetAccessControl($cursor, $sections)
        $raw = [Security.AccessControl.RawSecurityDescriptor]::new($actual.GetSecurityDescriptorBinaryForm(), 0)
        if ($null -eq $raw.Owner -or $raw.Owner.Value -notin $trusted -or $null -eq $raw.DiscretionaryAcl) { throw 'refused' }
        # Adding sibling directories alone cannot replace a CreateDirectory winner.
        # Reject FILE_WRITE_DATA as potential reparse-control access as well as
        # changes to existing namespace/security, including parent DELETE_CHILD.
        $mask = if ($cursor -ceq $parent) { [int64]0x500d0152 } else { [int64]0x500d0112 }
        foreach ($ace in $raw.DiscretionaryAcl) {
            if ($ace.AceType -notin @([Security.AccessControl.AceType]::AccessAllowed, [Security.AccessControl.AceType]::AccessDenied)) { throw 'refused' }
            $inheritOnly = ([int]$ace.AceFlags -band [int][Security.AccessControl.AceFlags]::InheritOnly) -ne 0
            if (!$inheritOnly -and $ace.AceType -eq [Security.AccessControl.AceType]::AccessAllowed -and
                $ace.SecurityIdentifier.Value -notin $trusted -and ([int64]$ace.AccessMask -band $mask) -ne 0) { throw 'refused' }
        }
        if (([int]$raw.ControlFlags -band [int][Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) -ne 0) { $protected = $true }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
        if ($cursor -eq '') { $cursor = $null }
    }
    if (!$protected) { throw 'refused' }
    if ($dmOperation -ceq 'create') {
        # Rust's exclusive creation witness and retained no-delete handle are
        # prerequisites; a current-user read-only ACL is not evidence that this process created it.
        $actual = [IO.Directory]::GetAccessControl($dmPath, $sections)
        $raw = [Security.AccessControl.RawSecurityDescriptor]::new($actual.GetSecurityDescriptorBinaryForm(), 0)
        if ($null -eq $raw.Owner -or $raw.Owner.Value -cne $sid -or $null -eq $raw.DiscretionaryAcl -or
            $raw.DiscretionaryAcl.Count -ne 1 -or
            ([int]$raw.ControlFlags -band [int][Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) -eq 0) { throw 'refused' }
        $ace = $raw.DiscretionaryAcl[0]
        if ($ace.AceType -ne [Security.AccessControl.AceType]::AccessAllowed -or [int]$ace.AceFlags -ne 0 -or
            $ace.SecurityIdentifier.Value -cne $sid -or $ace.AccessMask -ne 0x120089) { throw 'refused' }
        $security = [Security.AccessControl.DirectorySecurity]::new()
        $security.SetSecurityDescriptorSddlForm(('D:P(A;OICI;FA;;;' + $sid + ')'), [Security.AccessControl.AccessControlSections]::Access)
        [IO.Directory]::SetAccessControl($dmPath, $security)
        $actual = [IO.Directory]::GetAccessControl($dmPath, $sections)
        $raw = [Security.AccessControl.RawSecurityDescriptor]::new($actual.GetSecurityDescriptorBinaryForm(), 0)
        if ($raw.Owner.Value -cne $sid -or $null -eq $raw.DiscretionaryAcl -or $raw.DiscretionaryAcl.Count -ne 1 -or
            ([int]$raw.ControlFlags -band [int][Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) -eq 0) { throw 'refused' }
        $ace = $raw.DiscretionaryAcl[0]
        if ($ace.AceType -ne [Security.AccessControl.AceType]::AccessAllowed -or [int]$ace.AceFlags -ne 3 -or
            $ace.SecurityIdentifier.Value -cne $sid -or $ace.AccessMask -ne 0x1f01ff) { throw 'refused' }
    } elseif ($dmOperation -cne 'verify') { throw 'refused' }
    [Console]::Out.Write("ready`n")
    [Console]::Out.Flush()
    if ($dmInput.ReadLine() -cne 'close') { throw 'refused' }
    [Console]::Out.Write('ok')
} catch {
    exit 1
}
