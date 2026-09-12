"""Retained setup ownership for diagnostic drivers, not installation authority.

Operation completion is not success. An initial no-child label cannot resolve an
uncertain Install dispatch. Callers retain this object on failed retirement; no
PID/name/tree discovery or forced parent termination is provided here.
"""
from dataclasses import dataclass
import json
from pathlib import Path
import re
import time
import uuid


@dataclass
class DomainPlan:
    path: Path
    created: bool = False

    @classmethod
    def record(cls, parent, private_directory):
        """Write a private plan BEFORE creating the domain; plans do not own collisions."""
        for directory in (parent, private_directory):
            if (not directory.is_dir() or directory.resolve() != directory
                    or any(p.is_symlink() or p.is_junction() for p in (directory, *directory.parents))):
                raise RuntimeError("diagnostic parent is not an ordinary canonical directory")
        identity = str(uuid.uuid4())
        path = parent / f"dm-installed-{identity}"
        ticket = private_directory / f"installed-{identity}.private.json"
        with ticket.open("x", encoding="utf-8") as stream:
            json.dump({"domain": str(path), "creation_observed": False, "qualification": False}, stream)
        return cls(path)

    def create(self):
        if self.created:
            raise RuntimeError("diagnostic domain already created")
        self.path.mkdir()  # Exclusive creation only; the plan cannot bless existing bytes.
        self.created = True  # Keep the witness even if the subsequent record fails.
        with (self.path / "creation.private.json").open("x", encoding="utf-8") as stream:
            json.dump({"creation_observed": True, "qualification": False}, stream)


class SetupOwner:
    """Callbacks operate only against the exact retained Popen and its GUI receipts."""
    def __init__(self, process, snapshot, close_setup, quit_manager):
        self.process = process
        self.snapshot = snapshot
        self.close_setup = close_setup
        self.quit_manager = quit_manager
        self.sequence = 0
        self.child_id = None
        self.joined = False
        self.before_close = None
        self.close_requested = False
        self.quit_requested = False

    def request(self, dispatch):
        if self.close_requested:
            raise RuntimeError("setup close remains pending or joined")
        operation, _ = self._observe()
        if operation not in ("idle", "complete"):
            raise RuntimeError("setup operation remains uncertain")
        # Mark uncertainty BEFORE delivering the control. A thrown delivery call
        # is not evidence that setup did not receive it. Never replay it here.
        self.sequence += 1
        dispatch()

    def _observe(self):
        if self.process.poll() is not None:
            raise RuntimeError("setup owner exited before observation")
        observation = self.snapshot()
        if not isinstance(observation, tuple) or len(observation) != 2:
            raise RuntimeError("invalid setup observation")
        operation, lifetime = observation
        if not isinstance(operation, str) or not isinstance(lifetime, str):
            raise RuntimeError("invalid setup observation")
        match = re.fullmatch(r"Operation (0|[1-9][0-9]{0,19}): (idle|running|complete)", operation)
        if (not match or int(match[1]) != self.sequence or int(match[1]) > 2**64-1
                or (match[2] == "idle") != (self.sequence == 0)):
            raise RuntimeError("setup operation observation is unresolved")
        if lifetime == "No Manager process launched by this setup.":
            if self.child_id is not None:
                raise RuntimeError("retained Manager observation disappeared")
            return match[2], None
        if lifetime in ("Manager exit observed; retained child joined.", "Manager failed; retained child joined."):
            return match[2], None
        child = re.fullmatch(r"Owned Manager process: ([1-9][0-9]{0,9})", lifetime)
        if not child or int(child[1]) > 2**32-1:
            raise RuntimeError("invalid retained Manager observation")
        child_id = int(child[1])
        if self.child_id is not None and self.child_id != child_id:
            raise RuntimeError("retained Manager identity changed")
        self.child_id = child_id
        return match[2], child_id

    def quiesce(self, timeout=15):
        """Settle the action and join Manager, keeping setup alive for uninstall."""
        if self.close_requested:
            raise RuntimeError("setup close remains pending or joined")
        return self._quiesce_until(time.monotonic() + timeout)

    def _quiesce_until(self, deadline):
        while time.monotonic() < deadline:
            try:
                state, child_id = self._observe()
            except RuntimeError:
                time.sleep(0.05)
                continue
            if child_id is not None:
                if not self.quit_requested:
                    self.quit_requested = True
                    self.quit_manager(child_id)
            elif state in ("idle", "complete"):
                return state
            time.sleep(0.05)
        raise RuntimeError("setup retirement unconfirmed; retain owner and domain")

    def retire(self, timeout=15):
        if self.joined:
            return
        if self.close_requested:
            # Delivery/wait failure may still have closed the window. Never replay.
            self.process.wait(timeout=timeout)
            self.joined = True
            return
        deadline = time.monotonic() + timeout
        state = self._quiesce_until(deadline)
        self.before_close = (state, "no retained Manager")
        self.close_requested = True
        self.close_setup()
        self.process.wait(timeout=max(0, deadline-time.monotonic()))
        self.joined = True
