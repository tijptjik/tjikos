import contextlib
import io
import os
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from tjikup.progress import Progress
from tjikup import runtime


class ProgressTests(unittest.TestCase):
    def test_redirected_output_only_has_final_rows(self):
        output = io.StringIO()
        with patch.dict(os.environ, {'TJIKUP_COLOR': '0'}), Progress(stream=output, live=False) as progress:
            progress.start('python', 'Python', 'SYNC')
            progress.start('icons', 'Icons')
            self.assertEqual(output.getvalue(), '')
            progress.finish('icons', 'Icons', note='no changes')
            progress.finish('python', 'Python', verb='SYNC', note='86 pkgs')
        self.assertEqual(len(output.getvalue().splitlines()), 2)
        self.assertNotIn('\x1b', output.getvalue())
        self.assertNotIn('interrupted', output.getvalue())

    def test_concurrent_rows_redraw_and_completion_is_recorded_once(self):
        output = io.StringIO()
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'report'
            with patch.dict(os.environ, {'TJIKUP_COLOR': '0', 'TJIKUP_REPORT_FILE': str(report)}), \
                    Progress(stream=output, live=True) as progress:
                progress.start('a', 'Python', 'SYNC')
                progress.start('b', 'Icons')
                self.assertEqual(progress.height, 2)
                self.assertIn('\x1b[1A', output.getvalue())
                progress.finish('b', 'Icons', note='no changes')
                self.assertEqual(progress.height, 1)
                self.assertEqual(list(progress.rows), ['a'])
                progress.finish('a', 'Python', verb='SYNC', note='86 pkgs')
            self.assertEqual(len(report.read_text().splitlines()), 2)
            self.assertEqual(progress.height, 0)

    def test_exception_clears_transient_rows(self):
        output = io.StringIO()
        with self.assertRaises(RuntimeError), Progress(stream=output, live=True) as progress:
            progress.start('task', 'Example')
            raise RuntimeError('cancelled')
        self.assertEqual(progress.height, 0)
        self.assertFalse(progress.thread.is_alive())
        self.assertIn('interrupted', output.getvalue())

    def test_parallel_completions_do_not_interleave_rows(self):
        output = io.StringIO()
        with patch.dict(os.environ, {'TJIKUP_COLOR': '0'}), Progress(stream=output, live=False) as progress:
            workers = [threading.Thread(target=progress.finish, args=(str(i), f'Task {i}')) for i in range(20)]
            for worker in workers:
                worker.start()
            for worker in workers:
                worker.join()
        self.assertEqual(len(output.getvalue().splitlines()), 20)

    def test_nested_stream_preserves_progress_capability(self):
        output = io.StringIO()
        with patch.dict(os.environ, {'TJIKUP_CHILD': '1', 'TJIKUP_PROGRESS': '1'}), contextlib.redirect_stdout(output):
            runtime.run_stream([sys.executable, '-c', "import os; print(os.environ['TJIKUP_PROGRESS'])"], Path.cwd())
        self.assertEqual(output.getvalue(), '1\n')

    def test_top_level_pipe_does_not_enable_cursor_controls(self):
        output = io.StringIO()
        with patch.dict(os.environ, {'TJIKUP_CHILD': '0', 'TJIKUP_PROGRESS': '1'}), contextlib.redirect_stdout(output):
            runtime.run_stream([sys.executable, '-c', "import os; print(os.environ['TJIKUP_PROGRESS'])"], Path.cwd())
        self.assertEqual(output.getvalue(), '0\n')


if __name__ == '__main__':
    unittest.main()
