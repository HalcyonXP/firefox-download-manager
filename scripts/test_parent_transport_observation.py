"""Plain receipt models, not SDK/process/installed execution evidence."""
import copy
import json
import unittest
from qualification.parent_transport_observation import Observation, ObserverClient
from qualification.parent_observation import ObserverClient as FixtureClient

N='11111111-1111-4111-8111-111111111111';C='22222222-2222-4222-8222-222222222222'


def native():
    return {'version':1,'connection':1,'receipt':{'transport_admission_observed':True,'capture_ready':False,'successful':True,
            'launcher':{'spawn_called':True,'hooks_removed':True,'successful':True,'transport':{
                'startup':'started','process_waited':True,'exit_code':0,'pipes_closed':True,'io_settled':True,'forced':False,'successful':True}}}}


def snapshot(records=(),closed=False):
    return {'version':1,'qualification':False,'collector':C,'state':'closed' if closed else 'active',
            'removed':closed,'failed':False,'records':list(records)}


class ParentTransportObservationTests(unittest.TestCase):
    def test_bound_original_strings_and_independent_removal(self):
        evidence=Observation(N,C);self.assertEqual(evidence.accept(snapshot()),0)
        self.assertFalse(evidence.resource_retired())
        raw=json.dumps(native());self.assertEqual(evidence.accept(snapshot([raw])),1)
        self.assertEqual(evidence.require_retired(),1)
        with self.assertRaises(RuntimeError): evidence.require_removed()
        evidence.accept(snapshot([raw],True));value=evidence.require_removed()
        self.assertIs(value['capture_ready'],False);self.assertIs(value['qualification'],False)
        self.assertEqual(value['scope'],'installed-parent-transport-v1')
        with self.assertRaises(RuntimeError): evidence.accept(snapshot([raw]))
        self.assertFalse(evidence.resource_retired())

    def test_no_spawn_and_failed_but_joined_are_not_success(self):
        for mode in ('not-invoked','failed-exit','forced'):
            value=native();receipt=value['receipt'];launcher=receipt['launcher'];transport=launcher['transport']
            receipt['successful']=False;launcher['successful']=False
            if mode=='not-invoked': launcher.update(spawn_called=False,transport=None)
            elif mode=='failed-exit': transport.update(exit_code=2,successful=False)
            else: transport.update(forced=True,successful=False)
            evidence=Observation(N,C);evidence.accept(snapshot([json.dumps(value)],True))
            with self.subTest(mode=mode):
                self.assertTrue(evidence.resource_retired())
                with self.assertRaises(RuntimeError): evidence.require_removed()

    def test_missing_wait_pipe_io_or_unknown_creation_never_counts_retired(self):
        for field,value in [('process_waited',False),('pipes_closed',False),('io_settled',False),('exit_code',None),('startup','indeterminate')]:
            item=native();item['receipt']['launcher']['transport'][field]=value
            evidence=Observation(N,C);evidence.accept(snapshot([json.dumps(item)]))
            with self.subTest(field=field):
                self.assertFalse(evidence.resource_retired())
                with self.assertRaises(RuntimeError): evidence.require_retired()

    def test_closed_types_and_authority_are_not_truthy_or_open(self):
        mutations=[lambda x:x.update(version=True),lambda x:x.update(connection=True),lambda x:x.update(connection=0),
                   lambda x:x.update(connection=2**53),lambda x:x.update(extra=False),
                   lambda x:x['receipt'].update(capture_ready=True),lambda x:x['receipt'].update(successful=1),
                   lambda x:x['receipt']['launcher'].update(spawn_called=1),
                   lambda x:x['receipt']['launcher']['transport'].update(exit_code=False),
                   lambda x:x['receipt']['launcher']['transport'].update(process_waited=1)]
        for index,mutate in enumerate(mutations):
            value=native();mutate(value);evidence=Observation(N,C)
            with self.subTest(index=index),self.assertRaises(RuntimeError): evidence.accept(snapshot([json.dumps(value)]))
            with self.assertRaises(RuntimeError): evidence.accept(snapshot([json.dumps(native())]))

    def test_prefix_replacement_overflow_collector_and_corruption_stay_failed(self):
        raw=json.dumps(native())
        for changed in [snapshot([]),snapshot([raw,raw]),snapshot([raw+' ']),{**snapshot([raw]),'collector':N},
                        {**snapshot([raw]),'failed':True},snapshot(['{"version":1,"version":1}']),snapshot(['x'*4097])]:
            evidence=Observation(N,C);evidence.accept(snapshot([raw]))
            with self.subTest(changed=str(changed)[:40]),self.assertRaises((RuntimeError,ValueError)): evidence.accept(changed)
            self.assertFalse(evidence.resource_retired())

    def test_client_reuses_exact_owner_inverses_but_not_fixture_source_or_vocabulary(self):
        self.assertIs(ObserverClient._invoke,FixtureClient._invoke)
        self.assertIs(ObserverClient.remove,FixtureClient.remove)
        self.assertIs(ObserverClient.evidence_type,Observation)
        self.assertNotEqual(ObserverClient.source,FixtureClient.source)
        self.assertIn('download-manager-parent-retirement',ObserverClient.source)
        self.assertNotIn('download-manager-owned-parent-fixture',ObserverClient.source)
        self.assertNotEqual(ObserverClient.sandbox_prefix,FixtureClient.sandbox_prefix)


if __name__=='__main__': unittest.main()
