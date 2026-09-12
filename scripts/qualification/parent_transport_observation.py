"""Independent bounded shutdown observations; no process/admission/policy authority."""
import json
from pathlib import Path
from .parent_observation import ObserverClient as BaseClient, _keys, _uuid
from .support import unique_object, invalid_constant

ERROR = 'owned parent transport observation refused'


def record(raw):
    if type(raw) is not str or not raw.isascii() or not 0 < len(raw) <= 4096: raise RuntimeError(ERROR)
    value = json.loads(raw, object_pairs_hook=unique_object, parse_constant=invalid_constant)
    _keys(value, 'version connection receipt')
    if type(value['version']) is not int or value['version'] != 1 or type(value['connection']) is not int or not 0 < value['connection'] <= 2**53-1: raise RuntimeError(ERROR)
    receipt=value['receipt'];_keys(receipt,'transport_admission_observed capture_ready launcher successful')
    if receipt['capture_ready'] is not False or any(type(receipt[k]) is not bool for k in ('transport_admission_observed','successful')): raise RuntimeError(ERROR)
    launcher=receipt['launcher'];_keys(launcher,'spawn_called transport hooks_removed successful')
    if any(type(launcher[k]) is not bool for k in ('spawn_called','hooks_removed','successful')): raise RuntimeError(ERROR)
    transport=launcher['transport']
    if transport is not None:
        _keys(transport,'startup process_waited exit_code pipes_closed io_settled forced successful')
        if (transport['startup'] not in ('started','not-started','indeterminate')
                or any(type(transport[k]) is not bool for k in ('process_waited','pipes_closed','io_settled','forced','successful'))
                or (transport['exit_code'] is not None and (type(transport['exit_code']) is not int or not 0 <= transport['exit_code'] <= 0xffffffff))): raise RuntimeError(ERROR)
    return value


class Observation:
    def __init__(self, nonce, collector):
        if not _uuid(nonce) or not _uuid(collector) or nonce==collector: raise RuntimeError(ERROR)
        self.nonce,self.collector=nonce,collector
        self.records=();self.closed=self.removed=self.failed=False

    def invalidate(self): self.failed=True

    def accept(self, value):
        try:
            if self.failed: raise RuntimeError(ERROR)
            _keys(value,'version qualification collector state removed failed records')
            if (type(value['version']) is not int or value['version']!=1 or value['qualification'] is not False
                    or value['collector']!=self.collector or value['state'] not in ('active','closed')
                    or type(value['removed']) is not bool or value['failed'] is not False
                    or (value['removed'] and value['state']!='closed') or (self.closed and value['state']!='closed')
                    or (self.removed and not value['removed']) or type(value['records']) is not list
                    or len(value['records'])>1 or tuple(value['records'][:len(self.records)])!=self.records): raise RuntimeError(ERROR)
            for raw in value['records']: record(raw)
            self.records=tuple(value['records']);self.closed=value['state']=='closed';self.removed=value['removed']
            return len(self.records)
        except BaseException:
            self.failed=True
            raise

    def resource_retired(self):
        if self.failed or len(self.records)!=1: return False
        launcher=record(self.records[0])['receipt']['launcher'];transport=launcher['transport']
        if launcher['spawn_called'] is False:
            return transport is None or (transport['startup']!='started' and transport['process_waited'] is False
                    and transport['exit_code'] is None and transport['io_settled'] is True and transport['forced'] is False)
        if transport is None: return False
        return (transport['startup']=='started' and transport['process_waited'] is True
                and transport['pipes_closed'] is True and transport['io_settled'] is True
                and type(transport['exit_code']) is int)

    def require_retired(self):
        if not self.resource_retired(): raise RuntimeError(ERROR)
        value=record(self.records[0]);receipt=value['receipt'];launcher=receipt['launcher'];transport=launcher['transport']
        if (receipt['transport_admission_observed'] is not True or receipt['successful'] is not True
                or any(launcher[k] is not True for k in ('spawn_called','hooks_removed','successful'))
                or transport is None or transport['successful'] is not True or transport['exit_code'] != 0 or transport['forced'] is not False): raise RuntimeError(ERROR)
        return value['connection']

    def require_removed(self):
        connection=self.require_retired()
        if not self.closed or not self.removed: raise RuntimeError(ERROR)
        return {'version':1,'qualification':False,'scope':'installed-parent-transport-v1','connection':connection,
                'sdk_retirement_observed':True,'observer_removal_observed':True,'capture_ready':False}


class ObserverClient(BaseClient):
    source=Path(__file__).with_name('parent_transport_observer.js').read_text(encoding='utf-8')
    evidence_type=Observation
    sandbox_prefix='owned-parent-transport-observer-'
