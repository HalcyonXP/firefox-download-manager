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


def validate(value, *, fileless_experiment=False):
    if type(fileless_experiment) is not bool:
        raise RuntimeError('explicit fileless experiment mode required')
    if (not isinstance(value,dict) or set(value)!={'recommended','applied','preferences'}
            or value['recommended'] is not False or (value['applied'] is not None and value['applied'] is not False)
            or not isinstance(value['preferences'],dict) or set(value['preferences'])!=set(NAMES)):
        raise RuntimeError('owned Firefox automation policy refused')
    for name, entry in value['preferences'].items():
        if fileless_experiment and name=='extensions.experiments.enabled':
            if (not isinstance(entry,dict) or set(entry)!={'value','default','user'}
                    or entry['value'] is not True or entry['default'] is not False or entry['user'] is not True):
                raise RuntimeError('exact fileless experiment override required')
            continue
        if (not isinstance(entry,dict) or set(entry)!={'value','default','user'} or entry['user'] is not False
                or any(v is not None and type(v) is not bool for v in (entry['value'],entry['default']))
                or entry['value'] is not entry['default'] or (name in REQUIRED_ON and entry['value'] is not True)
                or (name=='app.update.disabledForTesting' and entry['value'] is True)
                or (fileless_experiment and name=='xpinstall.signatures.required' and entry['value'] is not True)):
            raise RuntimeError('owned Firefox protection baseline refused')
    return value


def observe(browser, *, fileless_experiment=False):
    return validate(browser.chrome(SNAPSHOT,[list(NAMES)]),fileless_experiment=fileless_experiment)


def unchanged(browser, before, *, fileless_experiment=False):
    if observe(browser,fileless_experiment=fileless_experiment)!=validate(before,fileless_experiment=fileless_experiment):
        raise RuntimeError('owned Firefox protection baseline changed')
