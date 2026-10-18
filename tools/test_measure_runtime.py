"""Exercise probe exit handling without launching an app or measuring hardware."""
import contextlib
import io
import json
import pathlib
import runpy
import subprocess
import unittest
from unittest.mock import patch


class ProbeExitTests(unittest.TestCase):
    def test_failed_primary_exit_reports_diagnostics_and_fails(self):
        class Primary:
            pid = 123

            def poll(self):
                return 7

            def wait(self, **kwargs):
                return 7

        def read_text(path, *args, **kwargs):
            if str(path).endswith('/stat'):
                return '123 (scanner) ' + ' '.join(['0'] * 30)
            return 'Rss: 10 kB\nPss: 5 kB\n'

        output = io.StringIO()
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch('sys.argv', ['measure-runtime.py', '/tmp/fake-scanner']))
            stack.enter_context(patch.dict('os.environ', {'WAYLAND_DISPLAY': '/tmp/fake-wayland'}))
            stack.enter_context(patch('subprocess.Popen', return_value=Primary()))
            stack.enter_context(patch('subprocess.run'))
            stack.enter_context(patch('time.sleep'))
            stack.enter_context(patch.object(pathlib.Path, 'exists', return_value=True))
            stack.enter_context(patch.object(pathlib.Path, 'read_text', read_text))
            stack.enter_context(patch.object(pathlib.Path, 'iterdir', return_value=iter([])))
            stack.enter_context(contextlib.redirect_stdout(output))
            with self.assertRaises(subprocess.CalledProcessError) as error:
                runpy.run_path(str(pathlib.Path(__file__).with_name('measure-runtime.py')), run_name='__main__')
        self.assertEqual(error.exception.returncode, 7)
        self.assertEqual(json.loads(output.getvalue())['exit'], 7)


if __name__ == '__main__':
    unittest.main()
