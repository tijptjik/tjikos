import contextlib
import io
import os
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from tjikup.progress import Progress, changed_spans, status_cells
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

    def test_redraw_keeps_cursor_hidden_until_last_task_finishes(self):
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        progress.start('a', 'Python')
        progress.start('b', 'Icons')
        progress.finish('a', 'Python', record=False)
        self.assertEqual(output.getvalue().count('\x1b[?25l'), 1)
        self.assertNotIn('\x1b[?25h', output.getvalue())
        progress.finish('b', 'Icons', record=False)
        self.assertEqual(output.getvalue().count('\x1b[?25h'), 1)
        self.assertTrue(output.getvalue().endswith('\x1b[?25h\x1b[?2026l'))
        # Every clear and replacement row belongs to a complete frame.
        frames = output.getvalue().split('\x1b[?2026h')[1:]
        self.assertEqual(len(frames), 4)
        self.assertTrue(all(frame.endswith('\x1b[?2026l') for frame in frames))

    def test_prompt_restores_cursor_and_releases_frame_before_waiting(self):
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        progress.start('a', 'Python')
        with self.assertRaises(RuntimeError):
            with progress.output():
                self.assertFalse(progress.cursor_hidden)
                self.assertTrue(output.getvalue().endswith('\x1b[?25h\x1b[?2026l'))
                output.write('Prompt\n')
                raise RuntimeError('prompt failed')
        self.assertTrue(progress.cursor_hidden)
        self.assertEqual(progress.height, 1)
        progress.discard('a')
        self.assertFalse(progress.cursor_hidden)

    def test_interrupt_and_render_failure_restore_cursor_and_end_frame(self):
        for error in (KeyboardInterrupt, RuntimeError):
            with self.subTest(error=error):
                output = io.StringIO()
                with self.assertRaises(error), Progress(stream=output, live=True) as progress:
                    progress.start('a', 'Python')
                    raise error()
                self.assertFalse(progress.cursor_hidden)
                self.assertTrue(output.getvalue().endswith('\x1b[?25h\x1b[?2026l'))
                self.assertEqual(output.getvalue().count('\x1b[?2026h'),
                                 output.getvalue().count('\x1b[?2026l'))
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        with patch.object(progress, '_print', side_effect=ValueError('render failed')):
            with self.assertRaises(ValueError):
                progress.start('a', 'Python')
        self.assertFalse(progress.cursor_hidden)
        self.assertTrue(output.getvalue().endswith('\x1b[?25h\x1b[?2026l'))

    def test_idle_tick_does_not_write_terminal_controls(self):
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        with patch.object(progress.stop, 'wait', side_effect=[False, True]):
            progress._tick()
        self.assertEqual(output.getvalue(), '')

    def test_tick_redraw_does_not_show_cursor_between_frames(self):
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        with patch('tjikup.progress.time.monotonic', return_value=0):
            progress.start('a', 'Python')
        output.seek(0)
        output.truncate()
        with patch.object(progress.stop, 'wait', side_effect=[False, False, True]), \
                patch('tjikup.progress.time.monotonic', side_effect=[.125, .25]):
            progress._tick()
        self.assertEqual(output.getvalue().count('\x1b[?2026h'), 2)
        self.assertEqual(output.getvalue().count('\x1b[?2026l'), 2)
        self.assertNotIn('\x1b[?25', output.getvalue())
        progress.discard('a')

    def test_tick_patches_spinner_and_timer_without_repainting_labels(self):
        output = io.StringIO()
        with patch.dict(os.environ, {'TJIKUP_COLOR': '1', 'TJIKUP_COLUMNS': '80'}):
            progress = Progress(stream=output, live=True)
            with patch('tjikup.progress.time.monotonic', return_value=0):
                progress.start('a', 'Python')
                progress.start('b', 'Icons')
            output.seek(0)
            output.truncate()
            with patch('tjikup.progress.time.monotonic', return_value=.125):
                progress._refresh()
            frame = output.getvalue()
            self.assertIn('\x1b[2A', frame)
            self.assertIn('\x1b[1A', frame)
            self.assertIn('\x1b[9G', frame)
            self.assertIn('⠙', frame)
            self.assertIn('\x1b7', frame)
            self.assertIn('\x1b8', frame)
            for repaint in ('Python', 'Icons', 'UPDATE', '\x1b[2K', '\n', '\r', '\x1b[?25'):
                self.assertNotIn(repaint, frame)
            self.assertEqual(progress.height, 2)

    def test_unchanged_tick_emits_nothing(self):
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        with patch('tjikup.progress.time.monotonic', return_value=0):
            progress.start('a', 'Python')
            output.seek(0)
            output.truncate()
            progress._refresh()
        self.assertEqual(output.getvalue(), '')

    def test_resize_redraws_once_then_returns_to_cell_patches(self):
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        with patch.dict(os.environ, {'TJIKUP_COLUMNS': '80'}):
            progress.start('a', 'Python (environment)')
        output.seek(0)
        output.truncate()
        with patch.dict(os.environ, {'TJIKUP_COLUMNS': '40'}):
            progress._refresh()
            self.assertIn('\x1b[2K', output.getvalue())
            self.assertEqual(progress.columns, 40)
            output.seek(0)
            output.truncate()
            progress._refresh()
            self.assertNotIn('\x1b[2K', output.getvalue())
            self.assertNotIn('Python', output.getvalue())

    def test_timer_growth_preserves_wide_text_and_grouped_cells(self):
        for width in ('24', '40', '80'):
            for group in (1, 2):
                with self.subTest(width=width, group=group), patch.dict(os.environ, {
                        'TJIKUP_COLUMNS': width, 'TJIKUP_COLOR': '1'}):
                    before = runtime.format_status('UPDATE', '⠋', '中é文 (environment)', '9.9s')
                    after = runtime.format_status('UPDATE', '⠙', '中é文 (environment)', '10.0s')
                    screen = status_cells(before)
                    expected = status_cells(after)
                    size = ((max(len(screen), len(expected)) + group - 1) // group) * group
                    screen.extend([(' ', '')] * (size - len(screen)))
                    expected.extend([(' ', '')] * (size - len(expected)))
                    for column, cells in changed_spans(before, after, group=group):
                        self.assertEqual(column % group, 0)
                        self.assertTrue(cells[0][0])
                        screen[column:column + len(cells)] = cells
                    self.assertEqual(screen, expected)

    def test_shrink_clears_reflowed_rows_before_native_output(self):
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        with patch.dict(os.environ, {'TJIKUP_COLUMNS': '120', 'TJIKUP_COLOR': '1'}):
            progress.start('a', 'Chezmoi (files and task preparation)', 'APPLY')
            progress.start('b', '中文配置')
        output.seek(0)
        output.truncate()
        with patch.dict(os.environ, {'TJIKUP_COLUMNS': '40'}):
            with progress.output():
                # Each old status row now occupies three physical lines.
                cleared = output.getvalue()
                self.assertIn('\r\x1b[6A', cleared)
                self.assertEqual(cleared.count('\x1b[2K'), 6)
                self.assertEqual(progress.height, 0)
                self.assertFalse(progress.rendered_widths)
                output.write('Native output\n')
            self.assertEqual(progress.height, 2)
            self.assertEqual(progress.columns, 40)
            output.seek(0)
            output.truncate()
            progress._clear()
        self.assertIn('\r\x1b[2A', output.getvalue())
        self.assertEqual(output.getvalue().count('\x1b[2K'), 2)

    def test_terminal_specific_text_sizing_can_keep_its_own_height(self):
        output = io.StringIO()
        progress = Progress(stream=output, live=True)
        progress.reflow_enabled = False
        with patch.dict(os.environ, {'TJIKUP_COLUMNS': '79', 'TJIKUP_COLOR': '1'}):
            progress.start('a', 'Chezmoi', 'APPLY')
        # The icon renderer reserves two physical lines per status row.
        progress.height = 2
        output.seek(0)
        output.truncate()
        with patch.dict(os.environ, {'TJIKUP_COLUMNS': '39'}):
            progress._clear()
        self.assertIn('\r\x1b[2A', output.getvalue())
        self.assertEqual(output.getvalue().count('\x1b[2K'), 2)

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
