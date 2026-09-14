"""Retained Node compiler and exclusive parent-fixture inputs; no browser/native run.

The caller retains ParentBuild BEFORE execute(), plus independent native-build
provenance. Supplied hashes establish integrity, not that the native image is an
executable or that its compiler was joined. No CLI or SDK execution authority.
"""
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import subprocess
import struct

from . import parent_input as inputs
from .installed import ROOT, ordinary
from .native import file_sha256
from .setup_owner import DomainPlan
from .support import ARTIFACTS

ERROR = 'parent fixture compiler refused; retain owner and domain'
BUILD_SOURCES = (*inputs.SOURCES, 'scripts/build-parent-probe.mjs',
                 'scripts/qualification/parent_build.py', 'scripts/qualification/parent_input.py',
                 'package.json', 'package-lock.json', 'node_modules/esbuild/package.json',
                 'node_modules/esbuild/lib/main.js', 'node_modules/@esbuild/win32-x64/package.json',
                 'node_modules/@esbuild/win32-x64/esbuild.exe')


@dataclass(frozen=True)
class BuildPins:
    """Externally retained inputs; not a build or execution receipt."""
    commit: str
    source_dirty: bool
    sources: tuple
    node: Path
    node_sha256: str
    image: Path
    image_sha256: str
    nonce: str


def _hex(value, count):
    return type(value) is str and re.fullmatch('[a-f0-9]{'+str(count)+'}',value) is not None


