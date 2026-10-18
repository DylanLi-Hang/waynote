import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("dev", Path(__file__).with_name("dev.py"))
dev = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dev)


class DevelopmentTest(unittest.TestCase):
    def test_environment_isolates_all_persistent_paths(self):
        root = Path("/tmp/waynote-test")
        with patch.dict(dev.os.environ, XDG_DATA_HOME="/real/data", XDG_CONFIG_HOME="/real/config"):
            env = dev.dev_env(root)
        self.assertEqual(env["WAYNOTE_DEV"], "1")
        for variable, directory in [("XDG_DATA_HOME", "data"), ("XDG_STATE_HOME", "state"),
                                    ("XDG_CONFIG_HOME", "config")]:
            self.assertEqual(env[variable], str(root / directory))

    def test_restart_preserves_edited_notes_and_config(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dev.seed(root)
            notes = root / "data/waynote/notes"
            note = next(notes.glob("*.md"))
            note.write_text("# Keep my development edits\n")
            config = root / "config/waynote/config.toml"
            config.write_text("tray_enabled = true\n")
            dev.seed(root)
            self.assertEqual(len(list(notes.glob("*.md"))), 1)
            self.assertEqual(note.read_text(), "# Keep my development edits\n")
            self.assertEqual(config.read_text(), "tray_enabled = true\n")

    def test_lua_callback_restored_from_original_source_not_numeric_id(self):
        line = 'hl.bind("SUPER + N", hl.dsp.exec_cmd("/nix/store/example/bin/waynote toggle"), { description = "Toggle Waynote notes" })'
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "hyprland.lua"
            config.write_text(line + "\n")
            binding = {"dispatcher": "__lua", "arg": "174", "description": "Toggle Waynote notes"}
            self.assertEqual(dev.original_binding([binding], config), line)
            config.write_text(line + "\n" + line + "\n")
            with self.assertRaises(RuntimeError):
                dev.original_binding([binding], config)

    def test_cleanup_restores_our_binding_but_preserves_user_changes(self):
        state = {"restore": "original()", "hyprland": "test"}
        with patch.dict(dev.os.environ, HYPRLAND_INSTANCE_SIGNATURE="test"), \
                patch.object(dev, "binds", return_value=[{"description": dev.DESCRIPTION}]), \
                patch.object(dev, "lua") as lua:
            dev.restore_binding(state)
            lua.assert_called_once_with('hl.unbind("SUPER + N"); original()')
        with patch.dict(dev.os.environ, HYPRLAND_INSTANCE_SIGNATURE="test"), \
                patch.object(dev, "binds", return_value=[{"description": "User changed it"}]), \
                patch.object(dev, "lua") as lua:
            dev.restore_binding(state)
            lua.assert_not_called()

    def test_partial_binding_failure_can_restore_after_unbind(self):
        state = {"restore": "original()", "hyprland": "test"}
        with patch.dict(dev.os.environ, HYPRLAND_INSTANCE_SIGNATURE="test"), \
                patch.object(dev, "binds", return_value=[]), patch.object(dev, "lua") as lua:
            dev.restore_binding(state)
            lua.assert_called_once()

    def test_orphan_cleanup_requires_both_dev_identity_and_data_path(self):
        binary = dev.os.fsencode(dev.REPO / "target/debug/waynote") + b"\0"
        environment = b"WAYNOTE_DEV=1\0" + dev.os.fsencode("XDG_DATA_HOME=" + str(dev.WORK / "data")) + b"\0"
        with patch.object(Path, "read_bytes", side_effect=[binary, environment]), \
                patch.object(dev.os, "kill") as kill:
            dev.stop_orphan({"child_pid": 999})
            kill.assert_called_once_with(999, dev.signal.SIGTERM)
        with patch.object(Path, "read_bytes", side_effect=[binary, b"WAYNOTE_DEV=1\0"]), \
                patch.object(dev.os, "kill") as kill:
            dev.stop_orphan({"child_pid": 999})
            kill.assert_not_called()

    def test_stale_pid_never_targets_this_process(self):
        self.assertFalse(dev.owned_process({"pid": dev.os.getpid()}))


if __name__ == "__main__":
    unittest.main()
