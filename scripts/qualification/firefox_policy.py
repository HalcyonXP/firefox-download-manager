"""Read-only owned-profile policy guard; never repair or enable protection settings."""
NAMES = (
    'xpinstall.signatures.required', 'extensions.experiments.enabled',
    'browser.safebrowsing.malware.enabled', 'browser.safebrowsing.phishing.enabled',
    'browser.safebrowsing.downloads.enabled', 'browser.safebrowsing.downloads.remote.enabled',
    'browser.safebrowsing.blockedURIs.enabled', 'app.update.disabledForTesting',
    'extensions.update.enabled', 'extensions.systemAddon.update.enabled',
)
REQUIRED_ON = frozenset(NAMES[2:6])
SNAPSHOT = """const names=arguments[0];const defaults=Services.prefs.getDefaultBranch('');
function bool(branch,name){const type=branch.getPrefType(name);
if(type===0)return null;if(type!==128)throw new Error('owned policy preference type refused');
return branch.getBoolPref(name);}
return {recommended:bool(Services.prefs,'remote.prefs.recommended'),
applied:bool(Services.prefs,'remote.prefs.recommended.applied'),
preferences:Object.fromEntries(names.map(name=>[name,{value:bool(Services.prefs,name),
default:bool(defaults,name),user:Services.prefs.prefHasUserValue(name)}]))};"""


def validate(value):
    if (not isinstance(value,dict) or set(value)!={'recommended','applied','preferences'}
            or value['recommended'] is not False or (value['applied'] is not None and value['applied'] is not False)
            or not isinstance(value['preferences'],dict) or set(value['preferences'])!=set(NAMES)):
        raise RuntimeError('owned Firefox automation policy refused')
    for name, entry in value['preferences'].items():
        if (not isinstance(entry,dict) or set(entry)!={'value','default','user'} or entry['user'] is not False
                or any(v is not None and type(v) is not bool for v in (entry['value'],entry['default']))
                or entry['value'] is not entry['default'] or (name in REQUIRED_ON and entry['value'] is not True)
                or (name=='app.update.disabledForTesting' and entry['value'] is True)):
            raise RuntimeError('owned Firefox protection baseline refused')
    return value


def observe(browser):
    return validate(browser.chrome(SNAPSHOT,[list(NAMES)]))


def unchanged(browser, before):
    if observe(browser)!=validate(before):
        raise RuntimeError('owned Firefox protection baseline changed')
