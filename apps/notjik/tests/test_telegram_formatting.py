import sys
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import telegram_bridge as telegram


def entity(kind, offset, length, **extra):
    return {"offset": offset, "length": length, "type": {"@type": "textEntityType" + kind, **extra}}


class TelegramFormattingTests(unittest.TestCase):
    def test_styles_use_telegram_entities_not_literal_markup(self):
        for kind, opening, closing in (
            ("Bold", "<b>", "</b>"), ("Italic", "<i>", "</i>"),
            ("Underline", "<u>", "</u>"), ("Strikethrough", "<s>", "</s>"),
            ("Code", '<font face="monospace">', "</font>"),
            ("Pre", '<font face="monospace">', "</font>"),
            ("PreCode", '<font face="monospace">', "</font>"),
            ("TextUrl", "<u>", "</u>"), ("Url", "<u>", "</u>"),
        ):
            with self.subTest(kind=kind):
                formatted = {"text": "styled <img> & **literal**", "entities": [entity(kind, 0, 6, url='file:///private')]}
                self.assertEqual(telegram.styled_text(formatted), opening + "styled" + closing + " &lt;img&gt; &amp; **literal**")

    def test_overlapping_styles_and_astral_unicode(self):
        formatted = {"text": "🙂 abcdef", "entities": [entity("Bold", 3, 4), entity("Strikethrough", 5, 4)]}
        self.assertEqual(telegram.styled_text(formatted), "🙂 <b>ab</b><b><s>cd</s></b><s>ef</s>")
        # A range splitting the emoji's surrogate pair or extending past the text
        # must never corrupt the message or style an unrelated substring.
        formatted["entities"] = [entity("Bold", 1, 1), entity("Italic", 3, 999), entity("Code", -1, 1)]
        self.assertEqual(telegram.styled_text(formatted), "🙂 abcdef")

    def test_code_spacing_and_line_breaks(self):
        formatted = {"text": "a  b\n    c", "entities": [entity("PreCode", 0, 10)]}
        self.assertEqual(telegram.styled_text(formatted), '<font face="monospace">a&nbsp; b<br>&nbsp;&nbsp;&nbsp; c</font>')

    def test_formatting_survives_custom_emoji_splits(self):
        formatted = {"text": "old 🙂 price", "entities": [entity("Strikethrough", 0, 12), entity("CustomEmoji", 4, 2, custom_emoji_id="12")]}
        parts = telegram.emoji_parts(formatted)
        self.assertEqual("".join(part["text"] for part in parts), formatted["text"])
        self.assertEqual(parts[0]["styledText"], "<s>old</s>")
        self.assertEqual(parts[-1]["styledText"], "<s>price</s>")
        emoji = next(part for part in parts if part["kind"] == "emoji")
        self.assertEqual(emoji["styledText"], "<s>🙂</s>")
        self.assertEqual(emoji["emojiId"], "12")
        self.assertNotIn("styledText", parts[1])  # Preserve standalone space width.

    def test_body_and_caption_styles_and_privacy(self):
        formatted = {"text": "old price", "entities": [entity("Strikethrough", 0, 3)]}
        for kind, key in (("messageText", "text"), ("messagePhoto", "caption"), ("messageVoiceNote", "caption")):
            with self.subTest(kind=kind):
                message = {"content": {"@type": kind, key: formatted}}
                body, data, _ = telegram.describe(message)
                self.assertEqual(body, "old price")
                self.assertEqual(data["styledText"], "<s>old</s> price")
                self.assertNotIn("styledText", telegram.describe(message, False)[1])
                self.assertNotIn("styledText", telegram.describe({**message, "can_be_saved": False})[1])
                secret = {"text": "hidden 🙂", "entities": [entity("Spoiler", 0, 6), entity("CustomEmoji", 7, 2, custom_emoji_id="12")]}
                body, data, file = telegram.describe({"content": {"@type": kind, key: secret}})
                self.assertEqual(body, "Message with spoiler · open in Telegram")
                self.assertNotIn("styledText", data)
                self.assertNotIn("parts", data)
                self.assertIsNone(file)

    def test_unformatted_message_keeps_plain_text_fallback(self):
        body, data, _ = telegram.describe({"content": {"@type": "messageText", "text": {"text": "<b>**literal**</b>"}}})
        self.assertEqual(body, "<b>**literal**</b>")
        self.assertNotIn("styledText", data)


if __name__ == "__main__":
    unittest.main()
