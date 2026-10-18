#!/usr/bin/env python3
"""Isolated Linux runtime probe; never forwards to the user's primary instance."""
import json, os, pathlib, subprocess, sys, tempfile, time
binary = str(pathlib.Path(sys.argv[1]).resolve())
backend = sys.argv[2] if len(sys.argv) > 2 else "tiny-skia"
wayland = os.environ.get("WAYLAND_DISPLAY", "wayland-0")
if not wayland.startswith("/"):
    wayland = os.path.join(os.environ["XDG_RUNTIME_DIR"], wayland)
def sample(pid):
    fields = pathlib.Path(f"/proc/{pid}/stat").read_text().split(") ", 1)[1].split()
    ticks = int(fields[11]) + int(fields[12])
    mem = {}
    for line in pathlib.Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
        if line.startswith(("Rss:", "Pss:")):
            key, value, _ = line.split(); mem[key[:-1]] = int(value)
    return ticks, mem
for mode in ["idle", "camera"]:
    with tempfile.TemporaryDirectory(prefix="qr-benchmark-") as runtime:
        env = dict(os.environ, XDG_RUNTIME_DIR=runtime, WAYLAND_DISPLAY=wayland, ICED_BACKEND=backend)
        with tempfile.TemporaryFile(mode="w+") as log:
            start = time.monotonic()
            proc = subprocess.Popen([binary] + (["--scan-camera"] if mode == "camera" else []), env=env, stderr=log)
            try:
                socket = pathlib.Path(runtime)/"qr-scanner/control.sock"
                while not socket.exists():
                    if proc.poll() is not None or time.monotonic()-start > 10: raise RuntimeError("startup failed")
                    time.sleep(.001)
                startup_ms = (time.monotonic()-start)*1000
                time.sleep(3)
                ticks, _ = sample(proc.pid); began = time.monotonic()
                time.sleep(10)
                end_ticks, mem = sample(proc.pid); elapsed = time.monotonic()-began
                camera_fds = [str(p.resolve()) for p in pathlib.Path(f"/proc/{proc.pid}/fd").iterdir() if str(p.resolve()).startswith("/dev/video")]
                for _ in range(3): subprocess.run([binary], env=env, check=True, timeout=5)
                subprocess.run([binary, "--quit"], env=env, check=True, timeout=5)
                code = proc.wait(timeout=5)
                log.seek(0)
                print(json.dumps(dict(backend=backend, mode=mode, startup_socket_ms=round(startup_ms,3), cpu_percent_one_core=round((end_ticks-ticks)/os.sysconf("SC_CLK_TCK")/elapsed*100,3), memory_kib=mem, sample_seconds=round(elapsed,3), camera_fds=camera_fds, exit=code, stderr=log.read())))
                if code != 0:
                    raise subprocess.CalledProcessError(code, [binary])
            finally:
                if proc.poll() is None:
                    proc.terminate(); proc.wait(timeout=5)