class ParentBuild:
    def __init__(self, plan, pins):
        if (not __debug__ or os.name!='nt' or struct.calcsize('P')!=8
                or type(plan) is not DomainPlan or plan.created is not True
                or type(pins) is not BuildPins or not _hex(pins.commit,40)
                or type(pins.source_dirty) is not bool or type(pins.sources) is not tuple
                or len(pins.sources)!=len(BUILD_SOURCES)
                or any(type(pair) is not tuple or len(pair)!=2 or pair[0]!=name or not _hex(pair[1],64)
                       for name,pair in zip(BUILD_SOURCES,pins.sources))
                or not isinstance(pins.node,Path) or pins.node.name.lower()!='node.exe' or not isinstance(pins.image,Path)
                or not _hex(pins.node_sha256,64) or not _hex(pins.image_sha256,64)
                or type(pins.nonce) is not str or re.fullmatch(inputs.UUID,pins.nonce) is None):
            raise RuntimeError(ERROR)
        self.plan,self.domain,self.pins=plan,plan.path,pins
        self.process=None
        self.consumed=self.start_attempted=self.joined=self.failed=False
        self.exit_code=None
        self.expected=self.archive_sha256=self.identity=None
        self.stage='planned'

    def __repr__(self): return '<ParentBuild redacted>'

    def _domain(self):
        if (self.plan.path!=self.domain or self.plan.created is not True
                or self.domain.parent!=ARTIFACTS
                or re.fullmatch('dm-installed-'+inputs.UUID,self.domain.name) is None):
            raise RuntimeError(ERROR)
        ordinary(self.domain)

    def _inputs(self):
        self._domain()
        commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True,encoding='utf-8',timeout=15).strip()
        dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,timeout=15))
        if commit!=self.pins.commit or dirty is not self.pins.source_dirty: raise RuntimeError(ERROR)
        for name,digest in self.pins.sources:
            if inputs.digest(inputs._bytes(ROOT/name,inputs.IMAGE_LIMIT))!=digest: raise RuntimeError(ERROR)
        ordinary(self.pins.node)
        if file_sha256(self.pins.node)!=self.pins.node_sha256: raise RuntimeError(ERROR)
        image=inputs._bytes(self.pins.image,inputs.IMAGE_LIMIT)
        if inputs.digest(image)!=self.pins.image_sha256: raise RuntimeError(ERROR)
        return image

    def _environment(self):
        # Do not inherit NODE_OPTIONS/NODE_PATH, coverage/warning sinks, binary
        # overrides or package-manager hooks. Nothing changes in the parent env.
        home=self.domain/'compiler-home';home.mkdir()
        temporary=self.domain/'compiler-temp';temporary.mkdir()
        local=home/'local';local.mkdir()
        roaming=home/'roaming';roaming.mkdir()
        windows=Path(os.environ['WINDIR'])
        return {'SYSTEMROOT':str(windows),'WINDIR':str(windows),'PATH':str(windows/'System32'),
                'HOME':str(home),'USERPROFILE':str(home),'LOCALAPPDATA':str(local),'APPDATA':str(roaming),
                'TEMP':str(temporary),'TMP':str(temporary),
                'ESBUILD_WORKER_THREADS':'0','ESBUILD_MAX_BUFFER':'16777216'}

    def join(self):
        """Wait the exact returned Node owner; timeout/cancellation cannot authorize retry."""
        if self.joined: return
        if self.process is None:
            if self.start_attempted: raise RuntimeError(ERROR)
            return
        try:
            code=self.process.wait(timeout=120)
            if type(code) is not int: raise RuntimeError(ERROR)
            self.exit_code,self.joined=code,True
            if code!=0: self.failed=True
        except BaseException:
            self.failed=True
            raise

    def cleanup(self):
        """Consume even an unstarted controller; never kill a compiler or its children."""
        self.consumed=True
        self.join()
        return self.cleanup_complete()

    def cleanup_complete(self):
        # This is the returned Node owner only, not independent descendant joins.
        return not self.start_attempted or (self.process is not None and self.joined)

    def execute(self):
        if self.consumed: raise RuntimeError(ERROR)
        self.consumed=True
        try:
            self.stage='claim'
            self._domain()
            if {p.name for p in self.domain.iterdir()}!={'creation.private.json'}: raise RuntimeError(ERROR)
            with (self.domain/'parent-build.private.json').open('x',encoding='utf-8') as out:
                json.dump({'version':1,'qualification':False,'scope':'parent-fixture-compiler'},out)
            self.stage='inputs'
            image=self._inputs()
            native=self.domain/'native';native.mkdir()
            command=native/'download-manager-native-host.exe'
            with command.open('xb') as out: out.write(image)
            manifest={'name':inputs.HOST,'type':'stdio','path':str(command),'allowed_extensions':[inputs.ADDON]}
            with (native/(inputs.HOST+'.json')).open('x',encoding='utf-8',newline='\n') as out:
                out.write(json.dumps(manifest,indent=2,ensure_ascii=False)+'\n')
            environment=self._environment()
            self._inputs()
            self.stage='compiler'
            self.start_attempted=True
            self.process=subprocess.Popen([str(self.pins.node),str(ROOT/'scripts/build-parent-probe.mjs'),
                                           str(self.domain),self.pins.nonce],cwd=ROOT,env=environment,
                                          stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,
                                          creationflags=subprocess.CREATE_NO_WINDOW)
            self.join()
            if not self.joined or self.failed or self.exit_code!=0: raise RuntimeError(ERROR)
            self.stage='archive'
            self._inputs()
            record=inputs._bytes(self.domain/'probe/probe.json')
            expected=inputs.BuildExpectation(self.pins.commit,self.pins.source_dirty,self.domain,
                                             self.pins.nonce,self.pins.image_sha256,inputs.digest(record))
            _,identity=inputs.pack(self.domain,expected,clean=not self.pins.source_dirty)
            archive_sha256=identity['xpi_sha256']
            if inputs.inspect(self.domain,expected,archive_sha256,clean=not self.pins.source_dirty)!=identity:
                raise RuntimeError(ERROR)
            self._inputs()
            self.expected,self.archive_sha256,self.identity=expected,archive_sha256,identity
            self.stage='complete'
            return self.receipt()
        except BaseException:
            # Owners/partial outputs remain on this object. The caller must invoke
            # cleanup and keep it if waits remain unknown; no implicit restart.
            self.expected=self.archive_sha256=self.identity=None
            self.failed=True
            raise

    def receipt(self):
        if (self.failed or self.stage!='complete' or not self.joined or self.exit_code!=0
                or self.expected is None or self.archive_sha256 is None): raise RuntimeError(ERROR)
        return {'version':1,'qualification':False,'scope':'parent-fixture-compiler',
                'compiler_pid':self.process.pid,'compiler_waited':True,'compiler_exit':0,
                'native_fixture_executed':False,'browser_executed':False,
                'independent_compiler_descendant_waits':False,
                'source_commit':self.pins.commit,'source_dirty':self.pins.source_dirty,
                'node_sha256':self.pins.node_sha256,'fixture_image_sha256':self.pins.image_sha256,
                'xpi_sha256':self.archive_sha256}
