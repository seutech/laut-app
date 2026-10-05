"""Explicit macOS integration check. No recording or text is sent anywhere."""
import subprocess
import sys

code = """
import errno, socket
try:
    socket.create_connection(('127.0.0.1', 9), timeout=1)
except OSError as error:
    assert error.errno == errno.EPERM, (error.errno, str(error))
else:
    raise AssertionError('Network was allowed')
print('Network operation denied by macOS sandbox')
"""
subprocess.run(["/usr/bin/sandbox-exec", "-p", "(version 1) (allow default) (deny network*)", sys.executable, "-c", code], check=True)
