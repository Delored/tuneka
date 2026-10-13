"""Fast, display-free tests for core validation and formatting helpers."""
import sys
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from app import (  # noqa: E402
    AUDIO_FORMATS,
    TunekaApp,
    info_for,
    parse_youtube_urls,
    safe_filename,
    safe_foldername,
    split_video,
    timestamp,
    valid_youtube_url,
    youtube_video_url_from_entry,
)
from i18n import translate_error  # noqa: E402


class YouTubeUrlTests(unittest.TestCase):
    def test_accepts_supported_https_hosts(self):
        for url in (
            "https://youtube.com/watch?v=abcdefghijk",
            "https://www.youtube.com/watch?v=abcdefghijk",
            "https://music.youtube.com/watch?v=abcdefghijk",
            "https://youtu.be/abcdefghijk",
        ):
            with self.subTest(url=url):
                self.assertTrue(valid_youtube_url(url))

    def test_rejects_unsafe_or_unrelated_urls(self):
        for url in (
            "http://youtube.com/watch?v=abcdefghijk",
            "https://youtube.com.example.org/watch?v=abcdefghijk",
            "https://example.org/?next=https://youtube.com",
            "https://user:password@youtube.com/watch?v=abcdefghijk",
            "https://youtube.com:444/watch?v=abcdefghijk",
        ):
            with self.subTest(url=url):
                self.assertFalse(valid_youtube_url(url))

    def test_playlist_entry_is_canonicalized_from_video_id(self):
        self.assertEqual(
            youtube_video_url_from_entry({"id": "abcdefghijk", "url": "https://evil.example/video"}),
            "https://www.youtube.com/watch?v=abcdefghijk",
        )

    def test_playlist_entry_rejects_untrusted_url(self):
        self.assertIsNone(youtube_video_url_from_entry({"url": "https://evil.example/watch?v=abcdefghijk"}))

    def test_playlist_entry_accepts_youtube_short_url(self):
        self.assertEqual(
            youtube_video_url_from_entry({"webpage_url": "https://youtu.be/abcdefghijk"}),
            "https://www.youtube.com/watch?v=abcdefghijk",
        )

    def test_multiple_links_are_deduplicated_and_invalid_links_reported(self):
        urls, invalid = parse_youtube_urls(
            "https://youtu.be/abcdefghijk, https://youtu.be/abcdefghijk; https://evil.example/video"
        )
        self.assertEqual(urls, ["https://youtu.be/abcdefghijk"])
        self.assertEqual(invalid, ["https://evil.example/video"])


class FilenameAndTimeTests(unittest.TestCase):
    def test_filename_removes_path_and_reserved_characters(self):
        self.assertEqual(safe_filename('A/B:C? "Track"'), "ABC Track")
        self.assertEqual(safe_filename("<>"), "track")

    def test_foldername_replaces_forbidden_characters_with_spaces(self):
        self.assertEqual(safe_foldername("A/B:C?"), "A B C")
        self.assertEqual(safe_foldername("CON"), "_CON")

    def test_timestamp_formats_hours_minutes_seconds_and_milliseconds(self):
        self.assertEqual(timestamp(3661.234), "01:01:01.234")


class ErrorLocalizationTests(unittest.TestCase):
    def test_translates_application_error_for_english_ui(self):
        self.assertEqual(
            translate_error("Завантаження перенаправлено на недозволене джерело.", "en"),
            "The download was redirected to an untrusted source.",
        )

    def test_keeps_external_diagnostics_while_translating_known_lines(self):
        message = "Не вдалося завантажити відео.\n\nERROR: extractor diagnostic"
        self.assertEqual(
            translate_error(message, "en"),
            "Could not download the video.\n\nERROR: extractor diagnostic",
        )

    def test_ukrainian_ui_keeps_original_error_text(self):
        message = "Завантаження перенаправлено на недозволене джерело."
        self.assertEqual(translate_error(message, "uk"), message)


