"""Metadata-only parent archive fixtures; never execute their payloads."""
import copy
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile
from qualification import parent_candidate as p


class ParentCandidateTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name).resolve();self.directory=self.root/'candidate';self.directory.mkdir()
        self.scope=patch.object(p,'ARTIFACTS',self.root);self.scope.start();self.addCleanup(self.scope.stop)
        self.files={n:b'metadata-only; not executable evidence' for n in p.PAYLOADS}
        self.files['manifest.json']=(p.ROOT/'extension/parent-bridge/manifest.json').read_bytes()
        self.files['parent-schema.json']=(p.ROOT/'extension/parent-bridge/schema.json').read_bytes()
        for n,b in self.files.items(): (self.directory/n).write_bytes(b)
        self.meta={'version':1,'candidate':True,'qualification':False,'mode':'parent-transport','capture_ready':False,
                   'source_commit':'a'*40,'source_dirty':False,'files':{n:hashlib.sha256(b).hexdigest() for n,b in self.files.items()}}
        self.write_meta()

    def write_meta(self): (self.directory/'BUILD.json').write_text(json.dumps(self.meta),encoding='utf-8')

    def test_exclusive_archive_external_pin_and_clean_source(self):
        identity=p.package(self.directory)
        xpi,meta=p.candidate_input(self.directory,identity['xpi_sha256'])
        self.assertEqual(meta,self.meta);self.assertEqual(p.inspect(xpi.read_bytes()),self.meta['files'])
        with self.assertRaises(FileExistsError): p.package(self.directory)
        for bad in (None,True,'A'*64,'0'*64):
            with self.subTest(pin=bad),self.assertRaises(RuntimeError): p.candidate_input(self.directory,bad)
        self.meta['source_dirty']=True;self.write_meta()
        with self.assertRaises(RuntimeError): p.candidate_input(self.directory,identity['xpi_sha256'])

    def test_closed_metadata_and_exact_boolean_types(self):
        original=copy.deepcopy(self.meta)
        for key,value in [('version',True),('candidate',1),('qualification',0),('capture_ready',True),
                          ('source_dirty',0),('mode','ordinary'),('source_commit','a'*39),('extra',False)]:
            with self.subTest(key=key):
                self.meta={**original,key:value};self.write_meta()
                with self.assertRaises(RuntimeError): p.inputs(self.directory,clean=True)

    def test_payload_hash_and_independent_authority(self):
        (self.directory/'parent-api.js').write_bytes(b'changed')
        with self.assertRaises(RuntimeError): p.inputs(self.directory,clean=True)
        for which,edit in [('manifest.json',lambda x:x.update(permissions=[*x['permissions'],'webRequest'])),
                          ('parent-schema.json',lambda x:x[0]['functions'].append({'name':'capture','type':'function','parameters':[]}))]:
            files=copy.deepcopy(self.files);value=json.loads(files[which]);edit(value);files[which]=json.dumps(value).encode()
            with self.subTest(which=which),self.assertRaises(RuntimeError): p.authority(files)

    def test_archive_refuses_noncanonical_suffix_order_attributes_and_duplicates(self):
        p.package(self.directory);data=(self.directory/p.ARCHIVE).read_bytes()
        with self.assertRaises(RuntimeError): p.inspect(data+b'not an archive member')
        for kind in ('order','attributes','duplicate','compression','comment'):
            out=io.BytesIO()
            with zipfile.ZipFile(out,'w') as archive:
                names=sorted(self.files,reverse=kind=='order')
                if kind=='duplicate': names[-1]=names[0]
                if kind=='comment': archive.comment=b'not allowed'
                for name in names:
                    entry=zipfile.ZipInfo(name,(1980,1,1,0,0,0));entry.create_system=3;entry.external_attr=0o100644<<16
                    if kind=='attributes': entry.external_attr|=1
                    if kind=='compression': entry.compress_type=zipfile.ZIP_DEFLATED
                    archive.writestr(entry,self.files[name])
            with self.subTest(kind=kind),self.assertRaises(RuntimeError): p.inspect(out.getvalue())

    def test_oversize_payload_duplicate_json_and_alias_refused(self):
        (self.directory/'parent-api.js').write_bytes(b'x'*(p.LIMIT+1))
        with self.assertRaises(RuntimeError): p.inputs(self.directory,clean=True)
        with self.assertRaises((RuntimeError,ValueError)): p.decode(b'{"version":1,"version":1}')
        with self.assertRaises(RuntimeError): p.inputs(self.directory/'..'/'candidate',clean=True)


if __name__=='__main__': unittest.main()
