import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import types
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('media_download', Path(__file__).parents[2] / 'Resources/media_download.py')
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)


class MediaDownloadTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        self.args = types.SimpleNamespace(folder=str(self.folder), url='https://www.youtube.com/watch?v=YE7VzlLtp-4', provider='YouTube',
                                          ytdlp='/yt-dlp', deno='/deno', ffmpeg='/ffmpeg', ffprobe='/ffprobe')

    def test_audio_only_validation(self):
        probe = {'format': {'duration': '10'}, 'streams': [{'codec_type': 'audio'}]}
        for extra, accepted in [(None, True), ({'codec_type': 'video', 'disposition': {'attached_pic': 1}}, True), ({'codec_type': 'video'}, False)]:
            data = {**probe, 'streams': probe['streams'] + ([extra] if extra else [])}
            with patch.object(media, 'run', return_value=json.dumps(data)):
                if accepted:
                    self.assertEqual(media.inspect_audio(self.folder / 'audio', self.folder, '/probe'), 10)
                else:
                    with self.assertRaises(ValueError):
                        media.inspect_audio(self.folder / 'video', self.folder, '/probe')
        for duration in ['nan', 'inf', '0', str(media.MAX_SECONDS + 1)]:
            probe['format']['duration'] = duration
            with patch.object(media, 'run', return_value=json.dumps(probe)), self.assertRaises(ValueError):
                media.inspect_audio(self.folder / 'audio', self.folder, '/probe')

    def test_private_redirect_rejected(self):
        for address in ['127.0.0.1', '192.168.1.1', '::1', '169.254.169.254']:
            with patch.object(socket, 'getaddrinfo', return_value=[(socket.AF_INET, socket.SOCK_STREAM, 6, '', (address, 443))]), self.assertRaises(ValueError):
                media.validate_public_url('https://example.com/track.mp3')
        for url in ['http://example.com/a.mp3', 'https://user:pass@example.com/a.mp3', 'https://example.com:8080/a.mp3']:
            with self.assertRaises(ValueError):
                media.validate_public_url(url)

    def test_checkpoint_is_reused_without_network(self):
        audio = self.folder / 'audio.m4a'; audio.write_bytes(b'fixture')
        result = {'filename': audio.name, 'fileSize': 7, 'source': {'url': self.args.url}}
        media.atomic_json(self.folder / 'result.json', result)
        with patch.object(media, 'run', side_effect=AssertionError('network must not run')):
            self.assertEqual(media.download(self.args), result)
        # A same-size file elsewhere or from another source is not an accepted checkpoint.
        result['filename'] = '../audio.m4a'; media.atomic_json(self.folder / 'result.json', result)
        with self.assertRaises(ValueError):
            media.download(self.args)
        result['filename'] = 'audio.m4a'; result['source']['url'] = 'https://other.example/a.mp3'
        media.atomic_json(self.folder / 'result.json', result)
        with self.assertRaises(ValueError):
            media.download(self.args)

    def test_incomplete_checkpoint_cannot_pass(self):
        (self.folder / 'audio.m4a').write_bytes(b'short')
        media.atomic_json(self.folder / 'result.json', {'filename': 'audio.m4a', 'fileSize': 500, 'source': {'url': self.args.url}})
        with patch.object(media, 'run', side_effect=RuntimeError('offline')), self.assertRaises(RuntimeError):
            media.download(self.args)
        self.assertFalse((self.folder / 'result.json').exists())

    def test_live_and_video_formats_rejected_before_download(self):
        for info in [{'is_live': True}, {'duration': 10, 'vcodec': 'h264', 'acodec': 'aac'}, {'duration': media.MAX_SECONDS + 1}]:
            with patch.object(media, 'run', return_value=json.dumps(info)) as run, self.assertRaises(ValueError):
                media.download(self.args)
            self.assertEqual(run.call_count, 1)
            self.assertFalse((self.folder / 'result.json').exists())

    def test_truncated_youtube_audio_does_not_publish(self):
        info = {'duration': 100, 'vcodec': 'none', 'acodec': 'aac', 'title': 'Test'}
        def run(*args, **kwargs):
            if '--dump-single-json' in args[0]:
                return json.dumps(info)
            (self.folder / 'source.m4a').write_bytes(b'truncated')
            return ''
        with patch.object(media, 'run', side_effect=run), patch.object(media, 'inspect_audio', return_value=50), self.assertRaises(ValueError):
            media.download(self.args)
        self.assertFalse((self.folder / 'result.json').exists())

    def test_timeouts_terminate_child(self):
        with self.assertRaises(TimeoutError):
            media.run([sys.executable, '-c', 'import time; time.sleep(30)'], self.folder, 'test', timeout=0.05)
        self.assertIsNone(media.ACTIVE)

    def test_parent_exit_interrupts_processing(self):
        with patch.object(media.os, 'getppid', return_value=media.PARENT_PID + 1), self.assertRaises(InterruptedError):
            media.download(self.args)

    def test_cancelling_helper_stops_downloader_process(self):
        fake = self.folder / 'fake-downloader'
        pidfile = self.folder / 'child.pid'
        fake.write_text('#!' + sys.executable + '\nimport os, time\nfrom pathlib import Path\nPath(' + repr(str(pidfile)) + ').write_text(str(os.getpid()))\ntime.sleep(30)\n')
        fake.chmod(0o700)
        args = [sys.executable, str(Path(media.__file__))]
        for name in ('url', 'provider', 'folder', 'deno', 'ffmpeg', 'ffprobe'):
            args += ['--' + name, getattr(self.args, name)]
        args += ['--ytdlp', str(fake)]
        child_pid = None
        with subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE) as helper:
            try:
                deadline = time.monotonic() + 5
                while not pidfile.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertTrue(pidfile.exists(), 'fake downloader never started')
                child_pid = int(pidfile.read_text())
                helper.terminate()
                helper.communicate(timeout=5)
                self.assertNotEqual(helper.returncode, 0)
                with self.assertRaises(ProcessLookupError):
                    os.kill(child_pid, 0)
                self.assertFalse((self.folder / 'result.json').exists())
            finally:
                if helper.poll() is None:
                    helper.kill(); helper.communicate()
                if child_pid is not None:
                    try:
                        os.kill(child_pid, 9)
                    except ProcessLookupError:
                        pass

    def test_tools_ignore_external_configuration_and_remote_code(self):
        flags = media.yt_arguments(self.args)
        for flag in ['--ignore-config', '--no-plugin-dirs', '--no-remote-components', '--no-playlist', '--abort-on-unavailable-fragments']:
            self.assertIn(flag, flags)
        self.assertNotIn('--cookies-from-browser', flags)
        self.assertNotIn('--netrc', flags)


if __name__ == '__main__':
    unittest.main()
