"""Native host restart/crash and live/offline handover through a resident launcher."""
import unittest

from browser_fixture import BrowserFixture


class BrowserReconnectTests(BrowserFixture):
    def setUp(self):
        super().setUp()
        self.profile([("saved", "https://example.org/offline", "Saved offline", 1, 1000, 0)])
        self.start()

    def live_history(self):
        return [{"id": "same-browser-id", "title": "Live visit",
                 "url": "https://example.org/live", "lastVisitTime": 2, "visitCount": 1}]

    def test_restart_and_crash_return_to_offline_then_new_session(self):
        self.assertIn("offline", self.query()["results"][0]["subtitle"])
        for crash in [False, True]:
            with self.subTest(crash=crash):
                bridge = self.bridge(self.live_history())
                live = self.query()["results"]
                self.assertEqual([r["name"] for r in live], ["Live visit"])
                self.assertNotIn("offline", live[0]["subtitle"])
                self.assertEqual(len(self.query("/tabs")["results"]), 1)
                self.assertEqual(len(self.query("/browser")["results"]), 1)
                old = self.query()["results"][0]
                bridge.close(crash=crash)
                self.assertEqual(bridge.path.exists(), crash)
                restarted = self.bridge(self.live_history())
                self.assertNotEqual(bridge.path.name, restarted.path.name)
                before = len(restarted.requests)
                stale = self.backend.request(op="activate", id=old["id"], mode="close")
                self.assertEqual(stale["kind"], "error")
                self.assertEqual(len(restarted.requests), before)
                new = self.query()["results"][0]
                self.assertNotEqual(old["id"], new["id"])
                self.assertEqual(new["name"], "Live visit")
                restarted.close()
                offline = self.query()["results"]
                self.assertEqual([r["name"] for r in offline], ["Saved offline"])
                self.assertIn("offline", offline[0]["subtitle"])
                self.assertIn("disconnected", self.query("/tabs")["message"])
                self.assertEqual(self.query("no matching application").get("message", ""), "")

    def test_empty_or_erroring_live_session_does_not_expose_offline_history(self):
        bridge = self.bridge([])
        self.assertEqual(self.query()["results"], [])
        self.assertEqual(self.query("/history offline")["results"], [])
        bridge.error = "Reload the updated bridge and allow history access"
        response = self.query()
        self.assertEqual(response["results"], [])
        self.assertIn("allow history access", response["message"])
        bridge.close()
        self.assertEqual(self.query()["results"][0]["name"], "Saved offline")

    def test_live_profiles_merge_by_recency_and_survive_one_profile_closing(self):
        first = self.bridge(self.live_history())
        second = self.bridge([{**self.live_history()[0], "title": "Newer visit", "lastVisitTime": 3}])
        rows = self.query()["results"]
        self.assertEqual([r["name"] for r in rows], ["Newer visit", "Live visit"])
        self.assertNotEqual(rows[0]["id"], rows[1]["id"])
        second.close()
        self.assertEqual([r["name"] for r in self.query()["results"]], ["Live visit"])
        first.close()
        self.assertIn("offline", self.query()["results"][0]["subtitle"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
