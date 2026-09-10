# Fixed Windows PowerShell/.NET file-security adapter. No profile, external command,
# P/Invoke, dynamic evaluation, registry access or network operation.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try {
    # Input reader is established by the fixed Rust-side bootstrap.
    # Only an operation and path arrive here; secret bytes never enter PowerShell.
    $request = ConvertFrom-Json -InputObject ($dmInput.ReadLine())
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try { $sid = $identity.User.Value } finally { $identity.Dispose() }
    if ($sid -notmatch '^S-1-(5-21|12-1)-[0-9]+-[0-9]+-[0-9]+-[0-9]+$') { throw 'refused' }

    # A retained directory lease prevents path replacement. Also require that
    # unprivileged other principals cannot mutate the parent or its children.
    $sections = [Security.AccessControl.AccessControlSections]::Owner -bor [Security.AccessControl.AccessControlSections]::Access
    $parent = [IO.Directory]::GetAccessControl([IO.Path]::GetDirectoryName($request.path), $sections)
    $raw = [Security.AccessControl.RawSecurityDescriptor]::new($parent.GetSecurityDescriptorBinaryForm(), 0)
    $trusted = @($sid, 'S-1-5-18', 'S-1-5-32-544')
    if ($null -eq $raw.Owner -or $raw.Owner.Value -notin $trusted -or $null -eq $raw.DiscretionaryAcl -or
        ([int]$raw.ControlFlags -band [int][Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) -eq 0) { throw 'refused' }
    foreach ($ace in $raw.DiscretionaryAcl) {
        if ($ace.AceType -notin @([Security.AccessControl.AceType]::AccessAllowed, [Security.AccessControl.AceType]::AccessDenied)) { throw 'refused' }
        if ($ace.AceType -eq [Security.AccessControl.AceType]::AccessAllowed -and
            ([int]$ace.AceFlags -band [int][Security.AccessControl.AceFlags]::InheritOnly) -eq 0 -and
            $ace.SecurityIdentifier.Value -notin $trusted -and
            ([int64]$ace.AccessMask -band [int64]0x500d0156) -ne 0) { throw 'refused' }
    }

    if ($request.operation -eq 'create') {
        $security = [Security.AccessControl.FileSecurity]::new()
        $security.SetSecurityDescriptorSddlForm(('O:' + $sid + 'D:P(A;;FA;;;' + $sid + ')'))
        # CreateNew and the supplied descriptor apply in the OS create operation.
        # Retain read/write access and deny deletion until Rust finishes writing.
        # A reader denying shared writes cannot open the incomplete record.
        $rights = [Security.AccessControl.FileSystemRights]::Read -bor [Security.AccessControl.FileSystemRights]::Write -bor [Security.AccessControl.FileSystemRights]::Synchronize
        $file = [IO.FileStream]::new($request.path, [IO.FileMode]::CreateNew,
            $rights, [IO.FileShare]::ReadWrite, 4096, [IO.FileOptions]::None, $security)
    } elseif ($request.operation -ne 'verify') { throw 'refused' }

    # Read back actual owner/protected DACL, not just the requested descriptor.
    $actual = [IO.File]::GetAccessControl($request.path, $sections)
    $raw = [Security.AccessControl.RawSecurityDescriptor]::new($actual.GetSecurityDescriptorBinaryForm(), 0)
    if ($null -eq $raw.Owner -or $raw.Owner.Value -ne $sid -or
        ([int]$raw.ControlFlags -band [int][Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) -eq 0 -or
        $null -eq $raw.DiscretionaryAcl -or $raw.DiscretionaryAcl.Count -ne 1) { throw 'refused' }
    $ace = $raw.DiscretionaryAcl[0]
    if ($ace.AceType -ne [Security.AccessControl.AceType]::AccessAllowed -or
        $ace.AceFlags -ne [Security.AccessControl.AceFlags]::None -or
        $ace.SecurityIdentifier.Value -ne $sid -or $ace.AccessMask -ne 0x1f01ff) { throw 'refused' }
    [Console]::Out.Write("ready`n")
    [Console]::Out.Flush()
    if ($dmInput.ReadLine() -cne 'close') { throw 'refused' }
    if ($null -ne $file) { $file.Dispose(); $file = $null }
    [Console]::Out.Write('ok')
    exit 0
} catch {
    # Never echo input, paths, SIDs, payloads or OS exception diagnostics.
    exit 1
} finally {
    if ($null -ne $file) { $file.Dispose() }
}