class AudioExportTests(unittest.TestCase):
    def test_video_without_chapters_is_reported_as_one_track(self):
        payload = json.dumps({"title": "Single mix", "duration": 90, "chapters": None})
        with patch("app.run", return_value=subprocess.CompletedProcess([], 0, payload, "")):
            video = info_for("https://youtube.com/watch?v=abcdefghijk")
        self.assertFalse(video["has_chapters"])
        self.assertEqual(len(video["chapters"]), 1)
        self.assertEqual(video["chapters"][0]["title"], "Single mix")

    def _export(self, audio_format, has_chapters):
        with tempfile.TemporaryDirectory() as folder:
            parent = Path(folder)
            ffmpeg_commands = []

            class FakeYtDlpProcess:
                def __init__(self, command, **_kwargs):
                    output_template = Path(command[command.index("-o") + 1])
                    self.source = Path(str(output_template).replace("%(ext)s", "webm"))
                    self.source.write_bytes(b"audio source")
                    self.stdout = iter([str(self.source)])
                    self.returncode = 0

                def wait(self):
                    return self.returncode

                def poll(self):
                    return self.returncode

            def fake_ffmpeg(command, timeout):
                ffmpeg_commands.append(command)
                Path(command[-1]).write_bytes(b"encoded audio")
                return subprocess.CompletedProcess(command, 0, "", "")

            video = {
                "title": "Mix: Test",
                "thumbnail": None,
                "has_chapters": has_chapters,
                "chapters": [{"index": 0, "title": "Track: One", "start": 0, "end": 60}],
            }
            with patch("app.subprocess.Popen", FakeYtDlpProcess), patch("app.run", side_effect=fake_ffmpeg):
                jobdir, tracks, cover_warning = split_video(
                    "https://youtube.com/watch?v=abcdefghijk", video, [0], parent,
                    lambda *_args: None, audio_format=audio_format,
                )

            track_exists = [track.is_file() for track in tracks]
            return parent, jobdir, tracks, cover_warning, ffmpeg_commands, track_exists

    def test_exports_supported_music_formats_with_the_matching_extension_and_codec(self):
        for audio_format, options in AUDIO_FORMATS.items():
            with self.subTest(audio_format=audio_format):
                parent, jobdir, tracks, _cover_warning, commands, track_exists = self._export(audio_format, True)
                self.assertEqual(jobdir, parent / "Mix Test")
                self.assertEqual(len(tracks), 1)
                self.assertEqual(tracks[0].suffix, f".{options['extension']}")
                self.assertTrue(track_exists[0])
                command = commands[0]
                for option in options["codec"]:
                    self.assertIn(option, command)

    def test_chapterless_audio_goes_in_the_selected_folder(self):
        parent, jobdir, tracks, _cover_warning, _commands, _exists = self._export("mp3", False)
        self.assertEqual(jobdir, parent)
        self.assertEqual(tracks[0].parent, parent)


class PreferencePersistenceTests(unittest.TestCase):
    def test_selected_video_and_music_defaults_roundtrip_through_settings(self):
        with tempfile.TemporaryDirectory() as folder, patch("app.SETTINGS_PATH", Path(folder) / "settings.json"):
            app = object.__new__(TunekaApp)
            app.download_parent = Path(folder) / "downloads"
            app.theme_name = "light"
            app.default_video_format = "webm"
            app.default_video_resolution = "1440p"
            app.default_playlist_output = "music"
            app.default_playlist_music_format = "flac"
            app.language = "en"

            app._save_settings()
            preferences = TunekaApp._load_preferences()

        self.assertEqual(preferences["default_video_format"], "webm")
        self.assertEqual(preferences["default_video_resolution"], "1440p")
        self.assertEqual(preferences["default_playlist_output"], "music")
        self.assertEqual(preferences["default_playlist_music_format"], "flac")
        self.assertEqual(preferences["language"], "en")

    def test_legacy_playlist_mp3_setting_migrates_to_music_mode(self):
        with tempfile.TemporaryDirectory() as folder, patch("app.SETTINGS_PATH", Path(folder) / "settings.json"):
            Path(folder, "settings.json").write_text(
                json.dumps({"default_playlist_output": "mp3"}), encoding="utf-8",
            )
            preferences = TunekaApp._load_preferences()
        self.assertEqual(preferences["default_playlist_output"], "music")


class PlaylistDownloadTests(unittest.TestCase):
    class FakeController:
        def check_cancelled(self):
            return None

    def setUp(self):
        self.app = object.__new__(TunekaApp)
        self.app._thread_progress = lambda *_args: None
        self.playlist = {"title": "Playlist: Test"}
        self.entries = [{"url": "https://youtube.com/watch?v=abcdefghijk", "title": "Video One"}]

    def test_music_files_are_written_to_a_playlist_folder_in_selected_format(self):
        with tempfile.TemporaryDirectory() as folder:
            parent = Path(folder)

            def fake_split(_url, _video, _selected, output_dir, _progress, **kwargs):
                target = output_dir / f"{kwargs['filename_prefix']}Video One.{kwargs['audio_format']}"
                target.write_bytes(b"music")
                return output_dir, [target], False

            with patch("app.info_for", return_value={"title": "Video One", "duration": 50}), \
                    patch("app.split_video", side_effect=fake_split) as split:
                result = self.app._playlist_download_worker(
                    self.playlist, self.entries, "music", "flac", "best", "mp4", parent,
                    self.FakeController(),
                )

        self.assertEqual(result["output_dir"], parent / "Playlist Test")
        self.assertEqual(result["files"][0].parent, result["output_dir"])
        self.assertEqual(result["files"][0].suffix, ".flac")
        self.assertEqual(split.call_args.kwargs["audio_format"], "flac")

    def test_video_files_are_written_to_a_playlist_folder_with_selected_options(self):
        with tempfile.TemporaryDirectory() as folder:
            parent = Path(folder)

            def fake_download(_url, _title, output_dir, _progress, **kwargs):
                target = output_dir / f"{kwargs['prefix']}Video One.{kwargs['container']}"
                target.write_bytes(b"video")
                return target

            with patch("app.download_video_file", side_effect=fake_download) as download:
                result = self.app._playlist_download_worker(
                    self.playlist, self.entries, "video", "mp3", "720p", "webm", parent,
                    self.FakeController(),
                )

        self.assertEqual(result["output_dir"], parent / "Playlist Test")
        self.assertEqual(result["files"][0].parent, result["output_dir"])
        self.assertEqual(result["files"][0].suffix, ".webm")
        self.assertEqual(download.call_args.kwargs["resolution"], "720p")
        self.assertEqual(download.call_args.kwargs["container"], "webm")


if __name__ == "__main__":
    unittest.main()
