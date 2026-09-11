"""Owned policy models only: no real browser, profile discovery or registration."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

from qualification import firefox, firefox_policy as policy
from qualification.support import ARTIFACTS


def baseline():
    return {'recommended':False,'applied':None,'preferences':{
        name:{'value':name not in ('extensions.experiments.enabled','app.update.disabledForTesting'),
              'default':name not in ('extensions.experiments.enabled','app.update.disabledForTesting'),'user':False}
        for name in policy.NAMES}}


class FirefoxPolicyTests(unittest.TestCase):
    def test_default_experiment_off_is_not_enabled_or_treated_as_protection_failure(self):
        value=baseline();self.assertEqual(policy.validate(value),value)
        self.assertFalse(value['preferences']['extensions.experiments.enabled']['value'])
        for setter in ('setBoolPref','setCharPref','clearUserPref','setIntPref'):self.assertNotIn(setter,policy.SNAPSHOT)
        self.assertIn('getDefaultBranch',policy.SNAPSHOT);self.assertIn('prefHasUserValue',policy.SNAPSHOT)

    def test_automation_overrides_and_malformed_receipts_refuse(self):
        value=baseline()
        for changed in [None,True,{**value,'recommended':True},{**value,'recommended':0},
                        {**value,'applied':True},{**value,'applied':0},{**value,'extra':True},
                        {**value,'preferences':{}}, {**value,'preferences':{'foreign':{}}}]:
            with self.assertRaises(RuntimeError):policy.validate(changed)
        for field,replacement in [('value',0),('default',0),('user',0),('user',True),('value',False)]:
            changed=copy.deepcopy(value);changed['preferences']['xpinstall.signatures.required'][field]=replacement
            with self.assertRaises(RuntimeError):policy.validate(changed)

    def test_weak_safe_browsing_and_test_disabled_updates_refuse_without_repair(self):
        required={'browser.safebrowsing.malware.enabled','browser.safebrowsing.phishing.enabled',
                  'browser.safebrowsing.downloads.enabled','browser.safebrowsing.downloads.remote.enabled'}
        self.assertEqual(policy.REQUIRED_ON,required)
        for name in required | {'app.update.disabledForTesting'}:
            value=baseline();replacement=name=='app.update.disabledForTesting'
            value['preferences'][name].update(value=replacement,default=replacement)
            with self.assertRaises(RuntimeError):policy.validate(value)
            self.assertIs(value['preferences'][name]['value'],replacement)

    def test_unchanged_compares_validated_full_default_and_effective_snapshots(self):
        value=baseline();browser=Mock();browser.chrome.return_value=copy.deepcopy(value)
        policy.unchanged(browser,value)
        browser.chrome.assert_called_with(policy.SNAPSHOT,[list(policy.NAMES)])
        changed=copy.deepcopy(value);changed['preferences']['extensions.experiments.enabled'].update(value=True,default=True)
        browser.chrome.return_value=changed
        with self.assertRaises(RuntimeError):policy.unchanged(browser,value)

    def test_actual_start_writes_current_opt_out_and_refuses_before_returning_a_weak_browser(self):
        ARTIFACTS.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ARTIFACTS,prefix='policy-model-') as directory:
            profile=Path(directory).resolve()/'profile';browser=firefox.Firefox(Path('unused'),profile,{})
            process=Mock();browser._require_apps_closed=Mock();browser.receive=Mock(return_value={'applicationType':'gecko','marionetteProtocol':3})
            browser.command=Mock(return_value={'capabilities':{'moz:profile':str(profile),'browserName':'firefox','browserVersion':'156.0'}})
            weak=baseline();weak['recommended']=True
            browser.chrome=Mock(side_effect=[{'value':True,'user':False},weak])
            with patch.object(firefox.socket,'socket') as reservation,patch.object(firefox.socket,'create_connection',return_value=Mock()), \
                 patch.object(firefox.subprocess,'Popen',return_value=process):
                reservation.return_value.__enter__.return_value.getsockname.return_value=('127.0.0.1',32100)
                with self.assertRaises(RuntimeError):browser.start()
            self.assertIs(browser.process,process);self.assertIsNone(browser.automation_policy)
            prefs=(profile/'user.js').read_text(encoding='utf-8')
            self.assertIn('user_pref("remote.prefs.recommended", false);',prefs)
            self.assertNotIn('marionette.prefs.recommended',prefs)
            for name in policy.NAMES:self.assertNotIn(json.dumps(name),prefs)

    def test_changed_policy_still_retires_and_joins_before_close_refuses(self):
        browser=firefox.Firefox(Path('unused'),Path('unused'),{});browser.verified=True
        browser.process=Mock();browser.connection=Mock();browser.command=Mock();browser._require_apps_closed=Mock()
        browser.automation_policy=baseline();browser.chrome=Mock(return_value={**baseline(),'applied':True})
        with self.assertRaises(RuntimeError):browser.close()
        browser.process.wait.assert_called_once_with(timeout=5)
        browser.command.assert_called_once_with('Marionette:Quit',{'flags':['eForceQuit']})
        self.assertTrue(browser.closed)


if __name__=='__main__':unittest.main()
