import importlib.util
import io
import os
from pathlib import Path
import sys
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/release'))
from download_transport import transport_url, safe_url
spec = importlib.util.spec_from_file_location('native_download', ROOT / 'tools/release/publish-native.py')
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class DownloadTransportTest(unittest.TestCase):
    def test_maps_only_configured_object_prefix(self):
        with patch.dict(os.environ, {'OSS_PUBLIC_BASE':'https://cdn.example/files', 'OSS_DOWNLOAD_BASE':'https://bucket.example'}):
            self.assertEqual(transport_url('https://cdn.example/files/a.zip'), 'https://bucket.example/a.zip')
            for url in ['https://cdn.example/files-other/a.zip', 'https://cdn.example.evil/files/a.zip',
                        'https://vendor.example/a.zip', 'https://cdn.example/files/a.zip?signature=private']:
                self.assertEqual(transport_url(url), url)

    def test_existing_immutable_url_is_preserved_while_read_uses_transport(self):
        with patch.dict(os.environ, {'OSS_PUBLIC_BASE':'https://cdn.example', 'OSS_DOWNLOAD_BASE':'https://bucket.example'}):
            with patch.object(native, 'get', return_value=b'original') as read:
                with patch.object(Path, 'read_bytes', return_value=b'original'):
                    result=native.immutable_upload(None,Path('artifact.zip'),'objects/digest/artifact.zip')
            self.assertEqual(result,'https://cdn.example/objects/digest/artifact.zip')
            self.assertEqual(transport_url(read.call_args.args[0]),'https://bucket.example/objects/digest/artifact.zip')

    def test_download_error_names_the_endpoint_and_404_alone_means_absent(self):
        with patch.dict(os.environ, {'OSS_DOWNLOAD_BASE':''}):
            url='https://vendor.example/tool.zip'
            with patch.object(native.urllib.request, 'urlopen', side_effect=urllib.error.URLError('handshake timed out')):
                with redirect_stdout(io.StringIO()), self.assertRaisesRegex(RuntimeError,'vendor.example/tool.zip.*handshake timed out'):
                    native.get(url)
            with patch.object(native.urllib.request, 'urlopen', side_effect=urllib.error.HTTPError(url,403,'Forbidden',{},None)):
                with redirect_stdout(io.StringIO()), self.assertRaisesRegex(RuntimeError,'HTTP 403'):
                    native.get(url)
            with patch.object(native.urllib.request, 'urlopen', side_effect=urllib.error.HTTPError(url,404,'Missing',{},None)):
                with redirect_stdout(io.StringIO()):
                    self.assertIsNone(native.get(url))

    def test_log_address_omits_credentials_and_query(self):
        self.assertEqual(safe_url('https://user:password@example.test/a?token=secret#secret'),'https://example.test/a')
