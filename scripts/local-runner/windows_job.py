"""Windows 10+ Job Object containment. No breakaway; create children in the job.

Uses PROC_THREAD_ATTRIBUTE_JOB_LIST so a crash between creation and assignment
cannot leave an uncontained process. Only this module is Windows-specific.
"""
import ctypes as C
from ctypes import wintypes as W
import os
import subprocess
import time

K = C.WinDLL("kernel32", use_last_error=True)
SIZE = C.c_size_t

class BasicLimit(C.Structure):
    _fields_ = [("per_process", C.c_longlong), ("per_job", C.c_longlong),
                ("flags", W.DWORD), ("minimum", SIZE), ("maximum", SIZE),
                ("active_limit", W.DWORD), ("affinity", SIZE),
                ("priority", W.DWORD), ("scheduling", W.DWORD)]

class IoCounters(C.Structure):
    _fields_ = [(x, C.c_ulonglong) for x in ("read_ops", "write_ops", "other_ops", "read_bytes", "write_bytes", "other_bytes")]

class ExtendedLimit(C.Structure):
    _fields_ = [("basic", BasicLimit), ("io", IoCounters),
                ("process_memory", SIZE), ("job_memory", SIZE),
                ("peak_process", SIZE), ("peak_job", SIZE)]

class Accounting(C.Structure):
    _fields_ = [("user", C.c_longlong), ("kernel", C.c_longlong),
                ("period_user", C.c_longlong), ("period_kernel", C.c_longlong),
                ("faults", W.DWORD), ("total", W.DWORD),
                ("active", W.DWORD), ("terminated", W.DWORD)]

class Startup(C.Structure):
    _fields_ = [("cb", W.DWORD), ("reserved", W.LPWSTR), ("desktop", W.LPWSTR),
                ("title", W.LPWSTR), ("x", W.DWORD), ("y", W.DWORD),
                ("xsize", W.DWORD), ("ysize", W.DWORD), ("xchars", W.DWORD),
                ("ychars", W.DWORD), ("fill", W.DWORD), ("flags", W.DWORD),
                ("show", W.WORD), ("reserved_size", W.WORD),
                ("reserved_data", C.c_void_p), ("stdin", W.HANDLE),
                ("stdout", W.HANDLE), ("stderr", W.HANDLE)]

class StartupEx(C.Structure):
    _fields_ = [("startup", Startup), ("attributes", C.c_void_p)]

class ProcessInfo(C.Structure):
    _fields_ = [("process", W.HANDLE), ("thread", W.HANDLE),
                ("pid", W.DWORD), ("tid", W.DWORD)]


def signature(name, result, *args):
    fn = getattr(K, name)
    fn.restype, fn.argtypes = result, args
    return fn

create_job = signature("CreateJobObjectW", W.HANDLE, C.c_void_p, W.LPCWSTR)
set_job = signature("SetInformationJobObject", W.BOOL, W.HANDLE, C.c_int, C.c_void_p, W.DWORD)
query_job = signature("QueryInformationJobObject", W.BOOL, W.HANDLE, C.c_int, C.c_void_p, W.DWORD, C.c_void_p)
terminate_job = signature("TerminateJobObject", W.BOOL, W.HANDLE, W.UINT)
close = signature("CloseHandle", W.BOOL, W.HANDLE)
init_attrs = signature("InitializeProcThreadAttributeList", W.BOOL, C.c_void_p, W.DWORD, W.DWORD, C.POINTER(SIZE))
update_attrs = signature("UpdateProcThreadAttribute", W.BOOL, C.c_void_p, W.DWORD, SIZE, C.c_void_p, SIZE, C.c_void_p, C.c_void_p)
delete_attrs = signature("DeleteProcThreadAttributeList", None, C.c_void_p)
create_process = signature("CreateProcessW", W.BOOL, W.LPCWSTR, W.LPWSTR, C.c_void_p, C.c_void_p, W.BOOL, W.DWORD, C.c_void_p, W.LPCWSTR, C.c_void_p, C.POINTER(ProcessInfo))
exit_code = signature("GetExitCodeProcess", W.BOOL, W.HANDLE, C.POINTER(W.DWORD))
wait = signature("WaitForSingleObject", W.DWORD, W.HANDLE, W.DWORD)


