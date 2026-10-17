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
            config.parent.mkdir(parents=True)
            config.write_text('default_color = "blue"\n')
            dev.seed(root)
            self.assertEqual(len(list(notes.glob("*.md"))), 1)
            self.assertEqual(note.read_text(), "# Keep my development edits\n")
            self.assertEqual(config.read_text(), 'default_color = "blue"\n')

    def test_lua_callback_restored_from_original_source_not_numeric_id(self):
        line = 'hl.bind("SUPER + N", hl.dsp.exec_cmd("/nix/store/example/bin/waynote toggle"), { description = "Toggle Waynote notes" })'
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "hyprland.lua"
            config.write_text(line + "\n")
            binding = {"dispatcher": "__lua", "arg": "174", "description": "Toggle Waynote notes"}
            self.assertEqual(dev.original_binding([binding], config), line)
            fallback = '    hl.bind("SUPER + N", hl.dsp.exec_cmd("noctalia notes"), { description = "Toggle sticky notes" })'
            config.write_text(fallback + "\n" + line + "\n")
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

    def test_orphan_identity_requires_both_dev_identity_and_data_path(self):
        binary = dev.os.fsencode(dev.REPO / "target/debug/waynote") + b"\0"
        environment = b"WAYNOTE_DEV=1\0" + dev.os.fsencode("XDG_DATA_HOME=" + str(dev.WORK / "data")) + b"\0"
        with patch.object(Path, "read_bytes", side_effect=[binary, environment]):
            self.assertTrue(dev.owned_child({"child_pid": 999}))
        with patch.object(Path, "read_bytes", side_effect=[binary, b"WAYNOTE_DEV=1\0"]):
            self.assertFalse(dev.owned_child({"child_pid": 999}))

    def test_empty_child_cmdline_never_reads_environ(self):
        with patch.object(Path, "read_bytes", return_value=b"") as read:
            self.assertFalse(dev.owned_child({"child_pid": 999}))
            read.assert_called_once()

    def test_child_exiting_between_identity_reads_is_not_owned(self):
        binary = dev.os.fsencode(dev.REPO / "target/debug/waynote") + b"\0"
        for state in ("Z", "X"):
            with self.subTest(state=state), \
                    patch.object(Path, "read_bytes", side_effect=[binary, PermissionError()]), \
                    patch.object(Path, "read_text", return_value=f"Name:\twaynote\nState:\t{state} (dead)\n"):
                self.assertFalse(dev.owned_child({"child_pid": 999}))

    def test_live_unreadable_child_is_not_forgotten(self):
        binary = dev.os.fsencode(dev.REPO / "target/debug/waynote") + b"\0"
        with patch.object(Path, "read_bytes", side_effect=[binary, PermissionError()]), \
                patch.object(Path, "read_text", return_value="State:\tS (sleeping)\n"):
            with self.assertRaises(PermissionError):
                dev.owned_child({"child_pid": 999})

    def test_rejected_quit_preserves_child_and_recovery_state_and_restores_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            state_file = Path(directory) / "session.json"
            state = {"pid": 123, "child_pid": 999, "restore": "original()"}
            state_file.write_text(dev.json.dumps(state))
            with patch.object(dev, "STATE", state_file), \
                    patch.object(dev, "owned_process", return_value=False), \
                    patch.object(dev, "owned_child", return_value=True), \
                    patch.object(dev, "execute") as execute, \
                    patch.object(dev.time, "monotonic", side_effect=[0, 6]), \
                    patch.object(dev, "restore_binding") as restore, \
                    patch.object(dev.os, "kill") as kill:
                with self.assertRaisesRegex(RuntimeError, "kept running"):
                    dev.stop()
                execute.assert_called_once_with(str(dev.REPO / "target/debug/waynote"), "quit",
                                                env=dev.dev_env(), timeout=5)
                kill.assert_not_called()
                restore.assert_called_once_with(state)
                self.assertEqual(dev.json.loads(state_file.read_text()), state)

    def test_stale_child_pid_never_cold_starts_app_to_quit(self):
        with patch.object(dev, "owned_child", return_value=False), patch.object(dev, "execute") as execute:
            dev.quit_child({"child_pid": 999})
            execute.assert_not_called()

    def test_floating_requires_exact_development_class_and_child_pid(self):
        from types import SimpleNamespace
        clients = [
            {"class": "dev.mryll.waynote", "pid": 123},
            {"class": dev.APP_CLASS, "pid": 456},
            {"class": dev.APP_CLASS, "pid": 123},
        ]
        with patch.object(dev, "execute", return_value=SimpleNamespace(stdout=dev.json.dumps(clients))):
            self.assertEqual(dev.dev_client({"child_pid": 123}), clients[2])
            self.assertIsNone(dev.dev_client({"child_pid": 789}))
        with patch.dict(dev.os.environ, HYPRLAND_INSTANCE_SIGNATURE="test"), \
                patch.object(dev, "dev_client", return_value=clients[2]), patch.object(dev, "lua") as lua:
            dev.float_window({"child_pid": 123})
            code = lua.call_args.args[0]
            self.assertIn('window.class == "dev.mryll.waynote.development" and window.pid == 123', code)
            self.assertIn('action = "enable"', code)
            self.assertNotIn("window_rule", code)

    def test_floating_restores_note_size_instead_of_tiled_size(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            layout = root / "state/waynote/layout.toml"
            layout.parent.mkdir(parents=True)
            with patch.object(dev, "WORK", root), \
                    patch.dict(dev.os.environ, HYPRLAND_INSTANCE_SIGNATURE="test"), \
                    patch.object(dev, "dev_client", return_value={"pid": 123}), \
                    patch.object(dev, "lua") as lua:
                dev.float_window({"child_pid": 123})
                self.assertIn("x = 400, y = 440", lua.call_args.args[0])
                layout.write_text("width = 420\nheight = 480\n")
                dev.float_window({"child_pid": 123})
                self.assertIn("x = 420, y = 480", lua.call_args.args[0])

    def test_stale_pid_never_targets_this_process(self):
        self.assertFalse(dev.owned_process({"pid": dev.os.getpid()}))


if __name__ == "__main__":
    unittest.main()
