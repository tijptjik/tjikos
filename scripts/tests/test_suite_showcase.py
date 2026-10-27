"""Capture preservation and selective time-edit contracts, without a desktop."""
import contextlib
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import suite_showcase as film

class FilmTests(unittest.TestCase):
    def test_selective_speed_preserves_normal_timing_outside_work(self):
        speed=[{'start':10,'end':22,'rate':3}]
        self.assertEqual(film.film_time(8,speed),8)
        self.assertEqual(film.film_time(10,speed),10)
        self.assertEqual(film.film_time(16,speed),12)
        self.assertEqual(film.film_time(22,speed),14)
        self.assertEqual(film.film_time(27,speed),19)

    def test_all_plans_work_without_starting_a_capture(self):
        for tool in film.STORIES:
            with (self.subTest(tool=tool),patch.object(sys,'argv',['film',tool,'--plan']),
                  patch.object(film,'record') as record,contextlib.redirect_stdout(io.StringIO())):
                film.main()
                record.assert_not_called()
        self.assertEqual(film.STORIES['greetjik'][1],'Start every session with a quip')

    def test_existing_master_receipt_and_review_are_never_replaced(self):
        with tempfile.TemporaryDirectory() as directory:
            for suffix in ['.mp4','.json','.review']:
                output=Path(directory)/(suffix[1:]+'.mp4')
                existing=output.with_suffix(suffix)
                if suffix=='.review':existing.mkdir();existing=existing/'keep'
                existing.write_text('original')
                with (self.subTest(suffix=suffix),patch.object(sys,'argv',['film','greetjik',str(output)]),
                      patch.object(film,'record') as record,contextlib.redirect_stderr(io.StringIO()),self.assertRaises(SystemExit)):
                    film.main()
                record.assert_not_called()
                self.assertEqual(existing.read_text(),'original')

if __name__=='__main__':unittest.main()
