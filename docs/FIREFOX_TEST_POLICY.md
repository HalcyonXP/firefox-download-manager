# Owned Firefox automation policy

## Corrected harness boundary

The shared `scripts/qualification/firefox.py` driver previously wrote `marionette.prefs.recommended=false`. Firefox 156 instead reads **`remote.prefs.recommended`**. The obsolete name did not opt out of automation overrides.

The clean `5e90bc6` fileless probe observed malware, phishing and download Safe Browsing preferences already false at its first owned-profile readback. They remained false through the attempt. Temporary installation refused with the fixed terms `experiment-apis`, `invalid-extension`, `privilege-required`; the retained browser exited with code0 and was joined, the driver was waited, and final process/registration absence passed. No service query or success report occurred.

Matching Firefox source calls `RecommendedPreferences.applyPreferences()` during Marionette startup. Unless the current opt-out is false, that module applies the observed Safe Browsing overrides, disables application/extension updates for testing, and changes other automation defaults. Comparing two readbacks **after startup** could not detect these earlier changes. The old opt-out name and this source path explain the harness defect; not every historical runtime preference was recorded.

The experiment refusal is separate: `browser/app/profile/firefox.js` explicitly defaults `extensions.experiments.enabled` to false, matching the observed value. Correcting the automation opt-out does not enable experimental APIs. No experiment/signature/Safe Browsing preference override is introduced by this correction.

Source: Mozilla revision [`574c275bcf5b4f86198c979b7e61f4a844aba0ea`](https://hg.mozilla.org/releases/mozilla-beta/file/574c275bcf5b4f86198c979b7e61f4a844aba0ea/), `remote/components/Marionette.sys.mjs`, `remote/shared/RecommendedPreferences.sys.mjs`, and `browser/app/profile/firefox.js`.

## Default-mode guards

- Write only the supported automation opt-out in the exclusively owned test profile **before** launch. This prevents automation from overriding defaults; it does not write individual protection settings.
- Before returning a usable browser owner, require recommended overrides disabled and their applied marker absent/false. Read a fixed set of signing, experiment, Safe Browsing and update preferences from both effective and default branches. Refuse user overrides, mismatches, malformed values, disabled core Safe Browsing or updates disabled for testing; never repair them.
- Retain the validated snapshot. Recheck it before shutdown; a changed/failed readback still requires retained browser retirement and joining before refusal. Probe reports/failure records carry this bounded policy receipt when available.
- Normal profiles are never inspected or changed. The clean2b643c2 and cleane1a982f fileless experiment-mode runs verified the corrected opt-out and selected default protection values through joined shutdown; ordinary capture/installation under the corrected policy remains unqualified. The fixed preference set is not an exhaustive policy-equivalence audit.

## Separate fileless experiment mode

`probe-download-protection.py --enable-fileless-experiment` explicitly selects an additional diagnostic profile mode; omission remains default mode. The execution flag and original preflights are still required. This mode exclusively creates a new profile, refuses any existing profile or combined companion/browser owner, and writes only `extensions.experiments.enabled=true` in addition to the existing fixed harness preferences. It does not accept arbitrary preference names or values.

The policy guard does not infer an exception from observed data: only this explicit mode accepts the exact experiment tuple **effective true / default false / user override true**. Every other selected preference retains the default-mode checks, and signing enforcement must also be true. Startup and shutdown use the same mode and verify stable snapshots. Reports/failure records distinguish `profile_mode: fileless-experiment` from `default`; normal profiles and product builds are unaffected.

This temporary capability test is not preservation of the experiment preference's default, ordinary XPI installation, native-file verdict integration or persistence qualification. Clean2b643c2 passed the fixed empty-context query; cleane1a982f additionally observed typed synthetic referrer/redirect getter consumption. Both passed the negative caller/replay cases with exactly this override and unchanged selected protection defaults. See [the exact source/receipt scope](FIREFOX_PROTECTION_BRIDGE.md#source-scoped-service-observation); no real-file or persistence result is established.

## Earlier evidence

Earlier campaigns using this shared driver cannot establish preservation of default Firefox protections or ordinary non-automation policy behavior. Their source-specific file hashes, task receipts, observed UI/cancellation behavior and joined lifetimes are not erased, but their automated environment must be considered when interpreting them. Protection parity, ordinary persistent installation and final paired acceptance remain unqualified. The recorded unsigned-install refusal still describes that isolated automated run, not normal-profile compatibility or a new signing requirement.

See [protection bridge](FIREFOX_PROTECTION_BRIDGE.md), [browser campaign](INSTALLED_BROWSER_SLICE.md), [persistence](FIREFOX_PERSISTENCE.md) and [delivery plan](PROJECT_PLAN.md).
