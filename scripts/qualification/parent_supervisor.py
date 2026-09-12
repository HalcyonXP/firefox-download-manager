"""Retain SDK diagnostic owners across failure; no process launch or kill fallback.

The live entry point must keep this supervisor alive while exit_code() is None.
That state is a hold, never permission to return from the owning process. A hold
UI/control channel is supplied by the reviewed caller, not by this library.
"""
import time

ERROR = 'SDK supervisor must retain unresolved owners'


class ParentSupervisor:
    def __init__(self):
        self.owners = []
        self.started = self.failed = self.cleanup_attempted = False
        self.interruption = None

    def __repr__(self):
        return '<ParentSupervisor redacted>'

    def retain(self, name, owner):
        if (name not in ('compiler', 'sdk') or any(key == name for key, _ in self.owners)
                or self.failed or self.cleanup_attempted
                or not callable(getattr(owner, 'cleanup', None))
                or not callable(getattr(owner, 'cleanup_complete', None))):
            raise RuntimeError(ERROR)
        self.owners.append((name, owner))
        return owner

    def _failure(self, error):
        self.failed = True
        if not isinstance(error, Exception) and self.interruption is None:
            self.interruption = error

    def execute(self, action):
        if self.started or self.cleanup_attempted:
            raise RuntimeError(ERROR)
        self.started = True
        try:
            action(self)
        except BaseException as error:
            self._failure(error)
        finally:
            self.cleanup()
        return self.status()

    def cleanup(self):
        if self.cleanup_attempted:
            return
        self.cleanup_attempted = True
        for _, owner in reversed(self.owners):
            try:
                if owner.cleanup() is not True:
                    self.failed = True
            except BaseException as error:
                self._failure(error)

    def _settled(self):
        settled = True
        for _, owner in self.owners:
            try:
                if owner.cleanup_complete() is not True:
                    if self.cleanup_attempted:
                        self.failed = True
                    settled = False
            except BaseException as error:
                self._failure(error)
                settled = False
        return settled

    def status(self):
        settled = self._settled()
        return {'version': 1, 'qualification': False, 'started': self.started,
                'failed': self.failed, 'cancelled': self.interruption is not None,
                'cleanup_attempted': self.cleanup_attempted,
                'held': not settled, 'owner_count': len(self.owners)}

    def exit_code(self):
        if not self.cleanup_attempted or not self._settled():
            return None
        return 1 if self.failed or not self.started else 0

    def finish(self):
        code = self.exit_code()
        if code is None:
            raise RuntimeError(ERROR)
        if self.interruption is not None:
            raise self.interruption
        return code


def serve(supervisor, action, read_command, write_status, notify_hold):
    """Fixed start/status/finish channel; EOF is never authority to drop owners.

    Callbacks belong to a reviewed local entry point, not browser/API inputs.
    A disconnected unresolved host stays alive with its retained supervisor.
    """
    try:
        while True:
            command = read_command()
            if command == 'start' and not supervisor.started and not supervisor.cleanup_attempted:
                supervisor.execute(action)
            elif command == 'status':
                pass
            elif command == 'finish':
                if supervisor.exit_code() is not None:
                    return supervisor.finish()
            else:
                raise RuntimeError(ERROR)
            write_status(supervisor.status())
    except BaseException as error:
        supervisor._failure(error)
        supervisor.cleanup()
        if supervisor.exit_code() is None:
            try:
                notify_hold(supervisor.status())
            except BaseException as notice_error:
                supervisor._failure(notice_error)
            # No command replay, process kill, new discovery or success on EOF.
            while supervisor.exit_code() is None:
                try:
                    time.sleep(1)
                except BaseException as interruption:
                    supervisor._failure(interruption)
        return supervisor.finish()
