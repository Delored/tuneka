"""Fast, display-free tests for core validation and formatting helpers."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from app import (  # noqa: E402
    parse_youtube_urls,
    safe_filename,
    safe_foldername,
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


if __name__ == "__main__":
    unittest.main()
