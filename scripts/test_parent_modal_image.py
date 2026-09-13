"""Owned private-image fixtures only; no browser, desktop or normal-profile capture."""
import base64
import struct
import tempfile
from pathlib import Path
import unittest
from unittest.mock import Mock,patch
import zlib
from qualification import parent_modal_image as m
from qualification import parent_installed as p
import test_parent_installed as models


def chunk(kind,body):
    return struct.pack('>I',len(body))+kind+body+struct.pack('>I',zlib.crc32(kind+body))


def image(width=1,height=1,raw=b'\0\xff\xff\xff\xff'):
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b'')


def response(data=None,**changes):
    return {'version':2,'kind':'common','prompt':'alert','message':'unknown','png':None if data is None else base64.b64encode(data).decode('ascii'),**changes}


class ModalImageTests(unittest.TestCase):
    def test_private_png_is_bounded_validated_and_exclusive(self):
        with tempfile.TemporaryDirectory() as directory:
            profile=Path(directory).resolve();summary=m.record(profile,response(image()))
            self.assertEqual(summary,{'state':'observed','kind':'common','prompt':'alert','message':'unknown','image_written':True})
            out=profile/'owned-modal.private.png';self.assertEqual(out.read_bytes(),image())
            with self.assertRaises(FileExistsError):m.record(profile,response(image()))
            self.assertEqual(out.read_bytes(),image())

    def test_authentication_unknown_and_other_dialogs_never_write_pixels(self):
        with tempfile.TemporaryDirectory() as directory:
            profile=Path(directory).resolve()
            for kind,prompt in [('common','promptPassword'),('common','promptUserAndPass'),('common','prompt'),('common','unknown'),('other','unknown')]:
                self.assertFalse(m.record(profile,response(None,kind=kind,prompt=prompt))['image_written'])
                with self.assertRaises(RuntimeError):m.record(profile,response(image(),kind=kind,prompt=prompt))
            self.assertEqual(list(profile.iterdir()),[])

    def test_malformed_envelopes_refuse_without_files(self):
        with tempfile.TemporaryDirectory() as directory:
            profile=Path(directory).resolve()
            for value in (None,response(version=True),response(kind=[]),response(prompt=[]),response(kind='other',prompt='alert'),{**response(),'extra':'opaque'}):
                with self.subTest(value=value),self.assertRaises(RuntimeError):m.record(profile,value)
            self.assertEqual(list(profile.iterdir()),[])

    def test_png_corruption_dimensions_metadata_and_inflate_excess_refuse(self):
        bad=image()[:-1]+b'x'
        text=image()[:-12]+chunk(b'tEXt',b'private metadata')+chunk(b'IEND',b'')
        for data in (b'',bad,image(1601),image(1,1201),image(raw=b'\0'*100000),image(raw=b'\5\0\0\0\0'),image()+b'extra',text):
            with self.subTest(length=len(data)),self.assertRaises(RuntimeError):m.png_bytes(base64.b64encode(data).decode('ascii'))
        for text in ('not base64','é','A'*1400001):
            with self.assertRaises(RuntimeError):m.png_bytes(text)

    def browser(self,profile):
        b,_=models.ParentInstalledTests().browser();b.profile=profile;b.verified=True;b.parent_transport_experiment=True
        b.load_attempted=False;b.tab_attempted=False;b.manager_handle=None
        b.command.side_effect=None;b.command.return_value={'handle':'control','type':'tab'}
        b.read_tab_state.return_value={'version':1,**{key:True for key in p.TAB_FIELDS}}
        b.read_modal_state=Mock(return_value=response(image()))
        return b

    def test_original_failure_stays_failed_with_private_image_and_no_retry(self):
        with tempfile.TemporaryDirectory() as directory:
            profile=Path(directory).resolve();b=self.browser(profile)
            with self.assertRaises(RuntimeError):b.load(Path('unused'))
            self.assertEqual(b.first_failure['stage'],'manager-tab-response-distinct')
            self.assertTrue(b.modal_failure['image_written']);self.assertTrue(b.failed);self.assertFalse(b.load_attempted)
            b.observe_tab_failure();b.read_modal_state.assert_called_once()
            self.assertNotIn('png',b.diagnostic()['modal_failure'])
            self.assertEqual((profile/'owned-modal.private.png').read_bytes(),image())

    def test_image_sink_failure_consumes_attempt_and_keeps_first_refusal(self):
        with tempfile.TemporaryDirectory() as directory:
            profile=Path(directory).resolve();b=self.browser(profile)
            with patch.object(p,'record_modal_image',side_effect=OSError('uncertain private sink')) as write:
                with self.assertRaises(RuntimeError):b.load(Path('unused'))
                b.observe_tab_failure();write.assert_called_once()
            self.assertEqual(b.modal_failure,{'state':'unavailable'})
            self.assertEqual(b.first_failure['stage'],'manager-tab-response-distinct')
            b.read_modal_state.assert_called_once();self.assertTrue(b.failed)

    def test_foreign_original_process_refuses_image_command(self):
        with tempfile.TemporaryDirectory() as directory:
            b=self.browser(Path(directory).resolve());b.process=Mock(pid=b.original.pid)
            with self.assertRaises(RuntimeError):b.load(Path('unused'))
            b.read_modal_state.assert_not_called();self.assertEqual(b.modal_failure,{'state':'unavailable'})

    def test_spotlight_classes_never_authorize_private_pixels(self):
        with tempfile.TemporaryDirectory() as directory:
            profile=Path(directory).resolve()
            for message in m.MESSAGES:
                value=response(kind='spotlight',prompt='unknown',message=message)
                summary=m.record(profile,value)
                self.assertEqual(summary['message'],message);self.assertFalse(summary['image_written'])
                with self.assertRaises(RuntimeError):m.record(profile,{**value,'png':base64.b64encode(image()).decode('ascii')})
            for value in (response(message='new-user-terms'),response(kind='spotlight'),response(message=[]),response(kind='spotlight',prompt='unknown',message='opaque-id'),response(version=1)):
                with self.assertRaises(RuntimeError):m.record(profile,value)
            self.assertEqual(list(profile.iterdir()),[])
