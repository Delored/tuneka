"""Native UI regression checks. Run with a real display or Xvfb; no network access."""
import sys
import time
import tempfile
import tkinter as tk
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app import TrackSplitApp, THEMES, draw_round_rect
from ui import _corner_tiles


class DesktopLayoutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        try:
            cls.root = tk.Tk()
        except tk.TclError as exc:
            raise unittest.SkipTest(f"Tk display required: {exc}")
        cls.errors = []
        cls.root.report_callback_exception = lambda *args: cls.errors.append(args)
        cls.root.geometry('1220x840+0+0')
        with patch.object(TrackSplitApp, '_bootstrap_tools', lambda self: None), \
             patch.object(TrackSplitApp, '_save_settings', lambda self: None):
            cls.app = TrackSplitApp(cls.root)
            cls.pump()
        cls.app._save_settings = lambda: None

    @classmethod
    def tearDownClass(cls):
        cls.root.destroy()

    @classmethod
    def pump(cls):
        for _ in range(5):
            cls.root.update()
            time.sleep(.03)

    def tearDown(self):
        self.assertEqual(self.errors, [])

    def assert_inside(self, widget, ancestor):
        self.assertTrue(widget.winfo_ismapped(), str(widget))
        x = widget.winfo_rootx()-ancestor.winfo_rootx()
        y = widget.winfo_rooty()-ancestor.winfo_rooty()
        self.assertGreaterEqual(x, 0)
        self.assertGreaterEqual(y, 0)
        self.assertLessEqual(x+widget.winfo_width(), ancestor.winfo_width())
        self.assertLessEqual(y+widget.winfo_height(), ancestor.winfo_height())

    def test_pages_and_download_dock_fit(self):
        a = self.app
        for size in ('980x700', '1220x840'):
            self.root.geometry(size)
            for page in ('music', 'playlist', 'video', 'options'):
                with self.subTest(size=size, page=page):
                    a._show_page(page)
                    if page == 'playlist':
                        a._set_playlist_output('video', persist=False)
                    self.pump()
                    for widget in (a.pause_download_button, a.cancel_download_button,
                                   a.open_folder_button, a.choose_folder_button):
                        self.assert_inside(widget, self.root)
                    if page == 'playlist':
                        for widget in (a.playlist_resolution, a.playlist_format, a.playlist_select_button):
                            self.assert_inside(widget, a.page_host)
                        self.assertGreaterEqual(a.playlist_tree.winfo_height(), 95)
                    if page == 'options':
                        for widget in (a.options_theme, a.options_playlist_output, a.options_format):
                            self.assert_inside(widget, a.page_host)
                    if page == 'music':
                        self.assertGreater(a.tree.winfo_height(), 100)

    def test_selection_marks_and_keyboard(self):
        a = self.app
        a._show_page('music')
        a.url.set('https://www.youtube.com/watch?v=fixture')
        a._show_video({'title': 'Дуже довга назва ' * 20, 'chapters': [
            {'index': i, 'title': f'Трек {i+1}', 'start': i*120, 'end': (i+1)*120}
            for i in range(5)]}, a.url.get())
        self.pump()
        self.assert_inside(a.select_button, a.page_host)
        self.assertFalse(a.tree.empty.winfo_ismapped())
        self.assertTrue(a.tree.item('0', 'text').startswith('✓'))
        a.tree.focus('0')
        a.tree._space()
        self.pump()
        self.assertNotIn('0', a.tree.selection())
        self.assertTrue(a.tree.item('0', 'text').startswith('○'))
        a.tree._select_all()
        self.pump()
        self.assertEqual(len(a.tree.selection()), 5)
        a.url.set('')
        self.pump()
        self.assertTrue(a.tree.empty.winfo_ismapped())
        self.assertEqual(a.url.get(), '')

    def test_light_theme_is_persisted(self):
        with tempfile.TemporaryDirectory() as folder, patch('app.SETTINGS_PATH', Path(folder) / 'settings.json'):
            self.app.theme_selector._choose('Світла')
            self.pump()
            TrackSplitApp._save_settings(self.app)
            self.assertEqual(TrackSplitApp._load_theme(), 'light')

    def test_styled_theme_popup_and_matched_source_actions(self):
        a = self.app
        self.assertEqual(a.playlist_find_button.button_height, a.find_button.button_height)
        self.assertEqual(a.playlist_find_button.button_height, a.video_check_button.button_height)
        self.assertEqual(a.playlist_find_button.button_height, a.video_check_button.button_height)
        self.assertEqual(a.playlist_find_button.button_height, 40)
        self.assertEqual(a.playlist_url_entry.winfo_reqheight(), 46)
        a.theme_selector._open()
        self.pump()
        popup = a.theme_selector.popup
        self.assertIsNotNone(popup)
        self.assertEqual(popup.overrideredirect(), 1)
        self.assertEqual(len(a.theme_selector.values), 3)
        popup_canvas = a.theme_selector._popup_canvas
        light_index = a.theme_selector.values.index('Світла')
        popup_canvas.event_generate('<Motion>', x=50, y=7 + light_index*36 + 18)
        self.pump()
        self.assertEqual(a.theme_selector._popup_index, light_index)
        popup_canvas.event_generate('<Button-1>', x=50, y=7 + light_index*36 + 18)
        self.pump()
        self.assertEqual(a.theme_name, 'light')
        self.assertFalse(popup.winfo_exists())

    def test_rounded_edges_have_antialias_pixels(self):
        canvas = tk.Canvas(self.root, width=40, height=40, bg='#ffffff', highlightthickness=0)
        canvas.pack()
        draw_round_rect(canvas, 40, 40, 16, '#b080e8')
        self.pump()
        tile = _corner_tiles[(str(canvas.tk), 16, '#b080e8', '#b080e8', '#ffffff', 'nw')]
        pixels = [tile.get(x, y) for x in range(tile.width()) for y in range(tile.height())]
        self.assertIn((255, 255, 255), pixels)
        self.assertIn((176, 128, 232), pixels)
        self.assertTrue(any(pixel not in ((255, 255, 255), (176, 128, 232)) for pixel in pixels))
        canvas.destroy()

    def test_theme_and_choice_roundtrip(self):
        a = self.app
        for theme in ('purple', 'light', 'olive', 'light', 'purple'):
            a._apply_theme(theme, save=False)
            self.pump()
            self.assertEqual(a.playlist_format.theme, THEMES[theme])
            self.assertEqual(a.tree.empty.cget('bg'), THEMES[theme]['card'])
            self.assertEqual(a.url_entry.placeholder.cget('bg'), THEMES[theme]['field'])
        for selector in (a.theme_selector, a.options_theme):
            for name in ('light', 'olive', 'purple'):
                selector._choose(THEMES[name]['label'])
                self.pump()
                self.assertEqual(a.theme_name, name)
                self.assertEqual(a.theme_selector.get(), THEMES[name]['label'])
                self.assertEqual(a.options_theme.get(), THEMES[name]['label'])
                self.assertEqual(a.theme_selector.cget('bg'), THEMES[name]['bg'])
        a.playlist_resolution.set('1080p')
        a.playlist_resolution._step(1)
        self.assertEqual(a.playlist_resolution.get(), '720p')
        a.playlist_format._choose('WebM')
        self.assertEqual(a.playlist_format.get(), 'WebM')


if __name__ == '__main__':
    unittest.main()