def checked(ok):
    if not ok:
        raise C.WinError(C.get_last_error())
    return ok


class Process:
    def __init__(self, info):
        self.handle = info.process
        close(info.thread)
        self.pid = info.pid
        self.code = None

    def poll(self):
        if self.code is not None:
            return self.code
        if wait(self.handle, 0) == 258:
            return None
        code = W.DWORD()
        checked(exit_code(self.handle, C.byref(code)))
        self.code = code.value
        close(self.handle)
        return self.code


class Job:
    def __init__(self, operation):
        # Global namespace preserves identity when reconnecting in another user session.
        # Access failure is an error, never proof that the old job is empty.
        self.handle = checked(create_job(None, "Global\\LVK-" + operation))
        limits = ExtendedLimit()
        limits.basic.flags = 0x2000  # KILL_ON_JOB_CLOSE, no breakaway flags.
        checked(set_job(self.handle, 9, C.byref(limits), C.sizeof(limits)))

    def spawn(self, argv, cwd, env, log):
        import msvcrt
        size = SIZE()
        init_attrs(None, 2, 0, C.byref(size))
        attrs = C.create_string_buffer(size.value)
        checked(init_attrs(attrs, 2, 0, C.byref(size)))
        null = open(os.devnull, "rb")
        handles = (W.HANDLE * 2)(msvcrt.get_osfhandle(null.fileno()), msvcrt.get_osfhandle(log.fileno()))
        jobs = (W.HANDLE * 1)(self.handle)
        for h in handles:
            os.set_handle_inheritable(h, True)
        try:
            checked(update_attrs(attrs, 0, 0x2000D, jobs, C.sizeof(jobs), None, None))
            checked(update_attrs(attrs, 0, 0x20002, handles, C.sizeof(handles), None, None))
            startup = StartupEx()
            startup.startup.cb = C.sizeof(startup)
            startup.startup.flags = 0x100  # STARTF_USESTDHANDLES
            startup.startup.stdin = handles[0]
            startup.startup.stdout = startup.startup.stderr = handles[1]
            startup.attributes = C.cast(attrs, C.c_void_p)
            info = ProcessInfo()
            environment = C.create_unicode_buffer("\0".join(f"{k}={v}" for k, v in sorted(env.items())) + "\0\0")
            line = C.create_unicode_buffer(subprocess.list2cmdline(argv))
            checked(create_process(None, line, None, None, True, 0x80000 | 0x400,
                                   environment, str(cwd), C.byref(startup), C.byref(info)))
            return Process(info)
        finally:
            for h in handles:
                os.set_handle_inheritable(h, False)
            null.close()
            delete_attrs(attrs)

    def stop(self):
        checked(terminate_job(self.handle, 1))
        deadline = time.monotonic() + 10
        while True:
            info = Accounting()
            checked(query_job(self.handle, 1, C.byref(info), C.sizeof(info), None))
            if info.active == 0:
                return
            if time.monotonic() >= deadline:
                raise RuntimeError("Job still has active processes; resources must remain held")
            time.sleep(.05)

    def close(self):
        if self.handle:
            self.stop()
            checked(close(self.handle))
            self.handle = None


def desktop_available():
    user = C.WinDLL("user32", use_last_error=True)
    user.OpenInputDesktop.argtypes = [W.DWORD, W.BOOL, W.DWORD]
    user.OpenInputDesktop.restype = W.HANDLE
    user.CloseDesktop.argtypes = [W.HANDLE]
    session = W.DWORD()
    fn = signature("ProcessIdToSessionId", W.BOOL, W.DWORD, C.POINTER(W.DWORD))
    if not fn(os.getpid(), C.byref(session)) or session.value == 0:
        return False
    desktop = user.OpenInputDesktop(0, False, 0x0001)
    if not desktop:
        return False
    user.CloseDesktop(desktop)
    return True
